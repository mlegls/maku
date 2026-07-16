//! The spawn path: meta resolution, element flattening, formations.

use super::*;
use crate::edn::Form;
use std::rc::Rc;


const RESERVED_KW_FIELD_KEYS: &[&str] = &[
    "style",
];

fn is_reserved_sym_field_key(key: &str) -> bool {
    RESERVED_KW_FIELD_KEYS.contains(&key)
}

fn is_numeric_field_value(v: &Val) -> bool {
    match v {
        Val::Num(_) => true,
        Val::Arr(items) => items.iter().all(is_numeric_field_value),
        _ => false,
    }
}

fn normalize_spawn_input(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<SpawnSlots, String> {
    let figure = evaluate(&items[1], env, ctx, world)?;
    let mut meta_forms = Vec::new();
    let mut computed_meta_pairs: Vec<(Val, Val)> = Vec::new();
    let mut colliders = Vec::new();
    for item in &items[2..] {
        if matches!(item, Form::Map(_)) {
            meta_forms.push(item.clone());
            continue;
        }
        let value = evaluate(item, env, ctx, world)?;
        match value {
            Val::ColliderProjector(specs) => colliders.extend(specs.iter().cloned()),
            v @ (Val::Arr(_) | Val::Nothing) => {
                colliders.extend(
                    flatten_collider_projectors(
                        "collider",
                        v,
                        None,
                    )?
                    .iter()
                    .cloned(),
                );
            }
            Val::Map(kvs) => computed_meta_pairs.extend(kvs.iter().cloned()),
            Val::DynLike(d) => computed_meta_pairs.extend(dynlike_meta_pairs(&d)?),
            _ => {}
        }
    }
    Ok(SpawnSlots {
        targets: SpawnSlotTypes::low_level(),
        figure,
        colliders,
        meta: SpawnMetaInput {
            forms: meta_forms,
            computed_pairs: computed_meta_pairs,
        },
    })
}

fn merge_spawn_meta(
    meta: SpawnMetaInput,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<SpawnMetaPlan, String> {
    // Meta maps merge per-key with later maps winning. Literal maps are
    // lifted through DynLike so static keys may carry dyn-valued fields;
    // computed maps arrive as already-evaluated pairs.
    let mut pairs: Vec<(Val, Val)> = Vec::new();
    for mf in meta.forms.iter().rev() {
        match mf {
            Form::Map(kvs) => {
                for (k, v) in kvs.iter() {
                    let kv = evaluate(k, env, ctx, world)?;
                    let vv = dynlike_to_val(&eval_dynlike_form(v, env, ctx, world)?)?;
                    pairs.push((kv, vv));
                }
            }
            m => {
                // a computed meta (variable, call): evaluated pairs only.
                match evaluate(m, env, ctx, world)? {
                    Val::Map(kvs) => pairs.extend(kvs.iter().cloned()),
                    Val::DynLike(d) => pairs.extend(dynlike_meta_pairs(&d)?),
                    _ => {}
                }
            }
        }
    }
    pairs.extend(meta.computed_pairs);
    Ok(SpawnMetaPlan {
        value: Val::Map(Rc::new(pairs)),
    })
}

fn plan_spawn(
    slots: SpawnSlots,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<SpawnPlan, String> {
    debug_assert_eq!(slots.targets, SpawnSlotTypes::low_level());
    let meta = merge_spawn_meta(slots.meta, env, ctx, world)?;
    let mut elems = Vec::new();
    flatten_elems(slots.figure, &mut Vec::new(), &mut elems)?;
    // rand in signal expressions is an ir constant per element (§5): draw a
    // capture vector per element over the site's shared marker programs, or
    // (extraction bail) clone the motion tree substituting drawn constants
    for e in elems.iter_mut() {
        if dyn_figure_has_rand(&e.dyn_figure) {
            e.dyn_figure = instantiate_rand_geometry(&e.dyn_figure, world);
        }
    }
    Ok(SpawnPlan {
        elems,
        meta,
        colliders: slots.colliders,
    })
}

fn build_entity_specs(
    elems: Vec<SpawnElem>,
    meta: &SpawnMetaPlan,
    collider_slots: Vec<ColliderProjectorValue>,
    _env: &Env,
    world: &mut World,
) -> Result<Vec<EntitySpec>, String> {
    let meta_value = &meta.value;
    let styles = resolve_style_fields(meta_value, &elems, world)?;
    let mut sym_fields: Vec<(FieldName, Symbol)> = Vec::new();
    if let Val::Map(kvs) = meta_value {
        for (k, v) in kvs.iter() {
            let (Val::Kw(field), Val::Kw(value)) = (k, v) else {
                continue;
            };
            if is_reserved_sym_field_key(field.as_ref()) {
                continue;
            }
            let field = world.field_sym(field.as_ref());
            let value = world.symbols.intern(value.as_ref());
            world.intern_sym_field_slot(field);
            if !sym_fields.iter().any(|(name, _)| *name == field) {
                sym_fields.push((field, value));
            }
        }
    }
    let mut dyn_cols: Vec<(Rc<str>, DynNum)> = Vec::new();
    let push_dyn_col = |dyn_cols: &mut Vec<(Rc<str>, DynNum)>, k: Rc<str>, v: DynNum| {
        if !dyn_cols.iter().any(|(existing, _)| existing.as_ref() == k.as_ref()) {
            dyn_cols.push((k, v));
        }
    };
    if let Val::Map(kvs) = meta_value {
        for (k, v) in kvs.iter() {
            let Val::Kw(k) = k else { continue };
            if is_reserved_sym_field_key(k.as_ref()) {
                continue;
            }
            if let Val::DynLike(d) = v {
                push_dyn_col(&mut dyn_cols, k.as_ref().into(), as_dyn_num(d)?);
            }
        }
    }
    // Top-level numeric fields initialize SoA fields. What a field means is
    // library/card code: hp is just a field, damage is just a field, etc.
    // Values may be arrays, binding per spawn element exactly like style axes
    // (leading-axis / by-length / nested-structural).
    let mut cols: Vec<(Rc<str>, Val)> = Vec::new();
    let push_col = |cols: &mut Vec<(Rc<str>, Val)>, k: Rc<str>, v: Val| {
        if !cols.iter().any(|(existing, _)| existing.as_ref() == k.as_ref()) {
            cols.push((k, v));
        }
    };
    if let Val::Map(kvs) = meta_value {
        for (k, v) in kvs.iter() {
            if let Val::Kw(k) = k {
                if !is_reserved_sym_field_key(k.as_ref()) && is_numeric_field_value(v) {
                    push_col(&mut cols, k.as_ref().into(), v.clone());
                }
            }
        }
    }
    // Collider sets are explicit spawn arguments. No genre defaults —
    // an entity with no colliders is inert to the contact pass (scenery);
    // what a "bullet" or "enemy" carries is the library's business
    // (bullet/enemy in lib/touhou.maku).
    let mut explicit_colliders = collider_slots;
    if explicit_colliders.is_empty() {
        explicit_colliders.push(ColliderProjectorValue::empty());
    }
    // per-element column resolution: same axis rules as styles
    let cols: Vec<Vec<(ColName, f64)>> = elems
        .iter()
        .enumerate()
        .map(|(i, e)| {
            cols.iter().map(|(k, v)| (world.intern_col(k.as_ref()), axis_num(v, e, i))).collect()
        })
        .collect();
    let dyn_cols: Rc<[(ColName, DynNum)]> = dyn_cols
        .into_iter()
        .map(|(k, v)| (world.intern_col(k.as_ref()), v))
        .collect::<Vec<_>>()
        .into();
    let shared_collider_projectors: Rc<[ColliderProjectorValue]> = explicit_colliders.into();
    let group = elems.len();
    let entities = elems
        .into_iter()
        .zip(styles)
        .zip(cols)
        .enumerate()
        .map(|(flat, ((e, style_fields), cols))| {
            // most elements carry no per-element collider spec (plain
            // bullets): share the spawn's projector Rc across the group —
            // an empty Stable spec materializes nothing, so dropping it is
            // behavior-neutral, and shared spec identity is what the sim's
            // per-pass collider plan memo keys on
            let empty_spec = matches!(&e.collider_projector_spec.expr,
                ColliderProjectorExpr::Stable(s) if s.is_empty());
            let collider_projector = if empty_spec {
                ColliderProjector { projectors: shared_collider_projectors.clone() }
            } else {
                let mut collider_projectors =
                    shared_collider_projectors.iter().cloned().collect::<Vec<_>>();
                collider_projectors.push(e.collider_projector_spec);
                ColliderProjector { projectors: collider_projectors.into() }
            };
            let mut sym_fields = sym_fields.clone();
            for (field, value) in style_fields {
                if !sym_fields.iter().any(|(name, _)| *name == field) {
                    sym_fields.push((field, value));
                }
            }
            let mut cols = cols;
            let mut dyn_cols = dyn_cols.iter().cloned().collect::<Vec<_>>();
            if group > 1 {
                // shared meta signals bind per element: array-valued
                // results select by the element's axis position
                for (_, d) in dyn_cols.iter_mut() {
                    *d = d.with_axis(&e.path, flat);
                }
            }
            for (key, seed) in e.fields.iter() {
                match seed {
                    FieldSeed::Num(n) => {
                        let col = world.intern_col(key.as_ref());
                        cols.retain(|(name, _)| *name != col);
                        dyn_cols.retain(|(name, _)| *name != col);
                        cols.push((col, *n));
                    }
                    FieldSeed::Dyn(d) => {
                        let col = world.intern_col(key.as_ref());
                        cols.retain(|(name, _)| *name != col);
                        dyn_cols.retain(|(name, _)| *name != col);
                        dyn_cols.push((col, d.clone()));
                    }
                    FieldSeed::Sym(s) => {
                        let field = world.field_sym(key.as_ref());
                        let value = world.symbols.intern(s.as_ref());
                        world.intern_sym_field_slot(field);
                        sym_fields.retain(|(name, _)| *name != field);
                        sym_fields.push((field, value));
                    }
                }
            }
            EntitySpec {
                dyn_figure: e.dyn_figure,
                cache_policy: e.cache_policy,
                sym_fields,
                cols,
                dyn_cols: dyn_cols.into(),
                collider_projector,
            }
        })
        .collect();
    Ok(entities)
}

pub(crate) fn sf_spawn(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let slots = normalize_spawn_input(items, env, ctx, world)?;
    let plan = plan_spawn(slots, env, ctx, world)?;
    let entities = build_entity_specs(
        plan.elems,
        &plan.meta,
        plan.colliders,
        env,
        world,
    )?;
    Ok(Val::Action(Rc::new(ActionV::Spawn { entities })))
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
        Val::ElemV(e) => {
            let start = out.len();
            flatten_elems(e.figure.clone(), path, out)?;
            for elem in &mut out[start..] {
                let mut fields = e.fields.iter().cloned().collect::<Vec<_>>();
                for (k, v) in elem.fields.iter() {
                    if !fields.iter().any(|(existing, _)| existing.as_ref() == k.as_ref()) {
                        fields.push((k.clone(), v.clone()));
                    }
                }
                elem.fields = fields.into();
            }
            Ok(())
        }
        Val::CurveV(l) => {
            let (dyn_figure, colliders, cache_policy) = match &l.backing {
                CurveBacking::Parametric { curve } => (
                    DynFigure::figure_curve(l.anchor.clone(), curve.clone()),
                    ColliderProjectorValue::empty(),
                    EntityCachePolicy::default(),
                ),
                CurveBacking::Trace { window } => (
                    DynFigure::pose(l.anchor.clone()),
                    ColliderProjectorValue::empty(),
                    EntityCachePolicy {
                        trace: Some(TracePolicy { window: Some(*window) }),
                    },
                ),
            };
            out.push(SpawnElem {
                dyn_figure,
                collider_projector_spec: colliders,
                cache_policy,
                path: path.clone(),
                fields: Rc::new([]),
            });
            Ok(())
        }
        other => {
            out.push(SpawnElem {
                dyn_figure: as_dyn_figure(other)?,
                collider_projector_spec: ColliderProjectorValue::empty(),
                cache_policy: EntityCachePolicy::default(),
                path: path.clone(),
                fields: Rc::new([]),
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
        DynNode::ConstFrame { child, .. } => dyn_has_rand(child),
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

/// One rand site's draw spec, recorded by `extract_rand` in walk order.
/// The walk order IS the RNG contract: capture draws at spawn must consume
/// `world.next_rand()` in exactly the order `subst_rand` would.
#[derive(Clone, Copy, Debug)]
pub enum RandSite {
    Range { a: f64, b: f64, floor: bool },
    Pm1,
}

fn capture_marker(slot: usize) -> Form {
    Form::List(vec![Form::Sym("%capture".into()), Form::Num(slot as f64)].into())
}

/// `subst_rand`'s walk, rewriting each rand site to a `(%capture i)` marker
/// instead of drawing — including subst_rand's literal-bound defaulting and
/// its non-recursion into rand argument positions.
pub(crate) fn extract_rand(f: &Form, sites: &mut Vec<RandSite>) -> Form {
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
                        let slot = sites.len();
                        sites.push(RandSite::Range { a, b, floor: s.as_ref() == "rand-int" });
                        return capture_marker(slot);
                    }
                    "randpm1" => {
                        let slot = sites.len();
                        sites.push(RandSite::Pm1);
                        return capture_marker(slot);
                    }
                    _ => {}
                }
            }
            Form::List(items.iter().map(|i| extract_rand(i, sites)).collect::<Vec<_>>().into())
        }
        Form::Vector(items) => {
            Form::Vector(items.iter().map(|i| extract_rand(i, sites)).collect::<Vec<_>>().into())
        }
        other => other.clone(),
    }
}

/// One entity's full capture vector: rand draws (slots 0..sites) in site
/// (= walk) order, then the node's fixed env-capture values.
pub(crate) fn draw_caps(ex: &ExtractedSig, world: &mut World) -> Rc<[f64]> {
    ex.sites
        .iter()
        .map(|s| match s {
            RandSite::Range { a, b, floor } => {
                let v = a + world.next_rand() * (b - a);
                if *floor { v.floor() } else { v }
            }
            RandSite::Pm1 => {
                if world.next_rand() < 0.5 { -1.0 } else { 1.0 }
            }
        })
        .chain(ex.env_caps.iter().copied())
        .collect()
}

/// Replace `(%capture i)` markers with the entity's drawn values — the
/// oracle's bridge back to plain interpreter evaluation of a marker form.
pub(crate) fn subst_captures(f: &Form, caps: &[f64]) -> Form {
    match f {
        Form::List(items) => {
            if items.len() == 2 {
                if let (Some(Form::Sym(s)), Some(Form::Num(slot))) = (items.first(), items.get(1)) {
                    if s.as_ref() == "%capture" {
                        return Form::Num(caps[*slot as usize]);
                    }
                }
            }
            Form::List(items.iter().map(|i| subst_captures(i, caps)).collect::<Vec<_>>().into())
        }
        Form::Vector(items) => {
            Form::Vector(items.iter().map(|i| subst_captures(i, caps)).collect::<Vec<_>>().into())
        }
        other => other.clone(),
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
        DynNode::ClosedPt { a, b, polar, env, rand, .. } => {
            match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Rc::new(DynNode::ClosedPt {
                    a: ex.forms[0].clone(),
                    b: ex.forms[1].clone(),
                    polar: *polar,
                    env: env.clone(),
                    programs: std::cell::OnceCell::from(Some((
                        ex.programs[0].clone(),
                        ex.programs[1].clone(),
                    ))),
                    rand: Some(Rc::new(RandCell::Caps(draw_caps(ex, world)))),
                }),
                // Bail (markers didn't lower) or a construction path that
                // skipped extraction: per-entity substitution, as ever.
                Some(_) | None if form_has_rand(a) || form_has_rand(b) => {
                    Rc::new(DynNode::ClosedPt {
                        a: subst_rand(a, world),
                        b: subst_rand(b, world),
                        polar: *polar,
                        env: env.clone(),
                        programs: std::cell::OnceCell::new(),
                        rand: None,
                    })
                }
                _ => d.clone(),
            }
        }
        DynNode::Vel { a, b, polar, env, rand, .. } => {
            match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Rc::new(DynNode::Vel {
                    a: ex.forms[0].clone(),
                    b: ex.forms[1].clone(),
                    polar: *polar,
                    env: env.clone(),
                    programs: std::cell::OnceCell::from(Some((
                        ex.programs[0].clone(),
                        ex.programs[1].clone(),
                    ))),
                    rand: Some(Rc::new(RandCell::Caps(draw_caps(ex, world)))),
                }),
                Some(_) | None if form_has_rand(a) || form_has_rand(b) => {
                    Rc::new(DynNode::Vel {
                        a: subst_rand(a, world),
                        b: subst_rand(b, world),
                        polar: *polar,
                        env: env.clone(),
                        programs: std::cell::OnceCell::new(),
                        rand: None,
                    })
                }
                _ => d.clone(),
            }
        }
        DynNode::RotExpr { form, env, rand, .. } => {
            match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Rc::new(DynNode::RotExpr {
                    form: ex.forms[0].clone(),
                    env: env.clone(),
                    program: std::cell::OnceCell::from(Some(ex.programs[0].clone())),
                    rand: Some(Rc::new(RandCell::Caps(draw_caps(ex, world)))),
                }),
                Some(_) | None if form_has_rand(form) => Rc::new(DynNode::RotExpr {
                    form: subst_rand(form, world),
                    env: env.clone(),
                    program: std::cell::OnceCell::new(),
                    rand: None,
                }),
                _ => d.clone(),
            }
        }
        DynNode::Translate { dx, dy, child } => Rc::new(DynNode::Translate {
            dx: *dx,
            dy: *dy,
            child: instantiate_rand(child, world),
        }),
        DynNode::Frame(a, b) => Rc::new(DynNode::Frame(
            instantiate_rand(a, world),
            instantiate_rand(b, world),
        )),
        DynNode::ConstFrame { pose, rot, child } => Rc::new(DynNode::ConstFrame {
            pose: *pose,
            rot: *rot,
            child: instantiate_rand(child, world),
        }),
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

pub(crate) fn kw_str(v: &Val) -> String {
    match v {
        Val::Kw(k) => k.to_string(),
        _ => String::new(),
    }
}

/// §5/F15: a meta axis array binds to the first array level (root to leaf)
/// whose length matches; otherwise it cycles on the flat index. Selection
/// rules shared by static columns, style values, and axis-bound dyn
/// signals (nested-structural, else by-length, else leading-cycle).
pub(crate) fn axis_select_val(v: &Val, path: &[(usize, usize)], flat: usize) -> Val {
    match v {
        Val::Arr(items) if items.iter().any(|x| matches!(x, Val::Arr(_))) => {
            let mut cur = v.clone();
            let mut depth = 0;
            loop {
                match cur {
                    Val::Arr(xs) if !xs.is_empty() => {
                        let idx = path.get(depth).map(|(_, i)| *i).unwrap_or(flat);
                        cur = xs[idx % xs.len()].clone();
                        depth += 1;
                    }
                    other => return other,
                }
            }
        }
        Val::Arr(items) if !items.is_empty() => {
            let len = items.len();
            for (axis_len, idx) in path {
                if *axis_len == len {
                    return items[idx % len].clone();
                }
            }
            items[flat % len].clone()
        }
        v => v.clone(),
    }
}

/// Numeric per-element resolution over the shared axis rules, for columns.
pub(crate) fn axis_num(v: &Val, elem: &SpawnElem, flat: usize) -> f64 {
    axis_select_val(v, &elem.path, flat).num().unwrap_or(0.0)
}

// NESTED arrays resolve STRUCTURALLY: depth in the meta value = axis
// along the element's root-to-leaf path, cycling at every level; a
// scalar reached early broadcasts to all deeper axes.
// [[:red :blue] :green :purple] over 10×3 → group 0 cycles red/blue
// inside, group 1 all green, group 2 all purple, group 3 wraps to
// [red blue]… Shape disambiguates where length cannot. Flat arrays:
// F15 by-length targeting, leading-first.
pub(crate) fn axis_value(v: &Val, elem: &SpawnElem, flat: usize) -> String {
    kw_str(&axis_select_val(v, &elem.path, flat))
}

pub(crate) fn resolve_style_fields(
    meta: &Val,
    elems: &[SpawnElem],
    world: &mut World,
) -> Result<Vec<Vec<(FieldName, Symbol)>>, String> {
    let style = map_get(meta, "style").unwrap_or(Val::Map(Rc::new(vec![])));
    let axes = ["family", "color", "variant"]
        .iter()
        .filter_map(|axis| map_get(&style, axis).map(|v| (*axis, v)))
        .collect::<Vec<_>>();
    Ok(elems
        .iter()
        .enumerate()
        .map(|(k, e)| {
            axes
                .iter()
                .filter_map(|(axis, v)| {
                    let value = axis_value(v, e, k);
                    if value.is_empty() {
                        return None;
                    }
                    let field = world.field_sym(axis);
                    let value = world.symbols.intern(value.as_str());
                    world.intern_sym_field_slot(field);
                    Some((field, value))
                })
                .collect()
        })
        .collect())
}
