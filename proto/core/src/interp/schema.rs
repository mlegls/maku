//! The load-time schema pass (openspec/specs/load-time-schema/spec.md):
//! one walk over the loaded card that resolves stream SCOPING — a free
//! `$name` neither lexically bound nor card-declared is a load error —
//! and collects the host-channel manifest as the set of
//! `(from-host :name ...)` sites. Runs before tick 0 at every load path
//! (fresh load, run/swap/add fragments).

use crate::edn::Form;
use crate::model::RenderFieldKind;
use std::rc::Rc;
use super::card::Card;
use std::collections::HashSet;

/// Names the pass always resolves: engine-provided streams.
const BUILTIN_STREAMS: [&str; 1] = ["tick"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGeometry { Point, Polyline }

#[derive(Clone, Debug)]
pub struct RenderKindDecl {
    pub name: Rc<str>,
    pub geometry: RenderGeometry,
    pub fields: Vec<(Rc<str>, RenderFieldKind)>,
}

pub struct CardSchema {
    /// Host inputs the card requires: bare names from (from-host :name) sites.
    pub host_channels: Vec<String>,
    /// Lints (never errors): e.g. set! on a stream whose top-level producer
    /// looks always-writing — the write survives only until the next refresh.
    pub warnings: Vec<String>,
    /// Fixed schemas declared by this card, in declaration order.
    pub render_kinds: Vec<RenderKindDecl>,
    /// Kinds directly visible at standing render emission sites.
    pub emitted_render_kinds: Vec<String>,
}

pub fn collect_card_schema(card: &Card) -> Result<CardSchema, String> {
    let mut render_kinds = Vec::new();
    for form in &card.render_kinds {
        let Form::List(items) = form else { unreachable!() };
        let Some(Form::Kw(name)) = items.get(1) else {
            return Err("defrender-kind: expected a keyword name".into());
        };
        let Some(Form::Map(opts)) = items.get(2) else {
            return Err(format!("defrender-kind :{name}: expected option map"));
        };
        let mut geometry = None;
        let mut fields = None;
        for (key, value) in opts.iter() {
            let Form::Kw(key) = key else { return Err(format!("defrender-kind :{name}: option keys must be keywords")) };
            match key.as_ref() {
                "geometry" => geometry = Some(match value {
                    Form::Kw(v) if v.as_ref() == "point" => RenderGeometry::Point,
                    Form::Kw(v) if v.as_ref() == "polyline" => RenderGeometry::Polyline,
                    _ => return Err(format!("defrender-kind :{name}: :geometry must be :point or :polyline")),
                }),
                "fields" => {
                    let Form::Map(map) = value else { return Err(format!("defrender-kind :{name}: :fields must be a map")) };
                    let mut out = Vec::new();
                    for (field, kind) in map.iter() {
                        let (Form::Kw(field), Form::Kw(kind)) = (field, kind) else {
                            return Err(format!("defrender-kind :{name}: fields must map keywords to :num or :sym"));
                        };
                        let kind = match kind.as_ref() {
                            "num" => RenderFieldKind::Num,
                            "sym" => RenderFieldKind::Sym,
                            _ => return Err(format!("defrender-kind :{name}: field :{field} must be :num or :sym")),
                        };
                        out.push((field.clone(), kind));
                    }
                    fields = Some(out);
                }
                _ => return Err(format!("defrender-kind :{name}: unknown option :{key}")),
            }
        }
        let decl = RenderKindDecl {
            name: name.clone(),
            geometry: geometry.ok_or_else(|| format!("defrender-kind :{name}: missing :geometry"))?,
            fields: fields.ok_or_else(|| format!("defrender-kind :{name}: missing :fields"))?,
        };
        if let Some(prior) = render_kinds.iter().find(|d: &&RenderKindDecl| d.name == decl.name) {
            if prior.geometry != decl.geometry || prior.fields != decl.fields {
                return Err(format!("defrender-kind :{name}: conflicting declaration"));
            }
        } else {
            render_kinds.push(decl);
        }
    }
    // Card-global stream names: top-level defs, plus every name published
    // by an (export! ...) form anywhere in the card (publication makes the
    // name readable card-wide once the exporting pattern runs).
    let mut globals: HashSet<String> =
        card.streams.iter().map(|(n, _)| n.to_string()).collect();
    for n in BUILTIN_STREAMS {
        globals.insert(n.to_string());
    }
    let mut pre = ExportScan { globals: &mut globals };
    for (_, init) in &card.streams {
        if let Some(f) = init {
            pre.walk(f);
        }
    }
    for f in card.stream_forms.iter().chain(card.tick_rules.iter()) {
        pre.walk(f);
    }
    for f in card.defs.values() {
        pre.walk(f);
    }
    for p in card.patterns.values() {
        for f in p.body.iter() {
            pre.walk(f);
        }
    }

    // top-level producers that look ALWAYS-writing (can't yield nothing):
    // anything but a conditional head. Feeds the set!-on-sealed-stream lint.
    let mut sealed: HashSet<String> = HashSet::new();
    for f in &card.stream_forms {
        if let Form::List(items) = f {
            if let (Some(Form::Sym(h)), Some(Form::Sym(n)), Some(expr)) =
                (items.first(), items.get(1), items.get(2))
            {
                if &**h == "bind!" && n.starts_with('$') {
                    let conditional = matches!(expr, Form::List(e)
                        if matches!(e.first(), Some(Form::Sym(eh))
                            if matches!(&**eh, "cond" | "if" | "when")));
                    if !conditional {
                        sealed.insert(n[1..].to_string());
                    }
                }
            }
        }
    }

    let mut cx = Cx { globals: &globals, hosts: Vec::new(), sealed: &sealed, warnings: Vec::new() };
    let empty: Vec<String> = Vec::new();
    for (name, init) in &card.streams {
        if let Some(f) = init {
            cx.walk(f, &empty).map_err(|e| format!("def ${}: {}", name, e))?;
        }
    }
    for f in &card.stream_forms {
        cx.walk(f, &empty)?;
    }
    for f in &card.tick_rules {
        cx.walk(f, &empty)?;
    }
    for (name, f) in &card.defs {
        cx.walk(f, &empty).map_err(|e| format!("def {}: {}", name, e))?;
    }
    for p in card.patterns.values() {
        let scope: Vec<String> = p
            .params
            .iter()
            .filter(|(n, _)| n.starts_with('$'))
            .map(|(n, _)| n.to_string())
            .collect();
        for (_, default) in &p.params {
            cx.walk(default, &empty)
                .map_err(|e| format!("pattern {}: {}", p.name, e))?;
        }
        for f in p.body.iter() {
            cx.walk(f, &scope).map_err(|e| format!("pattern {}: {}", p.name, e))?;
        }
    }
    let mut emitted_render_kinds = Vec::new();
    for form in &card.tick_rules {
        collect_emitted_render_kinds(form, &mut emitted_render_kinds);
    }
    Ok(CardSchema { host_channels: cx.hosts, warnings: cx.warnings, render_kinds, emitted_render_kinds })
}

fn collect_emitted_render_kinds(form: &Form, out: &mut Vec<String>) {
    if let Form::List(items) = form {
        let fields = if matches!(items.first(), Some(Form::Sym(h)) if h.as_ref() == "emit")
            && matches!(items.get(1), Some(Form::Kw(c)) if c.as_ref() == "render") {
            items.get(2)
        } else if matches!(items.first(), Some(Form::Sym(h)) if h.as_ref() == "render") {
            items.get(1)
        } else { None };
        if let Some(Form::Map(fields)) = fields {
                let kind = fields.iter().find_map(|(k, v)| match (k, v) {
                    (Form::Kw(k), Form::Kw(v)) if k.as_ref() == "kind" => Some(v.to_string()),
                    _ => None,
                }).unwrap_or_else(|| "default".into());
                if !out.contains(&kind) { out.push(kind); }
        }
    }
    for child in form_children(form) { collect_emitted_render_kinds(child, out); }
}

/// Pre-scan: (export! $x) / (export! $x :as $name) publication names
/// become card-global.
struct ExportScan<'a> {
    globals: &'a mut HashSet<String>,
}

