use super::engine::RenderKey;
use super::{
    evaluate, intern_kernel_program, row_predicate, Ctx, Env, FilterPlan, KernelInputRef,
    KernelLayout, KernelOp, KernelProgram, KernelRegister, KernelType, MaskBinaryOp, Symbol,
    World,
};
use crate::edn::Form;
use crate::sim::kernel::{
    FallbackPolicy, IterationDomain, KernelBindings, KernelInputBinding, KernelInputSource,
    KernelOutputBinding, KernelOutputTarget, KernelPlan, MergePolicy,
};
use std::rc::Rc;

pub(crate) struct CompiledTickForm {
    pub filter: FilterPlan,
    pub action: CompiledTickAction,
}

pub(crate) enum CompiledTickAction {
    Render(CompiledRender),
    /// The map body is exactly `(cull e)` over the row param: scan, then
    /// cull matched rows in row order (the interpreted phase order —
    /// entities-where completes before any cull applies).
    Cull,
    Update(MaskedUpdatePlan),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateValueKind {
    Num,
    Sym,
}

pub(crate) struct MaskedUpdatePlan {
    /// The complete predicate plan is retained with the value plan so the
    /// update artifact's identity includes both fixed-width computations.
    pub predicate: FilterPlan,
    pub value: KernelPlan,
    pub column: Symbol,
    pub kind: UpdateValueKind,
}

pub(crate) struct CompiledRender {
    /// Lowered semantic fields retained only to reinstall when the world's
    /// physical column schema changes (for example after initial spawns).
    pub fields: Vec<(Rc<str>, RenderKey, RowVal)>,
    /// Cached installation for the currently resolved column schema.
    pub projection: std::cell::RefCell<Option<Rc<RenderProjectionPlan>>>,
    /// Memoized host-facing batch schema, independent from kernel identity.
    pub schema: std::cell::RefCell<Option<Rc<crate::model::RenderSchema>>>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RenderProjectionConstant {
    Dynamic,
    Num(f64),
    Symbol(Symbol),
}

/// Type-local locations in `KernelOutputs` for one fixed render value.
///
/// A render field is a small tagged union at the semantic boundary, but the
/// kernel never carries that tag: number, symbol, and both presence bits are
/// separate typed outputs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderProjectionOutput {
    pub num: u16,
    pub symbol: u16,
    pub num_present: u16,
    pub symbol_present: u16,
    pub constant: RenderProjectionConstant,
}

#[derive(Debug)]
pub(crate) struct RenderProjectionField {
    pub key: Rc<str>,
    pub output: RenderProjectionOutput,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderProjectionFieldInputs {
    pub num_column: Option<usize>,
    pub symbol_column: Option<usize>,
    pub num: Option<u16>,
    pub symbol: Option<u16>,
    pub num_present: Option<u16>,
    pub symbol_present: Option<u16>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RenderProjectionDefault {
    Num(f64),
    Symbol(Symbol),
    PoseX { input: u16 },
    PoseY { input: u16 },
    PoseTheta { input: u16, has_theta: u16 },
}

/// Installation-time direct typed-slice artifact. It mirrors the validated
/// kernel's fixed outputs but avoids decoding and materializing its register
/// stream on the CPU render hot path.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RenderProjectionSource {
    Num(f64),
    Symbol(Symbol),
    PoseX { input: u16 },
    PoseY { input: u16 },
    PoseTheta { input: u16, has_theta: u16 },
    Field(RenderProjectionFieldInputs),
    FieldOr {
        field: RenderProjectionFieldInputs,
        default: RenderProjectionDefault,
    },
}

#[derive(Debug)]
pub(crate) struct RenderProjectionBackend {
    /// Geometry followed by extra fields, matching the program outputs.
    pub sources: Vec<RenderProjectionSource>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderProjectionColumn {
    pub field: Symbol,
    pub num: Option<usize>,
    pub symbol: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct RenderProjectionPlan {
    pub kernel: KernelPlan,
    pub kind: Rc<str>,
    pub needs_pose: bool,
    pub reads_theta: bool,
    pub columns: Vec<RenderProjectionColumn>,
    pub backend: RenderProjectionBackend,
    /// x, y, theta, scale, alpha, hue.
    pub geometry: [RenderProjectionOutput; 6],
    pub extras: Vec<RenderProjectionField>,
}

pub(crate) enum RowVal {
    Num(f64),
    Kw(Rc<str>),
    PoseX,
    PoseY,
    PoseTheta,
    Field(Rc<str>),
    FieldOr(Rc<str>, Box<RowVal>),
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

fn row_val(form: &Form, entity: &str, pose: Option<&str>, env: &Env, ctx: &Ctx) -> Option<RowVal> {
    match form {
        Form::Num(n) => Some(RowVal::Num(*n)),
        Form::Kw(k) => Some(RowVal::Kw(k.clone())),
        _ => {
            if let Some(pose) = pose {
                if let Some(field) = access(form, pose) {
                    return match field.as_ref() {
                        "x" => Some(RowVal::PoseX),
                        "y" => Some(RowVal::PoseY),
                        "th" => Some(RowVal::PoseTheta),
                        _ => None,
                    };
                }
            }
            if let Some(field) = access(form, entity) {
                return (!matches!(field.as_ref(), "pos" | "vel" | "t" | "tick" | "handle" | "kind"))
                    .then(|| RowVal::Field(field));
            }
            let args = call(form, "%value-or", env, ctx)?;
            let [value, default] = args else { return None };
            let RowVal::Field(field) = row_val(value, entity, pose, env, ctx)? else { return None };
            let default = row_val(default, entity, pose, env, ctx)?;
            if !matches!(default, RowVal::Num(_) | RowVal::Kw(_) | RowVal::PoseX | RowVal::PoseY | RowVal::PoseTheta) {
                return None;
            }
            Some(RowVal::FieldOr(field, Box::new(default)))
        }
    }
}

#[derive(Clone, Copy)]
struct RenderValueRegisters {
    num: u16,
    symbol: u16,
    num_present: u16,
    symbol_present: u16,
    constant: RenderProjectionConstant,
}

#[derive(Default)]
struct RenderProjectionBuilder {
    inputs: KernelLayout,
    registers: KernelLayout,
    output_layout: KernelLayout,
    ops: Vec<KernelOp>,
    bindings: KernelBindings,
    outputs: Vec<KernelRegister>,
    fields: Vec<((Option<usize>, Option<usize>), RenderValueRegisters)>,
    pose_x: Option<u16>,
    pose_y: Option<u16>,
    pose_theta: Option<u16>,
    pose_has_theta: Option<u16>,
    needs_pose: bool,
    reads_theta: bool,
}

fn increment_layout(layout: &mut KernelLayout, ty: KernelType) -> Option<u16> {
    let index = layout.count(ty);
    let next = index.checked_add(1)?;
    match ty {
        KernelType::F32 => layout.f32s = next,
        KernelType::F64 => layout.f64s = next,
        KernelType::U32 => layout.u32s = next,
        KernelType::U64 => layout.u64s = next,
        KernelType::Symbol => layout.symbols = next,
        KernelType::Handle => layout.handles = next,
        KernelType::Mask => layout.masks = next,
    }
    Some(index)
}

fn input_ref(ty: KernelType, index: u16) -> KernelInputRef {
    match ty {
        KernelType::F32 => KernelInputRef::F32(index),
        KernelType::F64 => KernelInputRef::F64(index),
        KernelType::U32 => KernelInputRef::U32(index),
        KernelType::U64 => KernelInputRef::U64(index),
        KernelType::Symbol => KernelInputRef::Symbol(index),
        KernelType::Handle => KernelInputRef::Handle(index),
        KernelType::Mask => KernelInputRef::Mask(index),
    }
}

impl RenderProjectionBuilder {
    fn reg(&mut self, ty: KernelType) -> Option<u16> {
        increment_layout(&mut self.registers, ty)
    }

    fn load(&mut self, source: KernelInputSource, ty: KernelType) -> Option<u16> {
        let input = input_ref(ty, increment_layout(&mut self.inputs, ty)?);
        self.bindings.inputs.push(KernelInputBinding { input, source });
        let dst = self.reg(ty)?;
        self.ops.push(match input {
            KernelInputRef::F32(input) => KernelOp::LoadF32 { dst, input },
            KernelInputRef::F64(input) => KernelOp::LoadF64 { dst, input },
            KernelInputRef::U32(input) => KernelOp::LoadU32 { dst, input },
            KernelInputRef::U64(input) => KernelOp::LoadU64 { dst, input },
            KernelInputRef::Symbol(input) => KernelOp::LoadSymbol { dst, input },
            KernelInputRef::Handle(input) => KernelOp::LoadHandle { dst, input },
            KernelInputRef::Mask(input) => KernelOp::LoadMask { dst, input },
        });
        Some(dst)
    }

    fn const_num(&mut self, value: f64) -> Option<u16> {
        let dst = self.reg(KernelType::F64)?;
        self.ops.push(KernelOp::ConstF64 { dst, bits: value.to_bits() });
        Some(dst)
    }

    fn const_symbol(&mut self, value: Symbol) -> Option<u16> {
        let dst = self.reg(KernelType::Symbol)?;
        self.ops.push(KernelOp::ConstSymbol { dst, value: value.0 });
        Some(dst)
    }

    fn const_mask(&mut self, value: bool) -> Option<u16> {
        let dst = self.reg(KernelType::Mask)?;
        self.ops.push(KernelOp::ConstMask { dst, value });
        Some(dst)
    }

    fn mask_binary(&mut self, op: MaskBinaryOp, a: u16, b: u16) -> Option<u16> {
        let dst = self.reg(KernelType::Mask)?;
        self.ops.push(KernelOp::MaskBinary { op, dst, a, b });
        Some(dst)
    }

    fn mask_not(&mut self, x: u16) -> Option<u16> {
        let dst = self.reg(KernelType::Mask)?;
        self.ops.push(KernelOp::MaskNot { dst, x });
        Some(dst)
    }

    fn select_num(&mut self, mask: u16, yes: u16, no: u16) -> Option<u16> {
        let dst = self.reg(KernelType::F64)?;
        self.ops.push(KernelOp::SelectF64 { dst, mask, yes, no });
        Some(dst)
    }

    fn select_symbol(&mut self, mask: u16, yes: u16, no: u16) -> Option<u16> {
        let dst = self.reg(KernelType::Symbol)?;
        self.ops.push(KernelOp::SelectSymbol { dst, mask, yes, no });
        Some(dst)
    }

    fn select_mask(&mut self, mask: u16, yes: u16, no: u16) -> Option<u16> {
        let dst = self.reg(KernelType::Mask)?;
        self.ops.push(KernelOp::SelectMask { dst, mask, yes, no });
        Some(dst)
    }

    fn number(&mut self, value: f64) -> Option<RenderValueRegisters> {
        Some(RenderValueRegisters {
            num: self.const_num(value)?,
            symbol: self.const_symbol(Symbol(0))?,
            num_present: self.const_mask(true)?,
            symbol_present: self.const_mask(false)?,
            constant: RenderProjectionConstant::Num(value),
        })
    }

    fn symbol(&mut self, value: Symbol) -> Option<RenderValueRegisters> {
        Some(RenderValueRegisters {
            num: self.const_num(0.0)?,
            symbol: self.const_symbol(value)?,
            num_present: self.const_mask(false)?,
            symbol_present: self.const_mask(true)?,
            constant: RenderProjectionConstant::Symbol(value),
        })
    }

    fn pose_component(&mut self, slot: u16) -> Option<u16> {
        self.needs_pose = true;
        let cached = match slot {
            0 => self.pose_x,
            1 => self.pose_y,
            2 => self.pose_theta,
            _ => return None,
        };
        if let Some(register) = cached {
            return Some(register);
        }
        let register = self.load(KernelInputSource::Capture { slot }, KernelType::F64)?;
        match slot {
            0 => self.pose_x = Some(register),
            1 => self.pose_y = Some(register),
            2 => self.pose_theta = Some(register),
            _ => unreachable!(),
        }
        Some(register)
    }

    fn pose_presence(&mut self) -> Option<u16> {
        self.needs_pose = true;
        if let Some(register) = self.pose_has_theta {
            return Some(register);
        }
        let register = self.load(KernelInputSource::Capture { slot: 3 }, KernelType::Mask)?;
        self.pose_has_theta = Some(register);
        Some(register)
    }

    fn pose_value(&mut self, slot: u16) -> Option<RenderValueRegisters> {
        let component = self.pose_component(slot)?;
        let num = if slot == 2 {
            self.reads_theta = true;
            let present = self.pose_presence()?;
            let zero = self.const_num(0.0)?;
            self.select_num(present, component, zero)?
        } else {
            component
        };
        Some(RenderValueRegisters {
            num,
            symbol: self.const_symbol(Symbol(0))?,
            num_present: self.const_mask(true)?,
            symbol_present: self.const_mask(false)?,
            constant: RenderProjectionConstant::Dynamic,
        })
    }

    fn field_value(&mut self, name: &str, world: &mut World) -> Option<RenderValueRegisters> {
        let slots = world.field_slots(name);
        if let Some((_, registers)) = self.fields.iter().find(|((num, symbol), _)| {
            *num == slots.num && *symbol == slots.sym
        }) {
            return Some(*registers);
        }
        let num_column = slots.num.map(u16::try_from).transpose().ok()?;
        let symbol_column = slots.sym.map(u16::try_from).transpose().ok()?;
        let num = match num_column {
            Some(column) => {
                self.load(KernelInputSource::Direct { column }, KernelType::F64)?
            }
            None => self.const_num(0.0)?,
        };
        let symbol = match symbol_column {
            Some(column) => {
                self.load(KernelInputSource::Direct { column }, KernelType::Symbol)?
            }
            None => self.const_symbol(Symbol(0))?,
        };
        let num_present = match num_column {
            Some(slot) => {
                self.load(KernelInputSource::State { slot }, KernelType::Mask)?
            }
            None => self.const_mask(false)?,
        };
        let symbol_present = match symbol_column {
            Some(channel) => {
                self.load(KernelInputSource::Channel { channel }, KernelType::Mask)?
            }
            None => self.const_mask(false)?,
        };
        // Entity field lookup gives symbol storage precedence when malformed
        // runtime data occupies both physical columns.
        let no_symbol = self.mask_not(symbol_present)?;
        let num_present = self.mask_binary(MaskBinaryOp::And, num_present, no_symbol)?;
        let registers = RenderValueRegisters {
            num,
            symbol,
            num_present,
            symbol_present,
            constant: RenderProjectionConstant::Dynamic,
        };
        self.fields.push(((slots.num, slots.sym), registers));
        Some(registers)
    }

    fn lower(&mut self, value: &RowVal, world: &mut World) -> Option<RenderValueRegisters> {
        match value {
            RowVal::Num(value) => self.number(*value),
            RowVal::Kw(value) => {
                let value = world.field_sym(value);
                self.symbol(value)
            }
            RowVal::PoseX => self.pose_value(0),
            RowVal::PoseY => self.pose_value(1),
            RowVal::PoseTheta => self.pose_value(2),
            RowVal::Field(name) => self.field_value(name, world),
            RowVal::FieldOr(name, default) => {
                let value = self.field_value(name, world)?;
                let default = self.lower(default, world)?;
                let present =
                    self.mask_binary(MaskBinaryOp::Or, value.num_present, value.symbol_present)?;
                Some(RenderValueRegisters {
                    num: self.select_num(present, value.num, default.num)?,
                    symbol: self.select_symbol(present, value.symbol, default.symbol)?,
                    num_present: self.select_mask(
                        present,
                        value.num_present,
                        default.num_present,
                    )?,
                    symbol_present: self.select_mask(
                        present,
                        value.symbol_present,
                        default.symbol_present,
                    )?,
                    constant: RenderProjectionConstant::Dynamic,
                })
            }
        }
    }

    fn bind_output(&mut self, register: KernelRegister) -> Option<u16> {
        let flat = u16::try_from(self.outputs.len()).ok()?;
        let typed = increment_layout(&mut self.output_layout, register.ty())?;
        self.outputs.push(register);
        self.bindings.outputs.push(KernelOutputBinding {
            output: flat,
            target: KernelOutputTarget::Driver,
        });
        Some(typed)
    }

    fn output(&mut self, value: RenderValueRegisters) -> Option<RenderProjectionOutput> {
        Some(RenderProjectionOutput {
            num: self.bind_output(KernelRegister::F64(value.num))?,
            symbol: self.bind_output(KernelRegister::Symbol(value.symbol))?,
            num_present: self.bind_output(KernelRegister::Mask(value.num_present))?,
            symbol_present: self.bind_output(KernelRegister::Mask(value.symbol_present))?,
            constant: value.constant,
        })
    }
}

fn projection_input(
    kernel: &KernelPlan,
    ty: KernelType,
    source: KernelInputSource,
) -> Option<u16> {
    kernel.bindings().inputs.iter().find_map(|binding| {
        (binding.input.ty() == ty && binding.source == source)
            .then_some(binding.input.index())
    })
}

fn projection_field_inputs(
    kernel: &KernelPlan,
    name: &str,
    world: &mut World,
) -> Option<RenderProjectionFieldInputs> {
    let slots = world.field_slots(name);
    let num_slot = slots.num.map(u16::try_from).transpose().ok()?;
    let symbol_slot = slots.sym.map(u16::try_from).transpose().ok()?;
    Some(RenderProjectionFieldInputs {
        num_column: slots.num,
        symbol_column: slots.sym,
        num: match num_slot {
            Some(column) => Some(projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Direct { column },
            )?),
            None => None,
        },
        symbol: match symbol_slot {
            Some(column) => Some(projection_input(
                kernel,
                KernelType::Symbol,
                KernelInputSource::Direct { column },
            )?),
            None => None,
        },
        num_present: match num_slot {
            Some(slot) => Some(projection_input(
                kernel,
                KernelType::Mask,
                KernelInputSource::State { slot },
            )?),
            None => None,
        },
        symbol_present: match symbol_slot {
            Some(channel) => Some(projection_input(
                kernel,
                KernelType::Mask,
                KernelInputSource::Channel { channel },
            )?),
            None => None,
        },
    })
}

fn projection_default(
    kernel: &KernelPlan,
    value: &RowVal,
    world: &mut World,
) -> Option<RenderProjectionDefault> {
    match value {
        RowVal::Num(value) => Some(RenderProjectionDefault::Num(*value)),
        RowVal::Kw(value) => Some(RenderProjectionDefault::Symbol(world.field_sym(value))),
        RowVal::PoseX => Some(RenderProjectionDefault::PoseX {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 0 },
            )?,
        }),
        RowVal::PoseY => Some(RenderProjectionDefault::PoseY {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 1 },
            )?,
        }),
        RowVal::PoseTheta => Some(RenderProjectionDefault::PoseTheta {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 2 },
            )?,
            has_theta: projection_input(
                kernel,
                KernelType::Mask,
                KernelInputSource::Capture { slot: 3 },
            )?,
        }),
        RowVal::Field(_) | RowVal::FieldOr(_, _) => None,
    }
}

