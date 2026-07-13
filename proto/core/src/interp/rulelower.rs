use super::engine::RenderKey;
use super::{
    evaluate, row_predicate, Ctx, Env, KernelInput, KernelInputSource, KernelOp, KernelOutput,
    KernelProgram, KernelType, World,
};
use crate::edn::Form;
use crate::sim::kernel::{
    CaptureBinding, DirectInputBinding, FallbackPolicy, FilterPlan, IterationDomain,
    KernelBindings, KernelPlan, MaskedUpdatePlan, MergePolicy, OutputBinding,
    PoseComponent, PoseInputBinding, PresenceBinding, RenderProjectionPlan,
};
use std::rc::Rc;

pub(crate) struct CompiledTickForm {
    pub filter: FilterPlan,
    pub action: CompiledTickAction,
}

pub(crate) enum CompiledTickAction {
    Render(CompiledRender),
    /// The map body is exactly `(cull e)` over the row param: scan, then
    /// cull matched rows in row order after the complete predicate pass.
    Cull,
    Update {
        plan: MaskedUpdatePlan,
        column: super::Symbol,
        kind: UpdateValueKind,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum UpdateValueKind {
    Num,
    Sym,
}

pub(crate) struct CompiledRender {
    pub needs_pose: bool,
    pub kind: Rc<str>,
    pub fields: Vec<ProjectedRenderField>,
    pub plan: RenderProjectionPlan,
    /// Memoized batch schema: rebuilt only when observed column kinds
    /// change (stable once dynamic-kind fields settle).
    pub schema: std::cell::RefCell<Option<Rc<crate::model::RenderSchema>>>,
}

pub(crate) struct ProjectedRenderField {
    pub key: Rc<str>,
    pub slot: RenderKey,
    pub value: ProjectedValue,
}

#[derive(Clone, Copy)]
pub(crate) enum ProjectedValue {
    Num { value: u16, present: u16 },
    Sym { value: u16, present: u16 },
    Dynamic {
        num: u16,
        num_present: u16,
        sym: u16,
        sym_present: u16,
    },
}

enum RenderExpr {
    Num(f64),
    Kw(Rc<str>),
    PoseX,
    PoseY,
    PoseTheta,
    Field(Rc<str>),
    FieldOr(Rc<str>, Box<RenderExpr>),
}

#[derive(Default)]
struct RenderBuilder {
    ops: Vec<KernelOp>,
    register_types: Vec<KernelType>,
    inputs: Vec<KernelInput>,
    outputs: Vec<KernelOutput>,
    bindings: KernelBindings,
}

impl RenderBuilder {
    fn reg(&mut self, ty: KernelType) -> Option<u16> {
        let register = u16::try_from(self.register_types.len()).ok()?;
        self.register_types.push(ty);
        Some(register)
    }

    fn push(&mut self, ty: KernelType, op: impl FnOnce(u16) -> KernelOp) -> Option<u16> {
        let dst = self.reg(ty)?;
        self.ops.push(op(dst));
        Some(dst)
    }

    fn output(&mut self, register: u16, ty: KernelType, column: u16) -> Option<u16> {
        let output = u16::try_from(self.outputs.len()).ok()?;
        self.outputs.push(KernelOutput { register, ty });
        self.bindings.outputs.push(OutputBinding { output, column, ty });
        Some(output)
    }

    fn present_output(&mut self, register: u16, column: u16) -> Option<u16> {
        let output = self.output(register, KernelType::Mask, column)?;
        self.bindings.presence.push(PresenceBinding { output, column });
        Some(output)
    }

    fn input(&mut self, source: KernelInputSource, ty: KernelType) -> Option<u16> {
        let input = u16::try_from(self.inputs.len()).ok()?;
        self.inputs.push(KernelInput { source, ty });
        Some(input)
    }

    fn constant_mask(&mut self, value: bool) -> Option<u16> {
        self.push(KernelType::Mask, |dst| KernelOp::ConstMask { dst, v: value })
    }

    fn direct(&mut self, column: usize, ty: KernelType) -> Option<u16> {
        let column = u16::try_from(column).ok()?;
        let input = self.input(KernelInputSource::Direct(column), ty)?;
        self.bindings.direct.push(DirectInputBinding { input, column, ty });
        self.push(ty, |dst| KernelOp::Load { dst, input })
    }

    fn pose(&mut self, component: PoseComponent, ty: KernelType) -> Option<u16> {
        let source = match component {
            PoseComponent::X => KernelInputSource::PositionX,
            PoseComponent::Y => KernelInputSource::PositionY,
            PoseComponent::Theta => KernelInputSource::State(0),
            PoseComponent::HasTheta => KernelInputSource::State(1),
        };
        let input = self.input(source, ty)?;
        self.bindings.pose.push(PoseInputBinding { input, component, ty });
        self.push(ty, |dst| KernelOp::Load { dst, input })
    }

    fn lower(&mut self, expr: &RenderExpr, world: &mut World, column: u16) -> Option<ProjectedValue> {
        match expr {
            RenderExpr::Num(value) => {
                let value = self.push(KernelType::F64, |dst| KernelOp::Const { dst, v: *value })?;
                let present = self.constant_mask(true)?;
                Some(ProjectedValue::Num {
                    value: self.output(value, KernelType::F64, column)?,
                    present: self.present_output(present, column)?,
                })
            }
            RenderExpr::Kw(value) => {
                let symbol = world.field_sym(value);
                let slot = u16::try_from(symbol.0).ok()?;
                let input = self.input(KernelInputSource::Capture(slot), KernelType::Symbol)?;
                self.bindings.captures.push(CaptureBinding {
                    input,
                    slot,
                    ty: KernelType::Symbol,
                });
                let value = self.push(KernelType::Symbol, |dst| KernelOp::Load { dst, input })?;
                let present = self.constant_mask(true)?;
                Some(ProjectedValue::Sym {
                    value: self.output(value, KernelType::Symbol, column)?,
                    present: self.present_output(present, column)?,
                })
            }
            RenderExpr::PoseX | RenderExpr::PoseY => {
                let component = if matches!(expr, RenderExpr::PoseX) {
                    PoseComponent::X
                } else {
                    PoseComponent::Y
                };
                let value = self.pose(component, KernelType::F64)?;
                let present = self.constant_mask(true)?;
                Some(ProjectedValue::Num {
                    value: self.output(value, KernelType::F64, column)?,
                    present: self.present_output(present, column)?,
                })
            }
            RenderExpr::PoseTheta => {
                let theta = self.pose(PoseComponent::Theta, KernelType::F64)?;
                let has_theta = self.pose(PoseComponent::HasTheta, KernelType::Mask)?;
                let zero = self.push(KernelType::F64, |dst| KernelOp::Const { dst, v: 0.0 })?;
                let value = self.push(KernelType::F64, |dst| KernelOp::SelectF64 {
                    dst,
                    mask: has_theta,
                    yes: theta,
                    no: zero,
                })?;
                let present = self.constant_mask(true)?;
                Some(ProjectedValue::Num {
                    value: self.output(value, KernelType::F64, column)?,
                    present: self.present_output(present, column)?,
                })
            }
            RenderExpr::Field(name) | RenderExpr::FieldOr(name, _) => {
                let field = world.field_sym(name);
                let num_slot = world.intern_col_slot(field);
                let sym_slot = world.intern_sym_field_slot(field);
                let num = self.direct(num_slot, KernelType::F64)?;
                let num_present = self.direct(num_slot, KernelType::Mask)?;
                let sym = self.direct(sym_slot, KernelType::Symbol)?;
                let sym_present = self.direct(sym_slot, KernelType::Mask)?;
                let (num, num_present, sym, sym_present) = match expr {
                    RenderExpr::Field(_) => (num, num_present, sym, sym_present),
                    RenderExpr::FieldOr(_, default) => {
                        let default = self.lower_register(default, world)?;
                        match default {
                            RegisterValue::Num { value, present } => {
                                let selected = self.push(KernelType::F64, |dst| KernelOp::SelectF64 {
                                    dst,
                                    mask: num_present,
                                    yes: num,
                                    no: value,
                                })?;
                                let absent_sym = self.push(KernelType::Mask, |dst| KernelOp::MaskNot {
                                    dst,
                                    x: sym_present,
                                })?;
                                let selected_present = self.push(KernelType::Mask, |dst| KernelOp::MaskAnd {
                                    dst,
                                    a: present,
                                    b: absent_sym,
                                })?;
                                let present = self.push(KernelType::Mask, |dst| KernelOp::MaskOr {
                                    dst,
                                    a: num_present,
                                    b: selected_present,
                                })?;
                                (selected, present, sym, sym_present)
                            }
                            RegisterValue::Sym { value, present } => {
                                let selected = self.push(KernelType::Symbol, |dst| KernelOp::SelectSymbol {
                                    dst,
                                    mask: sym_present,
                                    yes: sym,
                                    no: value,
                                })?;
                                let absent_num = self.push(KernelType::Mask, |dst| KernelOp::MaskNot {
                                    dst,
                                    x: num_present,
                                })?;
                                let selected_present = self.push(KernelType::Mask, |dst| KernelOp::MaskAnd {
                                    dst,
                                    a: present,
                                    b: absent_num,
                                })?;
                                let present = self.push(KernelType::Mask, |dst| KernelOp::MaskOr {
                                    dst,
                                    a: sym_present,
                                    b: selected_present,
                                })?;
                                (num, num_present, selected, present)
                            }
                        }
                    }
                    _ => unreachable!(),
                };
                Some(ProjectedValue::Dynamic {
                    num: self.output(num, KernelType::F64, column)?,
                    num_present: self.present_output(num_present, column)?,
                    sym: self.output(sym, KernelType::Symbol, column)?,
                    sym_present: self.present_output(sym_present, column)?,
                })
            }
        }
    }

    fn lower_register(&mut self, expr: &RenderExpr, world: &mut World) -> Option<RegisterValue> {
        match expr {
            RenderExpr::Num(value) => Some(RegisterValue::Num {
                value: self.push(KernelType::F64, |dst| KernelOp::Const { dst, v: *value })?,
                present: self.constant_mask(true)?,
            }),
            RenderExpr::Kw(value) => {
                let symbol = world.field_sym(value);
                let slot = u16::try_from(symbol.0).ok()?;
                let input = self.input(KernelInputSource::Capture(slot), KernelType::Symbol)?;
                self.bindings.captures.push(CaptureBinding {
                    input,
                    slot,
                    ty: KernelType::Symbol,
                });
                Some(RegisterValue::Sym {
                    value: self.push(KernelType::Symbol, |dst| KernelOp::Load { dst, input })?,
                    present: self.constant_mask(true)?,
                })
            }
            RenderExpr::PoseX | RenderExpr::PoseY | RenderExpr::PoseTheta => {
                let component = match expr {
                    RenderExpr::PoseX => PoseComponent::X,
                    RenderExpr::PoseY => PoseComponent::Y,
                    RenderExpr::PoseTheta => PoseComponent::Theta,
                    _ => unreachable!(),
                };
                let raw = self.pose(component, KernelType::F64)?;
                let value = if matches!(expr, RenderExpr::PoseTheta) {
                    let has = self.pose(PoseComponent::HasTheta, KernelType::Mask)?;
                    let zero = self.push(KernelType::F64, |dst| KernelOp::Const { dst, v: 0.0 })?;
                    self.push(KernelType::F64, |dst| KernelOp::SelectF64 {
                        dst,
                        mask: has,
                        yes: raw,
                        no: zero,
                    })?
                } else {
                    raw
                };
                Some(RegisterValue::Num { value, present: self.constant_mask(true)? })
            }
            RenderExpr::Field(_) | RenderExpr::FieldOr(_, _) => None,
        }
    }
}

enum RegisterValue {
    Num { value: u16, present: u16 },
    Sym { value: u16, present: u16 },
}

const HEADS: [&str; 6] = ["map", "entities-where", "emit", "let", "%value-or", "fn"];

fn unshadowed(name: &str, env: &Env, ctx: &Ctx) -> bool {
    env.lookup(name).is_none() && !ctx.sig.defs.contains_key(name)
}

fn sym(form: &Form) -> Option<&str> {
    match form { Form::Sym(s) => Some(s), _ => None }
}

fn call<'a>(form: &'a Form, head: &str, env: &Env, ctx: &Ctx) -> Option<&'a [Form]> {
    let Form::List(items) = form else { return None };
    if sym(items.first()?)? != head || !unshadowed(head, env, ctx) { return None; }
    Some(&items[1..])
}

