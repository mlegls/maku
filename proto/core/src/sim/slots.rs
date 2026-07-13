use super::*;
use super::kernel::{
    self, DynFieldPlan, FallbackPolicy, IterationDomain, KernelLanes, KernelOutputs, KernelPlan,
    KernelScratch, KernelValue, MergePolicy, TickAxisBinding, TickAxisSource,
};

const CURVE_R: f64 = 0.08; // compatibility curve half-width for collision

fn dyn_field_plan(dyn_num: &DynNum, output: ColName, sig: &SigEnv) -> Option<DynFieldPlan> {
    let output_column = u16::try_from(output.0).ok()?;
    let program = dyn_num.lowered_field_program(sig)?;
    let mut value = KernelPlan::new(program, IterationDomain::DynFieldRows);
    value.fallback = FallbackPolicy::WholePlanInterpreted;
    value.merge = MergePolicy::Direct;
    for (input, descriptor) in value.program.inputs.iter().enumerate() {
        let source = match descriptor.source {
            KernelInputSource::Tick => TickAxisSource::Tick,
            KernelInputSource::Axis => TickAxisSource::Axis,
            _ => return None,
        };
        value.bindings.tick_axis.push(TickAxisBinding {
            input: u16::try_from(input).ok()?,
            source,
            ty: descriptor.ty,
        });
    }
    Some(DynFieldPlan {
        value,
        output_column,
    })
}

fn execute_dyn_field_plan(
    plan: &DynFieldPlan,
    tau: f64,
    scratch: &mut KernelScratch,
    outputs: &mut KernelOutputs,
) -> Option<f64> {
    let mut values = Vec::with_capacity(plan.value.program.inputs.len());
    for (input, descriptor) in plan.value.program.inputs.iter().enumerate() {
        let binding = plan.value.bindings.tick_axis.iter().find(|binding| {
            binding.input as usize == input && binding.ty == descriptor.ty
        })?;
        let value = match (descriptor.source, binding.source, binding.ty) {
            (KernelInputSource::Tick, TickAxisSource::Tick, KernelType::F64) => tau,
            (KernelInputSource::Axis, TickAxisSource::Axis, KernelType::F64) => 0.0,
            _ => return None,
        };
        values.push(KernelValue::F64(value));
    }
    let inputs = KernelLanes { lanes: 1, values };
    kernel::execute(&plan.value, &inputs, scratch, outputs).ok()?;
    match outputs.output(0, 0)? {
        KernelValue::F64(value) => Some(value),
        _ => None,
    }
}

/// Refresh fixed-width dynamic numeric columns before collider/render drivers
/// read them. Unsupported variable-shaped forms and any rejected typed plan
/// take the existing semantic evaluator path for the entire field.
pub(super) fn refresh_dyn_field_columns(world: &mut World, sig: &SigEnv) -> Result<(), String> {
    let tick = world.tick;
    let state = MotionState::default();
    let tick_rate = world.tick_rate();
    let oracle = oracle_enabled();
    let mut shared: Option<crate::fxhash::FxHashMap<(usize, usize, u64), Val>> = None;
    let mut scratch = KernelScratch::default();
    let mut outputs = KernelOutputs::default();
    for row in 0..world.entities.len() {
        if !world.entities.is_alive(row) {
            continue;
        }
        let tau = world.entity_tau(row, tick);
        let mut row_sig = None;
        let row_sig = sig.for_row(world.entities.overrides(row), &mut row_sig);
        for (column, dyn_num) in world.entities.dyn_cols(row).iter() {
            let planned = dyn_field_plan(dyn_num, *column, row_sig).and_then(|plan| {
                (u32::from(plan.output_column) == column.0)
                    .then(|| execute_dyn_field_plan(&plan, tau, &mut scratch, &mut outputs))
                    .flatten()
            });
            let value = match planned {
                Some(value) => {
                    if oracle {
                        let expected = eval_dyn_with_tick_rate(
                            dyn_num,
                            tau,
                            &state,
                            row_sig,
                            tick_rate,
                        )
                        .map_err(|error| format!("dyn meta field: {}", error))?;
                        assert_eq!(
                            value, expected,
                            "dyn field projection mismatch for row {row}, column {}",
                            column.0
                        );
                    }
                    value
                }
                None => match dyn_num.repr() {
                    NumDynRepr::AxisSel {
                        form,
                        env,
                        path,
                        flat,
                    } => {
                        let key = (row_sig.overrides.is_none())
                            .then(|| {
                                form_identity(form)
                                    .map(|form| (form, env.identity(), tau.to_bits()))
                            })
                            .flatten();
                        let hit =
                            key.and_then(|key| shared.as_ref().and_then(|m| m.get(&key).cloned()));
                        let value = match hit {
                            Some(value) => Ok(value),
                            None => {
                                let value = eval_sig_at_rate(
                                    form,
                                    env,
                                    row_sig,
                                    tau,
                                    0.0,
                                    None,
                                    None,
                                    tick_rate,
                                );
                                if let (Some(key), Ok(value)) = (key, &value) {
                                    shared
                                        .get_or_insert_with(Default::default)
                                        .insert(key, value.clone());
                                }
                                value
                            }
                        };
                        value.and_then(|value| axis_select_val(&value, path, *flat).num())
                    }
                    _ => eval_dyn_with_tick_rate(dyn_num, tau, &state, row_sig, tick_rate),
                }
                .map_err(|error| format!("dyn meta field: {}", error))?,
            };
            world.col_set_sym_at(row, *column, value);
        }
    }
    Ok(())
}

