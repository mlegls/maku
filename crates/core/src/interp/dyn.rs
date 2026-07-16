//! Prototype typed dynamic values.
//!
//! This is a transitional representation. The target `Dyn<T>` should become a
//! typed time-varying value/program with structure lifting and compile-time
//! schemas; this file is the current interpreter-backed shell.

use super::*;
use crate::edn::Form;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FixedInput {
    Tick,
    Axis,
    Slot(u16),
}

#[derive(Clone, Debug)]
pub(crate) struct FixedKernel {
    pub(crate) program: Rc<KernelProgram>,
    pub(crate) inputs: Vec<FixedInput>,
}

pub(crate) enum FixedScalar<'a> {
    Const(f64),
    Form(&'a Form, &'a Env),
}

#[derive(Default)]
struct FixedKernelBuilder {
    ops: Vec<KernelOp>,
    f64s: u16,
    masks: u16,
    inputs: Vec<FixedInput>,
}

impl FixedKernelBuilder {
    fn f64_reg(&mut self) -> Option<u16> {
        let dst = self.f64s;
        self.f64s = self.f64s.checked_add(1)?;
        Some(dst)
    }

    fn mask_reg(&mut self) -> Option<u16> {
        let dst = self.masks;
        self.masks = self.masks.checked_add(1)?;
        Some(dst)
    }

    fn input(&mut self, source: FixedInput) -> Option<u16> {
        if let Some(index) = self.inputs.iter().position(|existing| *existing == source) {
            return u16::try_from(index).ok();
        }
        let index = u16::try_from(self.inputs.len()).ok()?;
        self.inputs.push(source);
        Some(index)
    }

    fn constant(&mut self, value: f64) -> Option<u16> {
        let dst = self.f64_reg()?;
        self.ops.push(KernelOp::ConstF64 {
            dst,
            bits: value.to_bits(),
        });
        Some(dst)
    }

    fn load(&mut self, source: FixedInput) -> Option<u16> {
        let input = self.input(source)?;
        let dst = self.f64_reg()?;
        self.ops.push(KernelOp::LoadF64 { dst, input });
        Some(dst)
    }

    fn unary(&mut self, op: FloatUnaryOp, x: u16) -> Option<u16> {
        let dst = self.f64_reg()?;
        self.ops.push(KernelOp::F64Unary { op, dst, x });
        Some(dst)
    }

    fn binary(&mut self, op: FloatBinaryOp, a: u16, b: u16) -> Option<u16> {
        let dst = self.f64_reg()?;
        self.ops.push(KernelOp::F64Binary { op, dst, a, b });
        Some(dst)
    }

    fn compare(&mut self, op: FloatCompareOp, a: u16, b: u16) -> Option<u16> {
        let mask = self.mask_reg()?;
        self.ops.push(KernelOp::F64Compare {
            op,
            dst: mask,
            a,
            b,
        });
        let yes = self.constant(1.0)?;
        let no = self.constant(0.0)?;
        let dst = self.f64_reg()?;
        self.ops.push(KernelOp::SelectF64 {
            dst,
            mask,
            yes,
            no,
        });
        Some(dst)
    }

