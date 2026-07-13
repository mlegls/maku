use super::*;
use super::kernel::{
    self, FallbackPolicy, IterationDomain, KernelBindings, KernelInputBinding, KernelInputRef,
    KernelInputSource, KernelLanes, KernelOp, KernelOutputBinding, KernelOutputTarget,
    KernelOutputs, KernelPlan, KernelProgram, KernelRegister, KernelScratch, MergePolicy,
};
use super::slots::{
    bind_projector_scope, capsule_chain_collider_data, circle_collider_data, eval_collider_slot,
    materialize_collider_defs_into,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ColliderInput {
    Tick,
    Axis,
    WorldTick,
    Column(Symbol),
}

#[derive(Clone, Copy)]
enum ColliderGatherInput {
    Tick,
    Axis,
    WorldTick,
    Column(FieldSlots),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ColliderProjectionKey {
    program: u64,
    inputs: Vec<ColliderInput>,
    layout: Vec<u64>,
}

#[derive(Clone)]
enum ProjectedSampleSet {
    Values(Rc<[f64]>),
    Step(usize),
}

#[derive(Clone)]
enum ProjectedShape {
    Circle {
        layer: Symbol,
        radius: usize,
    },
    CapsuleChain {
        layer: Symbol,
        sample_set: ProjectedSampleSet,
        u_max: usize,
        width: usize,
        radius: usize,
    },
}
#[derive(Clone, Copy)]
enum ColliderOutputSource {
    Const(f64),
    Input(usize),
}

enum ColliderExecutor {
    Direct(Vec<ColliderOutputSource>),
    Kernel,
}


struct ColliderProjectionPlan {
    key: ColliderProjectionKey,
    kernel: KernelPlan,
    inputs: Vec<ColliderInput>,
    shapes: Vec<ProjectedShape>,
    executor: ColliderExecutor,
    direct_inputs: Vec<ColliderGatherInput>,
    circles_only: bool,
}

struct ColliderGroup {
    plan: Rc<ColliderProjectionPlan>,
    rows: Vec<usize>,
    inputs: Vec<Vec<f64>>,
    gathers: Vec<ColliderGatherInput>,
    outputs: KernelOutputs,
    fallback: bool,
}
struct ColliderProjectorHint {
    source: std::rc::Weak<[ColliderProjectorValue]>,
    plan: Option<Rc<ColliderProjectionPlan>>,
}
fn execute_direct_projection(
    outputs: &[ColliderOutputSource],
    inputs: &[Vec<f64>],
    lanes: usize,
) -> Result<(), ()> {
    if outputs.iter().any(|output| match output {
        ColliderOutputSource::Const(_) => false,
        ColliderOutputSource::Input(input) => inputs
            .get(*input)
            .is_none_or(|values| values.len() != lanes),
    }) {
        return Err(());
    }
    Ok(())
}



pub(super) struct ColliderScratch {
    pub(super) rows: Vec<ColliderData>,
    pub(super) ranges: Vec<std::ops::Range<usize>>,
    defs: Vec<DynCollider>,
    plans: crate::fxhash::FxHashMap<ColliderProjectionKey, Rc<ColliderProjectionPlan>>,
    projector_hints: crate::fxhash::FxHashMap<usize, ColliderProjectorHint>,
    groups: Vec<ColliderGroup>,
    group_index: crate::fxhash::FxHashMap<ColliderProjectionKey, usize>,
    source_groups: crate::fxhash::FxHashMap<usize, usize>,
    last_source_group: Option<(usize, usize)>,
    pool: Vec<ColliderGroup>,
    row_projection: Vec<Option<(usize, usize)>>,
    lanes: KernelLanes,
    exec: KernelScratch,
}

impl Default for ColliderScratch {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            ranges: Vec::new(),
            defs: Vec::new(),
            plans: Default::default(),
            projector_hints: Default::default(),
            groups: Vec::new(),
            group_index: Default::default(),
            source_groups: Default::default(),
            last_source_group: None,
            pool: Vec::new(),
            row_projection: Vec::new(),
            lanes: KernelLanes::default(),
            exec: KernelScratch::default(),
        }
    }
}

impl ColliderScratch {
    fn begin_pass(&mut self, len: usize) {
        self.rows.clear();
        self.ranges.clear();
        self.defs.clear();
        self.group_index.clear();
        self.source_groups.clear();
        self.last_source_group = None;
        self.row_projection.clear();
        self.row_projection.resize(len, None);
        self.pool.extend(self.groups.drain(..).map(|mut group| {
            group.rows.clear();
            for input in &mut group.inputs {
                input.clear();
            }
            group.outputs = KernelOutputs::default();
            group.fallback = false;
            group
        }));
        if self.ranges.capacity() < len {
            self.ranges.reserve_exact(len - self.ranges.capacity());
        }
    }

    fn push_empty(&mut self) {
        let at = self.rows.len();
        self.ranges.push(at..at);
    }