fn sample_curve_collider_frac(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    projection: &CapsuleChainSlot,
    tick_rate: f64,
) -> Option<Vec<(f64, f64)>> {
    sample_curve_projection(dyn_figure, tau, sig, 1.0, &projection.sample_set, projection.u_max, tick_rate)
}

fn sample_curve_projection(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
    sample_set: &SampleSet,
    u_max: f64,
    tick_rate: f64,
) -> Option<Vec<(f64, f64)>> {
    let state = MotionState::default();
    let Figure::Curve(curve) = eval_dyn_with_tick_rate(dyn_figure, tau, &state, sig, tick_rate).ok()? else {
        return None;
    };
    if frac <= 0.0 {
        return None;
    }
    let us: Vec<f64> = match (&curve.spec.domain, sample_set) {
        (_, SampleSet::Values(vals)) => {
            if vals.is_empty() {
                return None;
            }
            let n = ((vals.len() as f64) * frac.min(1.0)).ceil() as usize;
            vals.iter().take(n.max(2).min(vals.len())).copied().collect()
        }
        (CurveDomain::Values(vals), SampleSet::Step { .. }) => {
            if vals.is_empty() {
                return None;
            }
            let n = ((vals.len() as f64) * frac.min(1.0)).ceil() as usize;
            vals.iter().take(n.max(2).min(vals.len())).copied().collect()
        }
        (CurveDomain::Range { min, max }, SampleSet::Step { resolution }) => {
            let min = *min;
            let max = if u_max.is_finite() { u_max } else { *max };
            let end = min + (max - min) * frac.min(1.0);
            let span = (end - min).abs().max(0.01);
            let steps = ((span / resolution).ceil() as usize).clamp(2, 400);
            (0..=steps).map(|k| min + (end - min) * k as f64 / steps as f64).collect()
        }
    };
    let mut pts = Vec::with_capacity(us.len());
    for u in us {
        let local = eval_curve_pose_with_tick_rate(&curve.spec.eval, tau, u, &state, sig, tick_rate).ok()?;
        let w = curve.frame.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

fn bind_projector_scope(
    env: &Env,
    scope: Option<&ProjectorScope>,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
) -> Env {
    let Some(scope) = scope else {
        return env.clone();
    };
    let e_bound = e_view
        .cloned()
        .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
    let ctx_bound = ctx_view
        .cloned()
        .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
    env.clone()
        .bind(scope.entity.clone(), e_bound)
        .bind(scope.context.clone(), ctx_bound)
}

pub fn materialize_collider_defs_into(
    projector: &ColliderProjector,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
    world: &mut World,
    row: Option<usize>,
    out: &mut Vec<DynCollider>,
    tick_rate: f64,
) -> Result<(), String> {
    for list in projector.projectors.iter() {
        match &list.expr {
            ColliderProjectorExpr::Stable(slots) => {
                out.extend(slots.iter().cloned());
            }
            ColliderProjectorExpr::Circle(spec) => {
                // Bind semantic views only for retained source forms.
                if list.expr.needs_views() {
                    let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                    out.push(materialize_circle_projector(spec, &env, sig, world)?);
                } else {
                    out.push(materialize_circle_projector(spec, &spec.env, sig, world)?);
                }
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                if list.expr.needs_views() {
                    let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                    out.push(materialize_capsule_chain_projector(spec, &env, sig, world)?);
                } else {
                    out.push(materialize_capsule_chain_projector(spec, &spec.env, sig, world)?);
                }
            }
            ColliderProjectorExpr::Callable { params, body, env } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let mut env = env.clone();
                if let Some(param) = params.first() {
                    env = env.bind(param.clone(), e_bound.clone());
                }
                if let Some(param) = params.get(1) {
                    env = env.bind(param.clone(), ctx_bound.clone());
                }
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                run_ctx.projector_scope = match (params.first(), params.get(1)) {
                    (Some(entity), Some(context)) => Some(ProjectorScope {
                        entity: entity.clone(),
                        context: context.clone(),
                        figure: list.figure,
                    }),
                    _ => None,
                };
                let mut run_world = World::for_eval(tick_rate);
                run_world.symbols = world.symbols.clone();
                let mut last = Val::Nothing;
                for form in body.iter() {
                    last = evaluate(form, &env, &mut run_ctx, &mut run_world)?;
                }
                world.symbols = run_world.symbols;
                let specs = flatten_collider_projectors("collider", last, Some(list.figure))?;
                materialize_collider_defs_into(
                    &ColliderProjector { projectors: specs },
                    tau,
                    state,
                    sig,
                    Some(&e_bound),
                    Some(&ctx_bound),
                    world,
                    row,
                    out,
                    tick_rate,
                )?;
            }
            ColliderProjectorExpr::Cond { clauses, env, scope } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let env = bind_projector_scope(env, scope.as_ref(), Some(&e_bound), Some(&ctx_bound));
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                run_ctx.projector_scope = scope.clone();
                let mut run_world = World::for_eval(tick_rate);
                run_world.symbols = world.symbols.clone();
                for (pred, child) in clauses.iter() {
                    let enabled = match pred {
                        Some(pred) => truthy_pub(&evaluate(pred, &env, &mut run_ctx, &mut run_world)?),
                        None => true,
                    };
                    if enabled {
                        world.symbols = run_world.symbols;
                        materialize_collider_defs_into(
                            &ColliderProjector { projectors: child.clone() },
                            tau,
                            state,
                            sig,
                            Some(&e_bound),
                            Some(&ctx_bound),
                            world,
                            row,
                            out,
                            tick_rate,
                        )?;
                        return Ok(());
                    }
                }
                world.symbols = run_world.symbols;
            }
        }
    }
    Ok(())
}

