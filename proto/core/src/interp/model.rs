//! Central model surface.
//!
//! This module is intentionally mostly re-exports for now: it gives the
//! prototype one obvious place to read the core type vocabulary while the
//! interpreter and runtime are still being split apart.

pub use super::{
    ColliderProjector, Curve, CurveDomain, CurveEval, Dyn, DynFigure, DynKind, DynNum, DynPose,
    EntityRef, EntityStore, Figure, FigureDynRepr, ParametricCurve, Pose, RenderProjector,
    SampleSet, World, WorldFields,
};
