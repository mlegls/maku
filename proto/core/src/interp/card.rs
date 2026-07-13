//! Card loading: top-level defs, defns, defpatterns (callable).

use crate::edn::Form;
use super::{rewrite_card, FigureProjectorKind};
use std::collections::HashMap;
use std::rc::Rc;


// ---------------------------------------------------------------------------
// Card: top-level definitions.

#[derive(Clone)]
pub struct Pattern {
    pub name: String,
    pub params: Vec<(Rc<str>, Form)>,
    pub body: Rc<[Form]>,
}

/// (defmacro name [params…] body…): a tree transformation (§10). Arguments
/// arrive UNEVALUATED as forms; the body (usually quasiquoted) returns a
/// form, which evaluates in the caller's scope. Unhygienic in the classic
/// way — templates should use unusual names for introduced bindings.
#[derive(Clone)]
pub struct Macro {
    pub params: Vec<Rc<str>>,
    pub body: Rc<[Form]>,
}

pub struct Card {
    pub patterns: HashMap<String, Pattern>,
    pub order: Vec<String>,
    pub defs: HashMap<String, Form>,
    pub macros: HashMap<String, Macro>,
    /// (deftick body...) — evaluated when the card is installed, registering
    /// per-tick rules into World data.
    pub tick_rules: Vec<Form>,
    /// (def $name init?) — top-level stream declarations, in card order.
    /// Bare names (no sigil); ids allocate at install.
    pub streams: Vec<(Rc<str>, Option<Form>)>,
    /// Top-level render-kind declarations, validated by the load-time schema pass.
    pub render_kinds: Vec<Form>,
    /// Top-level (bind! ...) / (export! ...) forms, run at install in
    /// card order — producers attach before any pattern executes.
    pub stream_forms: Vec<Form>,
}