pub fn eval_collider_slot(
    dyn_figure: &DynFigure,
    slot: &DynCollider,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
    tick_rate: f64,
) -> ColliderData {
    match slot.repr() {
        ColliderDynRepr::Slot(projection) => match &projection.shape {
            ColliderSlotShape::Circle { radius } => {
                let state = MotionState::default();
                let radius = eval_dyn_with_tick_rate(radius, tau, &state, sig, tick_rate).unwrap_or(0.0);
                circle_collider_data(dyn_figure, projection.layer, radius, scale, pose, trace, traced)
            }
            ColliderSlotShape::CapsuleChain { radius, slot: curve_slot } => {
                let state = MotionState::default();
                let radius = eval_dyn_with_tick_rate(radius, tau, &state, sig, tick_rate).unwrap_or(0.0);
                capsule_chain_collider_data(
                    dyn_figure, projection.layer, radius, curve_slot, tau, sig, scale, trace, traced,
                    tick_rate,
                )
            }
        },
    }
}

fn circle_collider_data(
    dyn_figure: &DynFigure,
    layer: Symbol,
    radius: f64,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
) -> ColliderData {
    match dyn_figure.repr() {
        FigureDynRepr::Pose(_) if traced => {
            let points: Vec<(f64, f64)> = trace.iter().map(|p| (p.x, p.y)).collect();
            if points.len() < 2 {
                ColliderData::None
            } else {
                ColliderData::CapsuleChain {
                    layer,
                    points,
                    radius: CURVE_R + radius * scale,
                }
            }
        }
        FigureDynRepr::Pose(_) => ColliderData::Circle {
            layer,
            center: (pose.x, pose.y),
            radius: radius * scale,
        },
        FigureDynRepr::Curve { .. } => ColliderData::None,
    }
}

