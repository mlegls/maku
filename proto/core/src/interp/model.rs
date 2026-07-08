//! Central model surface.
//!
//! This module is intentionally mostly re-exports for now: it gives the
//! prototype one obvious place to read the core type vocabulary while the
//! interpreter and runtime are still being split apart.

pub use super::{
    ColliderProjector, Curve, CurveEval, Dyn, DynFigure, DynKind, DynNum, DynPose, EntityRef,
    EntityStore, Figure, FigureDynRepr, ParametricCurve, RenderProjector, World, WorldFields,
};
pub use crate::model::{CurveDomain, Pose, SampleSet};
