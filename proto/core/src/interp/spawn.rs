//! The spawn path: meta resolution, element flattening, formations.

use super::*;
use crate::edn::Form;
use std::rc::Rc;


/// Meta keys whose values are signals sampled later (§7): never evaluated at
/// spawn time (they reference slot-bound t).
/// Meta tags whose values are NOT evaluated at spawn: signal-valued tags
/// (contain t), and :expose (whose $names are channel DESIGNATORS, not
/// reads — evaluated, $boss-hp would resolve as a channel read).
const SIGNAL_TAGS: &[&str] = &["hue", "facing", "expose"];

pub(crate) fn parse_expose(meta: Option<&Form>) -> Rc<[(Rc<str>, Rc<str>)]> {
    let mut out = Vec::new();
    if let Some(Form::Map(kvs)) = meta {
        for (k, v) in kvs.iter() {
            if matches!(k, Form::Kw(kw) if &**kw == "expose") {
                if let Form::Map(pairs) = v {
                    for (col, chan) in pairs.iter() {
                        let Form::Kw(col) = col else { continue };
                        let chan: Option<Rc<str>> = match chan {
                            Form::Sym(s) if s.starts_with('$') => Some(s[1..].into()),
                            Form::Kw(c) => Some(c.as_ref().into()),
                            _ => None,
                        };
                        if let Some(chan) = chan {
                            out.push((col.as_ref().into(), chan));
                        }
                    }
                }
            }
        }
    }
    out.into()
}

