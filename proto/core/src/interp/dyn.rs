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
    fn eval_dyn(
        d: &Dyn<Self>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
    ) -> Result<Self, String>;
}

pub fn eval_dyn<T: DynEval>(
    d: &Dyn<T>,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<T, String> {
    T::eval_dyn(d, tau, state, sig)
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
        DynPose::pose_node(Rc::new(DynNode::Frame(frame, self.node().clone())))
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
