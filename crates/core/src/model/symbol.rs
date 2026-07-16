//! Interned symbol ids shared by model and runtime boundary rows.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

pub type ColName = Symbol;
pub type FieldName = Symbol;
