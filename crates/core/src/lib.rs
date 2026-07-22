//! Maku's supported embedding surface.
//!
//! Hosts normally use [`host`], source helpers in [`source`], and the typed
//! ordered transport in [`render`]. Modules marked hidden are implementation
//! details retained for in-workspace backend development and carry no
//! compatibility promise before 1.0.

#[doc(hidden)]
pub mod edn;
mod fxhash;
pub mod host;
#[cfg(feature = "touhou")]
pub mod touhou;
#[cfg(feature = "macroquad")]
pub mod macroquad;
#[doc(hidden)]
pub mod interp;
#[doc(hidden)]
pub mod model;
#[doc(hidden)]
pub mod session;
#[doc(hidden)]
pub mod sim;

/// Card-source helpers needed by native and virtual-filesystem hosts.
pub mod source {
    pub use crate::edn::{expand_card, expand_card_with, expand_src, stdlib};
}

/// Stable typed, ordered render transport consumed by custom renderers and
/// genre render packs.
pub mod render {
    pub use crate::model::{
        Column, NumColumn, RenderBatch, RenderData, RenderFieldKind, RenderItem, RenderRow,
        RenderSchema,
    };
}
