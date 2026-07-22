//! Host-configurable Touhou render pack.
//!
//! The module consumes core's ordered render rows and typed batches, resolves
//! Touhou profile semantics, and emits reusable fixed-layout sprite streams,
//! indexed ribbons, and one ordered material command stream. GPU resources and
//! submission remain host-owned.

mod color;
mod frame;
mod profile;
mod renderer;
mod stock;

pub use frame::*;
pub use profile::*;
pub use renderer::*;

#[cfg(test)]
mod tests;
