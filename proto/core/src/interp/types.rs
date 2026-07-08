//! Target type vocabulary for semantic elaboration.
//!
//! This module is deliberately descriptive today. It names the type and
//! representation layers that future inference/lowering should target without
//! forcing the current interpreter to type-check every expression yet.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Num,
    Kw,
    Handle,
    Nothing,
    Pose,
    Curve,
    Figure,
    ColliderData,
    RenderData(RenderKind),
    Meta,
    EntitySet,
    Action,
    List(Box<Type>),
    Array(Box<Type>),
    Vec { len: usize, elem: Box<Type> },
    Mat { rows: usize, cols: usize, elem: Box<Type> },
    Record(Vec<FieldType>),
    Option(Box<Type>),
    Dyn { class: Option<DynClass>, value: Box<Type> },
    Fn { arg: Box<Type>, ret: Box<Type> },
    Var(TypeVar),
}

impl Type {
    pub fn dyn_of(value: Type) -> Type {
        Type::Dyn { class: None, value: Box::new(value) }
    }

    pub fn list_of(value: Type) -> Type {
        Type::List(Box::new(value))
    }

    pub fn dyn_list_of(value: Type) -> Type {
        Type::dyn_of(Type::list_of(value))
    }

    pub fn spawn_figure() -> Type {
        Type::dyn_of(Type::Figure)
    }

    pub fn spawn_colliders() -> Type {
        Type::dyn_list_of(Type::ColliderData)
    }

    pub fn spawn_renderers() -> Type {
        Type::dyn_list_of(Type::RenderData(RenderKind::Any))
    }

    pub fn spawn_meta() -> Type {
        Type::dyn_of(Type::Meta)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldType {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RenderKind {
    Any,
    Named(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderKindSchema {
    pub kind: RenderKind,
    pub fields: Vec<FieldType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeVar(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpectedType {
    Any,
    Exact(Type),
    DynOf(Type),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynClass {
    Const,
    Closed,
    PiecewiseClosed,
    Integrated,
    Scanned,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReprClass {
    Scalar,
    List,
    Array,
    Vec { len: usize },
    Mat { rows: usize, cols: usize },
    Record,
    Dyn(DynClass),
}