fn access(form: &Form, subject: &str) -> Option<Rc<str>> {
    let Form::List(items) = form else { return None };
    let [Form::Kw(field), Form::Sym(target)] = &items[..] else { return None };
    (target.as_ref() == subject).then(|| field.clone())
}

fn render_expr(
    form: &Form,
    entity: &str,
    pose: Option<&str>,
    env: &Env,
    ctx: &Ctx,
) -> Option<RenderExpr> {
    match form {
        Form::Num(n) => Some(RenderExpr::Num(*n)),
        Form::Kw(k) => Some(RenderExpr::Kw(k.clone())),
        _ => {
            if let Some(pose) = pose {
                if let Some(field) = access(form, pose) {
                    return match field.as_ref() {
                        "x" => Some(RenderExpr::PoseX),
                        "y" => Some(RenderExpr::PoseY),
                        "th" => Some(RenderExpr::PoseTheta),
                        _ => None,
                    };
                }
            }
            if let Some(field) = access(form, entity) {
                return (!matches!(field.as_ref(), "pos" | "vel" | "t" | "tick" | "handle" | "kind"))
                    .then(|| RenderExpr::Field(field));
            }
            let args = call(form, "%value-or", env, ctx)?;
            let [value, default] = args else { return None };
            let RenderExpr::Field(field) = render_expr(value, entity, pose, env, ctx)? else {
                return None;
            };
            let default = render_expr(default, entity, pose, env, ctx)?;
            if !matches!(
                default,
                RenderExpr::Num(_)
                    | RenderExpr::Kw(_)
                    | RenderExpr::PoseX
                    | RenderExpr::PoseY
                    | RenderExpr::PoseTheta
            ) {
                return None;
            }
            Some(RenderExpr::FieldOr(field, Box::new(default)))
        }
    }
}

