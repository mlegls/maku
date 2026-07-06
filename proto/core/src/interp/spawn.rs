//! The spawn path: meta resolution, element flattening, formations.

use super::*;
use crate::edn::Form;
use std::rc::Rc;


/// Meta keys whose values are signals sampled later (§7): never evaluated at
/// spawn time (they reference slot-bound t).
/// Meta tags whose values are NOT evaluated at spawn: signal-valued tags
/// (contain t), and :expose (whose $names are channel DESIGNATORS, not
/// reads — evaluated, $some-channel would resolve as a channel read).
const SIGNAL_TAGS: &[&str] = &["hue", "scale", "facing", "opacity", "expose"];

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
    let metas = &items[2..];
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
                    Val::DynStruct(d) => pairs.extend(dyn_struct_meta_pairs(&d)?),
                    _ => {}
                }
            }
        }
    }
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
    let sigs = resolve_sigs(metas, env, elems.len());
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
                    Some(Val::Kw(k)) => k.to_string(),
                    _ => return Err("triggers: missing :event".into()),
                };
                let cull = matches!(get("cull"), Some(Val::Bool(true)));
                rules.push(TriggerRule::new(&event, &col, leq, cull));
            }
            rules.into()
        }
        _ => Vec::new().into(),
    };
    // :damage n | {:hit n ...} (DMK player() map) | (fn [self other] n)
    let damage = match map_get(&meta, "damage") {
        Some(Val::Num(n)) => Val::Num(n),
        Some(f @ (Val::Fn { .. } | Val::Builtin(_))) => f,
        Some(Val::Map(kvs)) => Val::Num(
            kvs.iter()
                .find_map(|(k, v)| match (k, v) {
                    (Val::Kw(kw), Val::Num(n)) if &**kw == "hit" => Some(*n),
                    _ => None,
                })
                .unwrap_or(1.0),
        ),
        _ => Val::Num(1.0),
    };
    let hitbox = match map_get(&meta, "hitbox") {
        Some(Val::Num(n)) => Some(n),
        _ => None,
    };
    // :expose {$some-hp :hp}: publish this entity's column as a derived
    // channel — the declarative form of "sim-computed world fact" (§3).
    // Parsed from the RAW form ($names here are channel designators, like
    // the keys of (with {$rank 0.5} …) — not reads).
    let expose: Rc<[(Rc<str>, Rc<str>)]> = parse_expose(metas);
    // collider set: archetype data, one Rc shared by every spawned element.
    // No genre defaults — an entity with no :colliders is inert to the
    // contact pass (scenery); what a "bullet" or "enemy" carries is the
    // library's business (spawn-bullet/spawn-enemy in lib/touhou.maku).
    // :hitbox r resizes the PRIMARY (first) collider — the generic knob
    // that lets a template's default collider set fit a bigger sprite.
    let colliders: Rc<[DynCollider]> = match map_get(&meta, "colliders") {
        Some(v) => {
            let items = dyn_arr(&DynStruct::from_val(v)).ok_or("colliders: expected array")?;
            let mut cs = Vec::new();
            for it in items.iter() {
                cs.push(parse_collider_slot(it)?);
            }
            if let (Some(r), Some(first)) = (hitbox, cs.first_mut()) {
                let slot = first.slot();
                let layer = match slot.shape {
                    ColliderSlotShape::Circle { .. } => Some(slot.layer.clone()),
                    ColliderSlotShape::CapsuleChain { .. } => None,
                };
                if let Some(layer) = layer {
                    *first = DynCollider::collider_circle_const(layer, r);
                }
            }
            cs.into()
        }
        _ => Vec::new().into(),
    };
    let renderers: Rc<[DynRender]> = match map_get(&meta, "renderers") {
        Some(v) => {
            let items = dyn_arr(&DynStruct::from_val(v)).ok_or("renderers: expected array")?;
            items
                .iter()
                .map(parse_render_slot)
                .collect::<Result<Vec<_>, _>>()?
                .into()
        }
        _ => Vec::new().into(),
    };
    // per-element column resolution: same axis rules as styles
    let cols: Vec<Vec<(Rc<str>, f64)>> = elems
        .iter()
        .enumerate()
        .map(|(i, e)| {
            cols.iter().map(|(k, v)| (k.clone(), axis_num(v, e, i))).collect()
        })
        .collect();
    let dyns = elems
        .into_iter()
        .map(|e| SpawnMade {
            dyn_figure: e.dyn_figure,
            colliders: e.colliders,
            curve_collider: e.curve_collider,
            renderers: e.renderers,
            cache_policy: e.cache_policy,
        })
        .collect();
    Ok(Val::Action(Rc::new(ActionV::Spawn {
        dyns,
        styles,
        sigs,
        team,
        cols,
        triggers,
        damage,
        colliders,
        renderers,
        expose,
    })))
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
            let (dyn_figure, colliders, curve_collider, renderers, cache_policy) = match &l.backing {
                CurveBacking::Parametric {
                    curve,
                    sample_set,
                    u_max_sig,
                    warn,
                    active,
                    width,
                    fill_sig,
                } => {
                    (
                        DynFigure::figure_curve(l.anchor.clone(), curve.clone()),
                        Vec::new().into(),
                        Some(CapsuleChainSlot {
                            sample_set: sample_set.clone(),
                            u_max_sig: u_max_sig.clone(),
                            width: *width,
                            activity: SlotActivity {
                                warn: *warn,
                                active: *active,
                                hot_frac_sig: fill_sig.clone(),
                            },
                        }),
                        vec![DynRender::render_polyline(CurveRenderSlot {
                            sample_set: sample_set.clone(),
                            u_max_sig: u_max_sig.clone(),
                            width: *width,
                            activity: SlotActivity {
                                warn: *warn,
                                active: *active,
                                hot_frac_sig: fill_sig.clone(),
                            },
                        })]
                        .into(),
                        EntityCachePolicy::default(),
                    )
                }
                CurveBacking::Trace { window } => (
                    DynFigure::pose(l.anchor.clone()),
                    Vec::new().into(),
                    None,
                    Vec::new().into(),
                    EntityCachePolicy {
                        trace: Some(TracePolicy { window: Some(*window) }),
                    },
                ),
            };
            out.push(SpawnElem {
                dyn_figure,
                colliders,
                curve_collider,
                renderers,
                cache_policy,
                path: path.clone(),
            });
            Ok(())
        }
        other => {
            out.push(SpawnElem {
                dyn_figure: DynFigure::pose(as_dyn(other)?),
                colliders: Vec::new().into(),
                curve_collider: None,
                renderers: Vec::new().into(),
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

fn dyn_arr(v: &DynStruct) -> Option<Vec<DynStruct>> {
    match v {
        DynStruct::Arr(items) => Some(items.iter().cloned().collect()),
        DynStruct::Const(Val::Arr(items)) => Some(items.iter().cloned().map(DynStruct::from_val).collect()),
        _ => None,
    }
}

fn dyn_struct_to_val(v: &DynStruct) -> Result<Val, String> {
    if v.is_dynamic() {
        Ok(Val::DynStruct(Rc::new(v.clone())))
    } else {
        v.eval(0.0, &MotionState::new(), &SigEnv::default())
    }
}

fn dyn_struct_meta_pairs(v: &DynStruct) -> Result<Vec<(Val, Val)>, String> {
    match v {
        DynStruct::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((k.clone(), dyn_struct_to_val(v)?)))
            .collect(),
        DynStruct::Const(Val::Map(kvs)) => Ok(kvs.iter().cloned().collect()),
        _ => Err("spawn meta: expected map".into()),
    }
}

fn dyn_map_get(m: &DynStruct, key: &str) -> Option<DynStruct> {
    match m {
        DynStruct::Map(kvs) => {
            for (k, v) in kvs.iter() {
                if matches!(k, Val::Kw(kw) if &**kw == key) {
                    return Some(v.clone());
                }
            }
            None
        }
        DynStruct::Const(v) => map_get(v, key).map(DynStruct::from_val),
        _ => None,
    }
}

fn dyn_kw(v: &DynStruct) -> Option<Rc<str>> {
    match v {
        DynStruct::Const(Val::Kw(k)) => Some(k.clone()),
        _ => None,
    }
}

fn dyn_num(v: &DynStruct) -> Result<DynNum, String> {
    match v {
        DynStruct::Num(d) => Ok(d.clone()),
        DynStruct::Const(Val::Num(n)) => Ok(DynNum::num(*n)),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

fn dyn_num_static(v: &DynStruct) -> Result<f64, String> {
    match v {
        DynStruct::Const(Val::Num(n)) => Ok(*n),
        DynStruct::Num(d) => match d.repr() {
            NumDynRepr::Const(n) => Ok(*n),
            NumDynRepr::Expr { .. } => Err("expected static number".into()),
        },
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

fn dyn_map_num(m: &DynStruct, key: &str, default: f64) -> Result<f64, String> {
    match dyn_map_get(m, key) {
        Some(v) => dyn_num_static(&v),
        None => Ok(default),
    }
}

fn dyn_map_num_any(m: &DynStruct, keys: &[&str], default: f64) -> Result<DynNum, String> {
    for key in keys {
        if let Some(v) = dyn_map_get(m, key) {
            return dyn_num(&v);
        }
    }
    Ok(DynNum::num(default))
}

fn parse_sample_set(opts: &DynStruct) -> Result<SampleSet, String> {
    if let Some(vals) = dyn_map_get(opts, "samples").and_then(|v| dyn_arr(&v)) {
        let mut out = Vec::with_capacity(vals.len());
        for v in vals.iter() {
            out.push(dyn_num_static(v)?);
        }
        return Ok(SampleSet::Values(out.into()));
    }
    Ok(SampleSet::Step {
        resolution: dyn_map_num(opts, "resolution", 0.1)?,
    })
}

fn parse_slot_activity(opts: &DynStruct) -> Result<SlotActivity, String> {
    Ok(SlotActivity {
        warn: dyn_map_num(opts, "warn", 0.0)?,
        active: dyn_map_num(opts, "active", f64::INFINITY)?,
        // Full structural Dyn<T> coercion should replace this parser-level
        // shortcut; for now explicit low-level spawn specs are static data.
        hot_frac_sig: None,
    })
}

fn parse_capsule_chain_slot(opts: &DynStruct) -> Result<CapsuleChainSlot, String> {
    Ok(CapsuleChainSlot {
        sample_set: parse_sample_set(opts)?,
        u_max_sig: None,
        width: dyn_map_num(opts, "width", 1.0)?,
        activity: parse_slot_activity(opts)?,
    })
}

fn parse_shape_spec(v: &DynStruct) -> Result<(Rc<str>, DynStruct), String> {
    match v {
        DynStruct::Arr(items) if items.len() == 2 => {
            let Some(shape) = dyn_kw(&items[0]) else {
                return Err("shape: expected keyword shape name".into());
            };
            Ok((shape, items[1].clone()))
        }
        DynStruct::Const(Val::Kw(shape)) => {
            Ok((shape.clone(), DynStruct::Map(Rc::new(Vec::new()))))
        }
        _ => Err("shape: expected [:shape opts]".into()),
    }
}

fn parse_collider_slot(v: &DynStruct) -> Result<DynCollider, String> {
    if !matches!(v, DynStruct::Map(_) | DynStruct::Const(Val::Map(_))) {
        return Err("colliders: expected maps".into());
    }
    let layer = match dyn_map_get(v, "layer").and_then(|v| dyn_kw(&v)) {
        Some(k) => k,
        _ => return Err("colliders: missing :layer".into()),
    };
    if let Some(shape_v) = dyn_map_get(v, "shape") {
        let (shape, opts) = parse_shape_spec(&shape_v)?;
        return match shape.as_ref() {
            "circle" => Ok(DynCollider::collider_circle(
                layer,
                dyn_map_num_any(&opts, &["radius", "r"], 0.08)?,
            )),
            "capsule-chain" => Ok(DynCollider::collider_capsule_chain(
                layer,
                dyn_map_num_any(&opts, &["radius", "r"], 0.08)?,
                parse_capsule_chain_slot(&opts)?,
            )),
            _ => Err(format!("colliders: unknown shape :{}", shape)),
        };
    }
    match dyn_map_get(v, "r") {
        Some(r) => Ok(DynCollider::collider_circle(layer, dyn_num(&r)?)),
        _ => Err("colliders: missing :r or :shape".into()),
    }
}

fn parse_render_slot(v: &DynStruct) -> Result<DynRender, String> {
    if !matches!(v, DynStruct::Map(_) | DynStruct::Const(Val::Map(_))) {
        return Err("renderers: expected maps".into());
    }
    let shape_v = dyn_map_get(v, "shape")
        .unwrap_or_else(|| DynStruct::Const(Val::Kw("polyline".into())));
    let (shape, opts) = parse_shape_spec(&shape_v)?;
    match shape.as_ref() {
        "polyline" => Ok(DynRender::render_polyline(CurveRenderSlot {
            sample_set: parse_sample_set(&opts)?,
            u_max_sig: None,
            width: dyn_map_num(&opts, "width", 1.0)?,
            activity: parse_slot_activity(&opts)?,
        })),
        _ => Err(format!("renderers: unknown shape :{}", shape)),
    }
}

pub(crate) fn kw_str(v: &Val) -> String {
    match v {
        Val::Kw(k) => k.to_string(),
        Val::Str(s) => s.to_string(),
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
        .map(|k| Pose { x: 0.0, y: 0.0, th: k as f64 * 360.0 / n as f64 })
        .collect();
    formation(poses, items.get(2), env, ctx, world)
}

pub(crate) fn sf_arrow(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let back = evaluate(&items[2], env, ctx, world)?.num()?;
    let side = evaluate(&items[3], env, ctx, world)?.num()?;
    let half = (n - 1) / 2;
    let poses = (-half..=(n - 1 - half))
        .map(|j| Pose { x: -back * (j.abs() as f64), y: side * j as f64, th: 0.0 })
        .collect();
    formation(poses, items.get(4), env, ctx, world)
}

pub(crate) fn sf_fan(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let step = evaluate(&items[2], env, ctx, world)?.num()?;
    let mid = (n - 1) as f64 / 2.0;
    let poses = (0..n)
        .map(|k| Pose { x: 0.0, y: 0.0, th: (k as f64 - mid) * step })
        .collect();
    formation(poses, items.get(3), env, ctx, world)
}