    fn begin_row(&self) -> usize {
        self.rows.len()
    }

    fn finish_row(&mut self, start: usize) {
        self.ranges.push(start..self.rows.len());
    }

    fn projection_plan(
        &mut self,
        projector: &ColliderProjector,
        world: &World,
        sig: &SigEnv,
    ) -> Option<Rc<ColliderProjectionPlan>> {
        let source = Rc::as_ptr(&projector.projectors) as *const () as usize;
        if let Some(hint) = self.projector_hints.get(&source) {
            if hint
                .source
                .upgrade()
                .is_some_and(|cached| Rc::ptr_eq(&cached, &projector.projectors))
            {
                return hint.plan.clone();
            }
        }
        let compiled = compile_projector(projector, world, sig);
        let plan = compiled.map(|plan| {
            if let Some(existing) = self.plans.get(&plan.key) {
                return existing.clone();
            }
            let plan = Rc::new(plan);
            self.plans.insert(plan.key.clone(), plan.clone());
            plan
        });
        self.projector_hints.insert(
            source,
            ColliderProjectorHint {
                source: Rc::downgrade(&projector.projectors),
                plan: plan.clone(),
            },
        );
        plan
    }

    fn push_projected_row(
        &mut self,
        projector: &ColliderProjector,
        row: usize,
        world: &World,
        sig: &SigEnv,
        tau: f64,
    ) {
        let source = Rc::as_ptr(&projector.projectors) as *const () as usize;
        let group_index = match self.last_source_group {
            Some((last_source, index)) if last_source == source => index,
            _ => match self.source_groups.get(&source).copied() {
                Some(index) => index,
                None => {
                let Some(plan) = self.projection_plan(projector, world, sig) else {
                    self.source_groups.insert(source, usize::MAX);
                    return;
                };
                let index = match self.group_index.get(&plan.key).copied() {
                    Some(index) => index,
                    None => {
                        let Some(gathers) = plan
                            .inputs
                            .iter()
                            .copied()
                            .map(|input| match input {
                                ColliderInput::Tick => Some(ColliderGatherInput::Tick),
                                ColliderInput::Axis => Some(ColliderGatherInput::Axis),
                                ColliderInput::WorldTick => Some(ColliderGatherInput::WorldTick),
                                ColliderInput::Column(symbol) => {
                                    let name = world.symbols.resolve(symbol)?;
                                    Some(ColliderGatherInput::Column(world.field_slots(name)))
                                }
                            })
                            .collect::<Option<Vec<_>>>()
                        else {
                            self.source_groups.insert(source, usize::MAX);
                            return;
                        };
                        let mut group = self.pool.pop().unwrap_or_else(|| ColliderGroup {
                            plan: plan.clone(),
                            rows: Vec::new(),
                            inputs: Vec::new(),
                            gathers: Vec::new(),
                            outputs: KernelOutputs::default(),
                            fallback: false,
                        });
                        group.plan = plan.clone();
                        group.gathers = gathers;
                        group.inputs.resize_with(plan.inputs.len(), Vec::new);
                        let index = self.groups.len();
                        self.groups.push(group);
                        self.group_index.insert(plan.key.clone(), index);
                        index
                    }
                };
                self.source_groups.insert(source, index);
                index
            }
            },
        };
        self.last_source_group = Some((source, group_index));
        if group_index == usize::MAX {
            return;
        }
        let group = &mut self.groups[group_index];
        let lane = group.rows.len();
        group.rows.push(row);
        for (column, source) in group.inputs.iter_mut().zip(group.gathers.iter().copied()) {
            let value = match source {
                ColliderGatherInput::Tick => Ok(tau),
                ColliderGatherInput::Axis => Ok(0.0),
                ColliderGatherInput::WorldTick => Ok(world.tick as f64),
                ColliderGatherInput::Column(slots) => entity_field_at_slots(row, slots, world).num(),
            };
            match value {
                Ok(value) => column.push(value),
                Err(_) => {
                    group.fallback = true;
                    column.push(0.0);
                }
            }
        }
        self.row_projection[row] = Some((group_index, lane));
    }

