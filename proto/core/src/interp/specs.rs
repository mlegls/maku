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
    if let Some(frac) = dynlike_map_get(opts, "fraction")
        .or_else(|| dynlike_map_get(opts, "frac"))
    {
        return Ok(SlotActivity {
            warn: 0.0,
            active: f64::INFINITY,
            hot_frac_sig: Some(as_dyn_num(&frac)?),
        });
    }
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

pub(crate) fn empty_projector_spec_list() -> DynLike {
    DynLike::List(Vec::new().into())
}

pub(crate) fn as_render(v: &DynLike) -> Result<DynRender, String> {
    if !matches!(v, DynLike::Map(_)) {
        return Err("renderers: expected maps".into());
    }
    let shape_v = dynlike_map_get(v, "shape").unwrap_or_else(|| {
        DynLike::Atom(DataAtom::Kw("polyline".into()))
    });
    let (shape, opts) = as_shape_spec(&shape_v)?;
    match shape.as_ref() {
        "point" | "dot" => Ok(DynRender::render_point(PointRenderSlot {
            facing: match dynlike_map_get(&opts, "facing") {
                Some(DynLike::Atom(DataAtom::Nothing)) | None => None,
                Some(v) => Some(as_dyn_num(&v)?),
            },
            scale: dynlike_map_get(&opts, "scale")
                .map(|v| as_dyn_num(&v))
                .transpose()?
                .unwrap_or_else(|| DynNum::num(1.0)),
            hue: dynlike_map_get(&opts, "hue")
                .map(|v| as_dyn_num(&v))
                .transpose()?
                .unwrap_or_else(|| DynNum::num(0.0)),
            opacity: dynlike_map_get(&opts, "opacity")
                .or_else(|| dynlike_map_get(&opts, "alpha"))
                .map(|v| as_dyn_num(&v))
                .transpose()?
                .unwrap_or_else(|| DynNum::num(1.0)),
        })),
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

pub(crate) fn as_renderer_projector_spec(v: &DynLike) -> Result<RendererProjectorSpec, String> {
    if !v.is_dynamic() {
        as_stable_render_slots(v)?;
    }
    Ok(RendererProjectorSpec::checked(v.clone()))
}
