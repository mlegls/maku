//! Typed, fixed-width kernel plans and the permanent op-major SoA executor.
//!
//! Programs are validated when constructed and plans are validated when
//! installed. Execution therefore performs only concrete slice operations:
//! each operation is decoded once, outside its lane loop, and no lane carries
//! a tagged value.

pub use crate::interp::{
    intern_kernel_program, kernel_program_for_num, EaseKind, FloatBinaryOp, FloatCompareOp,
    FloatUnaryOp, IntegerBinaryOp, IntegerCompareOp, KernelInputRef, KernelLayout, KernelOp,
    KernelProgram, KernelProgramId, KernelRegister, KernelType, KernelValidationError,
    MaskBinaryOp, NumKernelBridge, NumKernelInputSource,
};
use std::rc::Rc;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaskLane(u8);

impl MaskLane {
    pub const FALSE: Self = Self(0);
    pub const TRUE: Self = Self(1);

    pub const fn new(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }

    pub const fn get(self) -> bool {
        self.0 != 0
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

/// Driver-owned source for one declared, type-local program input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelInputSource {
    Direct { column: u16 },
    Indirect {
        handle: KernelInputRef,
        column: u16,
        stale: StaleHandlePolicy,
    },
    Capture { slot: u16 },
    Channel { channel: u16 },
    Tick,
    Axis,
    State { slot: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelInputBinding {
    pub input: KernelInputRef,
    pub source: KernelInputSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelOutputTarget {
    Column { column: u16 },
    NextState { slot: u16 },
    Presence { column: u16 },
    Driver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelOutputBinding {
    /// Index in the program's flattened output descriptor order.
    pub output: u16,
    pub target: KernelOutputTarget,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KernelBindings {
    pub inputs: Vec<KernelInputBinding>,
    pub outputs: Vec<KernelOutputBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelPlanError {
    MissingInput(KernelInputRef),
    DuplicateInput(KernelInputRef),
    UndeclaredInput(KernelInputRef),
    InvalidIndirectHandle(KernelInputRef),
    MissingOutput(u16),
    DuplicateOutput(u16),
    UndeclaredOutput(u16),
    PresenceOutputNotMask(u16),
}

/// A validated installation of one pure program into a driver domain.
/// Fields which affect execution are immutable after construction.
#[derive(Clone, Debug)]
pub struct KernelPlan {
    id: KernelProgramId,
    program: Rc<KernelProgram>,
    domain: IterationDomain,
    bindings: KernelBindings,
    fallback: FallbackPolicy,
    merge: MergePolicy,
    output_layout: KernelLayout,
}

impl KernelPlan {
    pub fn new(
        program: Rc<KernelProgram>,
        domain: IterationDomain,
        bindings: KernelBindings,
        fallback: FallbackPolicy,
        merge: MergePolicy,
    ) -> Result<Self, KernelPlanError> {
        validate_bindings(&program, &bindings)?;
        let mut output_layout = KernelLayout::default();
        for output in program.outputs() {
            increment_layout(&mut output_layout, output.ty());
        }
        Ok(Self {
            id: program.id(),
            program,
            domain,
            bindings,
            fallback,
            merge,
            output_layout,
        })
    }

    pub const fn id(&self) -> KernelProgramId {
        self.id
    }

    pub fn program(&self) -> &KernelProgram {
        &self.program
    }

    pub const fn domain(&self) -> IterationDomain {
        self.domain
    }

    pub fn bindings(&self) -> &KernelBindings {
        &self.bindings
    }

    pub const fn fallback(&self) -> FallbackPolicy {
        self.fallback
    }

    pub const fn merge(&self) -> MergePolicy {
        self.merge
    }
}

fn increment_layout(layout: &mut KernelLayout, ty: KernelType) {
    match ty {
        KernelType::F32 => layout.f32s += 1,
        KernelType::F64 => layout.f64s += 1,
        KernelType::U32 => layout.u32s += 1,
        KernelType::U64 => layout.u64s += 1,
        KernelType::Symbol => layout.symbols += 1,
        KernelType::Handle => layout.handles += 1,
        KernelType::Mask => layout.masks += 1,
    }
}

fn total_layout(layout: KernelLayout) -> usize {
    usize::from(layout.f32s)
        + usize::from(layout.f64s)
        + usize::from(layout.u32s)
        + usize::from(layout.u64s)
        + usize::from(layout.symbols)
        + usize::from(layout.handles)
        + usize::from(layout.masks)
}

fn validate_bindings(program: &KernelProgram, bindings: &KernelBindings) -> Result<(), KernelPlanError> {
    let inputs = program.inputs();
    let mut seen = KernelSeen::new(inputs);
    for binding in &bindings.inputs {
        if binding.input.index() >= inputs.count(binding.input.ty()) {
            return Err(KernelPlanError::UndeclaredInput(binding.input));
        }
        if !seen.mark(binding.input) {
            return Err(KernelPlanError::DuplicateInput(binding.input));
        }
        if let KernelInputSource::Indirect { handle, .. } = binding.source {
            if !matches!(handle, KernelInputRef::Handle(index) if index < inputs.handles) {
                return Err(KernelPlanError::InvalidIndirectHandle(handle));
            }
        }
    }
    if bindings.inputs.len() != total_layout(inputs) {
        if let Some(input) = seen.first_unset() {
            return Err(KernelPlanError::MissingInput(input));
        }
    }
    if let Some(input) = seen.first_unset() {
        return Err(KernelPlanError::MissingInput(input));
    }

    let output_count = program.outputs().len();
    let mut output_seen = vec![false; output_count];
    for binding in &bindings.outputs {
        let output = usize::from(binding.output);
        let Some(seen) = output_seen.get_mut(output) else {
            return Err(KernelPlanError::UndeclaredOutput(binding.output));
        };
        if *seen {
            return Err(KernelPlanError::DuplicateOutput(binding.output));
        }
        if matches!(binding.target, KernelOutputTarget::Presence { .. })
            && program.outputs()[output].ty() != KernelType::Mask
        {
            return Err(KernelPlanError::PresenceOutputNotMask(binding.output));
        }
        *seen = true;
    }
    if let Some(output) = output_seen.iter().position(|seen| !seen) {
        return Err(KernelPlanError::MissingOutput(output as u16));
    }
    Ok(())
}

struct KernelSeen {
    f32s: Vec<bool>,
    f64s: Vec<bool>,
    u32s: Vec<bool>,
    u64s: Vec<bool>,
    symbols: Vec<bool>,
    handles: Vec<bool>,
    masks: Vec<bool>,
}

impl KernelSeen {
    fn new(layout: KernelLayout) -> Self {
        Self {
            f32s: vec![false; usize::from(layout.f32s)],
            f64s: vec![false; usize::from(layout.f64s)],
            u32s: vec![false; usize::from(layout.u32s)],
            u64s: vec![false; usize::from(layout.u64s)],
            symbols: vec![false; usize::from(layout.symbols)],
            handles: vec![false; usize::from(layout.handles)],
            masks: vec![false; usize::from(layout.masks)],
        }
    }

    fn mark(&mut self, input: KernelInputRef) -> bool {
        let slot = match input {
            KernelInputRef::F32(index) => &mut self.f32s[usize::from(index)],
            KernelInputRef::F64(index) => &mut self.f64s[usize::from(index)],
            KernelInputRef::U32(index) => &mut self.u32s[usize::from(index)],
            KernelInputRef::U64(index) => &mut self.u64s[usize::from(index)],
            KernelInputRef::Symbol(index) => &mut self.symbols[usize::from(index)],
            KernelInputRef::Handle(index) => &mut self.handles[usize::from(index)],
            KernelInputRef::Mask(index) => &mut self.masks[usize::from(index)],
        };
        let was_unset = !*slot;
        *slot = true;
        was_unset
    }

    fn first_unset(&self) -> Option<KernelInputRef> {
        self.f32s
            .iter()
            .position(|seen| !seen)
            .map(|index| KernelInputRef::F32(index as u16))
            .or_else(|| self.f64s.iter().position(|seen| !seen).map(|index| KernelInputRef::F64(index as u16)))
            .or_else(|| self.u32s.iter().position(|seen| !seen).map(|index| KernelInputRef::U32(index as u16)))
            .or_else(|| self.u64s.iter().position(|seen| !seen).map(|index| KernelInputRef::U64(index as u16)))
            .or_else(|| self.symbols.iter().position(|seen| !seen).map(|index| KernelInputRef::Symbol(index as u16)))
            .or_else(|| self.handles.iter().position(|seen| !seen).map(|index| KernelInputRef::Handle(index as u16)))
            .or_else(|| self.masks.iter().position(|seen| !seen).map(|index| KernelInputRef::Mask(index as u16)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct KernelLanes {
    pub lanes: usize,
    /// Input-major concrete storage: `values[input * lanes + lane]`.
    pub f32s: Vec<f32>,
    pub f64s: Vec<f64>,
    pub u32s: Vec<u32>,
    pub u64s: Vec<u64>,
    pub symbols: Vec<u32>,
    pub handles: Vec<u64>,
    pub masks: Vec<MaskLane>,
}

#[derive(Clone, Debug, Default)]
pub struct KernelOutputs {
    pub lanes: usize,
    /// Output-major concrete storage within each output type.
    pub f32s: Vec<f32>,
    pub f64s: Vec<f64>,
    pub u32s: Vec<u32>,
    pub u64s: Vec<u64>,
    pub symbols: Vec<u32>,
    pub handles: Vec<u64>,
    pub masks: Vec<MaskLane>,
}

#[derive(Default)]
pub struct KernelScratch {
    f32s: Vec<f32>,
    f64s: Vec<f64>,
    u32s: Vec<u32>,
    u64s: Vec<u64>,
    symbols: Vec<u32>,
    handles: Vec<u64>,
    masks: Vec<MaskLane>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelExecError {
    LaneCountOverflow,
    InputLength {
        ty: KernelType,
        expected: usize,
        actual: usize,
    },
}

/// Execute a validated plan in op-major order over concrete SoA buffers.
/// There is no operation after input-shape validation which can fail, so
/// output columns are never partially committed before a fallback decision.
pub fn execute(
    plan: &KernelPlan,
    inputs: &KernelLanes,
    scratch: &mut KernelScratch,
    outputs: &mut KernelOutputs,
) -> Result<(), KernelExecError> {
    validate_input_lengths(plan.program.inputs(), inputs)?;
    prepare_scratch(plan.program.registers(), inputs.lanes, scratch)?;

    for op in plan.program.ops().iter().copied() {
        execute_op(op, inputs, scratch, inputs.lanes);
    }

    prepare_outputs(plan.output_layout, inputs.lanes, outputs)?;
    let mut slots = KernelLayout::default();
    for output in plan.program.outputs().iter().copied() {
        match output {
            KernelRegister::F32(register) => {
                copy_register(&scratch.f32s, register, &mut outputs.f32s, slots.f32s, inputs.lanes);
                slots.f32s += 1;
            }
            KernelRegister::F64(register) => {
                copy_register(&scratch.f64s, register, &mut outputs.f64s, slots.f64s, inputs.lanes);
                slots.f64s += 1;
            }
            KernelRegister::U32(register) => {
                copy_register(&scratch.u32s, register, &mut outputs.u32s, slots.u32s, inputs.lanes);
                slots.u32s += 1;
            }
            KernelRegister::U64(register) => {
                copy_register(&scratch.u64s, register, &mut outputs.u64s, slots.u64s, inputs.lanes);
                slots.u64s += 1;
            }
            KernelRegister::Symbol(register) => {
                copy_register(&scratch.symbols, register, &mut outputs.symbols, slots.symbols, inputs.lanes);
                slots.symbols += 1;
            }
            KernelRegister::Handle(register) => {
                copy_register(&scratch.handles, register, &mut outputs.handles, slots.handles, inputs.lanes);
                slots.handles += 1;
            }
            KernelRegister::Mask(register) => {
                copy_register(&scratch.masks, register, &mut outputs.masks, slots.masks, inputs.lanes);
                slots.masks += 1;
            }
        }
    }
    outputs.lanes = inputs.lanes;
    Ok(())
}

fn checked_len(count: u16, lanes: usize) -> Result<usize, KernelExecError> {
    usize::from(count).checked_mul(lanes).ok_or(KernelExecError::LaneCountOverflow)
}

fn validate_len(ty: KernelType, count: u16, lanes: usize, actual: usize) -> Result<(), KernelExecError> {
    let expected = checked_len(count, lanes)?;
    if expected == actual {
        Ok(())
    } else {
        Err(KernelExecError::InputLength { ty, expected, actual })
    }
}

fn validate_input_lengths(layout: KernelLayout, inputs: &KernelLanes) -> Result<(), KernelExecError> {
    validate_len(KernelType::F32, layout.f32s, inputs.lanes, inputs.f32s.len())?;
    validate_len(KernelType::F64, layout.f64s, inputs.lanes, inputs.f64s.len())?;
    validate_len(KernelType::U32, layout.u32s, inputs.lanes, inputs.u32s.len())?;
    validate_len(KernelType::U64, layout.u64s, inputs.lanes, inputs.u64s.len())?;
    validate_len(KernelType::Symbol, layout.symbols, inputs.lanes, inputs.symbols.len())?;
    validate_len(KernelType::Handle, layout.handles, inputs.lanes, inputs.handles.len())?;
    validate_len(KernelType::Mask, layout.masks, inputs.lanes, inputs.masks.len())
}

fn prepare_scratch(layout: KernelLayout, lanes: usize, scratch: &mut KernelScratch) -> Result<(), KernelExecError> {
    scratch.f32s.resize(checked_len(layout.f32s, lanes)?, 0.0);
    scratch.f64s.resize(checked_len(layout.f64s, lanes)?, 0.0);
    scratch.u32s.resize(checked_len(layout.u32s, lanes)?, 0);
    scratch.u64s.resize(checked_len(layout.u64s, lanes)?, 0);
    scratch.symbols.resize(checked_len(layout.symbols, lanes)?, 0);
    scratch.handles.resize(checked_len(layout.handles, lanes)?, 0);
    scratch.masks.resize(checked_len(layout.masks, lanes)?, MaskLane::FALSE);
    Ok(())
}

fn prepare_outputs(layout: KernelLayout, lanes: usize, outputs: &mut KernelOutputs) -> Result<(), KernelExecError> {
    outputs.f32s.resize(checked_len(layout.f32s, lanes)?, 0.0);
    outputs.f64s.resize(checked_len(layout.f64s, lanes)?, 0.0);
    outputs.u32s.resize(checked_len(layout.u32s, lanes)?, 0);
    outputs.u64s.resize(checked_len(layout.u64s, lanes)?, 0);
    outputs.symbols.resize(checked_len(layout.symbols, lanes)?, 0);
    outputs.handles.resize(checked_len(layout.handles, lanes)?, 0);
    outputs.masks.resize(checked_len(layout.masks, lanes)?, MaskLane::FALSE);
    Ok(())
}

#[inline]
fn input_slice<T>(values: &[T], input: u16, lanes: usize) -> &[T] {
    let start = usize::from(input) * lanes;
    &values[start..start + lanes]
}

#[inline]
fn register_slice<T>(values: &[T], register: u16, lanes: usize) -> &[T] {
    let start = usize::from(register) * lanes;
    &values[start..start + lanes]
}

#[inline]
fn destination_slice<T>(values: &mut [T], destination: u16, lanes: usize) -> (&[T], &mut [T]) {
    let start = usize::from(destination) * lanes;
    let (before, destination_and_after) = values.split_at_mut(start);
    (before, &mut destination_and_after[..lanes])
}

#[inline]
fn copy_register<T: Copy>(
    registers: &[T],
    register: u16,
    outputs: &mut [T],
    output: u16,
    lanes: usize,
) {
    let source = register_slice(registers, register, lanes);
    let start = usize::from(output) * lanes;
    outputs[start..start + lanes].copy_from_slice(source);
}

#[inline]
fn load_register<T: Copy>(registers: &mut [T], destination: u16, inputs: &[T], input: u16, lanes: usize) {
    let source = input_slice(inputs, input, lanes);
    let start = usize::from(destination) * lanes;
    registers[start..start + lanes].copy_from_slice(source);
}

#[inline]
fn fill_register<T: Copy>(registers: &mut [T], destination: u16, lanes: usize, value: T) {
    let start = usize::from(destination) * lanes;
    registers[start..start + lanes].fill(value);
}

#[inline]
fn unary_register<T: Copy, F: FnMut(T) -> T>(registers: &mut [T], destination: u16, x: u16, lanes: usize, mut f: F) {
    let (before, destination) = destination_slice(registers, destination, lanes);
    let x = register_slice(before, x, lanes);
    for lane in 0..lanes {
        destination[lane] = f(x[lane]);
    }
}

#[inline]
fn binary_register<T: Copy, F: FnMut(T, T) -> T>(
    registers: &mut [T],
    destination: u16,
    a: u16,
    b: u16,
    lanes: usize,
    mut f: F,
) {
    let (before, destination) = destination_slice(registers, destination, lanes);
    let a = register_slice(before, a, lanes);
    let b = register_slice(before, b, lanes);
    for lane in 0..lanes {
        destination[lane] = f(a[lane], b[lane]);
    }
}

#[inline]
fn compare_register<T: Copy, F: FnMut(T, T) -> bool>(
    registers: &[T],
    masks: &mut [MaskLane],
    destination: u16,
    a: u16,
    b: u16,
    lanes: usize,
    mut f: F,
) {
    let a = register_slice(registers, a, lanes);
    let b = register_slice(registers, b, lanes);
    let start = usize::from(destination) * lanes;
    let destination = &mut masks[start..start + lanes];
    for lane in 0..lanes {
        destination[lane] = MaskLane::new(f(a[lane], b[lane]));
    }
}

#[inline]
fn select_register<T: Copy>(
    registers: &mut [T],
    masks: &[MaskLane],
    destination: u16,
    mask: u16,
    yes: u16,
    no: u16,
    lanes: usize,
) {
    let mask = register_slice(masks, mask, lanes);
    let (before, destination) = destination_slice(registers, destination, lanes);
    let yes = register_slice(before, yes, lanes);
    let no = register_slice(before, no, lanes);
    for lane in 0..lanes {
        destination[lane] = if mask[lane].get() { yes[lane] } else { no[lane] };
    }
}

#[inline]
fn select_mask_register(registers: &mut [MaskLane], destination: u16, mask: u16, yes: u16, no: u16, lanes: usize) {
    let (before, destination) = destination_slice(registers, destination, lanes);
    let mask = register_slice(before, mask, lanes);
    let yes = register_slice(before, yes, lanes);
    let no = register_slice(before, no, lanes);
    for lane in 0..lanes {
        destination[lane] = if mask[lane].get() { yes[lane] } else { no[lane] };
    }
}

#[inline]
fn convert_register<S: Copy, D, F: FnMut(S) -> D>(
    source: &[S],
    destination: &mut [D],
    source_register: u16,
    destination_register: u16,
    lanes: usize,
    mut f: F,
) {
    let source = register_slice(source, source_register, lanes);
    let start = usize::from(destination_register) * lanes;
    let destination = &mut destination[start..start + lanes];
    for lane in 0..lanes {
        destination[lane] = f(source[lane]);
    }
}
#[inline]
fn ease_f64(kind: EaseKind, value: f64) -> f64 {
    use std::f64::consts::FRAC_PI_2;
    let value = value.clamp(0.0, 1.0);
    match kind {
        EaseKind::InSine => 1.0 - (value * FRAC_PI_2).cos(),
        EaseKind::OutSine => (value * FRAC_PI_2).sin(),
        EaseKind::InOutSine => -(std::f64::consts::PI * value).cos() / 2.0 + 0.5,
    }
}


fn execute_op(op: KernelOp, inputs: &KernelLanes, scratch: &mut KernelScratch, lanes: usize) {
    match op {
        KernelOp::ConstF32 { dst, bits } => fill_register(&mut scratch.f32s, dst, lanes, f32::from_bits(bits)),
        KernelOp::ConstF64 { dst, bits } => fill_register(&mut scratch.f64s, dst, lanes, f64::from_bits(bits)),
        KernelOp::ConstU32 { dst, value } => fill_register(&mut scratch.u32s, dst, lanes, value),
        KernelOp::ConstU64 { dst, value } => fill_register(&mut scratch.u64s, dst, lanes, value),
        KernelOp::ConstSymbol { dst, value } => fill_register(&mut scratch.symbols, dst, lanes, value),
        KernelOp::ConstHandle { dst, value } => fill_register(&mut scratch.handles, dst, lanes, value),
        KernelOp::ConstMask { dst, value } => fill_register(&mut scratch.masks, dst, lanes, MaskLane::new(value)),
        KernelOp::LoadF32 { dst, input } => load_register(&mut scratch.f32s, dst, &inputs.f32s, input, lanes),
        KernelOp::LoadF64 { dst, input } => load_register(&mut scratch.f64s, dst, &inputs.f64s, input, lanes),
        KernelOp::LoadU32 { dst, input } => load_register(&mut scratch.u32s, dst, &inputs.u32s, input, lanes),
        KernelOp::LoadU64 { dst, input } => load_register(&mut scratch.u64s, dst, &inputs.u64s, input, lanes),
        KernelOp::LoadSymbol { dst, input } => load_register(&mut scratch.symbols, dst, &inputs.symbols, input, lanes),
        KernelOp::LoadHandle { dst, input } => load_register(&mut scratch.handles, dst, &inputs.handles, input, lanes),
        KernelOp::LoadMask { dst, input } => load_register(&mut scratch.masks, dst, &inputs.masks, input, lanes),
        KernelOp::F32Unary { op, dst, x } => match op {
            FloatUnaryOp::Neg => unary_register(&mut scratch.f32s, dst, x, lanes, |x| -x),
            FloatUnaryOp::Abs => unary_register(&mut scratch.f32s, dst, x, lanes, f32::abs),
            FloatUnaryOp::Floor => unary_register(&mut scratch.f32s, dst, x, lanes, f32::floor),
            FloatUnaryOp::Ceil => unary_register(&mut scratch.f32s, dst, x, lanes, f32::ceil),
            FloatUnaryOp::Round => unary_register(&mut scratch.f32s, dst, x, lanes, f32::round),
            FloatUnaryOp::Sqrt => unary_register(&mut scratch.f32s, dst, x, lanes, f32::sqrt),
            FloatUnaryOp::SinDegrees => unary_register(&mut scratch.f32s, dst, x, lanes, |x| x.to_radians().sin()),
            FloatUnaryOp::CosDegrees => unary_register(&mut scratch.f32s, dst, x, lanes, |x| x.to_radians().cos()),
        },
        KernelOp::F64Unary { op, dst, x } => match op {
            FloatUnaryOp::Neg => unary_register(&mut scratch.f64s, dst, x, lanes, |x| -x),
            FloatUnaryOp::Abs => unary_register(&mut scratch.f64s, dst, x, lanes, f64::abs),
            FloatUnaryOp::Floor => unary_register(&mut scratch.f64s, dst, x, lanes, f64::floor),
            FloatUnaryOp::Ceil => unary_register(&mut scratch.f64s, dst, x, lanes, f64::ceil),
            FloatUnaryOp::Round => unary_register(&mut scratch.f64s, dst, x, lanes, f64::round),
            FloatUnaryOp::Sqrt => unary_register(&mut scratch.f64s, dst, x, lanes, f64::sqrt),
            FloatUnaryOp::SinDegrees => unary_register(&mut scratch.f64s, dst, x, lanes, |x| x.to_radians().sin()),
            FloatUnaryOp::CosDegrees => unary_register(&mut scratch.f64s, dst, x, lanes, |x| x.to_radians().cos()),
        },
        KernelOp::F32Binary { op, dst, a, b } => match op {
            FloatBinaryOp::Add => binary_register(&mut scratch.f32s, dst, a, b, lanes, |a, b| a + b),
            FloatBinaryOp::Sub => binary_register(&mut scratch.f32s, dst, a, b, lanes, |a, b| a - b),
            FloatBinaryOp::Mul => binary_register(&mut scratch.f32s, dst, a, b, lanes, |a, b| a * b),
            FloatBinaryOp::Div => binary_register(&mut scratch.f32s, dst, a, b, lanes, |a, b| a / b),
            FloatBinaryOp::Min => binary_register(&mut scratch.f32s, dst, a, b, lanes, f32::min),
            FloatBinaryOp::Max => binary_register(&mut scratch.f32s, dst, a, b, lanes, f32::max),
            FloatBinaryOp::Pow => binary_register(&mut scratch.f32s, dst, a, b, lanes, f32::powf),
            FloatBinaryOp::Mod => binary_register(&mut scratch.f32s, dst, a, b, lanes, f32::rem_euclid),
            FloatBinaryOp::Quot => binary_register(&mut scratch.f32s, dst, a, b, lanes, |a, b| (a / b).trunc()),
            FloatBinaryOp::Atan2Degrees => {
                binary_register(&mut scratch.f32s, dst, a, b, lanes, |y, x| y.atan2(x).to_degrees())
            }
        },
        KernelOp::F64Binary { op, dst, a, b } => match op {
            FloatBinaryOp::Add => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| a + b),
            FloatBinaryOp::Sub => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| a - b),
            FloatBinaryOp::Mul => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| a * b),
            FloatBinaryOp::Div => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| a / b),
            FloatBinaryOp::Min => binary_register(&mut scratch.f64s, dst, a, b, lanes, f64::min),
            FloatBinaryOp::Max => binary_register(&mut scratch.f64s, dst, a, b, lanes, f64::max),
            FloatBinaryOp::Pow => binary_register(&mut scratch.f64s, dst, a, b, lanes, f64::powf),
            FloatBinaryOp::Mod => binary_register(&mut scratch.f64s, dst, a, b, lanes, f64::rem_euclid),
            FloatBinaryOp::Quot => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| (a / b).trunc()),
            FloatBinaryOp::Atan2Degrees => {
                binary_register(&mut scratch.f64s, dst, a, b, lanes, |y, x| y.atan2(x).to_degrees())
            }
        },
        KernelOp::U32Binary { op, dst, a, b } => match op {
            IntegerBinaryOp::Add => binary_register(&mut scratch.u32s, dst, a, b, lanes, u32::wrapping_add),
            IntegerBinaryOp::Sub => binary_register(&mut scratch.u32s, dst, a, b, lanes, u32::wrapping_sub),
            IntegerBinaryOp::Mul => binary_register(&mut scratch.u32s, dst, a, b, lanes, u32::wrapping_mul),
        },
        KernelOp::U64Binary { op, dst, a, b } => match op {
            IntegerBinaryOp::Add => binary_register(&mut scratch.u64s, dst, a, b, lanes, u64::wrapping_add),
            IntegerBinaryOp::Sub => binary_register(&mut scratch.u64s, dst, a, b, lanes, u64::wrapping_sub),
            IntegerBinaryOp::Mul => binary_register(&mut scratch.u64s, dst, a, b, lanes, u64::wrapping_mul),
        },
        KernelOp::F32Compare { op, dst, a, b } => match op {
            FloatCompareOp::Eq => compare_register(&scratch.f32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
            FloatCompareOp::Lt => compare_register(&scratch.f32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a < b),
            FloatCompareOp::Lte => compare_register(&scratch.f32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a <= b),
            FloatCompareOp::Gt => compare_register(&scratch.f32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a > b),
            FloatCompareOp::Gte => compare_register(&scratch.f32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a >= b),
        },
        KernelOp::F64Compare { op, dst, a, b } => match op {
            FloatCompareOp::Eq => compare_register(&scratch.f64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
            FloatCompareOp::Lt => compare_register(&scratch.f64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a < b),
            FloatCompareOp::Lte => compare_register(&scratch.f64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a <= b),
            FloatCompareOp::Gt => compare_register(&scratch.f64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a > b),
            FloatCompareOp::Gte => compare_register(&scratch.f64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a >= b),
        },
        KernelOp::F64NumericCompare { op, dst, a, b } => match op {
            FloatCompareOp::Eq => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| {
                if (a - b).abs() < 1e-9 { 1.0 } else { 0.0 }
            }),
            FloatCompareOp::Lt => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| {
                if a < b { 1.0 } else { 0.0 }
            }),
            FloatCompareOp::Lte => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| {
                if a <= b { 1.0 } else { 0.0 }
            }),
            FloatCompareOp::Gt => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| {
                if a > b { 1.0 } else { 0.0 }
            }),
            FloatCompareOp::Gte => binary_register(&mut scratch.f64s, dst, a, b, lanes, |a, b| {
                if a >= b { 1.0 } else { 0.0 }
            }),
        },
        KernelOp::F64NumericNot { dst, x } => unary_register(&mut scratch.f64s, dst, x, lanes, |x| {
            if x == 0.0 { 1.0 } else { 0.0 }
        }),
        KernelOp::F64Sine { dst, period, amp, x } => {
            let (before, destination) = destination_slice(&mut scratch.f64s, dst, lanes);
            let period = register_slice(before, period, lanes);
            let amp = register_slice(before, amp, lanes);
            let x = register_slice(before, x, lanes);
            for lane in 0..lanes {
                destination[lane] =
                    amp[lane] * (std::f64::consts::TAU * x[lane] / period[lane]).sin();
            }
        }
        KernelOp::F64Lerp { dst, a, b, ctrl, v1, v2 } => {
            let (before, destination) = destination_slice(&mut scratch.f64s, dst, lanes);
            let a = register_slice(before, a, lanes);
            let b = register_slice(before, b, lanes);
            let ctrl = register_slice(before, ctrl, lanes);
            let v1 = register_slice(before, v1, lanes);
            let v2 = register_slice(before, v2, lanes);
            for lane in 0..lanes {
                let r = ((ctrl[lane] - a[lane]) / (b[lane] - a[lane])).clamp(0.0, 1.0);
                destination[lane] = v1[lane] + r * (v2[lane] - v1[lane]);
            }
        }
        KernelOp::F64Lerp3 { dst, a1, b1, a2, b2, ctrl, v1, v2, v3 } => {
            let (before, destination) = destination_slice(&mut scratch.f64s, dst, lanes);
            let a1 = register_slice(before, a1, lanes);
            let b1 = register_slice(before, b1, lanes);
            let a2 = register_slice(before, a2, lanes);
            let b2 = register_slice(before, b2, lanes);
            let ctrl = register_slice(before, ctrl, lanes);
            let v1 = register_slice(before, v1, lanes);
            let v2 = register_slice(before, v2, lanes);
            let v3 = register_slice(before, v3, lanes);
            for lane in 0..lanes {
                destination[lane] = if ctrl[lane] < a2[lane] {
                    let r =
                        ((ctrl[lane] - a1[lane]) / (b1[lane] - a1[lane])).clamp(0.0, 1.0);
                    v1[lane] + r * (v2[lane] - v1[lane])
                } else {
                    let r =
                        ((ctrl[lane] - a2[lane]) / (b2[lane] - a2[lane])).clamp(0.0, 1.0);
                    v2[lane] + r * (v3[lane] - v2[lane])
                };
            }
        }
        KernelOp::F64Ease { dst, kind, x } => {
            unary_register(&mut scratch.f64s, dst, x, lanes, |x| ease_f64(kind, x))
        }
        KernelOp::F64LerpSmooth { dst, kind, a, b, ctrl, v1, v2 } => {
            let (before, destination) = destination_slice(&mut scratch.f64s, dst, lanes);
            let a = register_slice(before, a, lanes);
            let b = register_slice(before, b, lanes);
            let ctrl = register_slice(before, ctrl, lanes);
            let v1 = register_slice(before, v1, lanes);
            let v2 = register_slice(before, v2, lanes);
            for lane in 0..lanes {
                let r = ((ctrl[lane] - a[lane]) / (b[lane] - a[lane])).clamp(0.0, 1.0);
                destination[lane] = v1[lane] + ease_f64(kind, r) * (v2[lane] - v1[lane]);
            }
        }
        KernelOp::F64Lssht { dst, c, pv, f1, f2 } => {
            let (before, destination) = destination_slice(&mut scratch.f64s, dst, lanes);
            let c = register_slice(before, c, lanes);
            let pv = register_slice(before, pv, lanes);
            let f1 = register_slice(before, f1, lanes);
            let f2 = register_slice(before, f2, lanes);
            for lane in 0..lanes {
                let c = c[lane];
                let pv = pv[lane];
                let _w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
                let m = (c * f1[lane]).exp() + (c * f2[lane]).exp();
                destination[lane] = m.ln() / c;
            }
        }
        KernelOp::U32Compare { op, dst, a, b } => match op {
            IntegerCompareOp::Eq => compare_register(&scratch.u32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
            IntegerCompareOp::Lt => compare_register(&scratch.u32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a < b),
            IntegerCompareOp::Lte => compare_register(&scratch.u32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a <= b),
            IntegerCompareOp::Gt => compare_register(&scratch.u32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a > b),
            IntegerCompareOp::Gte => compare_register(&scratch.u32s, &mut scratch.masks, dst, a, b, lanes, |a, b| a >= b),
        },
        KernelOp::U64Compare { op, dst, a, b } => match op {
            IntegerCompareOp::Eq => compare_register(&scratch.u64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
            IntegerCompareOp::Lt => compare_register(&scratch.u64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a < b),
            IntegerCompareOp::Lte => compare_register(&scratch.u64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a <= b),
            IntegerCompareOp::Gt => compare_register(&scratch.u64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a > b),
            IntegerCompareOp::Gte => compare_register(&scratch.u64s, &mut scratch.masks, dst, a, b, lanes, |a, b| a >= b),
        },
        KernelOp::SymbolEq { dst, a, b } => compare_register(&scratch.symbols, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
        KernelOp::HandleEq { dst, a, b } => compare_register(&scratch.handles, &mut scratch.masks, dst, a, b, lanes, |a, b| a == b),
        KernelOp::MaskBinary { op, dst, a, b } => match op {
            MaskBinaryOp::And => binary_register(&mut scratch.masks, dst, a, b, lanes, |a, b| MaskLane::new(a.get() && b.get())),
            MaskBinaryOp::Or => binary_register(&mut scratch.masks, dst, a, b, lanes, |a, b| MaskLane::new(a.get() || b.get())),
            MaskBinaryOp::Xor => binary_register(&mut scratch.masks, dst, a, b, lanes, |a, b| MaskLane::new(a.get() ^ b.get())),
        },
        KernelOp::MaskNot { dst, x } => unary_register(&mut scratch.masks, dst, x, lanes, |x| MaskLane::new(!x.get())),
        KernelOp::SelectF32 { dst, mask, yes, no } => select_register(&mut scratch.f32s, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectF64 { dst, mask, yes, no } => select_register(&mut scratch.f64s, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectU32 { dst, mask, yes, no } => select_register(&mut scratch.u32s, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectU64 { dst, mask, yes, no } => select_register(&mut scratch.u64s, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectSymbol { dst, mask, yes, no } => select_register(&mut scratch.symbols, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectHandle { dst, mask, yes, no } => select_register(&mut scratch.handles, &scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::SelectMask { dst, mask, yes, no } => select_mask_register(&mut scratch.masks, dst, mask, yes, no, lanes),
        KernelOp::F32ToF64 { dst, x } => convert_register(&scratch.f32s, &mut scratch.f64s, x, dst, lanes, |x| x as f64),
        KernelOp::F64ToF32 { dst, x } => convert_register(&scratch.f64s, &mut scratch.f32s, x, dst, lanes, |x| x as f32),
        KernelOp::U32ToU64 { dst, x } => convert_register(&scratch.u32s, &mut scratch.u64s, x, dst, lanes, u64::from),
        KernelOp::U64ToU32 { dst, x } => convert_register(&scratch.u64s, &mut scratch.u32s, x, dst, lanes, |x| x as u32),
        KernelOp::U32ToF32 { dst, x } => convert_register(&scratch.u32s, &mut scratch.f32s, x, dst, lanes, |x| x as f32),
        KernelOp::U32ToF64 { dst, x } => convert_register(&scratch.u32s, &mut scratch.f64s, x, dst, lanes, |x| x as f64),
        KernelOp::U64ToF32 { dst, x } => convert_register(&scratch.u64s, &mut scratch.f32s, x, dst, lanes, |x| x as f32),
        KernelOp::U64ToF64 { dst, x } => convert_register(&scratch.u64s, &mut scratch.f64s, x, dst, lanes, |x| x as f64),
        KernelOp::F32ToU32 { dst, x } => convert_register(&scratch.f32s, &mut scratch.u32s, x, dst, lanes, |x| x as u32),
        KernelOp::F32ToU64 { dst, x } => convert_register(&scratch.f32s, &mut scratch.u64s, x, dst, lanes, |x| x as u64),
        KernelOp::F64ToU32 { dst, x } => convert_register(&scratch.f64s, &mut scratch.u32s, x, dst, lanes, |x| x as u32),
        KernelOp::F64ToU64 { dst, x } => convert_register(&scratch.f64s, &mut scratch.u64s, x, dst, lanes, |x| x as u64),
        KernelOp::U32ToSymbol { dst, x } => convert_register(&scratch.u32s, &mut scratch.symbols, x, dst, lanes, |x| x),
        KernelOp::SymbolToU32 { dst, x } => convert_register(&scratch.symbols, &mut scratch.u32s, x, dst, lanes, |x| x),
        KernelOp::U64ToHandle { dst, x } => convert_register(&scratch.u64s, &mut scratch.handles, x, dst, lanes, |x| x),
        KernelOp::HandleToU64 { dst, x } => convert_register(&scratch.handles, &mut scratch.u64s, x, dst, lanes, |x| x),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::read_all;
    use crate::interp::{lower_num_form, run_lanes, Env, NumOp, NumProgram};
    use std::collections::HashMap;
    use std::time::Instant;

    fn bindings(inputs: &[KernelInputRef], output_count: usize) -> KernelBindings {
        KernelBindings {
            inputs: inputs
                .iter()
                .copied()
                .enumerate()
                .map(|(slot, input)| KernelInputBinding {
                    input,
                    source: KernelInputSource::Direct { column: slot as u16 },
                })
                .collect(),
            outputs: (0..output_count)
                .map(|output| KernelOutputBinding {
                    output: output as u16,
                    target: KernelOutputTarget::Driver,
                })
                .collect(),
        }
    }

    fn plan(program: KernelProgram, inputs: &[KernelInputRef]) -> KernelPlan {
        let output_count = program.outputs().len();
        KernelPlan::new(
            Rc::new(program),
            IterationDomain::EntityRows,
            bindings(inputs, output_count),
            FallbackPolicy::WholePlanInterpreted,
            MergePolicy::Direct,
        )
        .unwrap()
    }

    #[test]
    fn executes_integer_symbol_mask_and_select_lanes() {
        let program = KernelProgram::new(
            KernelLayout { u32s: 1, symbols: 1, ..KernelLayout::default() },
            KernelLayout { u32s: 3, symbols: 3, masks: 1, ..KernelLayout::default() },
            vec![KernelRegister::U32(2), KernelRegister::Symbol(2), KernelRegister::Mask(0)],
            vec![
                KernelOp::LoadU32 { dst: 0, input: 0 },
                KernelOp::ConstU32 { dst: 1, value: 10 },
                KernelOp::LoadSymbol { dst: 0, input: 0 },
                KernelOp::ConstSymbol { dst: 1, value: 7 },
                KernelOp::SymbolEq { dst: 0, a: 0, b: 1 },
                KernelOp::U32Binary { op: IntegerBinaryOp::Add, dst: 2, a: 0, b: 1 },
                KernelOp::SelectSymbol { dst: 2, mask: 0, yes: 0, no: 1 },
            ],
        )
        .unwrap();
        let plan = plan(program, &[KernelInputRef::U32(0), KernelInputRef::Symbol(0)]);
        let inputs = KernelLanes {
            lanes: 3,
            u32s: vec![1, u32::MAX, 9],
            symbols: vec![7, 3, 7],
            ..KernelLanes::default()
        };
        let mut scratch = KernelScratch::default();
        let mut outputs = KernelOutputs::default();
        execute(&plan, &inputs, &mut scratch, &mut outputs).unwrap();
        assert_eq!(outputs.u32s, vec![11, 9, 19]);
        assert_eq!(outputs.symbols, vec![7, 7, 7]);
        assert_eq!(outputs.masks, vec![MaskLane::TRUE, MaskLane::FALSE, MaskLane::TRUE]);
    }

    #[test]
    fn preserves_optional_orientation_presence_and_flattened_outputs() {
        let program = KernelProgram::new(
            KernelLayout { f64s: 3, masks: 1, ..KernelLayout::default() },
            KernelLayout { f64s: 3, masks: 1, ..KernelLayout::default() },
            vec![
                KernelRegister::F64(0),
                KernelRegister::F64(1),
                KernelRegister::F64(2),
                KernelRegister::Mask(0),
            ],
            vec![
                KernelOp::LoadF64 { dst: 0, input: 0 },
                KernelOp::LoadF64 { dst: 1, input: 1 },
                KernelOp::LoadF64 { dst: 2, input: 2 },
                KernelOp::LoadMask { dst: 0, input: 0 },
            ],
        )
        .unwrap();
        let mut installed = bindings(
            &[
                KernelInputRef::F64(0),
                KernelInputRef::F64(1),
                KernelInputRef::F64(2),
                KernelInputRef::Mask(0),
            ],
            4,
        );
        installed.outputs[3].target = KernelOutputTarget::Presence { column: 9 };
        let plan = KernelPlan::new(
            Rc::new(program),
            IterationDomain::RenderRows,
            installed,
            FallbackPolicy::WholePlanInterpreted,
            MergePolicy::Direct,
        )
        .unwrap();
        let inputs = KernelLanes {
            lanes: 2,
            f64s: vec![1.0, 4.0, 2.0, 5.0, 0.0, 0.0],
            masks: vec![MaskLane::FALSE, MaskLane::TRUE],
            ..KernelLanes::default()
        };
        let mut scratch = KernelScratch::default();
        let mut outputs = KernelOutputs::default();
        execute(&plan, &inputs, &mut scratch, &mut outputs).unwrap();
        assert_eq!(outputs.f64s, vec![1.0, 4.0, 2.0, 5.0, 0.0, 0.0]);
        assert_eq!(outputs.masks, vec![MaskLane::FALSE, MaskLane::TRUE]);
        assert_eq!(outputs.lanes, 2);
    }

    #[test]
    fn structural_identity_includes_width_layout_outputs_and_op_order() {
        let f32_program = KernelProgram::new(
            KernelLayout { f32s: 1, ..KernelLayout::default() },
            KernelLayout { f32s: 1, ..KernelLayout::default() },
            vec![KernelRegister::F32(0)],
            vec![KernelOp::LoadF32 { dst: 0, input: 0 }],
        )
        .unwrap();
        let same = f32_program.clone();
        let f64_program = KernelProgram::new(
            KernelLayout { f64s: 1, ..KernelLayout::default() },
            KernelLayout { f64s: 1, ..KernelLayout::default() },
            vec![KernelRegister::F64(0)],
            vec![KernelOp::LoadF64 { dst: 0, input: 0 }],
        )
        .unwrap();
        let reversed_outputs = KernelProgram::new(
            KernelLayout::default(),
            KernelLayout { u32s: 2, ..KernelLayout::default() },
            vec![KernelRegister::U32(1), KernelRegister::U32(0)],
            vec![
                KernelOp::ConstU32 { dst: 0, value: 1 },
                KernelOp::ConstU32 { dst: 1, value: 2 },
            ],
        )
        .unwrap();
        let forward_outputs = KernelProgram::new(
            KernelLayout::default(),
            KernelLayout { u32s: 2, ..KernelLayout::default() },
            vec![KernelRegister::U32(0), KernelRegister::U32(1)],
            vec![
                KernelOp::ConstU32 { dst: 0, value: 1 },
                KernelOp::ConstU32 { dst: 1, value: 2 },
            ],
        )
        .unwrap();

        let first = intern_kernel_program(f32_program);
        let second = intern_kernel_program(same);
        assert!(Rc::ptr_eq(&first, &second));
        assert_ne!(first.id(), f64_program.id());
        assert_ne!(reversed_outputs.id(), forward_outputs.id());
    }

    #[test]
    fn rejects_invalid_programs_and_plans_before_execution() {
        let invalid = KernelProgram::new(
            KernelLayout::default(),
            KernelLayout { u32s: 1, ..KernelLayout::default() },
            vec![KernelRegister::U32(0)],
            vec![KernelOp::LoadU32 { dst: 0, input: 0 }],
        );
        assert!(matches!(invalid, Err(KernelValidationError::InvalidInput { .. })));

        let program = KernelProgram::new(
            KernelLayout { handles: 1, f64s: 1, ..KernelLayout::default() },
            KernelLayout { f64s: 1, ..KernelLayout::default() },
            vec![KernelRegister::F64(0)],
            vec![KernelOp::LoadF64 { dst: 0, input: 0 }],
        )
        .unwrap();
        let missing = KernelPlan::new(
            Rc::new(program.clone()),
            IterationDomain::EntityRows,
            bindings(&[KernelInputRef::F64(0)], 1),
            FallbackPolicy::WholePlanInterpreted,
            MergePolicy::Direct,
        );
        assert_eq!(missing.unwrap_err(), KernelPlanError::MissingInput(KernelInputRef::Handle(0)));

        let undeclared = KernelPlan::new(
            Rc::new(program),
            IterationDomain::EntityRows,
            bindings(
                &[KernelInputRef::F64(0), KernelInputRef::Handle(0), KernelInputRef::U64(0)],
                1,
            ),
            FallbackPolicy::WholePlanInterpreted,
            MergePolicy::Direct,
        );
        assert_eq!(undeclared.unwrap_err(), KernelPlanError::UndeclaredInput(KernelInputRef::U64(0)));
    }

    #[test]
    fn runtime_accepts_only_declared_input_shapes() {
        let program = KernelProgram::new(
            KernelLayout { u64s: 1, ..KernelLayout::default() },
            KernelLayout { u64s: 1, ..KernelLayout::default() },
            vec![KernelRegister::U64(0)],
            vec![KernelOp::LoadU64 { dst: 0, input: 0 }],
        )
        .unwrap();
        let plan = plan(program, &[KernelInputRef::U64(0)]);
        let inputs = KernelLanes { lanes: 2, u64s: vec![1, 2], u32s: vec![99], ..KernelLanes::default() };
        let error = execute(
            &plan,
            &inputs,
            &mut KernelScratch::default(),
            &mut KernelOutputs::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            KernelExecError::InputLength { ty: KernelType::U32, expected: 0, actual: 1 }
        );
    }

    fn f64_typed_plan() -> KernelPlan {
        let program = KernelProgram::new(
            KernelLayout { f64s: 2, ..KernelLayout::default() },
            KernelLayout { f64s: 5, ..KernelLayout::default() },
            vec![KernelRegister::F64(4)],
            vec![
                KernelOp::LoadF64 { dst: 0, input: 0 },
                KernelOp::LoadF64 { dst: 1, input: 1 },
                KernelOp::F64Binary { op: FloatBinaryOp::Mul, dst: 2, a: 0, b: 1 },
                KernelOp::ConstF64 { dst: 3, bits: 3.25_f64.to_bits() },
                KernelOp::F64Binary { op: FloatBinaryOp::Add, dst: 4, a: 2, b: 3 },
            ],
        )
        .unwrap();
        plan(program, &[KernelInputRef::F64(0), KernelInputRef::F64(1)])
    }

    fn f64_num_program() -> NumProgram {
        NumProgram {
            ops: vec![
                NumOp::Input { dst: 0, slot: 0 },
                NumOp::T { dst: 1 },
                NumOp::Mul { dst: 2, a: 0, b: 1 },
                NumOp::Const { dst: 3, v: 3.25 },
                NumOp::Add { dst: 4, a: 2, b: 3 },
            ],
            n_regs: 5,
            n_inputs: 1,
            aux: None,
            result: 4,
        }
    }

    #[test]
    fn typed_f64_specialization_matches_run_lanes_bits() {
        let lanes = 257;
        let captures: Vec<f64> = (0..lanes).map(|lane| lane as f64 * 0.25 - 5.0).collect();
        let tau: Vec<f64> = (0..lanes).map(|lane| lane as f64 * 0.03125).collect();
        let typed_inputs = KernelLanes {
            lanes,
            f64s: captures.iter().chain(tau.iter()).copied().collect(),
            ..KernelLanes::default()
        };
        let mut typed_outputs = KernelOutputs::default();
        execute(
            &f64_typed_plan(),
            &typed_inputs,
            &mut KernelScratch::default(),
            &mut typed_outputs,
        )
        .unwrap();

        let mut old_registers = Vec::new();
        let mut old_outputs = Vec::new();
        run_lanes(
            &f64_num_program(),
            0.0,
            &tau,
            &vec![[0.0, 0.0]; lanes],
            &captures,
            &mut old_registers,
            &mut old_outputs,
        );
        assert_eq!(
            typed_outputs.f64s.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
            old_outputs.iter().map(|value| value.to_bits()).collect::<Vec<_>>()
        );
    }
    #[test]
    fn num_bridge_preserves_full_f64_op_order_and_input_metadata() {
        let sources = [
            "(+ (* 2 t) (- (:x pos) (/ (:y pos) 3)))",
            "(min (max t 2) (mod t 7))",
            "(sine 12.94 2 t)",
            "(lerp 0.3 1.4 t 0 2.6)",
            "(lerp3 0 1 1 2 t 0 5 9)",
            "(lerpsmooth eiosine 0 4 t 0 480)",
            "(+ (< t 3) (sqrt (abs t)))",
            "(quot (floor (* t 3)) (ceil (+ t 0.1)))",
            "(einsine (mod t 1))",
            "(lssht 0.5 t 2 4)",
        ];
        let lanes = 31;
        let tau = (0..lanes).map(|lane| lane as f64 * 0.37 - 2.0).collect::<Vec<_>>();
        let pos = (0..lanes)
            .map(|lane| [lane as f64 * 1.3 + 0.5, 5.0 - lane as f64])
            .collect::<Vec<_>>();
        let axis = 0.25;

        for source in sources {
            let form = read_all(source).unwrap().into_iter().next().unwrap();
            let num = lower_num_form(&form, &Env::empty(), &HashMap::new())
                .unwrap_or_else(|| panic!("unlowered bridge test source: {source}"));
            let bridge = kernel_program_for_num(&num).unwrap();
            let mut f64s = Vec::with_capacity(bridge.inputs.len() * lanes);
            for input in bridge.inputs.iter().copied() {
                match input {
                    NumKernelInputSource::Capture(slot) => {
                        panic!("unexpected capture {slot} in {source}")
                    }
                    NumKernelInputSource::Tick => f64s.extend_from_slice(&tau),
                    NumKernelInputSource::Axis => f64s.extend(std::iter::repeat_n(axis, lanes)),
                    NumKernelInputSource::PositionX => {
                        f64s.extend(pos.iter().map(|position| position[0]))
                    }
                    NumKernelInputSource::PositionY => {
                        f64s.extend(pos.iter().map(|position| position[1]))
                    }
                    NumKernelInputSource::Aux(index) => {
                        panic!("unexpected aux {index} in {source}")
                    }
                }
            }
            let input_refs = (0..bridge.inputs.len())
                .map(|input| KernelInputRef::F64(input as u16))
                .collect::<Vec<_>>();
            let typed_plan = plan(bridge.program.as_ref().clone(), &input_refs);
            let mut typed_outputs = KernelOutputs::default();
            execute(
                &typed_plan,
                &KernelLanes { lanes, f64s, ..KernelLanes::default() },
                &mut KernelScratch::default(),
                &mut typed_outputs,
            )
            .unwrap();

            let mut num_registers = Vec::new();
            let mut num_outputs = Vec::new();
            run_lanes(
                &num,
                axis,
                &tau,
                &pos,
                &[],
                &mut num_registers,
                &mut num_outputs,
            );
            assert_eq!(
                typed_outputs.f64s.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                num_outputs.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                "{source}"
            );
        }
    }


    /// Manual perf gate: run with
    /// `cargo test -p maku sim::kernel::tests::typed_f64_perf_harness --release -- --ignored --nocapture`.
    /// It intentionally reports timings rather than asserting on noisy CI.
    #[test]
    #[ignore]
    fn typed_f64_perf_harness() {
        let lanes = 65_536;
        let iterations = 100;
        let captures: Vec<f64> = (0..lanes).map(|lane| lane as f64 * 0.125 - 50.0).collect();
        let tau: Vec<f64> = (0..lanes).map(|lane| lane as f64 * 0.000_976_562_5).collect();
        let pos = vec![[0.0, 0.0]; lanes];
        let typed_inputs = KernelLanes {
            lanes,
            f64s: captures.iter().chain(tau.iter()).copied().collect(),
            ..KernelLanes::default()
        };
        let typed_plan = f64_typed_plan();
        let num_program = f64_num_program();
        let mut typed_scratch = KernelScratch::default();
        let mut typed_outputs = KernelOutputs::default();
        let mut num_scratch = Vec::new();
        let mut num_outputs = Vec::new();

        execute(&typed_plan, &typed_inputs, &mut typed_scratch, &mut typed_outputs).unwrap();
        run_lanes(&num_program, 0.0, &tau, &pos, &captures, &mut num_scratch, &mut num_outputs);

        let start = Instant::now();
        for _ in 0..iterations {
            run_lanes(&num_program, 0.0, &tau, &pos, &captures, &mut num_scratch, &mut num_outputs);
        }
        let num_elapsed = start.elapsed();
        let start = Instant::now();
        for _ in 0..iterations {
            execute(&typed_plan, &typed_inputs, &mut typed_scratch, &mut typed_outputs).unwrap();
        }
        let typed_elapsed = start.elapsed();
        assert_eq!(typed_outputs.f64s.last().unwrap().to_bits(), num_outputs.last().unwrap().to_bits());
        eprintln!(
            "typed F64 {:?}; NumProgram/run_lanes {:?}; ratio {:.3}",
            typed_elapsed,
            num_elapsed,
            typed_elapsed.as_secs_f64() / num_elapsed.as_secs_f64()
        );
    }
}