pub(crate) fn sf_spawn(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let dv = evaluate(&items[1], env, ctx, world)?;
    let meta = match items.get(2) {
        Some(Form::Map(kvs)) => {
            let mut pairs = Vec::new();
            for (k, v) in kvs.iter() {
                let kv = evaluate(k, env, ctx, world)?;
                let skip = matches!(&kv, Val::Kw(kw) if SIGNAL_TAGS.contains(&kw.as_ref()));
                let vv = if skip { Val::Nothing } else { evaluate(v, env, ctx, world)? };
                pairs.push((kv, vv));
            }
            Val::Map(Rc::new(pairs))
        }
        Some(m) => evaluate(m, env, ctx, world)?,
        None => Val::Map(Rc::new(vec![])),
    };
    let mut elems = Vec::new();
    flatten_elems(dv, &mut Vec::new(), &mut elems)?;
    // rand in signal expressions is an ir constant per element (§5): clone the
    // motion tree per element, substituting rand calls with drawn constants
    for e in elems.iter_mut() {
        if dyn_has_rand(&e.motion) {
            e.motion = instantiate_rand(&e.motion, world);
        }
    }
    let styles = resolve_styles(&meta, &elems)?;
    let hues = resolve_hue(items.get(2), &meta, env, elems.len());
    let team: Option<Rc<str>> = match map_get(&meta, "team") {
        Some(Val::Kw(k)) => Some(Rc::from(&*k)),
        _ => None,
    };
    // columns: :hp n is sugar for a col; :cols {:armor 2 ...} adds more.
    // Column values may be ARRAYS, binding per spawn element exactly like
    // style axes (leading-axis / by-length / nested-structural) — per-bullet
    // saved data: :cols {:ci (iota 8)} gives bullet k the column ci = k.
    // :team :enemy defaults hp to 1 so untyped enemies still die to a shot.
    let mut cols: Vec<(Rc<str>, Val)> = Vec::new();
    match map_get(&meta, "hp") {
        Some(v @ (Val::Num(_) | Val::Arr(_))) => cols.push(("hp".into(), v)),
        _ => {
            if team.as_deref() == Some("enemy") {
                cols.push(("hp".into(), Val::Num(1.0)));
            }
        }
    }
    if let Some(Val::Map(kvs)) = map_get(&meta, "cols") {
        for (k, v) in kvs.iter() {
            if let Val::Kw(k) = k {
                cols.push((k.as_ref().into(), v.clone()));
            }
        }
    }
    // triggers: explicit :triggers replaces the synthesized default
    // (hp col present → death rule: hp ≤ 0 → cull + event :died)
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
        _ => {
            if cols.iter().any(|(k, _): &(Rc<str>, _)| &**k == "hp") {
                vec![TriggerRule::new("died", "hp", 0.0, true)].into()
            } else {
                Vec::new().into()
            }
        }
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
    // :expose {:hp $boss-hp}: publish this entity's column as a derived
    // channel — the declarative form of "sim-computed world fact" (§3).
    // Parsed from the RAW form ($names here are channel designators, like
    // the keys of (with {$rank 0.5} …) — not reads); :keywords accepted too.
    let expose: Rc<[(Rc<str>, Rc<str>)]> = parse_expose(items.get(2));
    // collider set: archetype data, one Rc shared by every spawned element.
    // :colliders [{:layer :damage :r 0.1} ...] replaces the team default;
    // :hitbox r resizes the default primary collider.
    let colliders: Rc<[Collider]> = match map_get(&meta, "colliders") {
        Some(Val::Arr(items)) => {
            let mut cs = Vec::new();
            for it in items.iter() {
                let Val::Map(kvs) = it else {
                    return Err("colliders: expected maps".into());
                };
                let get = |name: &str| {
                    kvs.iter().find_map(|(k, v)| match k {
                        Val::Kw(kw) if &**kw == name => Some(v.clone()),
                        _ => None,
                    })
                };
                let layer = match get("layer") {
                    Some(Val::Kw(k)) => match &*k {
                        "damage" => Layer::Damage,
                        "graze" => Layer::Graze,
                        "shot" => Layer::Shot,
                        "hurt" => Layer::Hurt,
                        "player-hurt" => Layer::PlayerHurt,
                        other => return Err(format!("colliders: unknown layer :{}", other)),
                    },
                    _ => return Err("colliders: missing :layer".into()),
                };
                let r = match get("r") {
                    Some(Val::Num(n)) => n,
                    _ => return Err("colliders: missing :r".into()),
                };
                cs.push(Collider { layer, r });
            }
            cs.into()
        }
        _ => {
            let fam = styles.first().map(|s| s.family.as_str()).unwrap_or("");
            default_colliders(team.as_deref(), fam, hitbox).into()
        }
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
        .map(|e| SpawnMade { motion: e.motion, kind: e.kind })
        .collect();
    Ok(Val::Action(Rc::new(ActionV::Spawn {
        dyns,
        styles,
        hues,
        team,
        cols,
        triggers,
        damage,
        colliders,
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
        Val::Ext(l) => {
            out.push(SpawnElem {
                motion: l.anchor.clone(),
                kind: Kind::Laser {
                    shape: l.shape.clone(),
                    warn: l.warn,
                    active: l.active,
                    u_max: l.u_max,
                    u_max_sig: l.u_max_sig.clone(),
                    resolution: l.resolution,
                    width: l.width,
                },
                path: path.clone(),
            });
            Ok(())
        }
        Val::PatherV(pv) => {
            out.push(SpawnElem {
                motion: pv.anchor.clone(),
                kind: Kind::Pather { window: pv.window },
                path: path.clone(),
            });
            Ok(())
        }
        other => {
            out.push(SpawnElem { motion: as_dyn(other)?, kind: Kind::Point, path: path.clone() });
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

/// :hue is signal-valued meta (§7): keep the FORM and sample at render time.
pub(crate) fn resolve_hue(meta_form: Option<&Form>, meta: &Val, env: &Env, n: usize) -> Vec<Option<MetaSig>> {
    let has_hue = map_get(meta, "hue").is_some();
    if !has_hue {
        return vec![None; n];
    }
    // find the hue form in the meta map form
    if let Some(Form::Map(kvs)) = meta_form {
        for (k, v) in kvs.iter() {
            if let Form::Kw(kw) = k {
                if &**kw == "hue" {
                    return (0..n)
                        .map(|idx| Some(MetaSig { form: v.clone(), env: env.clone(), idx }))
                        .collect();
                }
            }
        }
    }
    vec![None; n]
}

pub(crate) fn formation(
    poses: Vec<Pose>,
    child: Option<&Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let arr = Val::Arr(Rc::new(poses.into_iter().map(Val::Pose).collect()));
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