pub fn load_card(forms: &[Form]) -> Result<Card, String> {
    let mut patterns = HashMap::new();
    let mut order = Vec::new();
    let mut defs = HashMap::new();
    let mut macros = HashMap::new();
    let mut tick_rules: Vec<Form> = Vec::new();
    let mut streams: Vec<(Rc<str>, Option<Form>)> = Vec::new();
    let mut stream_forms: Vec<Form> = Vec::new();
    let mut render_kinds: Vec<Form> = Vec::new();
    for f in forms {
        if let Form::List(items) = f {
            match items.first() {
                Some(Form::Sym(s)) if &**s == "defmacro" => {
                    let name = match items.get(1) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defmacro: expected name".into()),
                    };
                    let params: Vec<Rc<str>> = match items.get(2) {
                        Some(Form::Vector(ps)) => ps
                            .iter()
                            .map(|p| match p {
                                Form::Sym(n) => Ok(n.clone()),
                                _ => Err("defmacro: params must be symbols".to_string()),
                            })
                            .collect::<Result<_, _>>()?,
                        _ => return Err("defmacro: expected param vector".into()),
                    };
                    macros.insert(name, Macro { params, body: items[3..].to_vec().into() });
                }
                Some(Form::Sym(s)) if &**s == "defpattern" => {
                    let name = match items.get(1) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defpattern: expected name".into()),
                    };
                    let params = match items.get(2) {
                        Some(Form::Vector(ps)) => {
                            if ps.len() % 2 != 0 {
                                return Err(format!(
                                    "{}: param vector must be name/default pairs",
                                    name
                                ));
                            }
                            ps.chunks(2)
                                .map(|c| match &c[0] {
                                    Form::Sym(n) => Ok((n.clone(), c[1].clone())),
                                    _ => Err(format!("{}: bad param name", name)),
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        }
                        _ => return Err(format!("{}: expected param vector", name)),
                    };
                    let body: Rc<[Form]> = items[3..].to_vec().into();
                    order.retain(|n| n != &name);
                    order.push(name.clone());
                    patterns.insert(name.clone(), Pattern { name, params, body });
                }
                Some(Form::Sym(s)) if &**s == "def" => {
                    if let Some(Form::Sym(n)) = items.get(1) {
                        if let Some(bare) = n.strip_prefix('$') {
                            // (def $name init?) — a top-level stream.
                            // Redefinition replaces (import shadowing).
                            let bare: Rc<str> = bare.into();
                            streams.retain(|(k, _)| *k != bare);
                            streams.push((bare, items.get(2).cloned()));
                        } else {
                            defs.insert(n.to_string(), items[2].clone());
                        }
                    }
                }
                Some(Form::Sym(s)) if &**s == "bind!" || &**s == "export!" => {
                    stream_forms.push(f.clone());
                }
                Some(Form::Sym(s)) if &**s == "defn" => {
                    // (defn name [params] body...) → def name (fn [params] body...)
                    if let Some(Form::Sym(n)) = items.get(1) {
                        let mut fnform = vec![Form::sym("fn")];
                        fnform.extend(items[2..].iter().cloned());
                        defs.insert(n.to_string(), Form::list(fnform));
                    }
                }
                Some(Form::Sym(s)) if &**s == "defcollider" => {
                    // (defcollider name [e ctx] body...)
                    // (defcollider :pose name [e ctx] body...)
                    let (figure, name_idx) = match items.get(1) {
                        Some(Form::Kw(k)) => {
                            let figure = FigureProjectorKind::from_defcollider_keyword(k)?;
                            (figure, 2)
                        }
                        _ => (FigureProjectorKind::Pose, 1),
                    };
                    let name = match items.get(name_idx) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defcollider: expected name".into()),
                    };
                    let params_idx = name_idx + 1;
                    let body_idx = name_idx + 2;
                    let params: Vec<Form> = match items.get(params_idx) {
                        Some(Form::Vector(ps)) if ps.len() == 2 => {
                            ps.iter()
                                .map(|p| match p {
                                    Form::Sym(_) => Ok(p.clone()),
                                    _ => Err("defcollider: params must be symbols".to_string()),
                                })
                                .collect::<Result<_, _>>()?
                        }
                        Some(Form::Vector(_)) => {
                            return Err("defcollider: expected two parameters".into());
                        }
                        _ => return Err("defcollider: expected parameter vector".into()),
                    };
                    if items.len() <= body_idx {
                        return Err("defcollider: expected body".into());
                    }
                    let mut projector = vec![
                        Form::sym("collider"),
                        Form::Kw(figure.name().into()),
                        Form::Vector(params.into()),
                    ];
                    projector.extend(items[body_idx..].iter().cloned());
                    defs.insert(name, Form::list(projector));
                }
                Some(Form::Sym(s)) if &**s == "defchannel" => {
                    // (defchannel $name expr) is sugar over the stream
                    // kernel: def $name + bind! $name expr + export! $name.
                    // Redefinition replaces (imports first, card later —
                    // ordinary shadowing; the producer rebinds in place).
                    let Some(Form::Sym(n)) = items.get(1) else {
                        return Err("defchannel: expected a $channel name".into());
                    };
                    let Some(name) = n.strip_prefix('$') else {
                        return Err("defchannel: name must start with $".into());
                    };
                    let Some(expr) = items.get(2) else {
                        return Err(format!("defchannel ${}: expected an expression", name));
                    };
                    let bare: Rc<str> = name.into();
                    streams.retain(|(k, _)| *k != bare);
                    streams.push((bare, None));
                    stream_forms.push(Form::list(vec![
                        Form::sym("bind!"),
                        Form::Sym(n.clone()),
                        expr.clone(),
                    ]));
                    stream_forms.push(Form::list(vec![Form::sym("export!"), Form::Sym(n.clone())]));
                }
                Some(Form::Sym(s)) if &**s == "deftick" => {
                    tick_rules.push(expand_render_adapts(f)?);
                }
                Some(Form::Sym(s)) if &**s == "render-adapt" => {
                    let adapted = parse_render_adapter(items)?;
                    for rule in items.iter().skip(2) {
                        let rewritten = rewrite_render_form(rule, &adapted);
                        if matches!(&rewritten, Form::List(xs)
                            if matches!(xs.first(), Some(Form::Sym(h)) if h.as_ref() == "deftick")) {
                            tick_rules.push(expand_render_adapts(&rewritten)?);
                        } else {
                            return Err("render-adapt: top-level bodies must be deftick rules".into());
                        }
                    }
                }
                Some(Form::Sym(s)) if &**s == "defrender-kind" => {
                    render_kinds.push(f.clone());
                }
                _ => {}
            }
        }
    }
    let mut card = Card { patterns, order, defs, macros, tick_rules, streams, render_kinds, stream_forms };
    rewrite_card(&mut card);
    Ok(card)
}