    fn execute_groups(&mut self, world: &World, sig: &SigEnv) -> Result<(), String> {
        let oracle = oracle_enabled();
        for group in &mut self.groups {
            if !group.fallback {
                match &group.plan.executor {
                    ColliderExecutor::Direct(outputs) => {
                        group.fallback = execute_direct_projection(
                            outputs,
                            &group.inputs,
                            group.rows.len(),
                        )
                        .is_err();
                    }
                    ColliderExecutor::Kernel => {
                        self.lanes.lanes = group.rows.len();
                        self.lanes.f32s.clear();
                        self.lanes.f64s.clear();
                        self.lanes.u32s.clear();
                        self.lanes.u64s.clear();
                        self.lanes.symbols.clear();
                        self.lanes.handles.clear();
                        self.lanes.masks.clear();
                        for input in &group.inputs {
                            self.lanes.f64s.extend_from_slice(input);
                        }
                        group.fallback = kernel::execute(
                            &group.plan.kernel,
                            &self.lanes,
                            &mut self.exec,
                            &mut group.outputs,
                        )
                        .is_err();
                    }
                }
            }
            if oracle && !group.fallback {
                let lanes = group.rows.len();
                for (lane, &row) in group.rows.iter().enumerate() {
                    let projector = world
                        .entities
                        .collider_projector(row)
                        .ok_or_else(|| format!("colliders: missing projector for row {row}"))?;
                    let tau = world.entity_motion_tau(row, world.tick);
                    let expected = semantic_projected_values(projector, row, tau, world, sig)?;
                    assert_eq!(expected.len(), group.plan.kernel.program().outputs().len());
                    for (output, expected) in expected.into_iter().enumerate() {
                        let actual = match &group.plan.executor {
                            ColliderExecutor::Direct(outputs) => match outputs.get(output).unwrap() {
                                ColliderOutputSource::Const(value) => *value,
                                ColliderOutputSource::Input(input) => group.inputs[*input][lane],
                            },
                            ColliderExecutor::Kernel => group.outputs.f64s[output * lanes + lane],
                        };
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "collider fixed projection mismatch for row {row}, output {output}"
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

enum OwnedScalar {
    Const(f64),
    Form(Form, Env),
}

fn capture_form(slot: usize) -> Option<Form> {
    Some(Form::call(
        "%capture",
        vec![Form::Num(f64::from(u16::try_from(slot).ok()?))],
    ))
}

fn capture_input(inputs: &mut Vec<ColliderInput>, source: ColliderInput) -> Option<Form> {
    let slot = match inputs.iter().position(|existing| *existing == source) {
        Some(slot) => slot,
        None => {
            inputs.push(source);
            inputs.len() - 1
        }
    };
    capture_form(slot)
}

fn rewrite_projector_form(
    form: &Form,
    scope: Option<&ProjectorScope>,
    world: &World,
    inputs: &mut Vec<ColliderInput>,
) -> Option<Form> {
    if let (Some(scope), Form::List(items)) = (scope, form) {
        if let [Form::Kw(field), Form::Sym(subject)] = &items[..] {
            if *subject == scope.context {
                return match field.as_ref() {
                    "age" | "t" => Some(Form::sym("t")),
                    "tick" => capture_input(inputs, ColliderInput::WorldTick),
                    _ => None,
                };
            }
            if *subject == scope.entity {
                return match field.as_ref() {
                    "t" => Some(Form::sym("t")),
                    "tick" => capture_input(inputs, ColliderInput::WorldTick),
                    "pos" | "vel" | "handle" | "kind" => None,
                    name => capture_input(inputs, ColliderInput::Column(world.symbols.lookup(name)?)),
                };
            }
        }
    }
    match form {
        Form::List(items) => Some(Form::List(
            items
                .iter()
                .map(|item| rewrite_projector_form(item, scope, world, inputs))
                .collect::<Option<Vec<_>>>()?
                .into(),
        )),
        Form::Vector(_) | Form::Map(_) => None,
        _ => Some(form.clone()),
    }
}

fn projector_scalar(
    scalar: &ColliderScalarSource,
    env: &Env,
    scope: Option<&ProjectorScope>,
    world: &World,
    inputs: &mut Vec<ColliderInput>,
) -> Option<OwnedScalar> {
    match scalar {
        ColliderScalarSource::Const(value) => Some(OwnedScalar::Const(*value)),
        ColliderScalarSource::EntityCol(name) => {
            let form = capture_input(inputs, ColliderInput::Column(world.symbols.lookup(name)?))?;
            Some(OwnedScalar::Form(form, Env::empty()))
        }
        ColliderScalarSource::Expr(form) => Some(OwnedScalar::Form(
            rewrite_projector_form(form, scope, world, inputs)?,
            env.clone(),
        )),
    }
}

fn dyn_scalar(value: &DynNum) -> Option<OwnedScalar> {
    match value.repr() {
        NumDynRepr::Const(value) => Some(OwnedScalar::Const(*value)),
        NumDynRepr::Expr { form, env } => Some(OwnedScalar::Form(form.clone(), env.clone())),
        NumDynRepr::AxisSel { .. } => None,
    }
}

fn push_scalar(scalars: &mut Vec<OwnedScalar>, scalar: OwnedScalar) -> usize {
    let output = scalars.len();
    scalars.push(scalar);
    output
}
fn direct_kernel_outputs(program: &KernelProgram) -> Option<Vec<ColliderOutputSource>> {
    let mut registers = vec![None; usize::from(program.registers().f64s)];
    for op in program.ops().iter().copied() {
        match op {
            KernelOp::ConstF64 { dst, bits } => {
                registers[usize::from(dst)] = Some(ColliderOutputSource::Const(f64::from_bits(bits)));
            }
            KernelOp::LoadF64 { dst, input } => {
                registers[usize::from(dst)] =
                    Some(ColliderOutputSource::Input(usize::from(input)));
            }
            _ => return None,
        }
    }
    program
        .outputs()
        .iter()
        .copied()
        .map(|output| match output {
            KernelRegister::F64(register) => registers[usize::from(register)],
            _ => None,
        })
        .collect()
}


fn push_sample_layout(layout: &mut Vec<u64>, samples: &[f64]) {
    layout.push(samples.len() as u64);
    layout.extend(samples.iter().map(|value| value.to_bits()));
}


fn compile_projector(
    projector: &ColliderProjector,
    world: &World,
    sig: &SigEnv,
) -> Option<ColliderProjectionPlan> {
    let mut capture_inputs = Vec::new();
    let mut scalars = Vec::new();
    let mut shapes = Vec::new();
    let mut layout = Vec::new();
    for value in projector.projectors.iter() {
        match &value.expr {
            ColliderProjectorExpr::Stable(slots) => {
                for collider in slots.iter() {
                    let slot = collider.slot();
                    match &slot.shape {
                        ColliderSlotShape::Circle { radius } => {
                            let radius = push_scalar(&mut scalars, dyn_scalar(radius)?);
                            layout.extend([1, u64::from(slot.layer.0)]);
                            shapes.push(ProjectedShape::Circle {
                                layer: slot.layer,
                                radius,
                            });
                        }
                        ColliderSlotShape::CapsuleChain { radius, slot: capsule } => {
                            layout.extend([2, u64::from(slot.layer.0)]);
                            let sample_set = match &capsule.sample_set {
                                SampleSet::Values(samples) => {
                                    layout.push(1);
                                    push_sample_layout(&mut layout, samples);
                                    ProjectedSampleSet::Values(samples.clone())
                                }
                                SampleSet::Step { resolution } => {
                                    layout.push(2);
                                    ProjectedSampleSet::Step(push_scalar(
                                        &mut scalars,
                                        OwnedScalar::Const(*resolution),
                                    ))
                                }
                            };
                            let u_max = push_scalar(&mut scalars, OwnedScalar::Const(capsule.u_max));
                            let width = push_scalar(&mut scalars, OwnedScalar::Const(capsule.width));
                            let radius = push_scalar(&mut scalars, dyn_scalar(radius)?);
                            shapes.push(ProjectedShape::CapsuleChain {
                                layer: slot.layer,
                                sample_set,
                                u_max,
                                width,
                                radius,
                            });
                        }
                    }
                }
            }
            ColliderProjectorExpr::Circle(spec) => {
                let radius = push_scalar(
                    &mut scalars,
                    projector_scalar(&spec.radius, &spec.env, spec.scope.as_ref(), world, &mut capture_inputs)?,
                );
                layout.extend([1, u64::from(spec.layer.0)]);
                shapes.push(ProjectedShape::Circle {
                    layer: spec.layer,
                    radius,
                });
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                layout.extend([2, u64::from(spec.layer.0)]);
                let sample_set = match &spec.sample_set {
                    ProjectorSampleSet::Values(samples) => {
                        layout.push(1);
                        push_sample_layout(&mut layout, samples);
                        ProjectedSampleSet::Values(samples.clone())
                    }
                    ProjectorSampleSet::Step(resolution) => {
                        layout.push(2);
                        ProjectedSampleSet::Step(push_scalar(
                            &mut scalars,
                            projector_scalar(resolution, &spec.env, spec.scope.as_ref(), world, &mut capture_inputs)?,
                        ))
                    }
                };
                let u_max = push_scalar(
                    &mut scalars,
                    match &spec.u_max {
                        Some(value) => projector_scalar(value, &spec.env, spec.scope.as_ref(), world, &mut capture_inputs)?,
                        None => OwnedScalar::Const(10.0),
                    },
                );
                let width = push_scalar(
                    &mut scalars,
                    projector_scalar(&spec.width, &spec.env, spec.scope.as_ref(), world, &mut capture_inputs)?,
                );
                let radius = push_scalar(
                    &mut scalars,
                    projector_scalar(&spec.radius, &spec.env, spec.scope.as_ref(), world, &mut capture_inputs)?,
                );
                shapes.push(ProjectedShape::CapsuleChain {
                    layer: spec.layer,
                    sample_set,
                    u_max,
                    width,
                    radius,
                });
            }
            ColliderProjectorExpr::Callable { .. } | ColliderProjectorExpr::Cond { .. } => return None,
        }
    }
    let fixed_scalars = scalars
        .iter()
        .map(|scalar| match scalar {
            OwnedScalar::Const(value) => FixedScalar::Const(*value),
            OwnedScalar::Form(form, env) => FixedScalar::Form(form, env),
        })
        .collect::<Vec<_>>();
    let fixed = lower_fixed_scalars(&fixed_scalars, &sig.defs)?;
    let inputs = fixed
        .inputs
        .iter()
        .copied()
        .map(|input| match input {
            FixedInput::Tick => Some(ColliderInput::Tick),
            FixedInput::Axis => Some(ColliderInput::Axis),
            FixedInput::Slot(slot) => capture_inputs.get(usize::from(slot)).copied(),
        })
        .collect::<Option<Vec<_>>>()?;
    let direct_inputs = inputs
        .iter()
        .copied()
        .map(|input| match input {
            ColliderInput::Tick => Some(ColliderGatherInput::Tick),
            ColliderInput::Axis => Some(ColliderGatherInput::Axis),
            ColliderInput::WorldTick => Some(ColliderGatherInput::WorldTick),
            ColliderInput::Column(symbol) => {
                let name = world.symbols.resolve(symbol)?;
                Some(ColliderGatherInput::Column(world.field_slots(name)))
            }
        })
        .collect::<Option<Vec<_>>>()?;
    let executor = direct_kernel_outputs(&fixed.program)
        .map(ColliderExecutor::Direct)
        .unwrap_or(ColliderExecutor::Kernel);
    let input_bindings = inputs
        .iter()
        .copied()
        .enumerate()
        .map(|(index, source)| {
            let source = match source {
                ColliderInput::Tick => KernelInputSource::Tick,
                ColliderInput::Axis => KernelInputSource::Axis,
                ColliderInput::WorldTick => KernelInputSource::Capture { slot: 0 },
                ColliderInput::Column(column) => KernelInputSource::Direct {
                    column: u16::try_from(column.0).ok()?,
                },
            };
            Some(KernelInputBinding {
                input: KernelInputRef::F64(u16::try_from(index).ok()?),
                source,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let output_bindings = fixed
        .program
        .outputs()
        .iter()
        .enumerate()
        .map(|(output, _)| {
            Some(KernelOutputBinding {
                output: u16::try_from(output).ok()?,
                target: KernelOutputTarget::Driver,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let kernel = KernelPlan::new(
        fixed.program.clone(),
        IterationDomain::ColliderRows,
        KernelBindings {
            inputs: input_bindings,
            outputs: output_bindings,
        },
        FallbackPolicy::WholePlanInterpreted,
        MergePolicy::DriverOwned,
    )
    .ok()?;
    let circles_only = shapes
        .iter()
        .all(|shape| matches!(shape, ProjectedShape::Circle { .. }));
    Some(ColliderProjectionPlan {
        key: ColliderProjectionKey {
            program: fixed.program.id().0,
            inputs: inputs.clone(),
            layout,
        },
        kernel,
        inputs,
        shapes,
        direct_inputs,
        executor,
        circles_only,
    })
}

fn eval_projector_num(
    scalar: &ColliderScalarSource,
    env: &Env,
    sig: &SigEnv,
    world: &World,
    row: usize,
) -> Result<f64, String> {
    match scalar {
        ColliderScalarSource::Const(value) => Ok(*value),
        ColliderScalarSource::EntityCol(name) => entity_field_at(row, name, world, sig)?.num(),
        ColliderScalarSource::Expr(form) => {
            let mut ctx = Ctx::default();
            ctx.sig = sig.clone();
            let mut eval_world = World::with_entity_capacity(0);
            eval_world.symbols = world.symbols.clone();
            evaluate(form, env, &mut ctx, &mut eval_world)?.num()
        }
    }
}

fn semantic_projected_values(
    projector: &ColliderProjector,
    row: usize,
    tau: f64,
    world: &World,
    sig: &SigEnv,
) -> Result<Vec<f64>, String> {
    let mut values = Vec::new();
    let state = MotionState::default();
    let e_view = projector
        .needs_views()
        .then(|| entity_view(row, world, sig))
        .transpose()?;
    let ctx_view = Val::Map(Rc::new(vec![
        (Val::Kw("age".into()), Val::Num(tau)),
        (Val::Kw("t".into()), Val::Num(tau)),
        (Val::Kw("tick".into()), Val::Num(world.tick as f64)),
    ]));
    for value in projector.projectors.iter() {
        match &value.expr {
            ColliderProjectorExpr::Stable(slots) => {
                for collider in slots.iter() {
                    let slot = collider.slot();
                    match &slot.shape {
                        ColliderSlotShape::Circle { radius } => values.push(eval_dyn_with_tick_rate(
                            radius,
                            tau,
                            &state,
                            sig,
                            world.tick_rate(),
                        )?),
                        ColliderSlotShape::CapsuleChain { radius, slot } => {
                            if let SampleSet::Step { resolution } = slot.sample_set {
                                values.push(resolution);
                            }
                            values.push(slot.u_max);
                            values.push(slot.width);
                            values.push(eval_dyn_with_tick_rate(
                                radius,
                                tau,
                                &state,
                                sig,
                                world.tick_rate(),
                            )?);
                        }
                    }
                }
            }
            ColliderProjectorExpr::Circle(spec) => {
                let env = bind_projector_scope(
                    &spec.env,
                    spec.scope.as_ref(),
                    e_view.as_ref(),
                    Some(&ctx_view),
                );
                values.push(eval_projector_num(&spec.radius, &env, sig, world, row)?);
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                let env = bind_projector_scope(
                    &spec.env,
                    spec.scope.as_ref(),
                    e_view.as_ref(),
                    Some(&ctx_view),
                );
                if let ProjectorSampleSet::Step(resolution) = &spec.sample_set {
                    values.push(eval_projector_num(resolution, &env, sig, world, row)?);
                }
                values.push(match &spec.u_max {
                    Some(value) => eval_projector_num(value, &env, sig, world, row)?,
                    None => 10.0,
                });
                values.push(eval_projector_num(&spec.width, &env, sig, world, row)?);
                values.push(eval_projector_num(&spec.radius, &env, sig, world, row)?);
            }
            ColliderProjectorExpr::Callable { .. } | ColliderProjectorExpr::Cond { .. } => {
                return Err("collider: unsupported projector entered fixed oracle".into());
            }
        }
    }
    Ok(values)
}

fn project_colliders(
    group: &ColliderGroup,
    lane: usize,
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
    tick_rate: f64,
    mut emit: impl FnMut(ColliderData),
) {
    let plan = &group.plan;
    let value = |output: usize| match &plan.executor {
        ColliderExecutor::Direct(outputs) => match outputs.get(output).unwrap() {
            ColliderOutputSource::Const(value) => *value,
            ColliderOutputSource::Input(input) => group.inputs[*input][lane],
        },
        ColliderExecutor::Kernel => group.outputs.f64s[output * group.outputs.lanes + lane],
    };
    if plan.circles_only
        && !traced
        && matches!(dyn_figure.repr(), FigureDynRepr::Pose(_))
    {
        for shape in &plan.shapes {
            let ProjectedShape::Circle { layer, radius } = shape else {
                unreachable!()
            };
            emit(ColliderData::Circle {
                layer: *layer,
                center: (pose.x, pose.y),
                radius: value(*radius) * scale,
            });
        }
        return;
    }
    for shape in &plan.shapes {
        match shape {
            ProjectedShape::Circle { layer, radius } => emit(circle_collider_data(
                dyn_figure,
                *layer,
                value(*radius),
                scale,
                pose,
                trace,
                traced,
            )),
            ProjectedShape::CapsuleChain {
                layer,
                sample_set,
                u_max,
                width,
                radius,
            } => {
                let sample_set = match sample_set {
                    ProjectedSampleSet::Values(samples) => SampleSet::Values(samples.clone()),
                    ProjectedSampleSet::Step(output) => SampleSet::Step {
                        resolution: value(*output),
                    },
                };
                let slot = CapsuleChainSlot {
                    sample_set,
                    u_max: value(*u_max),
                    width: value(*width),
                };
                emit(capsule_chain_collider_data(
                    dyn_figure,
                    *layer,
                    value(*radius),
                    &slot,
                    tau,
                    sig,
                    scale,
                    trace,
                    traced,
                    tick_rate,
                ));
            }
        }
    }
}

fn materialize_colliders_into(
    dyn_figure: &DynFigure,
    projector: &ColliderProjector,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    pose: Pose,
    world: &mut World,
    row: usize,
    defs: &mut Vec<DynCollider>,
    out: &mut Vec<ColliderData>,
    tick_rate: f64,
) -> Result<(), String> {
    let (e_view, ctx_view) = if projector.needs_views() {
        (
            Some(entity_view(row, world, sig)?),
            Some(Val::Map(Rc::new(vec![
                (Val::Kw("age".into()), Val::Num(tau)),
                (Val::Kw("t".into()), Val::Num(tau)),
                (Val::Kw("tick".into()), Val::Num(world.tick as f64)),
            ]))),
        )
    } else {
        (None, None)
    };
    defs.clear();
    materialize_collider_defs_into(
        projector,
        tau,
        &MotionState::default(),
        sig,
        e_view.as_ref(),
        ctx_view.as_ref(),
        world,
        Some(row),
        defs,
        tick_rate,
    )
    .map_err(|error| format!("colliders: {error}"))?;
    let trace = world.entities.trace_samples(row);
    let traced = world.entities.is_traced(row);
    out.extend(defs.drain(..).map(|slot| {
        eval_collider_slot(
            dyn_figure,
            &slot,
            tau,
            sig,
            scale,
            pose,
            trace,
            traced,
            tick_rate,
        )
    }));
    Ok(())
}

impl Sim {
    /// Collision pass: batch fixed collider projections, scatter geometry in
    /// entity-row order, then hand the completed rows to the contact index.
    fn execute_direct_circle_batch(
        &mut self,
        tick: u64,
        sig: &SigEnv,
        closed_any: bool,
    ) -> Result<Option<Vec<bool>>, String> {
        let n = self.world.entities.len();
        self.collider_scratch.begin_pass(n);
        let scale_sym = self.world.symbols.lookup("scale");
        let mut eligible = Vec::with_capacity(n);
        for row in 0..n {
            if !self.world.entities.is_alive(row) {
                self.world.entities.set_sampled_pose(row, tick, None);
                self.collider_scratch.push_empty();
                eligible.push(false);
                continue;
            }
            let tau = self.world.entity_motion_tau(row, tick);
            let pose = if let Some(pose) = if closed_any { self.closed_pose_at(row) } else { None } {
                pose
            } else if let Some(pose) = self.fast_pos_pose(row, tau, sig) {
                pose
            } else {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(row)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {row}"))?;
                let readers = self.motion_readers(row);
                let mut row_sig = None;
                let row_sig = sig.for_row(self.world.entities.overrides(row), &mut row_sig);
                dyn_figure_pose_in(
                    dyn_figure,
                    tau,
                    MotionEvalCtx::with_tick_rate(
                        &MotionState::default(),
                        row_sig,
                        &readers,
                        self.world.tick_rate(),
                    )
                    .pos_only(),
                )?
            };
            self.world.entities.set_sampled_pose(row, tick, Some(pose));
            let scale = scale_sym
                .and_then(|symbol| self.world.col_get_sym_at(row, symbol))
                .unwrap_or(1.0);
            let projector = self
                .world
                .entities
                .collider_projector(row)
                .ok_or_else(|| format!("colliders: missing projector for row {row}"))?
                .clone();
            let Some(plan) = self
                .collider_scratch
                .projection_plan(&projector, &self.world, sig)
            else {
                return Ok(None);
            };
            if !plan.circles_only
                || self.world.entities.is_traced(row)
                || !self
                    .world
                    .entities
                    .dyn_figure(row)
                    .is_some_and(|figure| matches!(figure.repr(), FigureDynRepr::Pose(_)))
            {
                return Ok(None);
            }
            let ColliderExecutor::Direct(outputs) = &plan.executor else {
                return Ok(None);
            };
            let start = self.collider_scratch.begin_row();
            for shape in &plan.shapes {
                let ProjectedShape::Circle { layer, radius } = shape else {
                    unreachable!()
                };
                let radius = match &outputs[*radius] {
                    ColliderOutputSource::Const(value) => *value,
                    ColliderOutputSource::Input(input) => match plan.direct_inputs[*input] {
                        ColliderGatherInput::Tick => tau,
                        ColliderGatherInput::Axis => 0.0,
                        ColliderGatherInput::WorldTick => self.world.tick as f64,
                        ColliderGatherInput::Column(slots) => {
                            entity_field_at_slots(row, slots, &self.world)
                                .num()
                                .map_err(|error| format!("colliders: {error}"))?
                        }
                    },
                };
                self.collider_scratch.rows.push(ColliderData::Circle {
                    layer: *layer,
                    center: (pose.x, pose.y),
                    radius: radius * scale,
                });
            }
            self.collider_scratch.finish_row(start);
            eligible.push(true);
        }
        Ok(Some(eligible))
    }

    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        self.fill_closed_poses(tick, &sig)?;
        let closed_any = self.has_closed_poses();
        if !oracle_enabled() {
            if let Some(eligible) = self.execute_direct_circle_batch(tick, &sig, closed_any)? {
                if let Some(file) = probe {
                    crate::interp::profile::close("phase:collide-mat", file);
                }
                let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
                self.world.collision_index.capture(
                    &mut self.collider_scratch.rows,
                    &mut self.collider_scratch.ranges,
                    eligible,
                );
                if let Some(file) = probe {
                    crate::interp::profile::close("phase:collide-index", file);
                }
                return Ok(());
            }
        }
        let n = self.world.entities.len();
        let mut poses = Vec::with_capacity(n);
        let mut scales = Vec::with_capacity(n);
        self.collider_scratch.begin_pass(n);
        let scale_sym = self.world.symbols.lookup("scale");

        for row in 0..n {
            if !self.world.entities.is_alive(row) {
                self.world.entities.set_sampled_pose(row, tick, None);
                poses.push(None);
                scales.push(1.0);
                continue;
            }
            let tau = self.world.entity_motion_tau(row, tick);
            let pose = if let Some(pose) = if closed_any { self.closed_pose_at(row) } else { None } {
                pose
            } else if let Some(pose) = self.fast_pos_pose(row, tau, &sig) {
                pose
            } else {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(row)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {row}"))?;
                let readers = self.motion_readers(row);
                let mut row_sig = None;
                let row_sig = sig.for_row(self.world.entities.overrides(row), &mut row_sig);
                dyn_figure_pose_in(
                    dyn_figure,
                    tau,
                    MotionEvalCtx::with_tick_rate(
                        &MotionState::default(),
                        row_sig,
                        &readers,
                        self.world.tick_rate(),
                    )
                    .pos_only(),
                )?
            };
            self.world.entities.set_sampled_pose(row, tick, Some(pose));
            poses.push(Some(pose));
            let scale = scale_sym
                .and_then(|symbol| self.world.col_get_sym_at(row, symbol))
                .unwrap_or(1.0);
            scales.push(scale);
            let projector = self
                .world
                .entities
                .collider_projector(row)
                .ok_or_else(|| format!("colliders: missing projector for row {row}"))?;
            self.collider_scratch
                .push_projected_row(projector, row, &self.world, &sig, tau);
        }
        self.collider_scratch.execute_groups(&self.world, &sig)?;

        let mut groups = std::mem::take(&mut self.collider_scratch.groups);
        let alive = poses.iter().filter(|pose| pose.is_some()).count();
        let projected = groups.iter().map(|group| group.rows.len()).sum::<usize>() == alive
            && groups.iter().all(|group| !group.fallback);
        let ordered = groups
            .windows(2)
            .all(|pair| pair[0].rows.last().zip(pair[1].rows.first()).is_none_or(|(a, b)| a < b));
        if projected && ordered {
            let mut next_row = 0;
            for group in &groups {
                for (lane, &row) in group.rows.iter().enumerate() {
                    while next_row < row {
                        self.collider_scratch.push_empty();
                        next_row += 1;
                    }
                    let pose = poses[row].expect("projected collider row must have a pose");
                    let tau = self.world.entity_motion_tau(row, tick);
                    let dyn_figure = self
                        .world
                        .entities
                        .dyn_figure(row)
                        .ok_or_else(|| format!("colliders: missing dyn figure for row {row}"))?;
                    let trace = self.world.entities.trace_samples(row);
                    let traced = self.world.entities.is_traced(row);
                    let start = self.collider_scratch.begin_row();
                    project_colliders(
                        group,
                        lane,
                        dyn_figure,
                        tau,
                        &sig,
                        scales[row],
                        pose,
                        trace,
                        traced,
                        self.world.tick_rate(),
                        |collider| self.collider_scratch.rows.push(collider),
                    );
                    self.collider_scratch.finish_row(start);
                    next_row = row + 1;
                }
            }
            while next_row < n {
                self.collider_scratch.push_empty();
                next_row += 1;
            }
        } else {
            for row in 0..n {
                let Some(pose) = poses[row] else {
                    self.collider_scratch.push_empty();
                    continue;
                };
                let tau = self.world.entity_motion_tau(row, tick);
                let start = self.collider_scratch.begin_row();
                let tick_rate = self.world.tick_rate();
                match self.collider_scratch.row_projection[row] {
                    Some((group, lane)) if !groups[group].fallback => {
                        let dyn_figure = self
                            .world
                            .entities
                            .dyn_figure(row)
                            .ok_or_else(|| format!("colliders: missing dyn figure for row {row}"))?;
                        let trace = self.world.entities.trace_samples(row);
                        let traced = self.world.entities.is_traced(row);
                        project_colliders(
                            &groups[group],
                            lane,
                            dyn_figure,
                            tau,
                            &sig,
                            scales[row],
                            pose,
                            trace,
                            traced,
                            tick_rate,
                            |collider| self.collider_scratch.rows.push(collider),
                        );
                    }
                    _ => {
                        let dyn_figure = self
                            .world
                            .entities
                            .dyn_figure(row)
                            .ok_or_else(|| format!("colliders: missing dyn figure for row {row}"))?
                            .clone();
                        let projector = self
                            .world
                            .entities
                            .collider_projector(row)
                            .ok_or_else(|| format!("colliders: missing projector for row {row}"))?
                            .clone();
                        materialize_colliders_into(
                            &dyn_figure,
                            &projector,
                            tau,
                            &sig,
                            scales[row],
                            pose,
                            &mut self.world,
                            row,
                            &mut self.collider_scratch.defs,
                            &mut self.collider_scratch.rows,
                            tick_rate,
                        )?;
                    }
                }
                self.collider_scratch.finish_row(start);
            }
        }
        self.collider_scratch.groups = std::mem::take(&mut groups);

        if let Some(file) = probe {
            crate::interp::profile::close("phase:collide-mat", file);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        let eligible = (0..n)
            .map(|row| self.world.entities.is_alive(row) && poses[row].is_some())
            .collect::<Vec<_>>();
        self.world.collision_index.capture(
            &mut self.collider_scratch.rows,
            &mut self.collider_scratch.ranges,
            eligible,
        );
        if let Some(file) = probe {
            crate::interp::profile::close("phase:collide-index", file);
        }
        Ok(())
    }
}
