//! Stable entity handles exposed across runtime boundaries.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityRef {
    pub row: usize,
    pub generation: u32,
}
