//! Source-semantic types used by the frontend checker and editor queries.
//!
//! These types describe language meaning only. Physical widths, storage
//! layouts, executable register classes, and backend eligibility belong to
//! lower layers and intentionally have no representation here.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unknown,
    Num,
    Symbol,
    /// Kept as a source-compatible spelling for older frontend consumers.
    Kw,
    String,
    Handle,
    Nothing,
    Pose,
    Curve,
    Figure,
    ColliderData,
    RenderData(RenderKind),
    Meta,
    MetaEnv,
    ColliderProjector,
    Projector {
        kind: ProjectorKind,
        figure: FigureKind,
        render: Option<RenderKind>,
    },
    EntityView(EntityShape),
    EntitySet,
    Action,
    List(Box<Type>),
    Array(Box<Type>),
    Vec { len: usize, elem: Box<Type> },
    Mat { rows: usize, cols: usize, elem: Box<Type> },
    Record(RecordType),
    Option(Box<Type>),
    Dyn { class: Option<DynClass>, value: Box<Type> },
    Fn { args: Vec<Type>, ret: Box<Type> },
    Var(TypeVar),
}

impl Type {
    pub fn dyn_of(value: Type) -> Type {
        Type::Dyn { class: None, value: Box::new(value) }
    }

    pub fn const_dyn_of(value: Type) -> Type {
        Type::Dyn { class: Some(DynClass::Const), value: Box::new(value) }
    }

    pub fn list_of(value: Type) -> Type {
        Type::List(Box::new(value))
    }

    pub fn dyn_list_of(value: Type) -> Type {
        Type::dyn_of(Type::list_of(value))
    }

    pub fn function(args: impl Into<Vec<Type>>, ret: Type) -> Type {
        Type::Fn { args: args.into(), ret: Box::new(ret) }
    }

    pub fn record(fields: impl IntoIterator<Item = (impl Into<String>, Type)>) -> Type {
        Type::Record(RecordType {
            fields: fields
                .into_iter()
                .map(|(name, ty)| (name.into(), ty))
                .collect(),
            open: false,
        })
    }

    pub fn open_record(fields: impl IntoIterator<Item = (impl Into<String>, Type)>) -> Type {
        let mut record = match Self::record(fields) {
            Type::Record(record) => record,
            _ => unreachable!(),
        };
        record.open = true;
        Type::Record(record)
    }

    pub fn spawn_figure() -> Type {
        Type::dyn_of(Type::Figure)
    }

    pub fn spawn_colliders() -> Type {
        Type::list_of(Type::ColliderProjector)
    }