impl ExportScan<'_> {
    fn walk(&mut self, form: &Form) {
        if let Form::List(items) = form {
            if let Some(Form::Sym(head)) = items.first() {
                if &**head == "quasiquote" {
                    return;
                }
                if &**head == "export!" {
                    let public = match (items.get(2), items.get(3)) {
                        (Some(Form::Kw(k)), Some(Form::Sym(n)))
                            if &**k == "as" && n.starts_with('$') =>
                        {
                            Some(&n[1..])
                        }
                        _ => match items.get(1) {
                            Some(Form::Sym(n)) if n.starts_with('$') => Some(&n[1..]),
                            _ => None,
                        },
                    };
                    if let Some(p) = public {
                        self.globals.insert(p.to_string());
                    }
                }
            }
        }
        for child in form_children(form) {
            self.walk(child);
        }
    }
}

struct Cx<'a> {
    globals: &'a HashSet<String>,
    hosts: Vec<String>,
    sealed: &'a HashSet<String>,
    warnings: Vec<String>,
}

impl Cx<'_> {
    fn resolve(&self, sigiled: &str, scope: &[String]) -> Result<(), String> {
        if scope.iter().any(|s| s == sigiled) || self.globals.contains(&sigiled[1..]) {
            Ok(())
        } else {
            Err(format!(
                "unbound stream {} (declare it: (def {}) or a local (let [{} ...] ...))",
                sigiled, sigiled, sigiled
            ))
        }
    }

    fn walk(&mut self, form: &Form, scope: &[String]) -> Result<(), String> {
        match form {
            Form::Sym(s) if s.starts_with('$') => self.resolve(s, scope),
            Form::List(items) => {
                let head = match items.first() {
                    Some(Form::Sym(h)) => Some(&**h),
                    _ => None,
                };
                match head {
                    // macro templates aren't card code
                    Some("quasiquote") => Ok(()),
                    Some("from-host") => {
                        if let Some(Form::Kw(name)) = items.get(1) {
                            if !self.hosts.iter().any(|h| h == &**name) {
                                self.hosts.push(name.to_string());
                            }
                        }
                        for f in items.iter().skip(2) {
                            self.walk(f, scope)?;
                        }
                        Ok(())
                    }
                    // (channel $x default): the explicit soft read — the
                    // name may be host-injected without a declaration
                    Some("channel") => {
                        for f in items.iter().skip(2) {
                            self.walk(f, scope)?;
                        }
                        Ok(())
                    }
                    // with references existing streams; its map introduces
                    // no names into the body scope.
                    Some("with") => {
                        let Some(Form::Map(binds)) = items.get(1) else {
                            return self.walk_children(items, scope);
                        };
                        for (target, value) in binds.iter() {
                            self.walk(target, scope)?;
                            self.walk(value, scope)?;
                        }
                        for body in items.iter().skip(2) {
                            self.walk(body, scope)?;
                        }
                        Ok(())
                    }
                    Some("set!") => {
                        if let Some(Form::Sym(n)) = items.get(1) {
                            if n.starts_with('$')
                                && !scope.iter().any(|s| s == &**n)
                                && self.sealed.contains(&n[1..])
                            {
                                let w = format!(
                                    "set! on {}: its producer overwrites at every refresh",
                                    n
                                );
                                if !self.warnings.contains(&w) {
                                    self.warnings.push(w);
                                }
                            }
                        }
                        self.walk_children(items, scope)
                    }
                    // (export! $x :as $name): $name declares, $x reads
                    Some("export!") => {
                        if let Some(f) = items.get(1) {
                            self.walk(f, scope)?;
                        }
                        Ok(())
                    }
                    Some("let") | Some("loop") => {
                        let Some(Form::Vector(binds)) = items.get(1) else {
                            return self.walk_children(items, scope);
                        };
                        let mut inner = scope.to_vec();
                        for c in binds.chunks(2) {
                            if c.len() == 2 {
                                self.walk(&c[1], &inner)?;
                            }
                            if let Form::Sym(n) = &c[0] {
                                if n.starts_with('$') {
                                    inner.push(n.to_string());
                                }
                            }
                        }
                        for f in items.iter().skip(2) {
                            self.walk(f, &inner)?;
                        }
                        Ok(())
                    }
                    Some("fn") => {
                        let Some(Form::Vector(params)) = items.get(1) else {
                            return self.walk_children(items, scope);
                        };
                        let mut inner = scope.to_vec();
                        for p in params.iter() {
                            if let Form::Sym(n) = p {
                                if n.starts_with('$') {
                                    inner.push(n.to_string());
                                }
                            }
                        }
                        for f in items.iter().skip(2) {
                            self.walk(f, &inner)?;
                        }
                        Ok(())
                    }
                    _ => self.walk_children(items, scope),
                }
            }
            _ => {
                for child in form_children(form) {
                    self.walk(child, scope)?;
                }
                Ok(())
            }
        }
    }

    fn walk_children(&mut self, items: &[Form], scope: &[String]) -> Result<(), String> {
        for f in items {
            self.walk(f, scope)?;
        }
        Ok(())
    }
}

fn form_children(form: &Form) -> Box<dyn Iterator<Item = &Form> + '_> {
    match form {
        Form::List(items) | Form::Vector(items) => Box::new(items.iter()),
        Form::Map(kvs) => Box::new(kvs.iter().flat_map(|(k, v)| [k, v])),
        _ => Box::new(std::iter::empty()),
    }
}
