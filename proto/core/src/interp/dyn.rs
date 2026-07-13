//! Prototype typed dynamic values.
//!
//! This is a transitional representation. The target `Dyn<T>` should become a
//! typed time-varying value/program with structure lifting and compile-time
//! schemas; this file is the current interpreter-backed shell.

use super::*;
use crate::edn::Form;
use std::rc::Rc;

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
