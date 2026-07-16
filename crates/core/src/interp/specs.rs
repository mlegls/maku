//! Source-level collider/render spec interpretation.
//!
//! These are still interpreter-side schema checks over `DynLike`; the emitted
//! runtime rows live in `model`, and the built-in projector cases live in the
//! collider/render modules.

use super::*;

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

fn reject_lifecycle_keys(kind: &str, opts: &DynLike, keys: &[&str]) -> Result<(), String> {
    for key in keys {
        if dynlike_map_get(opts, key).is_some() {
            return Err(format!(
                "{}: :{} is lifecycle policy; put lifecycle logic in a defcollider body or render rule over entity fields",
                kind,
                key
            ));
        }
    }
    Ok(())
}

pub(crate) fn as_capsule_chain_slot(opts: &DynLike) -> Result<CapsuleChainSlot, String> {
    reject_lifecycle_keys("capsule-chain-collider", opts, &["warn", "active", "fill", "fraction", "frac"])?;
    Ok(CapsuleChainSlot {
        sample_set: as_sample_set(opts)?,
        u_max: dynlike_map_as_static_num(opts, "u-max", 10.0)?,
        width: dynlike_map_as_static_num(opts, "width", 1.0)?,
    })
}