fn projection_source(
    kernel: &KernelPlan,
    value: &RowVal,
    world: &mut World,
) -> Option<RenderProjectionSource> {
    match value {
        RowVal::Num(value) => Some(RenderProjectionSource::Num(*value)),
        RowVal::Kw(value) => Some(RenderProjectionSource::Symbol(world.field_sym(value))),
        RowVal::PoseX => Some(RenderProjectionSource::PoseX {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 0 },
            )?,
        }),
        RowVal::PoseY => Some(RenderProjectionSource::PoseY {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 1 },
            )?,
        }),
        RowVal::PoseTheta => Some(RenderProjectionSource::PoseTheta {
            input: projection_input(
                kernel,
                KernelType::F64,
                KernelInputSource::Capture { slot: 2 },
            )?,
            has_theta: projection_input(
                kernel,
                KernelType::Mask,
                KernelInputSource::Capture { slot: 3 },
            )?,
        }),
        RowVal::Field(name) => Some(RenderProjectionSource::Field(
            projection_field_inputs(kernel, name, world)?,
        )),
        RowVal::FieldOr(name, default) => Some(RenderProjectionSource::FieldOr {
            field: projection_field_inputs(kernel, name, world)?,
            default: projection_default(kernel, default, world)?,
        }),
    }
}