fn reads_pose(expr: &RenderExpr) -> bool {
    match expr {
        RenderExpr::PoseX | RenderExpr::PoseY | RenderExpr::PoseTheta => true,
        RenderExpr::FieldOr(_, default) => reads_pose(default),
        _ => false,
    }
}

pub(crate) fn lower_tick_form(form: &Form, env: &Env, ctx: &mut Ctx, world: &mut World) -> Option<CompiledTickForm> {
    let args = call(form, "map", env, ctx)?;
    let [fnform, query] = args else { return None };
    let query_args = call(query, "entities-where", env, ctx)?;
    let [predform] = query_args else { return None };
    let pred_args = call(predform, "fn", env, ctx)?;
    if pred_args.len() < 2 || !matches!(&pred_args[0], Form::Vector(_)) { return None; }
    let filter = row_predicate(&evaluate(predform, env, ctx, world).ok()?, ctx)?.lower(world)?;

    let fn_args = call(fnform, "fn", env, ctx)?;
    let [Form::Vector(params), body] = fn_args else { return None };
    let [Form::Sym(entity)] = &params[..] else { return None };
    if matches!(entity.as_ref(), "&" | "*" | "=") || HEADS.contains(&entity.as_ref()) { return None; }

    // the cull-rule shape: body is exactly (cull e); anything else about
    // the body (extra args, a shadowing param named cull) bails the form
    if let Some(args) = call(body, "cull", env, ctx) {
        return (entity.as_ref() != "cull"
            && matches!(args, [Form::Sym(target)] if target == entity))
            .then(|| CompiledTickForm { filter, action: CompiledTickAction::Cull });
    }
    // Exact fixed-width update: `(change-col e :field (fn [_] literal))`.
    // The value program is pure; the driver queues its outputs in matched
    // row order, preserving the ordinary next-tick pending-write boundary.
    if let Some(args) = call(body, "change-col", env, ctx) {
        let [Form::Sym(target), Form::Kw(column), value_fn] = args else {
            return None;
        };
        if target != entity {
            return None;
        }
        let value_args = call(value_fn, "fn", env, ctx)?;
        let [Form::Vector(params), value] = value_args else {
            return None;
        };
        if !matches!(&params[..], [Form::Sym(_)]) {
            return None;
        }
        let column = world.field_sym(column);
        let column_id = u16::try_from(column.0).ok()?;
        let mut bindings = KernelBindings::default();
        let (ops, register_types, inputs, kind) = match value {
            Form::Num(value) => (
                vec![KernelOp::Const { dst: 0, v: *value }],
                vec![KernelType::F64],
                Vec::new(),
                UpdateValueKind::Num,
            ),
            Form::Kw(value) => {
                let value = world.field_sym(value);
                let slot = u16::try_from(value.0).ok()?;
                bindings.captures.push(CaptureBinding {
                    input: 0,
                    slot,
                    ty: KernelType::Symbol,
                });
                (
                    vec![KernelOp::Load { dst: 0, input: 0 }],
                    vec![KernelType::Symbol],
                    vec![KernelInput {
                        source: KernelInputSource::Capture(slot),
                        ty: KernelType::Symbol,
                    }],
                    UpdateValueKind::Sym,
                )
            }
            _ => return None,
        };
        let ty = match kind {
            UpdateValueKind::Num => KernelType::F64,
            UpdateValueKind::Sym => KernelType::Symbol,
        };
        let program = Rc::new(KernelProgram {
            ops,
            register_types,
            inputs,
            outputs: vec![KernelOutput { register: 0, ty }],
            n_inputs: bindings.captures.len(),
            aux: None,
        });
        let mut value = KernelPlan::new(program, IterationDomain::EntityRows);
        value.bindings = bindings;
        value.bindings.outputs.push(OutputBinding {
            output: 0,
            column: column_id,
            ty,
        });
        value.fallback = FallbackPolicy::WholePlanInterpreted;
        value.merge = MergePolicy::CanonicalRowOrder;
        let output = value.bindings.outputs[0];
        return Some(CompiledTickForm {
            filter: filter.clone(),
            action: CompiledTickAction::Update {
                plan: MaskedUpdatePlan {
                    predicate: filter.predicate,
                    value,
                    output,
                },
                column,
                kind,
            },
        });
    }

    let (pose, emit_form) = if let Some(let_args) = call(body, "let", env, ctx) {
        let [Form::Vector(bindings), emit] = let_args else { return None };
        let [Form::Sym(pose), value] = &bindings[..] else { return None };
        if pose == entity || HEADS.contains(&pose.as_ref())
            || access(value, entity.as_ref()).as_deref() != Some("pos") { return None; }
        (Some(pose.as_ref()), emit)
    } else {
        (None, body)
    };
    let emit_args = call(emit_form, "emit", env, ctx)?;
    let [Form::Kw(channel), Form::Map(kvs)] = emit_args else { return None };
    if channel.as_ref() != "render" { return None; }
    let mut expressions = Vec::with_capacity(kvs.len());
    let mut has_shape = false;
    for (key, value) in kvs.iter() {
        let Form::Kw(key) = key else { return None };
        if key.as_ref() == "shape" {
            if !matches!(value, Form::Kw(shape) if matches!(shape.as_ref(), "point" | "dot")) {
                return None;
            }
            has_shape = true;
        }
        expressions.push((
            key.clone(),
            RenderKey::from_name(key),
            render_expr(value, entity, pose, env, ctx)?,
        ));
    }
    if !has_shape {
        return None;
    }
    let needs_pose = expressions.iter().any(|(_, _, value)| reads_pose(value));
    let kind = expressions
        .iter()
        .find_map(|(_, slot, value)| matches!(slot, RenderKey::Kind).then_some(value))
        .map(|value| match value {
            RenderExpr::Kw(kind) => Some(kind.clone()),
            _ => None,
        })
        .unwrap_or_else(|| Some(Rc::from("default")))?;
    let mut builder = RenderBuilder::default();
    let mut fields = Vec::with_capacity(expressions.len());
    for (column, (key, slot, value)) in expressions
        .iter()
        .filter(|(_, slot, _)| !matches!(slot, RenderKey::Kind | RenderKey::Shape))
        .enumerate()
    {
        fields.push(ProjectedRenderField {
            key: key.clone(),
            slot: *slot,
            value: builder.lower(value, world, u16::try_from(column).ok()?)?,
        });
    }
    let program = Rc::new(KernelProgram {
        ops: builder.ops,
        register_types: builder.register_types,
        inputs: builder.inputs,
        outputs: builder.outputs,
        n_inputs: builder.bindings.captures.len(),
        aux: None,
    });
    let mut projection = KernelPlan::new(program, IterationDomain::RenderRows);
    projection.bindings = builder.bindings;
    projection.fallback = FallbackPolicy::WholePlanInterpreted;
    projection.merge = MergePolicy::DriverOwned;
    Some(CompiledTickForm {
        filter,
        action: CompiledTickAction::Render(CompiledRender {
            needs_pose,
            kind,
            fields,
            plan: RenderProjectionPlan { projection },
            schema: std::cell::RefCell::new(None),
        }),
    })
}
