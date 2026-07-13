use crate::interp::{EaseKind, KernelInputSource, KernelOp, KernelProgram, KernelType};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KernelProgramId(pub usize);

impl KernelProgramId {
    pub fn of(program: &Rc<KernelProgram>) -> Self {
        Self(Rc::as_ptr(program) as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationDomain {
    MotionRows,
    DynFieldRows,
    EntityRows,
    RenderRows,
    ColliderRows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleHandlePolicy {
    Missing,
    AbortPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackPolicy {
    WholePlanInterpreted,
    IrLoop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergePolicy {
    Direct,
    CanonicalRowOrder,
    DriverOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectInputBinding {
    pub input: u16,
    pub column: u16,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndirectInputBinding {
    pub input: u16,
    pub handle_input: u16,
    pub column: u16,
    pub ty: KernelType,
    pub stale: StaleHandlePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureBinding {
    pub input: u16,
    pub slot: u16,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelBinding {
    pub input: u16,
    pub channel: u16,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickAxisSource {
    Tick,
    Axis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickAxisBinding {
    pub input: u16,
    pub source: TickAxisSource,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateBinding {
    pub input: u16,
    pub slot: u16,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputBinding {
    pub output: u16,
    pub column: u16,
    pub ty: KernelType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresenceBinding {
    pub output: u16,
    pub column: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KernelBindings {
    pub direct: Vec<DirectInputBinding>,
    pub indirect: Vec<IndirectInputBinding>,
    pub captures: Vec<CaptureBinding>,
    pub channels: Vec<ChannelBinding>,
    pub tick_axis: Vec<TickAxisBinding>,
    pub state: Vec<StateBinding>,
    pub outputs: Vec<OutputBinding>,
    pub presence: Vec<PresenceBinding>,
}

#[derive(Clone, Debug)]
pub struct KernelPlan {
    pub id: KernelProgramId,
    pub program: Rc<KernelProgram>,
    pub domain: IterationDomain,
    pub bindings: KernelBindings,
    pub fallback: FallbackPolicy,
    pub merge: MergePolicy,
    pub supported: bool,
}

impl KernelPlan {
    pub fn new(program: Rc<KernelProgram>, domain: IterationDomain) -> Self {
        Self {
            id: KernelProgramId::of(&program),
            program,
            domain,
            bindings: KernelBindings::default(),
            fallback: FallbackPolicy::WholePlanInterpreted,
            merge: MergePolicy::Direct,
            supported: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MotionPlan {
    pub components: Vec<KernelPlan>,
}

#[derive(Clone, Debug)]
pub struct DynFieldPlan {
    pub value: KernelPlan,
    pub output_column: u16,
}

#[derive(Clone, Debug)]
pub struct FilterPlan {
    pub predicate: KernelPlan,
    pub short_circuit_prefix: usize,
}

#[derive(Clone, Debug)]
pub struct RenderProjectionPlan {
    pub projection: KernelPlan,
}

#[derive(Clone, Debug)]
pub struct ColliderProjectionPlan {
    pub projection: KernelPlan,
}

#[derive(Clone, Debug)]
pub struct MaskedUpdatePlan {
    pub predicate: KernelPlan,
    pub value: KernelPlan,
    pub output: OutputBinding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KernelValue {
    F32(f32),
    F64(f64),
    U32(u32),
    U64(u64),
    Symbol(u32),
    Handle(u64),
    Mask(bool),
}

impl KernelValue {
    pub fn ty(self) -> KernelType {
        match self {
            KernelValue::F32(_) => KernelType::F32,
            KernelValue::F64(_) => KernelType::F64,
            KernelValue::U32(_) => KernelType::U32,
            KernelValue::U64(_) => KernelType::U64,
            KernelValue::Symbol(_) => KernelType::Symbol,
            KernelValue::Handle(_) => KernelType::Handle,
            KernelValue::Mask(_) => KernelType::Mask,
        }
    }

    fn zero(ty: KernelType) -> Self {
        match ty {
            KernelType::F32 => KernelValue::F32(0.0),
            KernelType::F64 => KernelValue::F64(0.0),
            KernelType::U32 => KernelValue::U32(0),
            KernelType::U64 => KernelValue::U64(0),
            KernelType::Symbol => KernelValue::Symbol(0),
            KernelType::Handle => KernelValue::Handle(0),
            KernelType::Mask => KernelValue::Mask(false),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct KernelLanes {
    pub lanes: usize,
    /// Input-major storage: `values[input * lanes + lane]`.
    pub values: Vec<KernelValue>,
}

impl KernelLanes {
    pub fn input(&self, input: usize, lane: usize) -> Option<KernelValue> {
        self.values.get(input.checked_mul(self.lanes)?.checked_add(lane)?).copied()
    }
}

#[derive(Clone, Debug, Default)]
pub struct KernelOutputs {
    pub lanes: usize,
    /// Output-major storage: `values[output * lanes + lane]`.
    pub values: Vec<KernelValue>,
}

impl KernelOutputs {
    pub fn output(&self, output: usize, lane: usize) -> Option<KernelValue> {
        self.values.get(output.checked_mul(self.lanes)?.checked_add(lane)?).copied()
    }
}

#[derive(Default)]
pub struct KernelScratch {
    registers: Vec<KernelValue>,
    staged_outputs: Vec<KernelValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelExecError {
    UnsupportedPlan,
    InputCount { expected: usize, actual: usize },
    InputType { input: usize, expected: KernelType, actual: KernelType },
    InvalidRegister(u16),
    InvalidInput(u16),
    TypeMismatch { register: u16, expected: KernelType, actual: KernelType },
}

fn value_at(registers: &[KernelValue], register: u16) -> Result<KernelValue, KernelExecError> {
    registers.get(register as usize).copied().ok_or(KernelExecError::InvalidRegister(register))
}

fn f32_at(registers: &[KernelValue], register: u16) -> Result<f32, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::F32(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::F32, actual: value.ty() }),
    }
}

fn f64_at(registers: &[KernelValue], register: u16) -> Result<f64, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::F64(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::F64, actual: value.ty() }),
    }
}

fn u32_at(registers: &[KernelValue], register: u16) -> Result<u32, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::U32(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::U32, actual: value.ty() }),
    }
}

fn u64_at(registers: &[KernelValue], register: u16) -> Result<u64, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::U64(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::U64, actual: value.ty() }),
    }
}

fn symbol_at(registers: &[KernelValue], register: u16) -> Result<u32, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::Symbol(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::Symbol, actual: value.ty() }),
    }
}

fn handle_at(registers: &[KernelValue], register: u16) -> Result<u64, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::Handle(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::Handle, actual: value.ty() }),
    }
}

fn mask_at(registers: &[KernelValue], register: u16) -> Result<bool, KernelExecError> {
    match value_at(registers, register)? {
        KernelValue::Mask(value) => Ok(value),
        value => Err(KernelExecError::TypeMismatch { register, expected: KernelType::Mask, actual: value.ty() }),
    }
}

fn input_for_source(program: &KernelProgram, source: KernelInputSource) -> Option<usize> {
    program.inputs.iter().position(|input| input.source == source)
}

fn execute_lane(
    program: &KernelProgram,
    lane_inputs: &KernelLanes,
    lane: usize,
    registers: &mut [KernelValue],
) -> Result<(), KernelExecError> {
    for op in &program.ops {
        let dst = match *op {
            KernelOp::Const { dst, v } => (dst, KernelValue::F64(v)),
            KernelOp::ConstF32 { dst, v } => (dst, KernelValue::F32(v)),
            KernelOp::ConstU32 { dst, v } => (dst, KernelValue::U32(v)),
            KernelOp::ConstU64 { dst, v } => (dst, KernelValue::U64(v)),
            KernelOp::ConstMask { dst, v } => (dst, KernelValue::Mask(v)),
            KernelOp::Load { dst, input } => {
                let value = lane_inputs.input(input as usize, lane).ok_or(KernelExecError::InvalidInput(input))?;
                (dst, value)
            }
            KernelOp::Input { dst, slot } => {
                let input = input_for_source(program, KernelInputSource::Capture(slot)).ok_or(KernelExecError::InvalidInput(slot))?;
                let value = lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(slot))?;
                (dst, value)
            }
            KernelOp::T { dst } => {
                let input = input_for_source(program, KernelInputSource::Tick).ok_or(KernelExecError::InvalidInput(0))?;
                (dst, lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?)
            }
            KernelOp::U { dst } => {
                let input = input_for_source(program, KernelInputSource::Axis).ok_or(KernelExecError::InvalidInput(0))?;
                (dst, lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?)
            }
            KernelOp::PosX { dst } => {
                let input = input_for_source(program, KernelInputSource::PositionX).ok_or(KernelExecError::InvalidInput(0))?;
                (dst, lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?)
            }
            KernelOp::PosY { dst } => {
                let input = input_for_source(program, KernelInputSource::PositionY).ok_or(KernelExecError::InvalidInput(0))?;
                (dst, lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?)
            }
            KernelOp::AuxIn { dst, idx } => {
                let input = input_for_source(program, KernelInputSource::Aux(idx)).ok_or(KernelExecError::InvalidInput(idx))?;
                (dst, lane_inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?)
            }
            KernelOp::Add { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)? + f64_at(registers, b)?)),
            KernelOp::Sub { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)? - f64_at(registers, b)?)),
            KernelOp::Mul { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)? * f64_at(registers, b)?)),
            KernelOp::Div { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)? / f64_at(registers, b)?)),
            KernelOp::F32Add { dst, a, b } => (dst, KernelValue::F32(f32_at(registers, a)? + f32_at(registers, b)?)),
            KernelOp::F32Sub { dst, a, b } => (dst, KernelValue::F32(f32_at(registers, a)? - f32_at(registers, b)?)),
            KernelOp::F32Mul { dst, a, b } => (dst, KernelValue::F32(f32_at(registers, a)? * f32_at(registers, b)?)),
            KernelOp::F32Div { dst, a, b } => (dst, KernelValue::F32(f32_at(registers, a)? / f32_at(registers, b)?)),
            KernelOp::Eq { dst, a, b } => (dst, KernelValue::Mask((f64_at(registers, a)? - f64_at(registers, b)?).abs() < 1e-9)),
            KernelOp::Lt { dst, a, b } => (dst, KernelValue::Mask(f64_at(registers, a)? < f64_at(registers, b)?)),
            KernelOp::Gt { dst, a, b } => (dst, KernelValue::Mask(f64_at(registers, a)? > f64_at(registers, b)?)),
            KernelOp::Lte { dst, a, b } => (dst, KernelValue::Mask(f64_at(registers, a)? <= f64_at(registers, b)?)),
            KernelOp::Gte { dst, a, b } => (dst, KernelValue::Mask(f64_at(registers, a)? >= f64_at(registers, b)?)),
            KernelOp::U32Eq { dst, a, b } => (dst, KernelValue::Mask(u32_at(registers, a)? == u32_at(registers, b)?)),
            KernelOp::U32Lt { dst, a, b } => (dst, KernelValue::Mask(u32_at(registers, a)? < u32_at(registers, b)?)),
            KernelOp::U64Eq { dst, a, b } => (dst, KernelValue::Mask(u64_at(registers, a)? == u64_at(registers, b)?)),
            KernelOp::U64Lt { dst, a, b } => (dst, KernelValue::Mask(u64_at(registers, a)? < u64_at(registers, b)?)),
            KernelOp::SymbolEq { dst, a, b } => (dst, KernelValue::Mask(symbol_at(registers, a)? == symbol_at(registers, b)?)),
            KernelOp::HandleEq { dst, a, b } => (dst, KernelValue::Mask(handle_at(registers, a)? == handle_at(registers, b)?)),
            KernelOp::MaskAnd { dst, a, b } => (dst, KernelValue::Mask(mask_at(registers, a)? && mask_at(registers, b)?)),
            KernelOp::MaskOr { dst, a, b } => (dst, KernelValue::Mask(mask_at(registers, a)? || mask_at(registers, b)?)),
            KernelOp::MaskNot { dst, x } => (dst, KernelValue::Mask(!mask_at(registers, x)?)),
            KernelOp::SelectF32 { dst, mask, yes, no } => (dst, KernelValue::F32(if mask_at(registers, mask)? { f32_at(registers, yes)? } else { f32_at(registers, no)? })),
            KernelOp::SelectF64 { dst, mask, yes, no } => (dst, KernelValue::F64(if mask_at(registers, mask)? { f64_at(registers, yes)? } else { f64_at(registers, no)? })),
            KernelOp::SelectU32 { dst, mask, yes, no } => (dst, KernelValue::U32(if mask_at(registers, mask)? { u32_at(registers, yes)? } else { u32_at(registers, no)? })),
            KernelOp::SelectU64 { dst, mask, yes, no } => (dst, KernelValue::U64(if mask_at(registers, mask)? { u64_at(registers, yes)? } else { u64_at(registers, no)? })),
            KernelOp::SelectMask { dst, mask, yes, no } => (dst, KernelValue::Mask(if mask_at(registers, mask)? { mask_at(registers, yes)? } else { mask_at(registers, no)? })),
            KernelOp::F32ToF64 { dst, x } => (dst, KernelValue::F64(f32_at(registers, x)? as f64)),
            KernelOp::F64ToF32 { dst, x } => (dst, KernelValue::F32(f64_at(registers, x)? as f32)),
            KernelOp::U32ToF64 { dst, x } => (dst, KernelValue::F64(u32_at(registers, x)? as f64)),
            KernelOp::U64ToF64 { dst, x } => (dst, KernelValue::F64(u64_at(registers, x)? as f64)),
            KernelOp::Neg { dst, x } => (dst, KernelValue::F64(-f64_at(registers, x)?)),
            KernelOp::Not { dst, x } => (dst, KernelValue::Mask(f64_at(registers, x)? == 0.0)),
            KernelOp::Abs { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.abs())),
            KernelOp::Floor { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.floor())),
            KernelOp::Ceil { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.ceil())),
            KernelOp::Round { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.round())),
            KernelOp::Sin { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.to_radians().sin())),
            KernelOp::Cos { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.to_radians().cos())),
            KernelOp::Sqrt { dst, x } => (dst, KernelValue::F64(f64_at(registers, x)?.sqrt())),
            KernelOp::Pow { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)?.powf(f64_at(registers, b)?))),
            KernelOp::Min { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)?.min(f64_at(registers, b)?))),
            KernelOp::Max { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)?.max(f64_at(registers, b)?))),
            KernelOp::Mod { dst, a, b } => (dst, KernelValue::F64(f64_at(registers, a)?.rem_euclid(f64_at(registers, b)?))),
            KernelOp::Quot { dst, a, b } => (dst, KernelValue::F64((f64_at(registers, a)? / f64_at(registers, b)?).trunc())),
            KernelOp::Sine { dst, period, amp, x } => (dst, KernelValue::F64(f64_at(registers, amp)? * (std::f64::consts::TAU * f64_at(registers, x)? / f64_at(registers, period)?).sin())),
            KernelOp::Lerp { dst, a, b, ctrl, v1, v2 } => {
                let r = ((f64_at(registers, ctrl)? - f64_at(registers, a)?) / (f64_at(registers, b)? - f64_at(registers, a)?)).clamp(0.0, 1.0);
                (dst, KernelValue::F64(f64_at(registers, v1)? + r * (f64_at(registers, v2)? - f64_at(registers, v1)?)))
            }
            KernelOp::Lerp3 { dst, a1, b1, a2, b2, ctrl, v1, v2, v3 } => {
                let c = f64_at(registers, ctrl)?;
                let value = if c < f64_at(registers, a2)? {
                    let r = ((c - f64_at(registers, a1)?) / (f64_at(registers, b1)? - f64_at(registers, a1)?)).clamp(0.0, 1.0);
                    f64_at(registers, v1)? + r * (f64_at(registers, v2)? - f64_at(registers, v1)?)
                } else {
                    let r = ((c - f64_at(registers, a2)?) / (f64_at(registers, b2)? - f64_at(registers, a2)?)).clamp(0.0, 1.0);
                    f64_at(registers, v2)? + r * (f64_at(registers, v3)? - f64_at(registers, v2)?)
                };
                (dst, KernelValue::F64(value))
            }
            KernelOp::Ease { dst, kind, x } => (dst, KernelValue::F64(ease(kind, f64_at(registers, x)?))),
            KernelOp::LerpSmooth { dst, kind, a, b, ctrl, v1, v2 } => {
                let r = ((f64_at(registers, ctrl)? - f64_at(registers, a)?) / (f64_at(registers, b)? - f64_at(registers, a)?)).clamp(0.0, 1.0);
                (dst, KernelValue::F64(f64_at(registers, v1)? + ease(kind, r) * (f64_at(registers, v2)? - f64_at(registers, v1)?)))
            }
            KernelOp::Lssht { dst, c, pv, f1, f2 } => {
                let c = f64_at(registers, c)?;
                let pv = f64_at(registers, pv)?;
                let _w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
                let m = (c * f64_at(registers, f1)?).exp() + (c * f64_at(registers, f2)?).exp();
                (dst, KernelValue::F64(m.ln() / c))
            }
            KernelOp::Atan2 { dst, y, x } => (dst, KernelValue::F64(f64_at(registers, y)?.atan2(f64_at(registers, x)?).to_degrees())),
        };
        let slot = registers.get_mut(dst.0 as usize).ok_or(KernelExecError::InvalidRegister(dst.0))?;
        if let Some(expected) = program.register_types.get(dst.0 as usize) {
            if dst.1.ty() != *expected {
                return Err(KernelExecError::TypeMismatch { register: dst.0, expected: *expected, actual: dst.1.ty() });
            }
        }
        *slot = dst.1;
    }
    Ok(())
}