fn collect_projection_columns(
    value: &RowVal,
    world: &mut World,
    columns: &mut Vec<RenderProjectionColumn>,
) {
    let name = match value {
        RowVal::Field(name) | RowVal::FieldOr(name, _) => name,
        _ => return,
    };
    let field = world.field_sym(name);
    if columns.iter().any(|column| column.field == field) {
        return;
    }
    columns.push(RenderProjectionColumn {
        field,
        num: world.col_slot(field),
        symbol: world.sym_field_slot(field),
    });
}

pub(crate) fn render_projection_plan(
    fields: &[(Rc<str>, RenderKey, RowVal)],
    world: &mut World,
) -> Option<RenderProjectionPlan> {
    let mut kind = None;
    let mut shape = None;
    let mut x = None;
    let mut y = None;
    let mut theta = None;
    let mut facing = None;
    let mut scale = None;
    let mut alpha = None;
    let mut opacity = None;
    let mut hue = None;
    let mut extras = Vec::new();
    let set_first = |slot: &mut Option<usize>, index| {
        if slot.is_none() {
            *slot = Some(index);
        }
    };
    for (index, (_, key, _)) in fields.iter().enumerate() {
        match key {
            RenderKey::Kind => set_first(&mut kind, index),
            RenderKey::Shape => set_first(&mut shape, index),
            RenderKey::X => set_first(&mut x, index),
            RenderKey::Y => set_first(&mut y, index),
            RenderKey::Theta => set_first(&mut theta, index),
            RenderKey::Facing => set_first(&mut facing, index),
            RenderKey::Scale => set_first(&mut scale, index),
            RenderKey::Alpha => set_first(&mut alpha, index),
            RenderKey::Opacity => set_first(&mut opacity, index),
            RenderKey::Hue => set_first(&mut hue, index),
            // Variable-size geometry stays on the semantic render path.
            RenderKey::Points | RenderKey::Pts | RenderKey::Active => return None,
            RenderKey::Extra => extras.push(index),
        }
    }
    let RowVal::Kw(shape) = &fields[shape?].2 else {
        return None;
    };
    if !matches!(shape.as_ref(), "point" | "dot") {
        return None;
    }
    let kind = match kind {
        None => Rc::from("default"),
        Some(index) => match &fields[index].2 {
            RowVal::Kw(kind) => kind.clone(),
            _ => return None,
        },
    };
    let mut columns = Vec::new();
    for (_, _, value) in fields {
        collect_projection_columns(value, world, &mut columns);
    }

    let geometry_values = [
        (x, 0.0),
        (y, 0.0),
        (theta.or(facing), 0.0),
        (scale, 1.0),
        (alpha.or(opacity), 1.0),
        (hue, 0.0),
    ];
    let mut builder = RenderProjectionBuilder::default();
    let mut lower_geometry = |index: Option<usize>, default: f64| {
        let registers = match index {
            Some(index) => builder.lower(&fields[index].2, world)?,
            None => builder.number(default)?,
        };
        builder.output(registers)
    };
    let x = lower_geometry(geometry_values[0].0, geometry_values[0].1)?;
    let y = lower_geometry(geometry_values[1].0, geometry_values[1].1)?;
    let theta = lower_geometry(geometry_values[2].0, geometry_values[2].1)?;
    let scale = lower_geometry(geometry_values[3].0, geometry_values[3].1)?;
    let alpha = lower_geometry(geometry_values[4].0, geometry_values[4].1)?;
    let hue = lower_geometry(geometry_values[5].0, geometry_values[5].1)?;
    drop(lower_geometry);

    let mut output_fields = Vec::with_capacity(extras.len());
    for &index in &extras {
        let value = builder.lower(&fields[index].2, world)?;
        output_fields.push(RenderProjectionField {
            key: fields[index].0.clone(),
            output: builder.output(value)?,
        });
    }
    let needs_pose = builder.needs_pose;
    let reads_theta = builder.reads_theta;
    let program = intern_kernel_program(
        KernelProgram::new(
            builder.inputs,
            builder.registers,
            builder.outputs,
            builder.ops,
        )
        .ok()?,
    );
    let kernel = KernelPlan::new(
        program,
        IterationDomain::RenderRows,
        builder.bindings,
        FallbackPolicy::WholePlanInterpreted,
        MergePolicy::DriverOwned,
    )
    .ok()?;
    let mut sources = Vec::with_capacity(geometry_values.len() + extras.len());
    for (index, default) in geometry_values {
        sources.push(match index {
            Some(index) => projection_source(&kernel, &fields[index].2, world)?,
            None => RenderProjectionSource::Num(default),
        });
    }
    for index in extras {
        sources.push(projection_source(&kernel, &fields[index].2, world)?);
    }
    Some(RenderProjectionPlan {
        kernel,
        kind,
        needs_pose,
        reads_theta,
        columns,
        backend: RenderProjectionBackend { sources },
        geometry: [x, y, theta, scale, alpha, hue],
        extras: output_fields,
    })
}

