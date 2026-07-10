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

pub(crate) fn contains_bound_projector_context(form: &Form, scope: Option<&ProjectorScope>) -> bool {
    fn sym_matches(sym: &Rc<str>, name: &str) -> bool {
        sym.as_ref() == name
            || sym
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('.'))
    }
    let Some(scope) = scope else {
        return false;
    };
    match form {
        Form::Sym(s) => {
            sym_matches(s, scope.entity.as_ref())
                || sym_matches(s, scope.context.as_ref())
        }
        Form::List(items) | Form::Vector(items) => items
            .iter()
            .any(|item| contains_bound_projector_context(item, Some(scope))),
        Form::Map(kvs) => kvs
            .iter()
            .any(|(k, v)| {
                contains_bound_projector_context(k, Some(scope))
                    || contains_bound_projector_context(v, Some(scope))
            }),
        _ => false,
    }
}

pub(crate) fn contains_legacy_projector_context(form: &Form) -> bool {
    contains_bound_projector_context(
        form,
        Some(&ProjectorScope {
            entity: "e".into(),
            context: "ctx".into(),
            figure: FigureProjectorKind::Pose,
        }),
    )
}

fn opts_form(items: &[Form], name: &str) -> Result<Option<Form>, String> {
    if items.len() > 2 {
        return Err(format!("{}: expected optional override map", name));
    }
    Ok(items.get(1).cloned())
}

fn eval_opts(
    name: &str,
    opts: Option<&Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<DynLike, String> {
    let opts = match opts {
        Some(form) => eval_dynlike_form(form, env, ctx, world)?,
        None => DynLike::Map(Rc::new(Vec::new())),
    };
    if !matches!(opts, DynLike::Map(_)) {
        return Err(format!("{}: expected override map", name));
    }
    Ok(opts)
}

fn circle_projector_from_opts(opts: &DynLike, symbols: &mut SymbolTable) -> Result<DynCollider, String> {
    let layer = match dynlike_map_get(opts, "layer").and_then(|v| dynlike_kw(&v)) {
        Some(k) => symbols.intern(k.as_ref()),
        None => return Err("circle-collider: missing :layer".into()),
    };
    let radius = dynlike_map_as_dyn_num_any(opts, &["radius", "r"], 0.08)?;
    Ok(DynCollider::collider_circle(layer, radius))
}

fn static_kw_field(
    name: &str,
    opts: &Option<Form>,
    key: &str,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Rc<str>, String> {
    let Some(Form::Map(kvs)) = opts else {
        return Err(format!("{}: expected override map", name));
    };
    for (k, v) in kvs.iter() {
        let Form::Kw(kw) = k else {
            continue;
        };
        if &**kw != key {
            continue;
        }
        return match evaluate(v, env, ctx, world)? {
            Val::Kw(k) => Ok(k),
            other => Err(format!("{}: expected keyword for :{}, got {:?}", name, key, other)),
        };
    }
    Err(format!("{}: missing :{}", name, key))
}

fn form_map_value<'a>(opts: &'a Option<Form>, keys: &[&str]) -> Option<&'a Form> {
    let Some(Form::Map(kvs)) = opts else {
        return None;
    };
    for (k, v) in kvs.iter() {
        let Form::Kw(kw) = k else {
            continue;
        };
        if keys.iter().any(|key| *key == &**kw) {
            return Some(v);
        }
    }
    None
}

