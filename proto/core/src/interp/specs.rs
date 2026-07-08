//! Source-level collider/render spec interpretation.
//!
//! These are still interpreter-side schema checks over `DynLike`; the emitted
//! runtime rows live in `model`, and the built-in projector cases live in the
//! collider/render modules.

use super::*;
use std::rc::Rc;

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
        DynLike::Atom(DynAtom::Data(DataAtom::Kw(shape))) => {
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
    let shape_v = dynlike_map_get(v, "shape").unwrap_or_else(|| {
        DynLike::Atom(DynAtom::Data(DataAtom::Kw("polyline".into())))
    });
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
