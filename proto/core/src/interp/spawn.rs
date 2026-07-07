//! The spawn path: meta resolution, element flattening, formations.

use super::*;
use crate::edn::Form;
use std::rc::Rc;


/// Meta keys whose values are signals sampled later (§7): never evaluated at
/// spawn time (they reference slot-bound t).
/// Meta tags whose values are NOT evaluated at spawn: signal-valued tags
/// (contain t), and :expose (whose $names are channel DESIGNATORS, not
/// reads — evaluated, $some-channel would resolve as a channel read).
const SIGNAL_TAGS: &[&str] = &[
    "hue",
    "scale",
    "facing",
    "opacity",
    "expose",
];

fn spec_args(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<DynLike, String> {
    match items.len() {
        0 => Ok(empty_spec_list()),
        1 => {
            let one = eval_dynlike_form(&items[0], env, ctx, world)?;
            match one {
                DynLike::Map(_) => Ok(DynLike::List(vec![one].into())),
                other => Ok(other),
            }
        }
        _ => items
            .iter()
            .map(|i| eval_dynlike_form(i, env, ctx, world))
            .collect::<Result<Vec<_>, _>>()
            .map(|items| DynLike::List(items.into())),
    }
}

pub(crate) fn sf_colliders(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let specs = spec_args(&items[1..], env, ctx, world)?;
    as_collider_spec_list(&specs, &mut world.symbols)?;
    Ok(Val::ColliderSpecs(Rc::new(specs)))
}

pub(crate) fn sf_renderers(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let specs = spec_args(&items[1..], env, ctx, world)?;
    as_render_spec_list(&specs)?;
    Ok(Val::RenderSpecs(Rc::new(specs)))
}

pub(crate) fn parse_expose(metas: &[Form]) -> Rc<[(Rc<str>, Rc<str>)]> {
    let mut out = Vec::new();
    // several meta maps merge per-key, later wins: the LAST map carrying
    // :expose supplies the whole rule set
    for meta in metas.iter().rev() {
        let Form::Map(kvs) = meta else { continue };
        for (k, v) in kvs.iter() {
            if matches!(k, Form::Kw(kw) if &**kw == "expose") {
                if let Form::Map(pairs) = v {
                    for (chan, col) in pairs.iter() {
                        let chan: Option<Rc<str>> = match chan {
                            Form::Sym(s) if s.starts_with('$') => Some(s[1..].into()),
                            _ => None,
                        };
                        let Form::Kw(col) = col else { continue };
                        if let Some(chan) = chan {
                            out.push((col.as_ref().into(), chan));
                        }
                    }
                }
                return out.into();
            }
        }
    }
    out.into()
}

pub(crate) fn sf_spawn(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let dv = evaluate(&items[1], env, ctx, world)?;
    // meta: any number of maps, merged per-key with LATER maps winning —
    // what lets a library template prepend its defaults and pass the
    // caller's map through: (spawn d {defaults…} user-meta). map_get is
    // first-match, so pairs collect in reverse map order.
    let mut metas = Vec::new();
    let mut computed_meta_pairs: Vec<(Val, Val)> = Vec::new();
    let mut explicit_colliders = Vec::new();
    let mut explicit_renderers = Vec::new();
    for item in &items[2..] {
        if matches!(item, Form::Map(_)) {
            metas.push(item.clone());
            continue;
        }
        match evaluate(item, env, ctx, world)? {
            Val::ColliderSpecs(specs) => explicit_colliders.push((*specs).clone()),
            Val::RenderSpecs(specs) => explicit_renderers.push((*specs).clone()),
            Val::Map(kvs) => computed_meta_pairs.extend(kvs.iter().cloned()),
            Val::DynLike(d) => computed_meta_pairs.extend(dynlike_meta_pairs(&d)?),
            _ => {}
        }
    }
    let mut pairs: Vec<(Val, Val)> = Vec::new();
    for mf in metas.iter().rev() {
        match mf {
            Form::Map(kvs) => {
                for (k, v) in kvs.iter() {
                    let kv = evaluate(k, env, ctx, world)?;
                    let skip = matches!(&kv, Val::Kw(kw) if SIGNAL_TAGS.contains(&kw.as_ref()));
                    let vv = if skip { Val::Nothing } else { evaluate(v, env, ctx, world)? };
                    pairs.push((kv, vv));
                }
            }
            m => {
                // a computed meta (variable, call): evaluated pairs only —
                // signal tags need a literal map to stay unevaluated
                match evaluate(m, env, ctx, world)? {
                    Val::Map(kvs) => pairs.extend(kvs.iter().cloned()),
                    Val::DynLike(d) => pairs.extend(dynlike_meta_pairs(&d)?),
                    _ => {}
                }
            }
        }
    }
    pairs.extend(computed_meta_pairs);
    let meta = Val::Map(Rc::new(pairs));
    let mut elems = Vec::new();
    flatten_elems(dv, &mut Vec::new(), &mut elems)?;
    // rand in signal expressions is an ir constant per element (§5): clone the
    // motion tree per element, substituting rand calls with drawn constants
    for e in elems.iter_mut() {
        if dyn_figure_has_rand(&e.dyn_figure) {
            e.dyn_figure = instantiate_rand_geometry(&e.dyn_figure, world);
        }
    }
    let styles = resolve_styles(&meta, &elems)?;
    let sigs = resolve_sigs(&metas, env, elems.len());
    let team: Option<Rc<str>> = match map_get(&meta, "team") {
        Some(Val::Kw(k)) => Some(Rc::from(&*k)),
        _ => None,
    };
    // columns: :hp n is sugar for a col (the contact layer reads the hp
    // column by name; what zero hp MEANS is a trigger's business, and the
    // default death trigger is library code, not engine). :cols {:armor 2
    // ...} adds more; with several meta maps every :cols map contributes,
    // later maps' columns shadowing earlier ones (columns are independent
    // facts — they deep-merge where scalar keys replace).
    // Column values may be ARRAYS, binding per spawn element exactly like
    // style axes (leading-axis / by-length / nested-structural) — per-entity
    // saved data: :cols {:ci (iota 8)} gives bullet k the column ci = k.
    let mut cols: Vec<(Rc<str>, Val)> = Vec::new();
    if let Some(v @ (Val::Num(_) | Val::Arr(_))) = map_get(&meta, "hp") {
        cols.push(("hp".into(), v));
    }
    if let Val::Map(kvs) = &meta {
        for (k, v) in kvs.iter() {
            let is_cols = matches!(k, Val::Kw(kw) if &**kw == "cols");
            if !is_cols {
                continue;
            }
            if let Val::Map(cs) = v {
                for (k, v) in cs.iter() {
                    if let Val::Kw(k) = k {
                        cols.push((k.as_ref().into(), v.clone()));
                    }
                }
            }
        }
    }
    // triggers: data only — no synthesized rules (spawn-enemy in the lib
    // is where "hp ≤ 0 means death" lives)
    let triggers: Rc<[TriggerRule]> = match map_get(&meta, "triggers") {
        Some(Val::Arr(items)) => {
            let mut rules = Vec::new();
            for it in items.iter() {
                let Val::Map(kvs) = it else {
                    return Err("triggers: expected maps".into());
                };
                let get = |name: &str| {
                    kvs.iter().find_map(|(k, v)| match k {
                        Val::Kw(kw) if &**kw == name => Some(v.clone()),
                        _ => None,
                    })
                };
                let col = match get("col") {
                    Some(Val::Kw(k)) => k.to_string(),
                    _ => return Err("triggers: missing :col".into()),
                };
                let leq = match get("leq") {
                    Some(Val::Num(n)) => n,
                    _ => return Err("triggers: missing :leq".into()),
                };
                let event = match get("event") {
                    Some(Val::Kw(k)) => k,
                    _ => return Err("triggers: missing :event".into()),
                };
                let cull = matches!(get("cull"), Some(Val::Num(n)) if n != 0.0);
                let event_sym = world.symbols.intern(event.as_ref());
                rules.push(TriggerRule::new(event_sym, event.as_ref(), &col, leq, cull));
            }
            rules.into()
        }
        _ => Vec::new().into(),
    };
    // :damage n | {:hit n ...} (DMK player() map) lowers to an ordinary
    // numeric column read by Touhou contact rules.
    if let Some(v) = map_get(&meta, "damage") {
        match v {
            Val::Num(_) | Val::Arr(_) => cols.push(("damage".into(), v)),
            Val::Map(kvs) => {
                if let Some(hit) = kvs.iter().find_map(|(k, v)| match (k, v) {
                    (Val::Kw(kw), Val::Num(_)) if &**kw == "hit" => Some(v.clone()),
                    _ => None,
                }) {
                    cols.push(("damage".into(), hit));
                }
            }
            _ => {}
        }
    }
    let hitbox = match map_get(&meta, "hitbox") {
        Some(Val::Num(n)) => Some(n),
        _ => None,
    };
    // :expose {$some-hp :hp}: publish this entity's column as a derived
    // channel — the declarative form of "sim-computed world fact" (§3).
    // Parsed from the RAW form ($names here are channel designators, like
    // the keys of (with {$rank 0.5} …) — not reads).
    let expose: Rc<[(Rc<str>, Rc<str>)]> = parse_expose(&metas);
    // Collider/render sets are explicit spawn arguments. No genre defaults —
    // an entity with no colliders is inert to the contact pass (scenery);
    // what a "bullet" or "enemy" carries is the library's business
    // (spawn-bullet/spawn-enemy in lib/touhou.maku).
    // :hitbox r resizes the PRIMARY (first) collider — the generic knob
    // that lets a template's default collider set fit a bigger sprite.
    if explicit_colliders.is_empty() {
        explicit_colliders.push(empty_spec_list());
    }
    if explicit_renderers.is_empty() {
        explicit_renderers.push(empty_spec_list());
    }
    // per-element column resolution: same axis rules as styles
    let cols: Vec<Vec<(Rc<str>, f64)>> = elems
        .iter()
        .enumerate()
        .map(|(i, e)| {
            cols.iter().map(|(k, v)| (k.clone(), axis_num(v, e, i))).collect()
        })
        .collect();
    let shared_collider_specs: Rc<[ColliderSpecList]> = explicit_colliders.into();
    let shared_render_specs: Rc<[RenderSpecList]> = explicit_renderers.into();
    let entities = elems
        .into_iter()
        .zip(styles)
        .zip(sigs)
        .zip(cols)
        .map(|(((e, style), sigs), cols)| {
            let mut collider_specs = shared_collider_specs.iter().cloned().collect::<Vec<_>>();
            collider_specs.push(e.collider_specs);
            if let Some(radius) = hitbox {
                apply_primary_hitbox(&mut collider_specs, radius);
            }
            let mut render_specs = shared_render_specs.iter().cloned().collect::<Vec<_>>();
            render_specs.push(e.render_specs);
            EntitySpec {
                dyn_figure: e.dyn_figure,
                cache_policy: e.cache_policy,
                style,
                sigs,
                team: team.clone(),
                cols,
                triggers: triggers.clone(),
                collider_projector: collider_specs.into(),
                render_projector: render_specs.into(),
                expose: expose.clone(),
            }
        })
        .collect();
    Ok(Val::Action(Rc::new(ActionV::Spawn { entities })))
}

fn dynlike_num_from_dyn(d: &DynNum) -> DynLike {
    match d.repr() {
        NumDynRepr::Const(n) => DynLike::Atom(DataAtom::Num(*n)),
        NumDynRepr::Expr { form, env } => {
            DynLike::Dyn(DynVal::Expr { form: form.clone(), env: env.clone() })
        }
    }
}

fn dynlike_kw_atom(name: &str) -> DynLike {
    DynLike::Atom(DataAtom::Kw(name.into()))
}

fn dynlike_num(n: f64) -> DynLike {
    DynLike::Atom(DataAtom::Num(n))
}

fn dynlike_map(pairs: Vec<(&str, DynLike)>) -> DynLike {
    DynLike::Map(Rc::new(
        pairs
            .into_iter()
            .map(|(k, v)| (DataAtom::Kw(k.into()), v))
            .collect(),
    ))
}

fn sample_set_pairs(sample_set: &SampleSet) -> Vec<(&'static str, DynLike)> {
    match sample_set {
        SampleSet::Values(vals) => vec![(
            "samples",
            DynLike::List(vals.iter().copied().map(dynlike_num).collect::<Vec<_>>().into()),
        )],
        SampleSet::Step { resolution } => vec![("resolution", dynlike_num(*resolution))],
    }
}

fn curve_projection_spec(
    sample_set: &SampleSet,
    u_max_sig: Option<&DynNum>,
    width: f64,
    warn: f64,
    active: f64,
    fill_sig: Option<&DynNum>,
) -> DynLike {
    let mut opts = sample_set_pairs(sample_set);
    if let Some(u_max) = u_max_sig {
        opts.push(("u-max", dynlike_num_from_dyn(u_max)));
    }
    opts.push(("width", dynlike_num(width)));
    opts.push(("warn", dynlike_num(warn)));
    opts.push(("active", dynlike_num(active)));
    if let Some(fill) = fill_sig {
        opts.push(("fill", dynlike_num_from_dyn(fill)));
    }
    dynlike_map(vec![(
        "shape",
        DynLike::List(vec![dynlike_kw_atom("polyline"), dynlike_map(opts)].into()),
    )])
}

fn data_kw_is(k: &DataAtom, name: &str) -> bool {
    matches!(k, DataAtom::Kw(kw) if &**kw == name)
}

fn dynlike_map_set(m: &DynLike, key: &str, val: DynLike) -> DynLike {
    let DynLike::Map(kvs) = m else {
        return m.clone();
    };
    let mut out = Vec::with_capacity(kvs.len() + 1);
    let mut replaced = false;
    for (k, v) in kvs.iter() {
        if !replaced && data_kw_is(k, key) {
            out.push((k.clone(), val.clone()));
            replaced = true;
        } else {
            out.push((k.clone(), v.clone()));
        }
    }
    if !replaced {
        out.push((DataAtom::Kw(key.into()), val));
    }
    DynLike::Map(Rc::new(out))
}

fn replace_circle_spec_radius(spec: &DynLike, radius: f64) -> Option<DynLike> {
    if !matches!(spec, DynLike::Map(_)) {
        return None;
    }
    let radius = dynlike_num(radius);
    if let Some(shape_v) = dynlike_map_get(spec, "shape") {
        let Ok((shape, opts)) = as_shape_spec(&shape_v) else {
            return None;
        };
        if shape.as_ref() != "circle" {
            return None;
        }
        let opts = dynlike_map_set(&opts, "r", radius);
        let shape = DynLike::List(vec![dynlike_kw_atom("circle"), opts].into());
        return Some(dynlike_map_set(spec, "shape", shape));
    }
    if dynlike_map_get(spec, "r").is_some() {
        return Some(dynlike_map_set(spec, "r", radius));
    }
    None
}

fn replace_primary_hitbox_spec(list: &DynLike, radius: f64) -> Option<DynLike> {
    let DynLike::List(items) = list else {
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    let mut replaced = false;
    for item in items.iter() {
        if !replaced {
            if let Some(next) = replace_circle_spec_radius(item, radius) {
                out.push(next);
                replaced = true;
                continue;
            }
        }
        out.push(item.clone());
    }
    replaced.then(|| DynLike::List(out.into()))
}

fn apply_primary_hitbox(lists: &mut [ColliderSpecList], radius: f64) {
    for list in lists.iter_mut() {
        if let Some(next) = replace_primary_hitbox_spec(list, radius) {
            *list = next;
            break;
        }
    }
}

pub(crate) fn flatten_elems(
    v: Val,
    path: &mut Vec<(usize, usize)>,
    out: &mut Vec<SpawnElem>,
) -> Result<(), String> {
    match v {
        Val::Arr(items) => {
            let len = items.len();
            for (i, item) in items.iter().enumerate() {
                path.push((len, i));
                flatten_elems(item.clone(), path, out)?;
                path.pop();
            }
            Ok(())
        }
        Val::CurveV(l) => {
            let (dyn_figure, colliders, renderers, cache_policy) = match &l.backing {
                CurveBacking::Parametric {
                    curve,
                    sample_set,
                    u_max_sig,
                    warn,
                    active,
                    width,
                    fill_sig,
                } => {
                    let projection = curve_projection_spec(
                        sample_set,
                        u_max_sig.as_ref(),
                        *width,
                        *warn,
                        *active,
                        fill_sig.as_ref(),
                    );
                    (
                        DynFigure::figure_curve(l.anchor.clone(), curve.clone()),
                        empty_spec_list(),
                        DynLike::List(vec![projection].into()),
                        EntityCachePolicy::default(),
                    )
                }
                CurveBacking::Trace { window } => (
                    DynFigure::pose(l.anchor.clone()),
                    empty_spec_list(),
                    empty_spec_list(),
                    EntityCachePolicy {
                        trace: Some(TracePolicy { window: Some(*window) }),
                    },
                ),
            };
            out.push(SpawnElem {
                dyn_figure,
                collider_specs: colliders,
                render_specs: renderers,
                cache_policy,
                path: path.clone(),
            });
            Ok(())
        }
        other => {
            out.push(SpawnElem {
                dyn_figure: DynFigure::pose(as_dyn(other)?),
                collider_specs: empty_spec_list(),
                render_specs: empty_spec_list(),
                cache_policy: EntityCachePolicy::default(),
                path: path.clone(),
            });
            Ok(())
        }
    }
}

pub(crate) fn form_has_rand(f: &Form) -> bool {
    match f {
        Form::List(items) => {
            matches!(items.first(), Some(Form::Sym(s)) if matches!(s.as_ref(), "rand" | "rand-int" | "randpm1"))
                || items.iter().any(form_has_rand)
        }
        Form::Vector(items) => items.iter().any(form_has_rand),
        _ => false,
    }
}

pub(crate) fn dyn_has_rand(d: &DynNode) -> bool {
    match d {
        DynNode::ClosedPt { a, b, .. } | DynNode::Vel { a, b, .. } => {
            form_has_rand(a) || form_has_rand(b)
        }
        DynNode::RotExpr { form, .. } => form_has_rand(form),
        DynNode::Translate { child, .. } => dyn_has_rand(child),
        DynNode::Frame(a, b) => dyn_has_rand(a) || dyn_has_rand(b),
        _ => false,
    }
}

pub(crate) fn dyn_figure_has_rand(d: &DynFigure) -> bool {
    match d.repr() {
        FigureDynRepr::Pose(p) => dyn_has_rand(p.node()),
        FigureDynRepr::Curve { frame, curve } => {
            dyn_has_rand(frame.node())
                || matches!(&curve.eval, CurveEval::Expr(shape) if dyn_has_rand(shape.node()))
        }
    }
}

pub(crate) fn subst_rand(f: &Form, world: &mut World) -> Form {
    match f {
        Form::List(items) => {
            if let Some(Form::Sym(s)) = items.first() {
                match s.as_ref() {
                    "rand" | "rand-int" => {
                        let a = matches!(&items[1], Form::Num(_))
                            .then(|| if let Form::Num(n) = items[1] { n } else { 0.0 })
                            .unwrap_or(0.0);
                        let b = matches!(&items[2], Form::Num(_))
                            .then(|| if let Form::Num(n) = items[2] { n } else { 1.0 })
                            .unwrap_or(1.0);
                        let v = a + world.next_rand() * (b - a);
                        return Form::Num(if s.as_ref() == "rand-int" { v.floor() } else { v });
                    }
                    "randpm1" => {
                        return Form::Num(if world.next_rand() < 0.5 { -1.0 } else { 1.0 });
                    }
                    _ => {}
                }
            }
            Form::List(items.iter().map(|i| subst_rand(i, world)).collect::<Vec<_>>().into())
        }
        Form::Vector(items) => {
            Form::Vector(items.iter().map(|i| subst_rand(i, world)).collect::<Vec<_>>().into())
        }
        other => other.clone(),
    }
}

pub(crate) fn instantiate_rand(d: &Rc<DynNode>, world: &mut World) -> Rc<DynNode> {
    match &**d {
        DynNode::ClosedPt { a, b, polar, env } => Rc::new(DynNode::ClosedPt {
            a: subst_rand(a, world),
            b: subst_rand(b, world),
            polar: *polar,
            env: env.clone(),
        }),
        DynNode::Vel { a, b, polar, env } => Rc::new(DynNode::Vel {
            a: subst_rand(a, world),
            b: subst_rand(b, world),
            polar: *polar,
            env: env.clone(),
        }),
        DynNode::RotExpr { form, env } => Rc::new(DynNode::RotExpr {
            form: subst_rand(form, world),
            env: env.clone(),
        }),
        DynNode::Translate { dx, dy, child } => Rc::new(DynNode::Translate {
            dx: *dx,
            dy: *dy,
            child: instantiate_rand(child, world),
        }),
        DynNode::Frame(a, b) => Rc::new(DynNode::Frame(
            instantiate_rand(a, world),
            instantiate_rand(b, world),
        )),
        _ => d.clone(),
    }
}

pub(crate) fn instantiate_rand_geometry(d: &DynFigure, world: &mut World) -> DynFigure {
    match d.repr() {
        FigureDynRepr::Pose(p) => DynFigure::pose_node(instantiate_rand(p.node(), world)),
        FigureDynRepr::Curve { frame, curve } => {
            let eval = match &curve.eval {
                CurveEval::Straight => CurveEval::Straight,
                CurveEval::Expr(shape) => CurveEval::Expr(DynPose::pose_node(instantiate_rand(shape.node(), world))),
            };
            DynFigure::figure_curve(
                DynPose::pose_node(instantiate_rand(frame.node(), world)),
                ParametricCurve {
                    eval,
                    domain: curve.domain.clone(),
                },
            )
        }
    }
}

pub(crate) fn map_get(m: &Val, key: &str) -> Option<Val> {
    if let Val::Map(kvs) = m {
        for (k, v) in kvs.iter() {
            if let Val::Kw(kw) = k {
                if &**kw == key {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}

fn dynlike_arr(v: &DynLike) -> Option<Vec<DynLike>> {
    match v {
        DynLike::List(items) => Some(items.iter().cloned().collect()),
        _ => None,
    }
}

fn as_dynlike_list(v: &DynLike, what: &str) -> Result<Vec<DynLike>, String> {
    dynlike_arr(v).ok_or_else(|| format!("{}: expected array", what))
}

fn dynlike_to_val(v: &DynLike) -> Result<Val, String> {
    if v.is_dynamic() {
        Ok(Val::DynLike(Rc::new(v.clone())))
    } else {
        v.eval(0.0, &MotionState::new(), &SigEnv::default())
    }
}

fn dynlike_meta_pairs(v: &DynLike) -> Result<Vec<(Val, Val)>, String> {
    match v {
        DynLike::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((k.to_val(), dynlike_to_val(v)?)))
            .collect(),
        _ => Err("spawn meta: expected map".into()),
    }
}

fn dynlike_map_get(m: &DynLike, key: &str) -> Option<DynLike> {
    match m {
        DynLike::Map(kvs) => {
            for (k, v) in kvs.iter() {
                if matches!(k, DataAtom::Kw(kw) if &**kw == key) {
                    return Some(v.clone());
                }
            }
            None
        }
        _ => None,
    }
}

fn dynlike_kw(v: &DynLike) -> Option<Rc<str>> {
    match v {
        DynLike::Atom(DataAtom::Kw(k)) => Some(k.clone()),
        _ => None,
    }
}

fn as_dyn_num(v: &DynLike) -> Result<DynNum, String> {
    match v {
        DynLike::Dyn(DynVal::Expr { form, env }) => {
            Ok(DynNum::num_expr(form.clone(), env.clone()))
        }
        DynLike::Atom(DataAtom::Num(n)) => Ok(DynNum::num(*n)),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

fn as_static_num(v: &DynLike) -> Result<f64, String> {
    match v {
        DynLike::Atom(DataAtom::Num(n)) => Ok(*n),
        DynLike::Dyn(_) => Err("expected static number".into()),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

fn dynlike_map_as_static_num(m: &DynLike, key: &str, default: f64) -> Result<f64, String> {
    match dynlike_map_get(m, key) {
        Some(v) => as_static_num(&v),
        None => Ok(default),
    }
}

fn dynlike_map_as_dyn_num_any(m: &DynLike, keys: &[&str], default: f64) -> Result<DynNum, String> {
    for key in keys {
        if let Some(v) = dynlike_map_get(m, key) {
            return as_dyn_num(&v);
        }
    }
    Ok(DynNum::num(default))
}

fn as_sample_set(opts: &DynLike) -> Result<SampleSet, String> {
    if let Some(vals) = dynlike_map_get(opts, "samples").and_then(|v| dynlike_arr(&v)) {
        let mut out = Vec::with_capacity(vals.len());
        for v in vals.iter() {
            out.push(as_static_num(v)?);
        }
        return Ok(SampleSet::Values(out.into()));
    }
    Ok(SampleSet::Step {
        resolution: dynlike_map_as_static_num(opts, "resolution", 0.1)?,
    })
}

fn as_slot_activity(opts: &DynLike) -> Result<SlotActivity, String> {
    Ok(SlotActivity {
        warn: dynlike_map_as_static_num(opts, "warn", 0.0)?,
        active: dynlike_map_as_static_num(opts, "active", f64::INFINITY)?,
        hot_frac_sig: dynlike_map_get(opts, "fill")
            .map(|v| as_dyn_num(&v))
            .transpose()?,
    })
}

fn as_capsule_chain_slot(opts: &DynLike) -> Result<CapsuleChainSlot, String> {
    Ok(CapsuleChainSlot {
        sample_set: as_sample_set(opts)?,
        u_max_sig: dynlike_map_get(opts, "u-max")
            .map(|v| as_dyn_num(&v))
            .transpose()?,
        width: dynlike_map_as_static_num(opts, "width", 1.0)?,
        activity: as_slot_activity(opts)?,
    })
}

fn as_shape_spec(v: &DynLike) -> Result<(Rc<str>, DynLike), String> {
    match v {
        DynLike::List(items) if items.len() == 2 => {
            let Some(shape) = dynlike_kw(&items[0]) else {
                return Err("shape: expected keyword shape name".into());
            };
            Ok((shape, items[1].clone()))
        }
        DynLike::Atom(DataAtom::Kw(shape)) => {
            Ok((shape.clone(), DynLike::Map(Rc::new(Vec::new()))))
        }
        _ => Err("shape: expected [:shape opts]".into()),
    }
}

pub(crate) fn as_collider(v: &DynLike, symbols: &mut SymbolTable) -> Result<DynCollider, String> {
    if !matches!(v, DynLike::Map(_)) {
        return Err("colliders: expected maps".into());
    }
    let layer = match dynlike_map_get(v, "layer").and_then(|v| dynlike_kw(&v)) {
        Some(k) => symbols.intern(k.as_ref()),
        _ => return Err("colliders: missing :layer".into()),
    };
    if let Some(shape_v) = dynlike_map_get(v, "shape") {
        let (shape, opts) = as_shape_spec(&shape_v)?;
        return match shape.as_ref() {
            "circle" => Ok(DynCollider::collider_circle(
                layer,
                dynlike_map_as_dyn_num_any(&opts, &["radius", "r"], 0.08)?,
            )),
            "capsule-chain" => Ok(DynCollider::collider_capsule_chain(
                layer,
                dynlike_map_as_dyn_num_any(&opts, &["radius", "r"], 0.08)?,
                as_capsule_chain_slot(&opts)?,
            )),
            _ => Err(format!("colliders: unknown shape :{}", shape)),
        };
    }
    match dynlike_map_get(v, "r") {
        Some(r) => Ok(DynCollider::collider_circle(layer, as_dyn_num(&r)?)),
        _ => Err("colliders: missing :r or :shape".into()),
    }
}

pub(crate) fn as_stable_collider_slots(v: &DynLike, symbols: &mut SymbolTable) -> Result<Vec<DynCollider>, String> {
    as_dynlike_list(v, "colliders")?
        .iter()
        .map(|v| as_collider(v, symbols))
        .collect::<Result<Vec<_>, _>>()
}

pub(crate) fn empty_spec_list() -> DynLike {
    DynLike::List(Vec::new().into())
}

fn as_collider_spec_list(v: &DynLike, symbols: &mut SymbolTable) -> Result<ColliderSpecList, String> {
    if !v.is_dynamic() {
        as_stable_collider_slots(v, symbols)?;
    }
    Ok(v.clone())
}

pub(crate) fn as_render(v: &DynLike) -> Result<DynRender, String> {
    if !matches!(v, DynLike::Map(_)) {
        return Err("renderers: expected maps".into());
    }
    let shape_v = dynlike_map_get(v, "shape")
        .unwrap_or_else(|| DynLike::Atom(DataAtom::Kw("polyline".into())));
    let (shape, opts) = as_shape_spec(&shape_v)?;
    match shape.as_ref() {
        "polyline" => Ok(DynRender::render_polyline(CurveRenderSlot {
            sample_set: as_sample_set(&opts)?,
            u_max_sig: dynlike_map_get(&opts, "u-max")
                .map(|v| as_dyn_num(&v))
                .transpose()?,
            width: dynlike_map_as_static_num(&opts, "width", 1.0)?,
            activity: as_slot_activity(&opts)?,
        })),
        _ => Err(format!("renderers: unknown shape :{}", shape)),
    }
}

pub(crate) fn as_stable_render_slots(v: &DynLike) -> Result<Vec<DynRender>, String> {
    as_dynlike_list(v, "renderers")?
        .iter()
        .map(as_render)
        .collect::<Result<Vec<_>, _>>()
}

fn as_render_spec_list(v: &DynLike) -> Result<RenderSpecList, String> {
    if !v.is_dynamic() {
        as_stable_render_slots(v)?;
    }
    Ok(v.clone())
}

pub(crate) fn kw_str(v: &Val) -> String {
    match v {
        Val::Kw(k) => k.to_string(),
        _ => String::new(),
    }
}

/// §5/F15: a meta axis array binds to the first array level (root to leaf)
/// whose length matches; otherwise it cycles on the flat index.
/// Numeric per-element resolution: same axis rules as style values
/// (nested-structural, else by-length, else leading-cycle), for columns.
pub(crate) fn axis_num(v: &Val, elem: &SpawnElem, flat: usize) -> f64 {
    match v {
        Val::Arr(items) if items.iter().any(|x| matches!(x, Val::Arr(_))) => {
            let mut cur = v.clone();
            let mut depth = 0;
            loop {
                match cur {
                    Val::Arr(xs) if !xs.is_empty() => {
                        let idx = elem.path.get(depth).map(|(_, i)| *i).unwrap_or(flat);
                        cur = xs[idx % xs.len()].clone();
                        depth += 1;
                    }
                    other => return other.num().unwrap_or(0.0),
                }
            }
        }
        Val::Arr(items) if !items.is_empty() => {
            let len = items.len();
            for (axis_len, idx) in &elem.path {
                if *axis_len == len {
                    return items[idx % len].num().unwrap_or(0.0);
                }
            }
            items[flat % len].num().unwrap_or(0.0)
        }
        v => v.num().unwrap_or(0.0),
    }
}

pub(crate) fn axis_value(v: &Val, elem: &SpawnElem, flat: usize) -> String {
    match v {
        // NESTED arrays resolve STRUCTURALLY: depth in the meta value =
        // axis along the element's root-to-leaf path, cycling at every
        // level; a scalar reached early broadcasts to all deeper axes.
        // [[:red :blue] :green :purple] over 10×3 → group 0 cycles
        // red/blue inside, group 1 all green, group 2 all purple, group 3
        // wraps to [red blue]… Shape disambiguates where length cannot.
        Val::Arr(items) if items.iter().any(|x| matches!(x, Val::Arr(_))) => {
            let mut cur = v.clone();
            let mut depth = 0;
            loop {
                match cur {
                    Val::Arr(xs) if !xs.is_empty() => {
                        let idx = elem.path.get(depth).map(|(_, i)| *i).unwrap_or(flat);
                        cur = xs[idx % xs.len()].clone();
                        depth += 1;
                    }
                    other => return kw_str(&other),
                }
            }
        }
        // flat arrays: F15 by-length targeting, leading-first
        Val::Arr(items) if !items.is_empty() => {
            let len = items.len();
            for (axis_len, idx) in &elem.path {
                if *axis_len == len {
                    return kw_str(&items[idx % len]);
                }
            }
            kw_str(&items[flat % len])
        }
        v => kw_str(v),
    }
}

pub(crate) fn resolve_styles(meta: &Val, elems: &[SpawnElem]) -> Result<Vec<Style>, String> {
    let style = map_get(meta, "style").unwrap_or(Val::Map(Rc::new(vec![])));
    Ok(elems
        .iter()
        .enumerate()
        .map(|(k, e)| Style {
            family: map_get(&style, "family").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
            color: map_get(&style, "color").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
            variant: map_get(&style, "variant").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
        })
        .collect())
}

/// Signal-valued meta (§7): keep the FORM and sample at render time.
/// One tag's per-element signals — every element shares the form; array
/// values resolve per element via the carried idx. With several meta
/// maps, the last one carrying the tag wins (per-key merge).
fn resolve_tag(metas: &[Form], env: &Env, n: usize, tag: &str) -> Vec<Option<MetaSig>> {
    for meta_form in metas.iter().rev() {
        let Form::Map(kvs) = meta_form else { continue };
        for (k, v) in kvs.iter() {
            if let Form::Kw(kw) = k {
                if &**kw == tag {
                    return (0..n)
                        .map(|idx| Some(MetaSig { form: v.clone(), env: env.clone(), idx }))
                        .collect();
                }
            }
        }
    }
    vec![None; n]
}

/// All render-affecting signal tags (:hue :scale :facing :opacity), zipped
/// into one RenderSigs per element.
pub(crate) fn resolve_sigs(metas: &[Form], env: &Env, n: usize) -> Vec<RenderSigs> {
    let hue = resolve_tag(metas, env, n, "hue");
    let scale = resolve_tag(metas, env, n, "scale");
    let facing = resolve_tag(metas, env, n, "facing");
    let opacity = resolve_tag(metas, env, n, "opacity");
    hue.into_iter()
        .zip(scale)
        .zip(facing)
        .zip(opacity)
        .map(|(((hue, scale), facing), opacity)| RenderSigs { hue, scale, facing, opacity })
        .collect()
}

pub(crate) fn formation(
    poses: Vec<Pose>,
    child: Option<&Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let arr = Val::arr(poses.into_iter().map(Val::Pose).collect());
    match child {
        None => Ok(arr),
        Some(cf) => {
            let child = evaluate(cf, env, ctx, world)?;
            apply_frame_arr(&arr, child)
        }
    }
}

pub(crate) fn sf_circle(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as usize;
    if n == 0 {
        return Err("circle: zero elements".into());
    }
    let poses = (0..n)
        .map(|k| Pose::oriented(0.0, 0.0, k as f64 * 360.0 / n as f64))
        .collect();
    formation(poses, items.get(2), env, ctx, world)
}

pub(crate) fn sf_arrow(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let back = evaluate(&items[2], env, ctx, world)?.num()?;
    let side = evaluate(&items[3], env, ctx, world)?.num()?;
    let half = (n - 1) / 2;
    let poses = (-half..=(n - 1 - half))
        .map(|j| Pose::point(-back * (j.abs() as f64), side * j as f64))
        .collect();
    formation(poses, items.get(4), env, ctx, world)
}

pub(crate) fn sf_fan(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let step = evaluate(&items[2], env, ctx, world)?.num()?;
    let mid = (n - 1) as f64 / 2.0;
    let poses = (0..n)
        .map(|k| Pose::oriented(0.0, 0.0, (k as f64 - mid) * step))
        .collect();
    formation(poses, items.get(3), env, ctx, world)
}