    pub fn spawn_meta() -> Type {
        Type::dyn_of(Type::Meta)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown | Type::Var(_))
    }

    fn free_vars(&self, out: &mut BTreeSet<TypeVar>) {
        match self {
            Type::Var(var) => {
                out.insert(*var);
            }
            Type::List(value)
            | Type::Array(value)
            | Type::Option(value)
            | Type::Dyn { value, .. } => value.free_vars(out),
            Type::Vec { elem, .. } | Type::Mat { elem, .. } => elem.free_vars(out),
            Type::Record(record) => {
                for ty in record.fields.values() {
                    ty.free_vars(out);
                }
            }
            Type::Fn { args, ret } => {
                for arg in args {
                    arg.free_vars(out);
                }
                ret.free_vars(out);
            }
            _ => {}
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unknown => f.write_str("Unknown"),
            Type::Num => f.write_str("Num"),
            Type::Symbol | Type::Kw => f.write_str("Symbol"),
            Type::String => f.write_str("String"),
            Type::Handle => f.write_str("Handle"),
            Type::Nothing => f.write_str("Nothing"),
            Type::Pose => f.write_str("Pose"),
            Type::Curve => f.write_str("Curve"),
            Type::Figure => f.write_str("Figure"),
            Type::ColliderData => f.write_str("ColliderData"),
            Type::RenderData(kind) => write!(f, "RenderData<{}>", kind),
            Type::Meta => f.write_str("Meta"),
            Type::MetaEnv => f.write_str("MetaEnv"),
            Type::ColliderProjector => f.write_str("ColliderProjector"),
            Type::Projector { kind, figure, render } => {
                write!(f, "{}Projector<{}", kind, figure)?;
                if let Some(render) = render {
                    write!(f, ", {}", render)?;
                }
                f.write_str(">")
            }
            Type::EntityView(shape) => write!(f, "EntityView<{}>", shape),
            Type::EntitySet => f.write_str("EntitySet"),
            Type::Action => f.write_str("Action"),
            Type::List(value) => write!(f, "List<{}>", value),
            Type::Array(value) => write!(f, "Array<{}>", value),
            Type::Vec { len, elem } => write!(f, "Vec<{}, {}>", len, elem),
            Type::Mat { rows, cols, elem } => write!(f, "Mat<{}, {}, {}>", rows, cols, elem),
            Type::Record(record) => write!(f, "{}", record),
            Type::Option(value) => write!(f, "Option<{}>", value),
            Type::Dyn { class, value } => match class {
                Some(class) => write!(f, "Dyn<{}, {}>", value, class),
                None => write!(f, "Dyn<{}>", value),
            },
            Type::Fn { args, ret } => {
                f.write_str("Fn<(")?;
                for (index, arg) in args.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "), {}>", ret)
            }
            Type::Var(var) => write!(f, "'t{}", var.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FieldType {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RecordType {
    pub fields: BTreeMap<String, Type>,
    pub open: bool,
}

impl fmt::Display for RecordType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (index, (name, ty)) in self.fields.iter().enumerate() {
            if index != 0 {
                f.write_str(", ")?;
            }
            write!(f, ":{} {}", name, ty)?;
        }
        if self.open {
            if !self.fields.is_empty() {
                f.write_str(", ")?;
            }
            f.write_str("…")?;
        }
        f.write_str("}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RenderKind {
    Any,
    Named(String),
}

impl fmt::Display for RenderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderKind::Any => f.write_str("Any"),
            RenderKind::Named(name) => write!(f, ":{}", name),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FigureKind {
    Any,
    Pose,
    Curve,
}

impl fmt::Display for FigureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FigureKind::Any => f.write_str("Figure"),
            FigureKind::Pose => f.write_str("Pose"),
            FigureKind::Curve => f.write_str("Curve"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProjectorKind {
    Collider,
    Render,
}

impl fmt::Display for ProjectorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectorKind::Collider => f.write_str("Collider"),
            ProjectorKind::Render => f.write_str("Render"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntityShape {
    pub figure: FigureKind,
    pub fields: RecordType,
}

impl fmt::Display for EntityShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}", self.figure, self.fields)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderKindSchema {
    pub kind: RenderKind,
    pub fields: Vec<FieldType>,
    pub open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeVar(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpectedType {
    Any,
    Exact(Type),
    DynOf(Type),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DynClass {
    Const,
    Closed,
    PiecewiseClosed,
    Integrated,
    Scanned,
}

impl fmt::Display for DynClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DynClass::Const => "Const",
            DynClass::Closed => "Closed",
            DynClass::PiecewiseClosed => "PiecewiseClosed",
            DynClass::Integrated => "Integrated",
            DynClass::Scanned => "Scanned",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeScheme {
    pub quantified: Vec<TypeVar>,
    pub ty: Type,
}

impl TypeScheme {
    pub fn mono(ty: Type) -> Self {
        Self { quantified: Vec::new(), ty }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TypeEnv {
    scopes: Vec<HashMap<String, TypeScheme>>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self { scopes: vec![HashMap::new()] }
    }

    pub fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop(&mut self) {
        assert!(self.scopes.len() > 1, "cannot pop the root type scope");
        self.scopes.pop();
    }

    pub fn insert(&mut self, name: impl Into<String>, scheme: TypeScheme) {
        self.scopes.last_mut().expect("type environment has a root scope").insert(name.into(), scheme);
    }

    pub fn get(&self, name: &str) -> Option<&TypeScheme> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn free_vars(&self) -> BTreeSet<TypeVar> {
        let mut out = BTreeSet::new();
        for scheme in self.scopes.iter().flat_map(|scope| scope.values()) {
            let mut vars = BTreeSet::new();
            scheme.ty.free_vars(&mut vars);
            for quantified in &scheme.quantified {
                vars.remove(quantified);
            }
            out.extend(vars);
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnifyError {
    Mismatch { expected: Type, found: Type },
    Arity { expected: usize, found: usize },
    MissingField { field: String, expected: Type },
    UnexpectedField { field: String, found: Type },
    Infinite { var: TypeVar, ty: Type },
}

#[derive(Clone, Debug, Default)]
pub struct TypeContext {
    next_var: u32,
    substitutions: HashMap<TypeVar, Type>,
}

impl TypeContext {
    pub fn fresh(&mut self) -> Type {
        let var = TypeVar(self.next_var);
        self.next_var += 1;
        Type::Var(var)
    }

    pub fn resolve(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(var) => self
                .substitutions
                .get(var)
                .map(|ty| self.resolve(ty))
                .unwrap_or(Type::Var(*var)),
            Type::List(value) => Type::List(Box::new(self.resolve(value))),
            Type::Array(value) => Type::Array(Box::new(self.resolve(value))),
            Type::Vec { len, elem } => Type::Vec { len: *len, elem: Box::new(self.resolve(elem)) },
            Type::Mat { rows, cols, elem } => Type::Mat {
                rows: *rows,
                cols: *cols,
                elem: Box::new(self.resolve(elem)),
            },
            Type::Record(record) => Type::Record(RecordType {
                fields: record
                    .fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), self.resolve(ty)))
                    .collect(),
                open: record.open,
            }),
            Type::Option(value) => Type::Option(Box::new(self.resolve(value))),
            Type::Dyn { class, value } => Type::Dyn {
                class: *class,
                value: Box::new(self.resolve(value)),
            },
            Type::Fn { args, ret } => Type::Fn {
                args: args.iter().map(|arg| self.resolve(arg)).collect(),
                ret: Box::new(self.resolve(ret)),
            },
            other => other.clone(),
        }
    }

    pub fn unify(&mut self, expected: &Type, found: &Type) -> Result<Type, UnifyError> {
        let expected = self.resolve(expected);
        let found = self.resolve(found);
        if expected == found {
            return Ok(expected);
        }
        match (expected, found) {
            (Type::Unknown, found) | (found, Type::Unknown) => Ok(found),
            (Type::Kw, Type::Symbol) | (Type::Symbol, Type::Kw) => Ok(Type::Symbol),
            (Type::Var(var), ty) | (ty, Type::Var(var)) => self.bind(var, ty),
            (Type::Nothing, Type::Option(value)) | (Type::Option(value), Type::Nothing) => {
                Ok(Type::Option(value))
            }
            (Type::Option(a), Type::Option(b)) => {
                Ok(Type::Option(Box::new(self.unify(&a, &b)?)))
            }
            (Type::List(a), Type::List(b)) => Ok(Type::List(Box::new(self.unify(&a, &b)?))),
            (Type::Array(a), Type::Array(b)) => Ok(Type::Array(Box::new(self.unify(&a, &b)?))),
            (Type::Vec { len: a_len, elem: a }, Type::Vec { len: b_len, elem: b })
                if a_len == b_len =>
            {
                Ok(Type::Vec { len: a_len, elem: Box::new(self.unify(&a, &b)?) })
            }
            (
                Type::Mat { rows: ar, cols: ac, elem: a },
                Type::Mat { rows: br, cols: bc, elem: b },
            ) if ar == br && ac == bc => Ok(Type::Mat {
                rows: ar,
                cols: ac,
                elem: Box::new(self.unify(&a, &b)?),
            }),
            (Type::Record(a), Type::Record(b)) => self.unify_records(a, b),
            (
                Type::Dyn { class: a_class, value: a },
                Type::Dyn { class: b_class, value: b },
            ) if a_class.is_none() || b_class.is_none() || a_class == b_class => Ok(Type::Dyn {
                class: a_class.or(b_class),
                value: Box::new(self.unify(&a, &b)?),
            }),
            (Type::Fn { args: a, ret: ar }, Type::Fn { args: b, ret: br }) => {
                if a.len() != b.len() {
                    return Err(UnifyError::Arity { expected: a.len(), found: b.len() });
                }
                let args = a
                    .iter()
                    .zip(&b)
                    .map(|(a, b)| self.unify(a, b))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.unify(&ar, &br)?;
                Ok(Type::function(args, ret))
            }
            (expected, found) => Err(UnifyError::Mismatch { expected, found }),
        }
    }

    pub fn instantiate(&mut self, scheme: &TypeScheme) -> Type {
        let replacements: HashMap<_, _> =
            scheme.quantified.iter().map(|var| (*var, self.fresh())).collect();
        replace_vars(&scheme.ty, &replacements)
    }

    pub fn generalize(&self, env: &TypeEnv, ty: &Type) -> TypeScheme {
        let ty = self.resolve(ty);
        let mut vars = BTreeSet::new();
        ty.free_vars(&mut vars);
        let env_vars = env.free_vars();
        vars.retain(|var| !env_vars.contains(var));
        TypeScheme { quantified: vars.into_iter().collect(), ty }
    }

    fn bind(&mut self, var: TypeVar, ty: Type) -> Result<Type, UnifyError> {
        if ty == Type::Var(var) {
            return Ok(ty);
        }
        let mut vars = BTreeSet::new();
        ty.free_vars(&mut vars);
        if vars.contains(&var) {
            return Err(UnifyError::Infinite { var, ty });
        }
        self.substitutions.insert(var, ty.clone());
        Ok(ty)
    }

    fn unify_records(&mut self, expected: RecordType, found: RecordType) -> Result<Type, UnifyError> {
        let mut fields = BTreeMap::new();
        for (name, expected_ty) in &expected.fields {
            let Some(found_ty) = found.fields.get(name) else {
                return Err(UnifyError::MissingField {
                    field: name.clone(),
                    expected: expected_ty.clone(),
                });
            };
            fields.insert(name.clone(), self.unify(expected_ty, found_ty)?);
        }
        for (name, found_ty) in &found.fields {
            if !expected.fields.contains_key(name) {
                if !expected.open {
                    return Err(UnifyError::UnexpectedField {
                        field: name.clone(),
                        found: found_ty.clone(),
                    });
                }
                fields.insert(name.clone(), found_ty.clone());
            }
        }
        Ok(Type::Record(RecordType { fields, open: expected.open || found.open }))
    }
}

fn replace_vars(ty: &Type, replacements: &HashMap<TypeVar, Type>) -> Type {
    match ty {
        Type::Var(var) => replacements.get(var).cloned().unwrap_or(Type::Var(*var)),
        Type::List(value) => Type::List(Box::new(replace_vars(value, replacements))),
        Type::Array(value) => Type::Array(Box::new(replace_vars(value, replacements))),
        Type::Vec { len, elem } => {
            Type::Vec { len: *len, elem: Box::new(replace_vars(elem, replacements)) }
        }
        Type::Mat { rows, cols, elem } => Type::Mat {
            rows: *rows,
            cols: *cols,
            elem: Box::new(replace_vars(elem, replacements)),
        },
        Type::Record(record) => Type::Record(RecordType {
            fields: record
                .fields
                .iter()
                .map(|(name, ty)| (name.clone(), replace_vars(ty, replacements)))
                .collect(),
            open: record.open,
        }),
        Type::Option(value) => Type::Option(Box::new(replace_vars(value, replacements))),
        Type::Dyn { class, value } => Type::Dyn {
            class: *class,
            value: Box::new(replace_vars(value, replacements)),
        },
        Type::Fn { args, ret } => Type::Fn {
            args: args.iter().map(|arg| replace_vars(arg, replacements)).collect(),
            ret: Box::new(replace_vars(ret, replacements)),
        },
        other => other.clone(),
    }
}
