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
    /// Optional mask input receiving `false` for a stale/missing handle.
    pub presence_input: Option<u16>,
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
pub enum PoseComponent {
    X,
    Y,
    Theta,
    HasTheta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoseInputBinding {
    pub input: u16,
    pub component: PoseComponent,
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
    pub pose: Vec<PoseInputBinding>,
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
    StaleHandle { lane: usize },
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
            KernelOp::SelectSymbol { dst, mask, yes, no } => (dst, KernelValue::Symbol(if mask_at(registers, mask)? { symbol_at(registers, yes)? } else { symbol_at(registers, no)? })),
            KernelOp::SelectHandle { dst, mask, yes, no } => (dst, KernelValue::Handle(if mask_at(registers, mask)? { handle_at(registers, yes)? } else { handle_at(registers, no)? })),
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

/// Stage a declared indirect gather without exposing world access to the
/// program. Drivers validate generations and pass `None` for stale handles;
/// the binding decides whether that means an absent fixed-width value or a
/// whole-plan abort. Values and masks are appended only after every lane has
/// validated, preserving all-or-nothing fallback.
pub fn gather_indirect(
    binding: IndirectInputBinding,
    gathered: &[Option<KernelValue>],
    values: &mut Vec<KernelValue>,
    presence: &mut Vec<KernelValue>,
) -> Result<(), KernelExecError> {
    let mut staged_values = Vec::with_capacity(gathered.len());
    let mut staged_presence = Vec::with_capacity(gathered.len());
    for (lane, value) in gathered.iter().copied().enumerate() {
        match value {
            Some(value) if value.ty() == binding.ty => {
                staged_values.push(value);
                staged_presence.push(KernelValue::Mask(true));
            }
            Some(value) => {
                return Err(KernelExecError::InputType {
                    input: binding.input as usize,
                    expected: binding.ty,
                    actual: value.ty(),
                });
            }
            None if binding.stale == StaleHandlePolicy::Missing => {
                staged_values.push(KernelValue::zero(binding.ty));
                staged_presence.push(KernelValue::Mask(false));
            }
            None => return Err(KernelExecError::StaleHandle { lane }),
        }
    }
    values.extend(staged_values);
    if binding.presence_input.is_some() {
        presence.extend(staged_presence);
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::{intern_program, KernelInput, KernelOutput};

    fn plan(program: KernelProgram) -> KernelPlan {
        KernelPlan::new(Rc::new(program), IterationDomain::EntityRows)
    }

    #[test]
    fn typed_integer_mask_selection_and_multiple_outputs() {
        let program = KernelProgram {
            ops: vec![
                KernelOp::Load { dst: 0, input: 0 },
                KernelOp::ConstU32 { dst: 1, v: 7 },
                KernelOp::U32Eq { dst: 2, a: 0, b: 1 },
                KernelOp::Const { dst: 3, v: 10.0 },
                KernelOp::Const { dst: 4, v: -1.0 },
                KernelOp::SelectF64 { dst: 5, mask: 2, yes: 3, no: 4 },
            ],
            register_types: vec![
                KernelType::U32,
                KernelType::U32,
                KernelType::Mask,
                KernelType::F64,
                KernelType::F64,
                KernelType::F64,
            ],
            inputs: vec![KernelInput {
                source: KernelInputSource::Direct(0),
                ty: KernelType::U32,
            }],
            outputs: vec![
                KernelOutput { register: 2, ty: KernelType::Mask },
                KernelOutput { register: 5, ty: KernelType::F64 },
            ],
            n_inputs: 0,
            aux: None,
        };
        let inputs = KernelLanes {
            lanes: 2,
            values: vec![KernelValue::U32(7), KernelValue::U32(3)],
        };
        let mut scratch = KernelScratch::default();
        let mut outputs = KernelOutputs::default();
        execute(&plan(program), &inputs, &mut scratch, &mut outputs).unwrap();
        assert_eq!(outputs.values, vec![
            KernelValue::Mask(true),
            KernelValue::Mask(false),
            KernelValue::F64(10.0),
            KernelValue::F64(-1.0),
        ]);
    }

    #[test]
    fn optional_pose_orientation_keeps_presence_lane() {
        let program = KernelProgram {
            ops: vec![
                KernelOp::Load { dst: 0, input: 0 },
                KernelOp::Load { dst: 1, input: 1 },
                KernelOp::Const { dst: 2, v: 0.0 },
                KernelOp::SelectF64 { dst: 3, mask: 1, yes: 0, no: 2 },
            ],
            register_types: vec![
                KernelType::F64,
                KernelType::Mask,
                KernelType::F64,
                KernelType::F64,
            ],
            inputs: vec![
                KernelInput { source: KernelInputSource::State(0), ty: KernelType::F64 },
                KernelInput { source: KernelInputSource::State(1), ty: KernelType::Mask },
            ],
            outputs: vec![
                KernelOutput { register: 3, ty: KernelType::F64 },
                KernelOutput { register: 1, ty: KernelType::Mask },
            ],
            n_inputs: 0,
            aux: None,
        };
        let inputs = KernelLanes {
            lanes: 2,
            values: vec![
                KernelValue::F64(45.0),
                KernelValue::F64(0.0),
                KernelValue::Mask(true),
                KernelValue::Mask(false),
            ],
        };
        let mut scratch = KernelScratch::default();
        let mut outputs = KernelOutputs::default();
        execute(&plan(program), &inputs, &mut scratch, &mut outputs).unwrap();
        assert_eq!(outputs.output(0, 0), Some(KernelValue::F64(45.0)));
        assert_eq!(outputs.output(0, 1), Some(KernelValue::F64(0.0)));
        assert_eq!(outputs.output(1, 0), Some(KernelValue::Mask(true)));
        assert_eq!(outputs.output(1, 1), Some(KernelValue::Mask(false)));
    }

    #[test]
    fn driver_abort_leaves_previous_outputs_untouched() {
        let program = KernelProgram {
            ops: vec![KernelOp::Load { dst: 0, input: 0 }],
            register_types: vec![KernelType::U32],
            inputs: vec![KernelInput {
                source: KernelInputSource::Direct(0),
                ty: KernelType::U32,
            }],
            outputs: vec![KernelOutput { register: 0, ty: KernelType::U32 }],
            n_inputs: 0,
            aux: None,
        };
        let plan = plan(program);
        let mut scratch = KernelScratch::default();
        let mut outputs = KernelOutputs {
            lanes: 1,
            values: vec![KernelValue::U32(99)],
        };
        let bad = KernelLanes {
            lanes: 1,
            values: vec![KernelValue::F64(7.0)],
        };
        assert!(matches!(
            execute(&plan, &bad, &mut scratch, &mut outputs),
            Err(KernelExecError::InputType { .. })
        ));
        assert_eq!(outputs.values, vec![KernelValue::U32(99)]);

        let mut unsupported = plan;
        unsupported.supported = false;
        assert_eq!(
            execute(&unsupported, &bad, &mut scratch, &mut outputs),
            Err(KernelExecError::UnsupportedPlan)
        );
        assert_eq!(outputs.values, vec![KernelValue::U32(99)]);
    }

    #[test]
    fn stale_handle_gather_obeys_declared_policy() {
        let binding = IndirectInputBinding {
            input: 1,
            handle_input: 0,
            presence_input: Some(2),
            column: 4,
            ty: KernelType::F64,
            stale: StaleHandlePolicy::Missing,
        };
        let mut values = Vec::new();
        let mut presence = Vec::new();
        gather_indirect(
            binding,
            &[Some(KernelValue::F64(3.0)), None],
            &mut values,
            &mut presence,
        )
        .unwrap();
        assert_eq!(values, vec![KernelValue::F64(3.0), KernelValue::F64(0.0)]);
        assert_eq!(presence, vec![KernelValue::Mask(true), KernelValue::Mask(false)]);

        let abort = IndirectInputBinding {
            stale: StaleHandlePolicy::AbortPlan,
            ..binding
        };
        let mut untouched = vec![KernelValue::F64(8.0)];
        assert_eq!(
            gather_indirect(abort, &[None], &mut untouched, &mut Vec::new()),
            Err(KernelExecError::StaleHandle { lane: 0 })
        );
        assert_eq!(untouched, vec![KernelValue::F64(8.0)]);
    }

    #[test]
    fn width_is_part_of_interned_program_identity() {
        let make = |ty, op| KernelProgram {
            ops: vec![op],
            register_types: vec![ty],
            inputs: Vec::new(),
            outputs: vec![KernelOutput { register: 0, ty }],
            n_inputs: 0,
            aux: None,
        };
        let f32_program = intern_program(make(
            KernelType::F32,
            KernelOp::ConstF32 { dst: 0, v: 1.0 },
        ));
        let f64_program = intern_program(make(
            KernelType::F64,
            KernelOp::Const { dst: 0, v: 1.0 },
        ));
        assert!(!Rc::ptr_eq(&f32_program, &f64_program));
    }
}