fn fixed_update_plan(
    filter: FilterPlan,
    column: Symbol,
    value: &Form,
    world: &mut World,
) -> Option<MaskedUpdatePlan> {
    let (registers, output, ops, kind) = match value {
        Form::Num(value) => (
            KernelLayout {
                f64s: 1,
                ..KernelLayout::default()
            },
            KernelRegister::F64(0),
            vec![KernelOp::ConstF64 {
                dst: 0,
                bits: value.to_bits(),
            }],
            UpdateValueKind::Num,
        ),
        Form::Kw(value) => {
            let value = world.field_sym(value);
            (
                KernelLayout {
                    symbols: 1,
                    ..KernelLayout::default()
                },
                KernelRegister::Symbol(0),
                vec![KernelOp::ConstSymbol {
                    dst: 0,
                    value: value.0,
                }],
                UpdateValueKind::Sym,
            )
        }
        _ => return None,
    };
    let program = intern_kernel_program(
        KernelProgram::new(
            KernelLayout::default(),
            registers,
            vec![output],
            ops,
        )
        .ok()?,
    );
    let bindings = KernelBindings {
        inputs: Vec::new(),
        outputs: vec![KernelOutputBinding {
            output: 0,
            target: KernelOutputTarget::Driver,
        }],
    };
    let value = KernelPlan::new(
        program,
        IterationDomain::EntityRows,
        bindings,
        FallbackPolicy::WholePlanInterpreted,
        MergePolicy::CanonicalRowOrder,
    )
    .ok()?;
    Some(MaskedUpdatePlan {
        predicate: filter,
        value,
        column,
        kind,
    })
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

    // Exact fixed-width action shapes. Predicate execution always finishes
    // before either driver-owned effect is published.
    if let Some(args) = call(body, "cull", env, ctx) {
        return (entity.as_ref() != "cull"
            && matches!(args, [Form::Sym(target)] if target == entity))
            .then(|| CompiledTickForm { filter, action: CompiledTickAction::Cull });
    }
    if let Some(args) = call(body, "change-col", env, ctx) {
        let [Form::Sym(target), Form::Kw(column), value_fn] = args else {
            return None;
        };
        if entity.as_ref() == "change-col" || target != entity {
            return None;
        }
        let [Form::Vector(params), value] = call(value_fn, "fn", env, ctx)? else {
            return None;
        };
        if !matches!(&params[..], [Form::Sym(param)] if param.as_ref() != "&") {
            return None;
        }
        let column = world.field_sym(column);
        let plan = fixed_update_plan(filter.clone(), column, value, world)?;
        return Some(CompiledTickForm {
            filter,
            action: CompiledTickAction::Update(plan),
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
    let mut fields = Vec::with_capacity(kvs.len());
    for (key, value) in kvs.iter() {
        let Form::Kw(key) = key else { return None };
        fields.push((key.clone(), RenderKey::from_name(key), row_val(value, entity, pose, env, ctx)?));
    }
    let projection = render_projection_plan(&fields, world)?;
    Some(CompiledTickForm {
        filter,
        action: CompiledTickAction::Render(CompiledRender {
            fields,
            projection: std::cell::RefCell::new(Some(Rc::new(projection))),
            schema: std::cell::RefCell::new(None),
        }),
    })
}
