//! Deterministic workload generation and common result contracts for Maku baselines.

pub mod alloc;
pub mod contract;
pub mod fixture;
pub mod result;
pub mod summary;
pub mod verify;

pub use contract::*;
pub use fixture::*;
pub use result::*;
pub use verify::*;
