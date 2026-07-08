//! Central model surface.
//!
//! This module is intentionally mostly re-exports for now: it gives the
//! prototype one obvious place to read the core type vocabulary while the
//! interpreter and runtime are still being split apart.

pub use super::{
    ColliderProjector, Curve, CurveEval, Dyn, DynFigure, DynKind, DynNum, DynPose, EntityStore,
    Figure, FigureDynRepr, ParametricCurve, RenderProjector, World, WorldFields,
};
pub use crate::model::{
    ColName, ColliderData, CurveDomain, EntityRef, FieldName, Pose, RenderData, SampleSet, Symbol,
};
pub type DataAtom = crate::model::DataAtom<DynPose>;