fn ease(kind: EaseKind, value: f64) -> f64 {
    match kind {
        EaseKind::InSine => 1.0 - (value * std::f64::consts::FRAC_PI_2).cos(),
        EaseKind::OutSine => (value * std::f64::consts::FRAC_PI_2).sin(),
        EaseKind::InOutSine => -((std::f64::consts::PI * value).cos() - 1.0) / 2.0,
    }
}

pub fn execute(
    plan: &KernelPlan,
    inputs: &KernelLanes,
    scratch: &mut KernelScratch,
    outputs: &mut KernelOutputs,
) -> Result<(), KernelExecError> {
    if !plan.supported {
        return Err(KernelExecError::UnsupportedPlan);
    }
    let program = &plan.program;
    let expected_values = program.inputs.len().checked_mul(inputs.lanes).ok_or(KernelExecError::InputCount {
        expected: usize::MAX,
        actual: inputs.values.len(),
    })?;
    if inputs.values.len() != expected_values {
        return Err(KernelExecError::InputCount { expected: expected_values, actual: inputs.values.len() });
    }
    for (input, descriptor) in program.inputs.iter().enumerate() {
        for lane in 0..inputs.lanes {
            let value = inputs.input(input, lane).ok_or(KernelExecError::InvalidInput(input as u16))?;
            if value.ty() != descriptor.ty {
                return Err(KernelExecError::InputType { input, expected: descriptor.ty, actual: value.ty() });
            }
        }
    }

    scratch.registers.clear();
    scratch.registers.extend(program.register_types.iter().copied().map(KernelValue::zero));
    scratch.staged_outputs.clear();
    scratch.staged_outputs.reserve(program.outputs.len().saturating_mul(inputs.lanes));
    for lane in 0..inputs.lanes {
        for (register, ty) in scratch.registers.iter_mut().zip(program.register_types.iter().copied()) {
            *register = KernelValue::zero(ty);
        }
        execute_lane(program, inputs, lane, &mut scratch.registers)?;
        for output in &program.outputs {
            let value = value_at(&scratch.registers, output.register)?;
            if value.ty() != output.ty {
                return Err(KernelExecError::TypeMismatch { register: output.register, expected: output.ty, actual: value.ty() });
            }
            scratch.staged_outputs.push(value);
        }
    }

    // The lane loop stages lane-major values; transpose once into the stable
    // output-major ABI only after every lane succeeded, so driver fallback is
    // all-or-nothing and never observes a partial kernel write.
    outputs.values.clear();
    outputs.values.resize(program.outputs.len().saturating_mul(inputs.lanes), KernelValue::Mask(false));
    for lane in 0..inputs.lanes {
        for output in 0..program.outputs.len() {
            outputs.values[output * inputs.lanes + lane] = scratch.staged_outputs[lane * program.outputs.len() + output];
        }
    }
    outputs.lanes = inputs.lanes;
    Ok(())
}