fn circle_projector_spec_from_form(
    opts: Option<Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<CircleProjectorSpec, String> {
    if opts.as_ref().is_some_and(|form| !matches!(form, Form::Map(_))) {
        return Err("circle-collider: expected override map".into());
    }
    let layer = static_kw_field("circle-collider", &opts, "layer", env, ctx, world)
        .map(|k| world.symbols.intern(k.as_ref()))?;
    let radius = match form_map_value(&opts, &["radius", "r"]) {
        Some(form) if contains_bound_projector_context(form, ctx.projector_scope.as_ref()) => {
            ProjectorNum::Expr(form.clone())
        }
        Some(form) => {
            let radius = evaluate(form, env, ctx, world)?.num()?;
            ProjectorNum::Const(radius)
        }
        None => ProjectorNum::Const(0.08),
    };
    Ok(CircleProjectorSpec {
        layer,
        radius,
        env: env.clone(),
        scope: ctx.projector_scope.clone(),
    })
}

fn projector_num_from_form(
    name: &str,
    opts: &Option<Form>,
    keys: &[&str],
    default: f64,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<ProjectorNum, String> {
    match form_map_value(opts, keys) {
        Some(form) if contains_bound_projector_context(form, ctx.projector_scope.as_ref()) => {
            Ok(ProjectorNum::Expr(form.clone()))
        }
        Some(form) => {
            evaluate(form, env, ctx, world)
                .and_then(|v| v.num())
                .map(ProjectorNum::Const)
                .map_err(|e| format!("{}: {}", name, e))
        }
        None => Ok(ProjectorNum::Const(default)),
    }
}

fn optional_projector_num_from_form(
    name: &str,
    opts: &Option<Form>,
    keys: &[&str],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Option<ProjectorNum>, String> {
    match form_map_value(opts, keys) {
        Some(form) if contains_bound_projector_context(form, ctx.projector_scope.as_ref()) => {
            Ok(Some(ProjectorNum::Expr(form.clone())))
        }
        Some(form) => {
            evaluate(form, env, ctx, world)
                .and_then(|v| v.num())
                .map(ProjectorNum::Const)
                .map(Some)
                .map_err(|e| format!("{}: {}", name, e))
        }
        None => Ok(None),
    }
}

fn static_samples_from_form(opts: &Option<Form>, env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Option<Rc<[f64]>>, String> {
    let Some(form) = form_map_value(opts, &["samples"]) else {
        return Ok(None);
    };
    let val = evaluate(form, env, ctx, world)?;
    let Val::Arr(items) = val else {
        return Err("capsule-chain-collider: :samples expects an array".into());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        out.push(item.num()?);
    }
    Ok(Some(out.into()))
}

fn capsule_chain_projector_spec_from_form(
    opts: Option<Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<CapsuleChainProjectorSpec, String> {
    if opts.as_ref().is_some_and(|form| !matches!(form, Form::Map(_))) {
        return Err("capsule-chain-collider: expected override map".into());
    }
    for key in ["warn", "active", "fill", "fraction", "frac"] {
        if form_map_value(&opts, &[key]).is_some() {
            return Err(format!(
                "capsule-chain-collider: :{} is lifecycle policy; put lifecycle logic in a defcollider body over entity fields",
                key
            ));
        }
    }
    let layer = static_kw_field("capsule-chain-collider", &opts, "layer", env, ctx, world)
        .map(|k| world.symbols.intern(k.as_ref()))?;
    let sample_set = match static_samples_from_form(&opts, env, ctx, world)? {
        Some(samples) => ProjectorSampleSet::Values(samples),
        None => ProjectorSampleSet::Step(projector_num_from_form(
            "capsule-chain-collider",
            &opts,
            &["resolution"],
            0.1,
            env,
            ctx,
            world,
        )?),
    };
    Ok(CapsuleChainProjectorSpec {
        layer,
        radius: projector_num_from_form("capsule-chain-collider", &opts, &["radius", "r"], 0.08, env, ctx, world)?,
        sample_set,
        u_max: optional_projector_num_from_form("capsule-chain-collider", &opts, &["u-max"], env, ctx, world)?,
        width: projector_num_from_form("capsule-chain-collider", &opts, &["width"], 0.0, env, ctx, world)?,
        env: env.clone(),
        scope: ctx.projector_scope.clone(),
    })
}

fn capsule_chain_projector_from_opts(opts: &DynLike, symbols: &mut SymbolTable) -> Result<DynCollider, String> {
    let layer = match dynlike_map_get(opts, "layer").and_then(|v| dynlike_kw(&v)) {
        Some(k) => symbols.intern(k.as_ref()),
        None => return Err("capsule-chain-collider: missing :layer".into()),
    };
    let radius = dynlike_map_as_dyn_num_any(opts, &["radius", "r"], 0.08)?;
    let slot = as_capsule_chain_slot(opts)?;
    Ok(DynCollider::collider_capsule_chain(layer, radius, slot))
}

pub(crate) fn materialize_circle_projector(
    spec: &CircleProjectorSpec,
    env: &Env,
    sig: &SigEnv,
) -> Result<DynCollider, String> {
    let mut run_ctx = Ctx::default();
    run_ctx.sig = sig.clone();
    let mut run_world = World::default();
    let radius = match &spec.radius {
        ProjectorNum::Const(n) => *n,
        ProjectorNum::Expr(form) => {
            evaluate(form, env, &mut run_ctx, &mut run_world)?.num()?
        }
    };
    Ok(DynCollider::collider_circle_const(spec.layer, radius))
}

pub(crate) fn materialize_capsule_chain_projector(
    spec: &CapsuleChainProjectorSpec,
    env: &Env,
    sig: &SigEnv,
    symbols: &mut SymbolTable,
) -> Result<DynCollider, String> {
    let mut run_ctx = Ctx::default();
    run_ctx.sig = sig.clone();
    let mut run_world = World::default();
    run_world.symbols = symbols.clone();
    let mut eval_num = |n: &ProjectorNum| -> Result<f64, String> {
        match n {
            ProjectorNum::Const(n) => Ok(*n),
            ProjectorNum::Expr(form) => evaluate(form, env, &mut run_ctx, &mut run_world)?.num(),
        }
    };
    let sample_set = match &spec.sample_set {
        ProjectorSampleSet::Values(samples) => SampleSet::Values(samples.clone()),
        ProjectorSampleSet::Step(resolution) => SampleSet::Step {
            resolution: eval_num(resolution)?,
        },
    };
    let slot = CapsuleChainSlot {
        sample_set,
        u_max: spec.u_max.as_ref().map(&mut eval_num).transpose()?.unwrap_or(10.0),
        width: eval_num(&spec.width)?,
    };
    let slot = DynCollider::collider_capsule_chain(spec.layer, DynNum::num(eval_num(&spec.radius)?), slot);
    *symbols = run_world.symbols;
    Ok(slot)
}

pub(crate) fn sf_circle_collider(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let opts_form = opts_form(items, "circle-collider")?;
    if ctx
        .projector_scope
        .as_ref()
        .is_some_and(|scope| scope.figure != FigureProjectorKind::Pose)
    {
        return Err("circle-collider: requires a :pose projector scope".into());
    }
    if ctx.projector_scope.is_none()
        && opts_form.as_ref().is_some_and(contains_legacy_projector_context)
    {
        return Err("circle-collider: entity/context overrides require a projector scope".into());
    }
    if opts_form
        .as_ref()
        .is_some_and(|form| contains_bound_projector_context(form, ctx.projector_scope.as_ref()))
    {
        let spec = circle_projector_spec_from_form(opts_form, env, ctx, world)?;
        return Ok(collider_projector_value(ColliderProjectorValue::circle(spec)));
    }
    let opts = eval_opts("circle-collider", opts_form.as_ref(), env, ctx, world)?;
    let slot = circle_projector_from_opts(&opts, &mut world.symbols)?;
    Ok(collider_projector_value(ColliderProjectorValue::stable(vec![
        slot,
    ])))
}

pub(crate) fn sf_capsule_chain_collider(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let opts_form = opts_form(items, "capsule-chain-collider")?;
    if ctx.projector_scope.is_none()
        && opts_form.as_ref().is_some_and(contains_legacy_projector_context)
    {
        return Err("capsule-chain-collider: entity/context overrides require a projector scope".into());
    }
    if let Some(scope) = ctx.projector_scope.clone() {
        let spec = capsule_chain_projector_spec_from_form(opts_form, env, ctx, world)?;
        return Ok(collider_projector_value(ColliderProjectorValue::capsule_chain(
            scope.figure,
            spec,
        )));
    }
    if opts_form
        .as_ref()
        .is_some_and(|form| contains_bound_projector_context(form, ctx.projector_scope.as_ref()))
    {
        let spec = capsule_chain_projector_spec_from_form(opts_form, env, ctx, world)?;
        return Ok(collider_projector_value(ColliderProjectorValue::capsule_chain(
            FigureProjectorKind::Pose,
            spec,
        )));
    }
    let opts = eval_opts("capsule-chain-collider", opts_form.as_ref(), env, ctx, world)?;
    let slot = capsule_chain_projector_from_opts(&opts, &mut world.symbols)?;
    Ok(collider_projector_value(ColliderProjectorValue::stable(vec![
        slot,
    ])))
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
