//! Central model surface.
//!
//! This module is intentionally mostly re-exports for now: it gives the
//! prototype one obvious place to read the core type vocabulary while the
//! interpreter and runtime are still being split apart.

pub use super::{
    ColliderProjector, Dyn, DynFigure, DynKind, DynNum, DynPose, EntityRef, EntityStore, Figure,
    FigureDynRepr, Pose, RenderProjector, World, WorldFields,
};
