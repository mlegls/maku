use super::*;
use super::kernel::{
    self, FallbackPolicy, IterationDomain, KernelBindings, KernelInputBinding, KernelInputRef,
    KernelInputSource, KernelLanes, KernelOutputBinding, KernelOutputTarget, KernelOutputs,
    KernelPlan, KernelScratch, MergePolicy,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DynFieldPlanKey {
    program: u64,
    inputs: Vec<FixedInput>,
    output: ColName,
}

struct DynFieldPlan {
    key: DynFieldPlanKey,
    kernel: KernelPlan,
    inputs: Vec<FixedInput>,
    output: ColName,
}

struct DynFieldGroup {
    plan: Rc<DynFieldPlan>,
    rows: Vec<usize>,
    tau: Vec<f64>,
}
struct DynSourcePlans {
    source: std::rc::Weak<[(ColName, DynNum)]>,
    plans: Vec<Option<Rc<DynFieldPlan>>>,
}


#[derive(Default)]
pub(super) struct DynFieldScratch {
    plans: crate::fxhash::FxHashMap<DynFieldPlanKey, Rc<DynFieldPlan>>,
    groups: Vec<DynFieldGroup>,
    index: crate::fxhash::FxHashMap<DynFieldPlanKey, usize>,
    pool: Vec<DynFieldGroup>,
    sources: crate::fxhash::FxHashMap<usize, DynSourcePlans>,
    lanes: KernelLanes,
    outputs: KernelOutputs,
    exec: KernelScratch,
}

impl DynFieldScratch {
    fn begin_pass(&mut self) {
        self.index.clear();
        self.pool.extend(self.groups.drain(..).map(|mut group| {
            group.rows.clear();
            group.tau.clear();
            group
        }));
    }

    fn plan(&mut self, fixed: FixedKernel, output: ColName) -> Option<Rc<DynFieldPlan>> {
        let key = DynFieldPlanKey {
            program: fixed.program.id().0,
            inputs: fixed.inputs.clone(),
            output,
        };
        if let Some(plan) = self.plans.get(&key) {
            return Some(plan.clone());
        }
        let output_column = u16::try_from(output.0).ok()?;
        let inputs = fixed
            .inputs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, source)| {
                let source = match source {
                    FixedInput::Tick => KernelInputSource::Tick,
                    FixedInput::Axis => KernelInputSource::Axis,
                    FixedInput::Slot(slot) => KernelInputSource::Capture { slot },
                };
                Some(KernelInputBinding {
                    input: KernelInputRef::F64(u16::try_from(index).ok()?),
                    source,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let bindings = KernelBindings {
            inputs,
            outputs: vec![KernelOutputBinding {
                output: 0,
                target: KernelOutputTarget::Column {
                    column: output_column,
                },
            }],
        };
        let kernel = KernelPlan::new(
            fixed.program,
            IterationDomain::DynFieldRows,
            bindings,
            FallbackPolicy::WholePlanInterpreted,
            MergePolicy::Direct,
        )
        .ok()?;
        let plan = Rc::new(DynFieldPlan {
            key: key.clone(),
            kernel,
            inputs: fixed.inputs,
            output,
        });
        self.plans.insert(key, plan.clone());
        Some(plan)
    }

    fn push(&mut self, plan: Rc<DynFieldPlan>, row: usize, tau: f64) {
        let index = match self.index.get(&plan.key).copied() {
            Some(index) => index,
            None => {
                let mut group = self.pool.pop().unwrap_or_else(|| DynFieldGroup {
                    plan: plan.clone(),
                    rows: Vec::new(),
                    tau: Vec::new(),
                });
                group.plan = plan.clone();
                let index = self.groups.len();
                self.groups.push(group);
                self.index.insert(plan.key.clone(), index);
                index
            }
        };
        let group = &mut self.groups[index];
        group.rows.push(row);
        group.tau.push(tau);
    }
}

fn semantic_dyn_field(
    world: &World,
    sig: &SigEnv,
    row: usize,
    dyn_num: &DynNum,
    tau: f64,
    shared: &mut Option<crate::fxhash::FxHashMap<(usize, usize, u64), Val>>,
) -> Result<f64, String> {
    let state = MotionState::default();
    let tick_rate = world.tick_rate();
    let mut row_sig = None;
    let row_sig = sig.for_row(world.entities.overrides(row), &mut row_sig);
    match dyn_num.repr() {
        NumDynRepr::AxisSel {
            form,
            env,
            path,
            flat,
        } => {
            let key = (row_sig.overrides.is_none())
                .then(|| form_identity(form).map(|form| (form, env.identity(), tau.to_bits())))
                .flatten();
            let hit = key.and_then(|key| shared.as_ref().and_then(|values| values.get(&key).cloned()));
            let value = match hit {
                Some(value) => Ok(value),
                None => {
                    let value =
                        eval_sig_at_rate(form, env, row_sig, tau, 0.0, None, None, tick_rate);
                    if let (Some(key), Ok(value)) = (key, &value) {
                        shared.get_or_insert_with(Default::default).insert(key, value.clone());
                    }
                    value
                }
            };
            value.and_then(|value| axis_select_val(&value, path, *flat).num())
        }
        _ => eval_dyn_with_tick_rate(dyn_num, tau, &state, row_sig, tick_rate),
    }
    .map_err(|error| format!("dyn meta field: {}", error))
}

pub(super) fn refresh_dyn_field_columns(
    world: &mut World,
    sig: &SigEnv,
    scratch: &mut DynFieldScratch,
) -> Result<(), String> {
    scratch.begin_pass();
    let tick = world.tick;
    let oracle = oracle_enabled();
    let mut shared = None;
    let mut writes = Vec::new();

    for row in 0..world.entities.len() {
        if !world.entities.is_alive(row) {
            continue;
        }
        let tau = world.entity_tau(row, tick);
        let dyn_cols = world.entities.dyn_cols(row);
        if dyn_cols.is_empty() {
            continue;
        }
        let source = Rc::as_ptr(&dyn_cols) as *const () as usize;
        let cached = scratch
            .sources
            .get(&source)
            .and_then(|cached| cached.source.upgrade())
            .is_some_and(|cached| Rc::ptr_eq(&cached, &dyn_cols));
        if !cached {
            let plans = dyn_cols
                .iter()
                .map(|(output, dyn_num)| {
                    let scalar = match dyn_num.repr() {
                        NumDynRepr::Const(value) => FixedScalar::Const(*value),
                        NumDynRepr::Expr { form, env } => FixedScalar::Form(form, env),
                        NumDynRepr::AxisSel { .. } => return None,
                    };
                    let fixed = lower_fixed_scalars(&[scalar], &sig.defs)?;
                    scratch.plan(fixed, *output)
                })
                .collect();
            scratch.sources.insert(
                source,
                DynSourcePlans {
                    source: Rc::downgrade(&dyn_cols),
                    plans,
                },
            );
        }
        for (index, (output, dyn_num)) in dyn_cols.iter().enumerate() {
            match scratch
                .sources
                .get(&source)
                .and_then(|cached| cached.plans.get(index))
                .and_then(Clone::clone)
            {
                Some(plan) => scratch.push(plan, row, tau),
                None => writes.push((
                    row,
                    *output,
                    semantic_dyn_field(world, sig, row, dyn_num, tau, &mut shared)?,
                )),
            }
        }
    }

    for group in &mut scratch.groups {
        scratch.lanes.lanes = group.rows.len();
        scratch.lanes.f32s.clear();
        scratch.lanes.f64s.clear();
        scratch.lanes.u32s.clear();
        scratch.lanes.u64s.clear();
        scratch.lanes.symbols.clear();
        scratch.lanes.handles.clear();
        scratch.lanes.masks.clear();
        for input in group.plan.inputs.iter().copied() {
            match input {
                FixedInput::Tick => scratch.lanes.f64s.extend_from_slice(&group.tau),
                FixedInput::Axis => scratch
                    .lanes
                    .f64s
                    .extend(std::iter::repeat_n(0.0, group.rows.len())),
                FixedInput::Slot(_) => {
                    return Err("dyn meta field: unexpected capture binding".into());
                }
            }
        }
        let executed = kernel::execute(
            &group.plan.kernel,
            &scratch.lanes,
            &mut scratch.exec,
            &mut scratch.outputs,
        )
        .is_ok();
        if !executed {
            let mut fallback = Vec::with_capacity(group.rows.len());
            for (&row, &tau) in group.rows.iter().zip(&group.tau) {
                let dyn_cols = world.entities.dyn_cols(row);
                let dyn_num = dyn_cols
                    .iter()
                    .find_map(|(column, value)| (*column == group.plan.output).then_some(value))
                    .ok_or("dyn meta field: missing fallback column")?;
                fallback.push(semantic_dyn_field(
                    world,
                    sig,
                    row,
                    dyn_num,
                    tau,
                    &mut shared,
                )?);
            }
            writes.extend(
                group
                    .rows
                    .iter()
                    .copied()
                    .zip(fallback)
                    .map(|(row, value)| (row, group.plan.output, value)),
            );
            continue;
        }
        if oracle {
            for (lane, (&row, &tau)) in group.rows.iter().zip(&group.tau).enumerate() {
                let dyn_cols = world.entities.dyn_cols(row);
                let dyn_num = dyn_cols
                    .iter()
                    .find_map(|(column, value)| (*column == group.plan.output).then_some(value))
                    .ok_or("dyn meta field: missing oracle column")?;
                let expected =
                    semantic_dyn_field(world, sig, row, dyn_num, tau, &mut shared)?;
                let actual = scratch.outputs.f64s[lane];
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "dyn field projection mismatch for row {row}, column {}",
                    group.plan.output.0
                );
            }
        }
        writes.extend(
            group
                .rows
                .iter()
                .copied()
                .zip(scratch.outputs.f64s.iter().copied())
                .map(|(row, value)| (row, group.plan.output, value)),
        );
    }

    for (row, column, value) in writes {
        world.col_set_sym_at(row, column, value);
    }
    Ok(())
}

const CURVE_R: f64 = 0.08; // compatibility curve half-width for collision

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

pub(super) fn bind_projector_scope(
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
                // the env only feeds Expr slots; Const/EntityCol never read
                // it, so skip the scope binding (env clone + two view maps)
                if list.expr.needs_views() {
                    let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                    out.push(materialize_circle_projector(spec, &env, sig, world, row)?);
                } else {
                    out.push(materialize_circle_projector(spec, &spec.env, sig, world, row)?);
                }
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                if list.expr.needs_views() {
                    let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                    out.push(materialize_capsule_chain_projector(spec, &env, sig, world, row)?);
                } else {
                    out.push(materialize_capsule_chain_projector(spec, &spec.env, sig, world, row)?);
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

pub(super) fn circle_collider_data(
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

pub(super) fn capsule_chain_collider_data(
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