    fn append_program(&mut self, program: &NumProgram) -> Option<u16> {
        if !program.aux_free() {
            return None;
        }
        let mut registers = vec![None; program.n_regs];
        let get = |registers: &[Option<u16>], index: u16| registers.get(usize::from(index)).copied().flatten();
        for op in program.ops.iter().copied() {
            let (legacy_dst, value) = match op {
                NumOp::Const { dst, v } => (dst, self.constant(v)?),
                NumOp::Input { dst, slot } => (dst, self.load(FixedInput::Slot(slot))?),
                NumOp::T { dst } => (dst, self.load(FixedInput::Tick)?),
                NumOp::U { dst } => (dst, self.load(FixedInput::Axis)?),
                NumOp::Add { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Add, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Sub { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Sub, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Mul { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Mul, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Div { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Div, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Pow { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Pow, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Min { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Min, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Max { dst, a, b } => (
                    dst,
                    self.binary(FloatBinaryOp::Max, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Neg { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Neg, get(&registers, x)?)?,
                ),
                NumOp::Abs { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Abs, get(&registers, x)?)?,
                ),
                NumOp::Floor { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Floor, get(&registers, x)?)?,
                ),
                NumOp::Ceil { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Ceil, get(&registers, x)?)?,
                ),
                NumOp::Round { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Round, get(&registers, x)?)?,
                ),
                NumOp::Sin { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::SinDegrees, get(&registers, x)?)?,
                ),
                NumOp::Cos { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::CosDegrees, get(&registers, x)?)?,
                ),
                NumOp::Sqrt { dst, x } => (
                    dst,
                    self.unary(FloatUnaryOp::Sqrt, get(&registers, x)?)?,
                ),
                NumOp::Eq { dst, a, b } => {
                    let delta =
                        self.binary(FloatBinaryOp::Sub, get(&registers, a)?, get(&registers, b)?)?;
                    let delta = self.unary(FloatUnaryOp::Abs, delta)?;
                    let epsilon = self.constant(1e-9)?;
                    (dst, self.compare(FloatCompareOp::Lt, delta, epsilon)?)
                }
                NumOp::Lt { dst, a, b } => (
                    dst,
                    self.compare(FloatCompareOp::Lt, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Gt { dst, a, b } => (
                    dst,
                    self.compare(FloatCompareOp::Gt, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Lte { dst, a, b } => (
                    dst,
                    self.compare(FloatCompareOp::Lte, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Gte { dst, a, b } => (
                    dst,
                    self.compare(FloatCompareOp::Gte, get(&registers, a)?, get(&registers, b)?)?,
                ),
                NumOp::Not { dst, x } => {
                    let zero = self.constant(0.0)?;
                    (dst, self.compare(FloatCompareOp::Eq, get(&registers, x)?, zero)?)
                }
                NumOp::PosX { .. }
                | NumOp::PosY { .. }
                | NumOp::Mod { .. }
                | NumOp::Quot { .. }
                | NumOp::Sine { .. }
                | NumOp::Lerp { .. }
                | NumOp::Lerp3 { .. }
                | NumOp::Ease { .. }
                | NumOp::LerpSmooth { .. }
                | NumOp::Lssht { .. }
                | NumOp::AuxIn { .. }
                | NumOp::Atan2 { .. } => return None,
            };
            *registers.get_mut(usize::from(legacy_dst))? = Some(value);
        }
        registers.get(usize::from(program.result)).copied().flatten()
    }
}

pub(crate) fn lower_fixed_scalars(
    scalars: &[FixedScalar<'_>],
    defs: &std::collections::HashMap<String, Form>,
) -> Option<FixedKernel> {
    let mut builder = FixedKernelBuilder::default();
    let mut outputs = Vec::with_capacity(scalars.len());
    for scalar in scalars {
        let output = match scalar {
            FixedScalar::Const(value) => builder.constant(*value)?,
            FixedScalar::Form(form, env) => {
                let program = lower_num_form(form, env, defs)?;
                builder.append_program(&program)?
            }
        };
        outputs.push(KernelRegister::F64(output));
    }
    let inputs = KernelLayout {
        f64s: u16::try_from(builder.inputs.len()).ok()?,
        ..KernelLayout::default()
    };
    let registers = KernelLayout {
        f64s: builder.f64s,
        masks: builder.masks,
        ..KernelLayout::default()
    };
    let program = KernelProgram::new(inputs, registers, outputs, builder.ops).ok()?;
    Some(FixedKernel {
        program: intern_kernel_program(program),
        inputs: builder.inputs,
    })
}

pub trait DynKind {
    type Repr: Clone + std::fmt::Debug;
}

#[derive(Debug, Clone)]
pub struct Dyn<T: DynKind> {
    pub(crate) repr: T::Repr,
}

#[derive(Debug, Clone)]
pub enum NumDynRepr {
    Const(f64),
    Expr { form: Form, env: Env },
    /// A spawn-meta signal shared by a spawn group: an array-valued result
    /// binds per element with the style-axis rules (§5/F15), selected by
    /// the element's repeater path / flat index captured at spawn.
    AxisSel { form: Form, env: Env, path: Rc<[(usize, usize)]>, flat: usize },
}

#[derive(Debug, Clone)]
pub enum PoseDynRepr {
    Node(Rc<DynNode>),
}

#[derive(Debug, Clone)]
pub enum FigureDynRepr {
    Pose(DynPose),
    Curve { frame: DynPose, curve: ParametricCurve },
}

impl DynKind for f64 {
    type Repr = NumDynRepr;
}

impl DynKind for Pose {
    type Repr = PoseDynRepr;
}

impl DynKind for Figure {
    type Repr = FigureDynRepr;
}

pub trait DynEval: DynKind + Sized {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<Self>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<Self, String>;
}

pub fn eval_dyn<T: DynEval>(
    d: &Dyn<T>,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<T, String> {
    eval_dyn_with_tick_rate(d, tau, state, sig, TickTiming::default().rate())
}

pub fn eval_dyn_with_tick_rate<T: DynEval>(
    d: &Dyn<T>,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<T, String> {
    T::eval_dyn_with_tick_rate(d, tau, state, sig, tick_rate)
}

pub type DynNum = Dyn<f64>;
pub type DynPose = Dyn<Pose>;
pub type CurveEval = crate::model::CurveEval<DynPose>;
pub type ParametricCurve = crate::model::ParametricCurve<DynPose>;
pub type Curve = crate::model::Curve<DynPose>;
pub type Figure = crate::model::Figure<DynPose>;
pub type DynFigure = Dyn<Figure>;

impl Dyn<f64> {
    pub fn num(n: f64) -> DynNum {
        Dyn { repr: NumDynRepr::Const(n) }
    }

    pub fn num_expr(form: Form, env: Env) -> DynNum {
        Dyn { repr: NumDynRepr::Expr { form, env } }
    }

    /// The same signal bound to one spawn element: array results select
    /// by the element's axis position instead of erroring.
    pub fn with_axis(&self, path: &[(usize, usize)], flat: usize) -> DynNum {
        match &self.repr {
            NumDynRepr::Expr { form, env } => Dyn {
                repr: NumDynRepr::AxisSel {
                    form: form.clone(),
                    env: env.clone(),
                    path: path.into(),
                    flat,
                },
            },
            _ => self.clone(),
        }
    }

    pub fn repr(&self) -> &NumDynRepr {
        &self.repr
    }
}

impl Dyn<Pose> {
    pub fn pose_node(node: Rc<DynNode>) -> DynPose {
        Dyn { repr: PoseDynRepr::Node(node) }
    }

    pub fn node(&self) -> &Rc<DynNode> {
        match &self.repr {
            PoseDynRepr::Node(node) => node,
        }
    }

    pub fn into_node(self) -> Rc<DynNode> {
        match self.repr {
            PoseDynRepr::Node(node) => node,
        }
    }

    pub fn framed(&self, frame: Rc<DynNode>) -> DynPose {
        DynPose::pose_node(frame_node(frame, self.node().clone()))
    }
}

impl Dyn<Figure> {
    pub fn figure_const(f: Figure) -> DynFigure {
        match f {
            Figure::Pose(p) => DynFigure::pose_node(Rc::new(DynNode::Const(p))),
            Figure::Curve(c) => {
                let frame = DynPose::pose_node(Rc::new(DynNode::Const(c.frame)));
                DynFigure::figure_curve(frame, c.spec)
            }
        }
    }

    pub fn pose(d: DynPose) -> DynFigure {
        Dyn { repr: FigureDynRepr::Pose(d) }
    }

    pub fn pose_node(d: Rc<DynNode>) -> DynFigure {
        DynFigure::pose(DynPose::pose_node(d))
    }

    pub fn figure_curve(frame: DynPose, curve: ParametricCurve) -> DynFigure {
        Dyn { repr: FigureDynRepr::Curve { frame, curve } }
    }

    pub fn repr(&self) -> &FigureDynRepr {
        &self.repr
    }

    pub fn pose_dyn(&self) -> &Rc<DynNode> {
        match &self.repr {
            FigureDynRepr::Pose(d) => d.node(),
            FigureDynRepr::Curve { frame, .. } => frame.node(),
        }
    }

    pub fn curve(&self) -> Option<&ParametricCurve> {
        match &self.repr {
            FigureDynRepr::Curve { curve, .. } => Some(curve),
            FigureDynRepr::Pose(_) => None,
        }
    }

    pub fn framed(&self, frame: Pose) -> DynFigure {
        if frame == Pose::IDENTITY {
            return self.clone();
        }
        let parent = Rc::new(DynNode::Const(frame));
        match &self.repr {
            FigureDynRepr::Pose(d) => DynFigure::pose(d.framed(parent)),
            FigureDynRepr::Curve { frame: child, curve } => {
                DynFigure::figure_curve(child.framed(parent), curve.clone())
            }
        }
    }
}
