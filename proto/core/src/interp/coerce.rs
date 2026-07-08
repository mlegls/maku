//! Explicit value-to-dyn coercion machinery.
//!
//! This stays interpreter-local for now because deferred dyn values carry
//! interpreter forms and environments. The important boundary is that
//! language coercions are centralized as explicit branches, rather than hidden
//! behind Rust conversion traits.

use super::*;
use std::rc::Rc;

/// Atomic data values in the target runtime-data layer. `Legacy` is a
/// temporary bridge for interpreter/control values and pre-refactor data
/// atoms; typed dyn boundaries should consume the concrete variants first.
#[derive(Clone, Debug)]
pub enum DataAtom {
    Num(f64),
    Kw(Rc<str>),
    Figure(Figure),
    Handle(EntityRef),
    Nothing,
    Legacy(Val),
}

impl DataAtom {
    pub fn from_val(v: Val) -> DataAtom {
        match v {
            Val::Num(n) => DataAtom::Num(n),
            Val::Kw(k) => DataAtom::Kw(k),
            Val::Pose(p) => DataAtom::Figure(Figure::Pose(p)),
            Val::Figure(f) => DataAtom::Figure(f),
            Val::Handle(id) => DataAtom::Handle(id),
            Val::Nothing => DataAtom::Nothing,
            other => DataAtom::Legacy(other),
        }
    }

    pub fn to_val(&self) -> Val {
        match self {
            DataAtom::Num(n) => Val::Num(*n),
            DataAtom::Kw(k) => Val::Kw(k.clone()),
            DataAtom::Figure(Figure::Pose(p)) => Val::Pose(*p),
            DataAtom::Figure(f) => Val::Figure(f.clone()),
            DataAtom::Handle(id) => Val::Handle(*id),
            DataAtom::Nothing => Val::Nothing,
            DataAtom::Legacy(v) => v.clone(),
        }
    }
}

/// A value-level structure that can be lifted into a typed dyn at an
/// expected boundary. This is not source syntax and not a typed `Dyn<T>` by
/// itself: `DynLike::Dyn` keeps a deferred expression plus its environment,
/// while typed boundaries such as spawn interpret the structure with `as_*`
/// schema checks. Ordinary static structures still evaluate to ordinary
/// arrays/maps.
#[derive(Clone, Debug)]
pub enum DynLike {
    Atom(DataAtom),
    Dyn(DynVal),
    List(Rc<[DynLike]>),
    Map(Rc<Vec<(DataAtom, DynLike)>>),
}

#[derive(Clone, Debug)]
pub enum DynVal {
    Expr { form: Form, env: Env },
}

impl DynLike {
    pub fn is_dynamic(&self) -> bool {
        match self {
            DynLike::Atom(_) => false,
            DynLike::Dyn(_) => true,
            DynLike::List(items) => items.iter().any(DynLike::is_dynamic),
            DynLike::Map(kvs) => kvs.iter().any(|(_, v)| v.is_dynamic()),
        }
    }

    pub fn eval(&self, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Val, String> {
        match self {
            DynLike::Atom(v) => Ok(v.to_val()),
            DynLike::Dyn(DynVal::Expr { form, env }) => {
                eval_sig(form, env, sig, tau, 0.0, Some(read_scan(state, 0)), None)
            }
            DynLike::List(items) => items
                .iter()
                .map(|v| v.eval(tau, state, sig))
                .collect::<Result<Vec<_>, _>>()
                .map(Val::arr),
            DynLike::Map(kvs) => kvs
                .iter()
                .map(|(k, v)| Ok((k.to_val(), v.eval(tau, state, sig)?)))
                .collect::<Result<Vec<_>, String>>()
                .map(|pairs| Val::Map(Rc::new(pairs))),
        }
    }

    pub fn from_val(v: Val) -> DynLike {
        match v {
            Val::DynLike(d) => (*d).clone(),
            Val::Arr(items) => DynLike::List(
                items.iter().cloned().map(DynLike::from_val).collect::<Vec<_>>().into(),
            ),
            Val::Map(kvs) => DynLike::Map(Rc::new(
                kvs.iter()
                    .map(|(k, v)| (DataAtom::from_val(k.clone()), DynLike::from_val(v.clone())))
                    .collect(),
            )),
            other => DynLike::Atom(DataAtom::from_val(other)),
        }
    }
}

pub(crate) fn dynlike_arr(v: &DynLike) -> Option<Vec<DynLike>> {
    match v {
        DynLike::List(items) => Some(items.iter().cloned().collect()),
        _ => None,
    }
}

pub(crate) fn as_dynlike_list(v: &DynLike, what: &str) -> Result<Vec<DynLike>, String> {
    dynlike_arr(v).ok_or_else(|| format!("{}: expected array", what))
}

pub(crate) fn dynlike_to_val(v: &DynLike) -> Result<Val, String> {
    if v.is_dynamic() {
        Ok(Val::DynLike(Rc::new(v.clone())))
    } else {
        v.eval(0.0, &MotionState::new(), &SigEnv::default())
    }
}

pub(crate) fn dynlike_meta_pairs(v: &DynLike) -> Result<Vec<(Val, Val)>, String> {
    match v {
        DynLike::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((k.to_val(), dynlike_to_val(v)?)))
            .collect(),
        _ => Err("spawn meta: expected map".into()),
    }
}

