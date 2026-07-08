//! Source-level projector constructors and combinators.
//!
//! Spawn consumes projector values; it does not own the vocabulary for
//! constructing collider or renderer projectors.

use super::*;
use crate::edn::Form;
use std::rc::Rc;

fn spec_args(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<DynLike, String> {
    match items.len() {
        0 => Ok(empty_projector_spec_list()),
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
    let mut projectors = Vec::new();
    for item in &items[1..] {
        match evaluate(item, env, ctx, world)? {
            Val::ColliderProjector(projector) => {
                projectors.push(projector.as_ref().clone());
            }
            other => {
                return Err(format!("colliders: expected collider projector, got {:?}", other));
            }
        }
    }
    Ok(Val::ColliderProjector(Rc::new(
        ColliderProjectorValue::compose(projectors),
    )))
}

pub(crate) fn sf_circle_collider(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    if items.len() > 2 {
        return Err("circle-collider: expected optional override map".into());
    }
    let opts = if items.len() == 2 {
        eval_dynlike_form(&items[1], env, ctx, world)?
    } else {
        DynLike::Map(Rc::new(Vec::new()))
    };
    if !matches!(opts, DynLike::Map(_)) {
        return Err("circle-collider: expected override map".into());
    }
    let layer = match dynlike_map_get(&opts, "layer").and_then(|v| dynlike_kw(&v)) {
        Some(k) => world.symbols.intern(k.as_ref()),
        None => return Err("circle-collider: missing :layer".into()),
    };
    let radius = dynlike_map_as_dyn_num_any(&opts, &["radius", "r"], 0.08)?;
    Ok(Val::ColliderProjector(Rc::new(ColliderProjectorValue::stable(vec![
        DynCollider::collider_circle(layer, radius),
    ]))))
}

pub(crate) fn sf_capsule_chain_collider(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    if items.len() > 2 {
        return Err("capsule-chain-collider: expected optional override map".into());
    }
    let opts = if items.len() == 2 {
        eval_dynlike_form(&items[1], env, ctx, world)?
    } else {
        DynLike::Map(Rc::new(Vec::new()))
    };
    if !matches!(opts, DynLike::Map(_)) {
        return Err("capsule-chain-collider: expected override map".into());
    }
    let layer = match dynlike_map_get(&opts, "layer").and_then(|v| dynlike_kw(&v)) {
        Some(k) => world.symbols.intern(k.as_ref()),
        None => return Err("capsule-chain-collider: missing :layer".into()),
    };
    let radius = dynlike_map_as_dyn_num_any(&opts, &["radius", "r"], 0.08)?;
    let slot = as_capsule_chain_slot(&opts)?;
    Ok(Val::ColliderProjector(Rc::new(ColliderProjectorValue::stable(vec![
        DynCollider::collider_capsule_chain(layer, radius, slot),
    ]))))
}

pub(crate) fn sf_renderers(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let specs = spec_args(&items[1..], env, ctx, world)?;
    Ok(Val::RendererProjectorSpec(Rc::new(as_renderer_projector_spec(&specs)?)))
}