fn capsule_chain_collider_data(
    dyn_figure: &DynFigure,
    layer: Symbol,
    radius: f64,
    curve_slot: &CapsuleChainSlot,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    trace: &[Pose],
    traced: bool,
    tick_rate: f64,
) -> ColliderData {
    match dyn_figure.repr() {
        FigureDynRepr::Pose(_) if traced => {
            let points: Vec<(f64, f64)> = trace.iter().map(|p| (p.x, p.y)).collect();
            if points.len() < 2 {
                ColliderData::None
            } else {
                ColliderData::CapsuleChain {
                    layer,
                    points,
                    radius: CURVE_R * curve_slot.width + radius * scale,
                }
            }
        }
        FigureDynRepr::Curve { .. } => {
            let Some(points) = sample_curve_collider_frac(dyn_figure, tau, sig, curve_slot, tick_rate)
            else {
                return ColliderData::None;
            };
            ColliderData::CapsuleChain {
                layer,
                points,
                radius: CURVE_R * curve_slot.width + radius * scale,
            }
        }
        FigureDynRepr::Pose(_) => ColliderData::None,
    }
}

/// Execute a fully lowered primitive projector. All typed inputs for every
/// scalar are gathered before the first program executes; any missing,
/// mistyped, or unsupported binding rejects the whole projector so the caller
/// can run the semantic source form instead. Geometry remains driver-owned.
pub(super) fn materialize_projected_colliders(
    dyn_figure: &DynFigure,
    projector: &ColliderProjector,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    pose: Pose,
    world: &World,
    row: Option<usize>,
    trace: &[Pose],
    traced: bool,
    out: &mut Vec<ColliderData>,
    tick_rate: f64,
) -> Result<bool, String> {
    fn gather_scalar<'a>(
        scalar: &'a ProjectorScalar,
        row: Option<usize>,
        world: &World,
        gathered: &mut Vec<(&'a kernel::KernelPlan, KernelLanes)>,
    ) -> Option<()> {
        let plan = &scalar.projection.as_ref()?.projection;
        if !plan.supported {
            return None;
        }
        let mut values = Vec::with_capacity(plan.program.inputs.len());
        for (input, descriptor) in plan.program.inputs.iter().enumerate() {
            let KernelInputSource::Direct(column) = descriptor.source else {
                return None;
            };
            let binding = plan.bindings.direct.iter().find(|binding| {
                binding.input as usize == input
                    && binding.column == column
                    && binding.ty == descriptor.ty
            })?;
            if binding.ty != KernelType::F64 {
                return None;
            }
            let row = row?;
            let value = entity_field_sym_at(row, Some(Symbol(binding.column as u32)), world)
                .num()
                .ok()?;
            values.push(KernelValue::F64(value));
        }
        gathered.push((
            plan,
            KernelLanes {
                lanes: 1,
                values,
            },
        ));
        Some(())
    }

    let mut gathered = Vec::new();
    for value in projector.projectors.iter() {
        match &value.expr {
            ColliderProjectorExpr::Circle(spec) => {
                if gather_scalar(&spec.radius, row, world, &mut gathered).is_none() {
                    return Ok(false);
                }
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                if let ProjectorSampleSet::Step(resolution) = &spec.sample_set {
                    if gather_scalar(resolution, row, world, &mut gathered).is_none() {
                        return Ok(false);
                    }
                }
                if let Some(u_max) = &spec.u_max {
                    if gather_scalar(u_max, row, world, &mut gathered).is_none() {
                        return Ok(false);
                    }
                }
                if gather_scalar(&spec.width, row, world, &mut gathered).is_none()
                    || gather_scalar(&spec.radius, row, world, &mut gathered).is_none()
                {
                    return Ok(false);
                }
            }
            ColliderProjectorExpr::Stable(_)
            | ColliderProjectorExpr::Callable { .. }
            | ColliderProjectorExpr::Cond { .. } => return Ok(false),
        }
    }

    let mut projected = Vec::with_capacity(gathered.len());
    let mut scratch = KernelScratch::default();
    let mut outputs = KernelOutputs::default();
    for (plan, inputs) in &gathered {
        if kernel::execute(plan, inputs, &mut scratch, &mut outputs).is_err() {
            return Ok(false);
        }
        let Some(KernelValue::F64(value)) = outputs.output(0, 0) else {
            return Ok(false);
        };
        projected.push(value);
    }

    if oracle_enabled() {
        fn interpreted_scalar(
            scalar: &ProjectorScalar,
            env: &Env,
            sig: &SigEnv,
            world: &World,
            tick_rate: f64,
        ) -> Result<f64, String> {
            match &scalar.source {
                ProjectorScalarSource::Value(value) => Ok(*value),
                ProjectorScalarSource::Form(form) => {
                    let mut ctx = Ctx::default();
                    ctx.sig = sig.clone();
                    let mut eval_world = World::for_eval(tick_rate);
                    eval_world.symbols = world.symbols.clone();
                    evaluate(form, env, &mut ctx, &mut eval_world)?.num()
                }
            }
        }

        let e_view = row.map(|row| entity_view(row, world, sig)).transpose()?;
        let ctx_view = Val::Map(std::rc::Rc::new(vec![
            (Val::Kw("age".into()), Val::Num(tau)),
            (Val::Kw("t".into()), Val::Num(tau)),
            (Val::Kw("tick".into()), Val::Num(world.tick as f64)),
        ]));
        let mut expected = Vec::with_capacity(projected.len());
        for value in projector.projectors.iter() {
            match &value.expr {
                ColliderProjectorExpr::Circle(spec) => {
                    let env = bind_projector_scope(
                        &spec.env,
                        spec.scope.as_ref(),
                        e_view.as_ref(),
                        Some(&ctx_view),
                    );
                    expected.push(interpreted_scalar(
                        &spec.radius,
                        &env,
                        sig,
                        world,
                        tick_rate,
                    )?);
                }
                ColliderProjectorExpr::CapsuleChain(spec) => {
                    let env = bind_projector_scope(
                        &spec.env,
                        spec.scope.as_ref(),
                        e_view.as_ref(),
                        Some(&ctx_view),
                    );
                    if let ProjectorSampleSet::Step(resolution) = &spec.sample_set {
                        expected.push(interpreted_scalar(
                            resolution,
                            &env,
                            sig,
                            world,
                            tick_rate,
                        )?);
                    }
                    if let Some(u_max) = &spec.u_max {
                        expected.push(interpreted_scalar(
                            u_max,
                            &env,
                            sig,
                            world,
                            tick_rate,
                        )?);
                    }
                    expected.push(interpreted_scalar(
                        &spec.width,
                        &env,
                        sig,
                        world,
                        tick_rate,
                    )?);
                    expected.push(interpreted_scalar(
                        &spec.radius,
                        &env,
                        sig,
                        world,
                        tick_rate,
                    )?);
                }
                ColliderProjectorExpr::Stable(_)
                | ColliderProjectorExpr::Callable { .. }
                | ColliderProjectorExpr::Cond { .. } => unreachable!(),
            }
        }
        assert_eq!(projected, expected, "collider fixed projection mismatch");
    }

    let mut projected = projected.into_iter();
    for value in projector.projectors.iter() {
        match &value.expr {
            ColliderProjectorExpr::Circle(spec) => {
                let radius = projected
                    .next()
                    .ok_or("collider: missing projected circle radius")?;
                out.push(circle_collider_data(
                    dyn_figure, spec.layer, radius, scale, pose, trace, traced,
                ));
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                let sample_set = match &spec.sample_set {
                    ProjectorSampleSet::Values(samples) => SampleSet::Values(samples.clone()),
                    ProjectorSampleSet::Step(_) => SampleSet::Step {
                        resolution: projected
                            .next()
                            .ok_or("collider: missing projected capsule resolution")?,
                    },
                };
                let u_max = if spec.u_max.is_some() {
                    projected
                        .next()
                        .ok_or("collider: missing projected capsule u-max")?
                } else {
                    10.0
                };
                let width = projected
                    .next()
                    .ok_or("collider: missing projected capsule width")?;
                let radius = projected
                    .next()
                    .ok_or("collider: missing projected capsule radius")?;
                let slot = CapsuleChainSlot {
                    sample_set,
                    u_max,
                    width,
                };
                out.push(capsule_chain_collider_data(
                    dyn_figure, spec.layer, radius, &slot, tau, sig, scale, trace, traced, tick_rate,
                ));
            }
            ColliderProjectorExpr::Stable(_)
            | ColliderProjectorExpr::Callable { .. }
            | ColliderProjectorExpr::Cond { .. } => {
                return Err("collider: unsupported projector entered projected merge".into());
            }
        }
    }
    Ok(true)
}