pub(crate) fn dynlike_map_get(m: &DynLike, key: &str) -> Option<DynLike> {
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

pub(crate) fn dynlike_kw(v: &DynLike) -> Option<Rc<str>> {
    match v {
        DynLike::Atom(DataAtom::Kw(k)) => Some(k.clone()),
        _ => None,
    }
}

pub(crate) fn as_dyn_num(v: &DynLike) -> Result<DynNum, String> {
    match v {
        DynLike::Dyn(DynVal::Expr { form, env }) => Ok(DynNum::num_expr(form.clone(), env.clone())),
        DynLike::Atom(DataAtom::Num(n)) => Ok(DynNum::num(*n)),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

pub(crate) fn as_static_num(v: &DynLike) -> Result<f64, String> {
    match v {
        DynLike::Atom(DataAtom::Num(n)) => Ok(*n),
        DynLike::Dyn(_) => Err("expected static number".into()),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

pub(crate) fn dynlike_map_as_static_num(
    m: &DynLike,
    key: &str,
    default: f64,
) -> Result<f64, String> {
    match dynlike_map_get(m, key) {
        Some(v) => as_static_num(&v),
        None => Ok(default),
    }
}

pub(crate) fn dynlike_map_as_dyn_num_any(
    m: &DynLike,
    keys: &[&str],
    default: f64,
) -> Result<DynNum, String> {
    for key in keys {
        if let Some(v) = dynlike_map_get(m, key) {
            return as_dyn_num(&v);
        }
    }
    Ok(DynNum::num(default))
}

pub(crate) fn as_sample_set(opts: &DynLike) -> Result<SampleSet, String> {
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

pub(crate) fn as_slot_activity(opts: &DynLike) -> Result<SlotActivity, String> {
    Ok(SlotActivity {
        warn: dynlike_map_as_static_num(opts, "warn", 0.0)?,
        active: dynlike_map_as_static_num(opts, "active", f64::INFINITY)?,
        hot_frac_sig: dynlike_map_get(opts, "fill").map(|v| as_dyn_num(&v)).transpose()?,
    })
}

pub(crate) fn as_capsule_chain_slot(opts: &DynLike) -> Result<CapsuleChainSlot, String> {
    Ok(CapsuleChainSlot {
        sample_set: as_sample_set(opts)?,
        u_max_sig: dynlike_map_get(opts, "u-max").map(|v| as_dyn_num(&v)).transpose()?,
        width: dynlike_map_as_static_num(opts, "width", 1.0)?,
        activity: as_slot_activity(opts)?,
    })
}

pub(crate) fn as_shape_spec(v: &DynLike) -> Result<(Rc<str>, DynLike), String> {
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

pub(crate) fn as_stable_collider_slots(
    v: &DynLike,
    symbols: &mut SymbolTable,
) -> Result<Vec<DynCollider>, String> {
    let mut out = Vec::new();
    as_stable_collider_slots_into(v, symbols, &mut out)?;
    Ok(out)
}

pub(crate) fn as_stable_collider_slots_into(
    v: &DynLike,
    symbols: &mut SymbolTable,
    out: &mut Vec<DynCollider>,
) -> Result<(), String> {
    for v in as_dynlike_list(v, "colliders")? {
        out.push(as_collider(&v, symbols)?);
    }
    Ok(())
}

pub(crate) fn empty_spec_list() -> DynLike {
    DynLike::List(Vec::new().into())
}

pub(crate) fn as_collider_spec_list(
    v: &DynLike,
    symbols: &mut SymbolTable,
) -> Result<ColliderSpecList, String> {
    if !v.is_dynamic() {
        as_stable_collider_slots(v, symbols)?;
    }
    Ok(v.clone())
}

pub(crate) fn as_render(v: &DynLike) -> Result<DynRender, String> {
    if !matches!(v, DynLike::Map(_)) {
        return Err("renderers: expected maps".into());
    }
    let shape_v =
        dynlike_map_get(v, "shape").unwrap_or_else(|| DynLike::Atom(DataAtom::Kw("polyline".into())));
    let (shape, opts) = as_shape_spec(&shape_v)?;
    match shape.as_ref() {
        "polyline" => Ok(DynRender::render_polyline(CurveRenderSlot {
            sample_set: as_sample_set(&opts)?,
            u_max_sig: dynlike_map_get(&opts, "u-max").map(|v| as_dyn_num(&v)).transpose()?,
            width: dynlike_map_as_static_num(&opts, "width", 1.0)?,
            activity: as_slot_activity(&opts)?,
        })),
        _ => Err(format!("renderers: unknown shape :{}", shape)),
    }
}

pub(crate) fn as_stable_render_slots(v: &DynLike) -> Result<Vec<DynRender>, String> {
    let mut out = Vec::new();
    as_stable_render_slots_into(v, &mut out)?;
    Ok(out)
}

pub(crate) fn as_stable_render_slots_into(
    v: &DynLike,
    out: &mut Vec<DynRender>,
) -> Result<(), String> {
    for v in as_dynlike_list(v, "renderers")? {
        out.push(as_render(&v)?);
    }
    Ok(())
}

pub(crate) fn as_render_spec_list(v: &DynLike) -> Result<RenderSpecList, String> {
    if !v.is_dynamic() {
        as_stable_render_slots(v)?;
    }
    Ok(v.clone())
}