struct RenderAdapter {
    from: Rc<str>,
    to: Rc<str>,
    fields: Vec<(Rc<str>, Rc<str>)>,
}

fn parse_render_adapter(items: &[Form]) -> Result<RenderAdapter, String> {
    let Some(Form::Map(opts)) = items.get(1) else {
        return Err("render-adapt: expected option map".into());
    };
    let mut from = None;
    let mut to = None;
    let mut fields = None;
    for (key, value) in opts.iter() {
        let Form::Kw(key) = key else { return Err("render-adapt: option keys must be keywords".into()) };
        match key.as_ref() {
            "kind" => match value { Form::Kw(v) => from = Some(v.clone()), _ => return Err("render-adapt: :kind must be a keyword".into()) },
            "as" => match value { Form::Kw(v) => to = Some(v.clone()), _ => return Err("render-adapt: :as must be a keyword".into()) },
            "fields" => {
                let Form::Map(map) = value else { return Err("render-adapt: :fields must be a map".into()) };
                fields = Some(map.iter().map(|(a, b)| match (a, b) {
                    (Form::Kw(a), Form::Kw(b)) => Ok((a.clone(), b.clone())),
                    _ => Err("render-adapt: field mappings must be keywords".to_string()),
                }).collect::<Result<Vec<_>, _>>()?);
            }
            _ => return Err(format!("render-adapt: unknown option :{key}")),
        }
    }
    Ok(RenderAdapter {
        from: from.ok_or("render-adapt: missing :kind")?,
        to: to.ok_or("render-adapt: missing :as")?,
        fields: fields.ok_or("render-adapt: missing :fields")?,
    })
}

fn expand_render_adapts(form: &Form) -> Result<Form, String> {
    let Form::List(items) = form else { return Ok(form.clone()) };
    if matches!(items.first(), Some(Form::Sym(h)) if h.as_ref() == "render-adapt") {
        let adapter = parse_render_adapter(items)?;
        let mut seq = vec![Form::sym("seq")];
        for body in items.iter().skip(2) { seq.push(rewrite_render_form(body, &adapter)); }
        return Ok(Form::list(seq));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() { out.push(expand_render_adapts(item)?); }
    Ok(Form::list(out))
}

fn rewrite_render_form(form: &Form, adapter: &RenderAdapter) -> Form {
    match form {
        Form::List(items) => {
            let map_idx = if matches!(items.first(), Some(Form::Sym(h)) if h.as_ref() == "render") { Some(1) }
                else if matches!(items.first(), Some(Form::Sym(h)) if h.as_ref() == "emit")
                    && matches!(items.get(1), Some(Form::Kw(c)) if c.as_ref() == "render") { Some(2) }
                else { None };
            let mut out: Vec<Form> = items.iter().map(|f| rewrite_render_form(f, adapter)).collect();
            if let Some(i) = map_idx {
                if let Some(Form::Map(fields)) = items.get(i) {
                    let source = fields.iter().find_map(|(k, v)| match (k, v) {
                        (Form::Kw(k), Form::Kw(v)) if k.as_ref() == "kind" => Some(v.as_ref()),
                        _ => None,
                    }).unwrap_or("default");
                    if source == adapter.from.as_ref() {
                        let structural = |k: &str| matches!(k, "shape" | "kind" | "x" | "y" | "theta" | "facing" | "scale" | "alpha" | "opacity" | "hue" | "points" | "pts" | "active");
                        let mut picked = Vec::new();
                        for (key, value) in fields.iter() {
                            let Form::Kw(key) = key else { continue };
                            if key.as_ref() == "kind" {
                                picked.push((Form::Kw(key.clone()), Form::Kw(adapter.to.clone())));
                            } else if structural(key) {
                                picked.push((Form::Kw(key.clone()), rewrite_render_form(value, adapter)));
                            } else if let Some((_, target)) = adapter.fields.iter().find(|(from, _)| from == key) {
                                picked.push((Form::Kw(target.clone()), rewrite_render_form(value, adapter)));
                            }
                        }
                        if !fields.iter().any(|(k, _)| matches!(k, Form::Kw(k) if k.as_ref() == "kind")) {
                            picked.push((Form::kw("kind"), Form::Kw(adapter.to.clone())));
                        }
                        out[i] = Form::Map(picked.into());
                    }
                }
            }
            Form::list(out)
        }
        _ => form.clone(),
    }
}
