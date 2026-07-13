//! Conservative source-semantic inference and expected-type elaboration.
//!
//! A diagnostic becomes an enforced load error only when both sides are known
//! and the governing signature/schema proves them incompatible. Unknown names,
//! dynamic maps, unsupported macro shapes, and other checker limitations are
//! recorded without changing runtime validity.

use super::card::Card;
use super::provenance::{ExpansionFrame, FormPath, Provenance, ProvenanceMap, SourceSpan};
use super::schema::{CardSchema, RenderKindDecl};
use super::signatures::{
    action_slot_signature, builtin_signature, engine_signature, projector_field_type,
    projector_signature, Arity, Signature, SignatureKind,
};
use super::types::{
    DynClass, EntityShape, FigureKind, ProjectorKind, RecordType, Type, TypeContext, TypeEnv,
    TypeScheme, TypeVar, UnifyError,
};
use crate::edn::Form;
use crate::model::RenderFieldKind;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckMode {
    Diagnostic,
    #[default]
    Enforced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckConfidence {
    ProvenViolation,
    UncheckedDynamic,
    CheckerLimitation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    TypeMismatch,
    ArityMismatch,
    ActionPosition,
    QueryPredicate,
    SpawnFigure,
    SpawnMeta,
    ProjectorFigure,
    RenderField,
    UnknownRenderKind,
    UnknownEntityField,
    HostChannel,
    FailedCoercion,
    RecursiveInference,
    UncheckedForm,
}

impl DiagnosticCategory {
    pub fn code(self) -> &'static str {
        match self {
            DiagnosticCategory::TypeMismatch => "type/mismatch",
            DiagnosticCategory::ArityMismatch => "type/arity",
            DiagnosticCategory::ActionPosition => "type/action-position",
            DiagnosticCategory::QueryPredicate => "domain/query-predicate",
            DiagnosticCategory::SpawnFigure => "domain/spawn-figure",
            DiagnosticCategory::SpawnMeta => "domain/spawn-meta",
            DiagnosticCategory::ProjectorFigure => "domain/projector-figure",
            DiagnosticCategory::RenderField => "schema/render-field",
            DiagnosticCategory::UnknownRenderKind => "schema/render-kind",
            DiagnosticCategory::UnknownEntityField => "schema/entity-field",
            DiagnosticCategory::HostChannel => "schema/host-channel",
            DiagnosticCategory::FailedCoercion => "type/failed-coercion",
            DiagnosticCategory::RecursiveInference => "type/recursive-boundary",
            DiagnosticCategory::UncheckedForm => "type/unchecked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    Argument,
    Return,
    Action,
    Spawn,
    Meta,
    Query,
    EntityField,
    Projector,
    Render,
    HostChannel,
    Branch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundaryContext {
    pub kind: BoundaryKind,
    pub name: Rc<str>,
    pub index: Option<usize>,
    pub field: Option<Rc<str>>,
}

impl BoundaryContext {
    fn named(kind: BoundaryKind, name: impl Into<Rc<str>>) -> Self {
        Self { kind, name: name.into(), index: None, field: None }
    }

    fn argument(name: impl Into<Rc<str>>, index: usize) -> Self {
        Self { kind: BoundaryKind::Argument, name: name.into(), index: Some(index), field: None }
    }

    fn field(kind: BoundaryKind, name: impl Into<Rc<str>>, field: impl Into<Rc<str>>) -> Self {
        Self { kind, name: name.into(), index: None, field: Some(field.into()) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoercionKind {
    PoseToFigure,
    HomogeneousList,
    ConstantToDyn,
    SequenceDynStructure,
    NothingToAction,
}

impl fmt::Display for CoercionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CoercionKind::PoseToFigure => "Pose to Figure",
            CoercionKind::HomogeneousList => "homogeneous list recognition",
            CoercionKind::ConstantToDyn => "constant to Dyn lifting",
            CoercionKind::SequenceDynStructure => "structured Dyn sequencing",
            CoercionKind::NothingToAction => "Nothing to no-op Action",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoercionPathSegment {
    Field(Rc<str>),
    Element(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoercionFailure {
    pub path: Vec<CoercionPathSegment>,
    pub attempted: Vec<CoercionKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDiagnostic {
    pub category: DiagnosticCategory,
    pub confidence: CheckConfidence,
    pub primary_span: SourceSpan,
    pub expected: Option<Type>,
    pub found: Option<Type>,
    pub context: BoundaryContext,
    pub related_spans: Vec<SourceSpan>,
    pub expansion_stack: Vec<ExpansionFrame>,
    pub coercion_failure: Option<CoercionFailure>,
    pub message: String,
}

impl fmt::Display for TypeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {} in {}", self.category.code(), self.primary_span, self.context.name)?;
        if let Some(index) = self.context.index {
            write!(f, " argument {}", index + 1)?;
        }
        if let Some(field) = &self.context.field {
            write!(f, " field :{}", field)?;
        }
        write!(f, ": {}", self.message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedIdentity {
    Lexical { name: Rc<str>, depth: usize },
    Definition { name: Rc<str> },
    Builtin { name: Rc<str> },
    Special { name: Rc<str> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaIdentity {
    RenderKind(Rc<str>),
    RenderField { kind: Rc<str>, field: Rc<str> },
    EntityField(Rc<str>),
    HostChannel(Rc<str>),
    Projector(Rc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedNode {
    pub path: FormPath,
    pub resolved: Option<ResolvedIdentity>,
    pub inferred: Type,
    pub expected: Option<Type>,
    pub coercions: Vec<CoercionKind>,
    pub schema: Option<SchemaIdentity>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Default)]
pub struct CheckReport {
    pub nodes: Vec<TypedNode>,
    pub diagnostics: Vec<TypeDiagnostic>,
}

impl CheckReport {
    pub fn violations(&self) -> impl Iterator<Item = &TypeDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.confidence == CheckConfidence::ProvenViolation)
    }

    pub fn unchecked(&self) -> impl Iterator<Item = &TypeDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.confidence != CheckConfidence::ProvenViolation)
    }

    pub fn node(&self, path: &[usize]) -> Option<&TypedNode> {
        self.nodes.iter().find(|node| node.path == path)
    }

    pub fn enforce(&self, mode: CheckMode) -> Result<(), TypeCheckError> {
        if mode == CheckMode::Diagnostic {
            return Ok(());
        }
        let violations = self.violations().cloned().collect::<Vec<_>>();
        if violations.is_empty() {
            Ok(())
        } else {
            Err(TypeCheckError { diagnostics: violations })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeCheckError {
    pub diagnostics: Vec<TypeDiagnostic>,
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index != 0 {
                f.write_str("\n")?;
            }
            write!(f, "{}", diagnostic)?;
        }
        Ok(())
    }
}

pub fn check_forms(
    forms: &[Form],
    card: &Card,
    schema: &CardSchema,
    provenance: &ProvenanceMap,
) -> CheckReport {
    let mut traced = provenance.clone();
    let definitions = macro_definition_spans(forms, provenance);
    let mut ctx = super::Ctx::default();
    ctx.sig.defs = Rc::new(card.defs.clone());
    ctx.patterns = Rc::new(card.patterns.clone());
    ctx.macros = Rc::new(card.macros.clone());
    let mut world = super::World::default();
    let env = super::Env::empty();
    let mut expanded = Vec::with_capacity(forms.len());
    for (index, form) in forms.iter().enumerate() {
        let mut path = vec![index];
        let mut record = |path: &[usize], name: &Rc<str>| {
            let call_site = (0..=path.len())
                .rev()
                .find_map(|len| traced.get(&path[..len]))
                .map(|origin| origin.primary_span().clone())
                .unwrap_or_else(|| SourceSpan::synthetic("<generated>"));
            traced.record_expansion(
                path,
                name.clone(),
                call_site,
                definitions.get(name.as_ref()).cloned(),
            );
        };
        expanded.push(
            super::expand_macros_traced(
                form,
                &env,
                &mut ctx,
                &mut world,
                &mut path,
                &mut record,
            )
            .unwrap_or_else(|_| form.clone()),
        );
    }
    Checker::new(card, schema, &traced).check(&expanded)
}

fn macro_definition_spans(
    forms: &[Form],
    provenance: &ProvenanceMap,
) -> HashMap<String, SourceSpan> {
    let mut definitions = HashMap::new();
    for (index, form) in forms.iter().enumerate() {
        let Form::List(items) = form else { continue };
        if !matches!(items.first(), Some(Form::Sym(head)) if head.as_ref() == "defmacro") {
            continue;
        }
        let Some(Form::Sym(name)) = items.get(1) else { continue };
        if let Some(origin) = provenance.get(&[index]) {
            definitions.insert(name.to_string(), origin.authored.clone());
        }
    }
    definitions
}


struct Checker<'a> {
    card: &'a Card,
    schema: &'a CardSchema,
    provenance: &'a ProvenanceMap,
    types: TypeContext,
    env: TypeEnv,
    definitions: HashMap<String, Type>,
    report: CheckReport,
}

impl<'a> Checker<'a> {
    fn new(card: &'a Card, schema: &'a CardSchema, provenance: &'a ProvenanceMap) -> Self {
        let mut checker = Self {
            card,
            schema,
            provenance,
            types: TypeContext::default(),
            env: TypeEnv::new(),
            definitions: HashMap::new(),
            report: CheckReport::default(),
        };
        for name in card.defs.keys() {
            let ty = checker.types.fresh();
            checker.definitions.insert(name.clone(), ty.clone());
            checker.env.insert(name.clone(), TypeScheme::mono(ty));
        }
        checker
    }

    fn check(mut self, forms: &[Form]) -> CheckReport {
        self.predeclare(forms);
        for (index, form) in forms.iter().enumerate() {
            let mut path = vec![index];
            self.check_top_level(form, &mut path);
        }
        for node in &mut self.report.nodes {
            node.inferred = self.types.resolve(&node.inferred);
            node.expected = node.expected.as_ref().map(|ty| self.types.resolve(ty));
        }
        self.report
    }

    fn predeclare(&mut self, forms: &[Form]) {
        for form in forms {
            let Form::List(items) = form else { continue };
            let Some(Form::Sym(head)) = items.first() else { continue };
            match head.as_ref() {
                "def" | "defn" => {
                    if let Some(Form::Sym(name)) = items.get(1) {
                        let ty = self.types.fresh();
                        self.definitions.insert(name.to_string(), ty.clone());
                        self.env.insert(name.to_string(), TypeScheme::mono(ty));
                    }
                }
                _ => {}
            }
        }
    }

    fn check_top_level(&mut self, form: &Form, path: &mut FormPath) {
        let Form::List(items) = form else {
            let _ = self.infer(form, None, path, BoundaryContext::named(BoundaryKind::Return, "top-level"));
            return;
        };
        let Some(Form::Sym(head)) = items.first() else {
            let _ = self.infer(form, None, path, BoundaryContext::named(BoundaryKind::Return, "top-level"));
            return;
        };
        match head.as_ref() {
            "def" if items.len() >= 3 => {
                let Some(Form::Sym(name)) = items.get(1) else { return };
                path.push(2);
                let found = self.infer(&items[2], None, path, BoundaryContext::named(BoundaryKind::Return, name.clone()));
                path.pop();
                if let Some(expected) = self.definitions.get(name.as_ref()).cloned() {
                    let _ = self.types.unify(&expected, &found);
                    if is_generalizable(&items[2]) {
                        let scheme = self.types.generalize(&self.env, &found);
                        self.env.insert(name.to_string(), scheme);
                    }
                }
            }
            "defn" if items.len() >= 4 => {
                let Some(Form::Sym(name)) = items.get(1) else { return };
                let mut fn_items = vec![Form::sym("fn"), items[2].clone()];
                fn_items.extend(items[3..].iter().cloned());
                let form = Form::list(fn_items);
                path.push(2);
                let found = self.infer(&form, None, path, BoundaryContext::named(BoundaryKind::Return, name.clone()));
                path.pop();
                if let Some(expected) = self.definitions.get(name.as_ref()).cloned() {
                    if let Err(error) = self.types.unify(&expected, &found) {
                        self.report_unify(error, path, BoundaryContext::named(BoundaryKind::Return, name.clone()), None);
                    }
                    let scheme = self.types.generalize(&self.env, &found);
                    self.env.insert(name.to_string(), scheme);
                }
            }
            "defpattern" if items.len() >= 4 => {
                self.env.push();
                if let Some(Form::Vector(params)) = items.get(2) {
                    for pair in params.chunks(2) {
                        if let Some(Form::Sym(name)) = pair.first() {
                            let param_ty = if let Some(default) = pair.get(1) {
                                self.infer(default, None, path, BoundaryContext::named(BoundaryKind::Argument, name.clone()))
                            } else {
                                self.types.fresh()
                            };
                            self.env.insert(name.to_string(), TypeScheme::mono(param_ty));
                        }
                    }
                }
                for (index, body) in items.iter().enumerate().skip(3) {
                    path.push(index);
                    let _ = self.infer(
                        body,
                        Some(&Type::Action),
                        path,
                        BoundaryContext::named(BoundaryKind::Action, head.clone()),
                    );
                    path.pop();
                }
                self.env.pop();
            }
            "deftick" => {
                for (index, body) in items.iter().enumerate().skip(1) {
                    path.push(index);
                    let _ = self.infer(
                        body,
                        Some(&Type::Action),
                        path,
                        BoundaryContext::named(BoundaryKind::Action, "deftick"),
                    );
                    path.pop();
                }
            }
            "defmacro" | "defrender-kind" | "render-adapt" => {}
            _ => {
                let _ = self.infer(form, None, path, BoundaryContext::named(BoundaryKind::Return, "top-level"));
            }
        }
    }

    fn infer(
        &mut self,
        form: &Form,
        expected: Option<&Type>,
        path: &mut FormPath,
        context: BoundaryContext,
    ) -> Type {
        let inferred = match form {
            Form::Num(_) | Form::Bool(_) => Type::Num,
            Form::Str(_) => Type::Symbol,
            Form::Kw(_) => Type::Symbol,
            Form::Sym(name) => self.resolve_symbol(name, path, &context),
            Form::Vector(items) => self.infer_sequence(items, expected, path),
            Form::Map(fields) => self.infer_record(fields, path),
            Form::List(items) => self.infer_list(items, expected, path, &context),
        };
        self.apply_expected(inferred, expected, path, context, Vec::new())
    }

    fn resolve_symbol(&mut self, name: &Rc<str>, path: &FormPath, context: &BoundaryContext) -> Type {
        if let Some(scheme) = self.env.get(name).cloned() {
            let ty = self.types.instantiate(&scheme);
            self.push_node(path, ty.clone(), None, Vec::new(), None, Some(ResolvedIdentity::Lexical { name: name.clone(), depth: 0 }));
            return ty;
        }
        if let Some(ty) = self.definitions.get(name.as_ref()).cloned() {
            self.push_node(path, ty.clone(), None, Vec::new(), None, Some(ResolvedIdentity::Definition { name: name.clone() }));
            return ty;
        }
        if let Some(signature) = builtin_signature(name) {
            let ty = self.instantiate_signature(&signature);
            self.push_node(path, ty.clone(), None, Vec::new(), None, Some(ResolvedIdentity::Builtin { name: name.clone() }));
            return ty;
        }
        if name.starts_with('$') || matches!(name.as_ref(), "t" | "u" | "i" | "n" | "tick") {
            self.unchecked(path, context.clone(), format!("dynamic value {} retains runtime checking", name));
            Type::Unknown
        } else {
            self.unchecked(path, context.clone(), format!("unresolved form {} is not rejected by the checker", name));
            Type::Unknown
        }
    }

    fn infer_sequence(
        &mut self,
        items: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
    ) -> Type {
        let expected_element = expected.and_then(|expected| match self.types.resolve(expected) {
            Type::Array(element) | Type::List(element) | Type::Vec { elem: element, .. } => {
                Some(*element)
            }
            _ => None,
        });
        if items.is_empty() {
            return expected_element
                .map(|element| Type::List(Box::new(element)))
                .unwrap_or_else(|| Type::List(Box::new(Type::Unknown)));
        }
        let mut element = expected_element.clone().unwrap_or_else(|| self.types.fresh());
        let mut homogeneous = true;
        for (index, item) in items.iter().enumerate() {
            path.push(index);
            let found = self.infer(
                item,
                None,
                path,
                BoundaryContext::named(BoundaryKind::Argument, "collection element"),
            );
            if let Some(expected_element) = &expected_element {
                let diagnostic_start = self.report.diagnostics.len();
                let checked = self.apply_expected(
                    found,
                    Some(expected_element),
                    path,
                    BoundaryContext::named(BoundaryKind::Argument, "collection element"),
                    vec![CoercionKind::HomogeneousList],
                );
                if let Some(failure) = self.report.diagnostics[diagnostic_start..]
                    .iter_mut()
                    .find_map(|diagnostic| diagnostic.coercion_failure.as_mut())
                {
                    failure.path.push(CoercionPathSegment::Element(index));
                }
                element = expected_element.clone();
                homogeneous &= checked != Type::Unknown;
            } else {
                match self.types.unify(&element, &found) {
                    Ok(ty) => element = ty,
                    Err(_) => homogeneous = false,
                }
            }
            path.pop();
        }
        if homogeneous {
            Type::List(Box::new(self.types.resolve(&element)))
        } else {
            Type::List(Box::new(Type::Unknown))
        }
    }

    fn infer_record(&mut self, fields: &[(Form, Form)], path: &mut FormPath) -> Type {
        let mut record = BTreeMap::new();
        let mut open = false;
        for (index, (key, value)) in fields.iter().enumerate() {
            let Form::Kw(name) = key else {
                open = true;
                continue;
            };
            path.push(index * 2 + 1);
            let ty = self.infer(value, None, path, BoundaryContext::field(BoundaryKind::Meta, "record", name.clone()));
            path.pop();
            record.insert(name.to_string(), ty);
        }
        Type::Record(RecordType { fields: record, open })
    }

    fn infer_list(
        &mut self,
        items: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
        context: &BoundaryContext,
    ) -> Type {
        if items.is_empty() {
            return Type::List(Box::new(Type::Unknown));
        }
        if let Some(Form::Kw(field)) = items.first() {
            return self.infer_field_access(field, items, path, context);
        }
        let Some(Form::Sym(head)) = items.first() else {
            return self.infer_call(items, path, context);
        };
        match head.as_ref() {
            "fn" => self.infer_fn(items, expected, path, context),
            "let" | "loop" => self.infer_let(items, expected, path, context),
            "if" => self.infer_if(items, expected, path, context),
            "when" => self.infer_when(items, path, context),
            "do" => self.infer_body(&items[1..], expected, path, 1, context),
            "quote" | "quasiquote" => Type::Unknown,
            "live" => {
                let value = items.get(1).map(|item| {
                    path.push(1);
                    let ty = self.infer(item, None, path, context.clone());
                    path.pop();
                    ty
                }).unwrap_or(Type::Unknown);
                Type::Dyn { class: Some(DynClass::Closed), value: Box::new(value) }
            }
            "evolve" => {
                let value = items.get(1).map(|item| {
                    path.push(1);
                    let ty = self.infer(item, None, path, context.clone());
                    path.pop();
                    ty
                }).unwrap_or(Type::Unknown);
                Type::Dyn { class: Some(DynClass::Scanned), value: Box::new(value) }
            }
            "vel" => Type::Dyn { class: Some(DynClass::Integrated), value: Box::new(Type::Figure) },
            "curve" => Type::Curve,
            "fields" => items.get(1).map(|figure| {
                path.push(1);
                let ty = self.infer(figure, None, path, context.clone());
                path.pop();
                ty
            }).unwrap_or(Type::Unknown),
            "from-host" => {
                if let Some(Form::Kw(name)) = items.get(1) {
                    self.push_schema(path, SchemaIdentity::HostChannel(name.clone()));
                }
                self.unchecked(path, BoundaryContext::named(BoundaryKind::HostChannel, "from-host"), "host channel value type is supplied by the host".into());
                Type::Unknown
            }
            "entities-where" => self.infer_entities_where(items, path),
            "spawn" => self.infer_spawn(items, path),
            "emit" => self.infer_emit(items, path),
            "render" => self.infer_render_call(items, path),
            "circle-collider" | "capsule-chain-collider" => self.infer_projector(head, items, path),
            name if action_slot_signature(name).is_some() => {
                let signature = action_slot_signature(name).unwrap();
                self.check_signature_call(&signature, &items[1..], path, context)
            }
            name if engine_signature(name).is_some() => {
                let signature = engine_signature(name).unwrap();
                self.check_signature_call(&signature, &items[1..], path, context)
            }
            name if builtin_signature(name).is_some() && !self.is_shadowed(name) => {
                let signature = builtin_signature(name).unwrap();
                self.check_signature_call(&signature, &items[1..], path, context)
            }
            _ => self.infer_call(items, path, context),
        }
    }

    fn infer_field_access(
        &mut self,
        field: &Rc<str>,
        items: &[Form],
        path: &mut FormPath,
        context: &BoundaryContext,
    ) -> Type {
        if items.len() != 2 {
            for (index, item) in items.iter().enumerate().skip(1) {
                path.push(index);
                let _ = self.infer(item, None, path, context.clone());
                path.pop();
            }
            self.unchecked(
                path,
                BoundaryContext::field(BoundaryKind::EntityField, "keyword clause", field.clone()),
                format!("keyword clause :{} remains runtime-checked", field),
            );
            return Type::Unknown;
        }
        path.push(1);
        let subject = self.infer(&items[1], None, path, context.clone());
        path.pop();
        let special = match field.as_ref() {
            "pos" | "vel" => Some(Type::Pose),
            "t" | "tick" => Some(Type::Num),
            "handle" => Some(Type::Handle),
            "kind" => Some(Type::Symbol),
            _ => None,
        };
        if let Some(ty) = special {
            self.push_schema(path, SchemaIdentity::EntityField(field.clone()));
            return ty;
        }
        match self.types.resolve(&subject) {
            Type::Record(record) => match record.fields.get(field.as_ref()) {
                Some(ty) => ty.clone(),
                None if record.open => {
                    self.unchecked(
                        path,
                        BoundaryContext::field(BoundaryKind::EntityField, "open record", field.clone()),
                        format!("field :{} is resolved by the open runtime schema", field),
                    );
                    Type::Unknown
                }
                None => {
                    self.diagnostic(
                        DiagnosticCategory::UnknownEntityField,
                        CheckConfidence::ProvenViolation,
                        path,
                        None,
                        None,
                        BoundaryContext::field(BoundaryKind::EntityField, "record", field.clone()),
                        None,
                        format!("field :{} is absent from the closed record schema", field),
                    );
                    Type::Unknown
                }
            },
            Type::EntityView(shape) => match shape.fields.fields.get(field.as_ref()) {
                Some(ty) => ty.clone(),
                None => {
                    self.unchecked(
                        path,
                        BoundaryContext::field(BoundaryKind::EntityField, "entity view", field.clone()),
                        format!("entity field :{} is resolved by the collected runtime schema", field),
                    );
                    Type::Unknown
                }
            },
            Type::Handle => {
                self.unchecked(
                    path,
                    BoundaryContext::field(BoundaryKind::EntityField, "entity handle", field.clone()),
                    format!("entity field :{} is resolved by the collected runtime schema", field),
                );
                Type::Unknown
            }
            Type::Unknown | Type::Var(_) => {
                self.unchecked(
                    path,
                    BoundaryContext::field(BoundaryKind::EntityField, "dynamic value", field.clone()),
                    format!("field :{} cannot be proven before runtime", field),
                );
                Type::Unknown
            }
            found => {
                self.report_mismatch(
                    path,
                    Type::open_record([(field.to_string(), Type::Unknown)]),
                    found,
                    BoundaryContext::field(BoundaryKind::EntityField, "field access", field.clone()),
                    None,
                );
                Type::Unknown
            }
        }
    }

    fn infer_fn(
        &mut self,
        items: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
        context: &BoundaryContext,
    ) -> Type {
        let Some(Form::Vector(params)) = items.get(1) else {
            self.unchecked(path, context.clone(), "function with dynamic parameter form".into());
            return Type::Unknown;
        };
        if params.iter().any(|param| matches!(param, Form::Sym(name) if name.as_ref() == "&")) {
            self.unchecked(
                path,
                context.clone(),
                "variadic function inference remains runtime-checked".into(),
            );
            return Type::Unknown;
        }
        let expected_fn = expected.and_then(|ty| match self.types.resolve(ty) {
            Type::Fn { args, ret } if args.len() == params.len() => Some((args, *ret)),
            _ => None,
        });
        self.env.push();
        let mut args = Vec::new();
        for (index, param) in params.iter().enumerate() {
            let Some(name) = param_symbol(param) else {
                args.push(Type::Unknown);
                continue;
            };
            let ty = expected_fn
                .as_ref()
                .and_then(|(args, _)| args.get(index).cloned())
                .unwrap_or_else(|| self.types.fresh());
            self.env.insert(name.to_string(), TypeScheme::mono(ty.clone()));
            args.push(ty);
        }
        let expected_ret = expected_fn.as_ref().map(|(_, ret)| ret);
        let return_context = if context.kind == BoundaryKind::Query {
            context.clone()
        } else {
            BoundaryContext::named(BoundaryKind::Return, "function")
        };
        let result = self.infer_body(&items[2..], expected_ret, path, 2, &return_context);
        self.env.pop();
        Type::function(args.into_iter().map(|arg| self.types.resolve(&arg)).collect::<Vec<_>>(), self.types.resolve(&result))
    }

    fn infer_let(
        &mut self,
        items: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
        context: &BoundaryContext,
    ) -> Type {
        let Some(Form::Vector(bindings)) = items.get(1) else {
            self.unchecked(path, context.clone(), "let with dynamic binding form".into());
            return Type::Unknown;
        };
        self.env.push();
        for (pair_index, pair) in bindings.chunks(2).enumerate() {
            let [Form::Sym(name), value] = pair else { continue };
            path.extend([1, pair_index * 2 + 1]);
            let ty = self.infer(value, None, path, BoundaryContext::named(BoundaryKind::Return, name.clone()));
            path.truncate(path.len() - 2);
            let scheme = if is_generalizable(value) {
                self.types.generalize(&self.env, &ty)
            } else {
                TypeScheme::mono(ty)
            };
            self.env.insert(name.to_string(), scheme);
        }
        let result = self.infer_body(&items[2..], expected, path, 2, context);
        self.env.pop();
        result
    }

    fn infer_if(
        &mut self,
        items: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
        context: &BoundaryContext,
    ) -> Type {
        if let Some(test) = items.get(1) {
            path.push(1);
            let _ = self.infer(test, Some(&Type::Num), path, BoundaryContext::named(BoundaryKind::Branch, "if condition"));
            path.pop();
        }
        let then_ty = items.get(2).map(|form| {
            path.push(2);
            let ty = self.infer(form, expected, path, context.clone());
            path.pop();
            ty
        }).unwrap_or_else(|| no_op_branch_type(expected));
        let else_ty = items.get(3).map(|form| {
            path.push(3);
            let ty = self.infer(form, expected, path, context.clone());
            path.pop();
            ty
        }).unwrap_or_else(|| no_op_branch_type(expected));
        match self.types.unify(&then_ty, &else_ty) {
            Ok(ty) => ty,
            Err(error) => {
                let diagnostic_start = self.report.diagnostics.len();
                self.report_unify(error, path, BoundaryContext::named(BoundaryKind::Branch, "if branches"), None);
                if expected.is_none() {
                    for diagnostic in &mut self.report.diagnostics[diagnostic_start..] {
                        diagnostic.confidence = CheckConfidence::CheckerLimitation;
                    }
                }
                Type::Unknown
            }
        }
    }

    fn infer_when(&mut self, items: &[Form], path: &mut FormPath, context: &BoundaryContext) -> Type {
        if let Some(test) = items.get(1) {
            path.push(1);
            let _ = self.infer(test, Some(&Type::Num), path, BoundaryContext::named(BoundaryKind::Branch, "when condition"));
            path.pop();
        }
        let body = self.infer_body(&items[2..], None, path, 2, context);
        Type::Option(Box::new(body))
    }

    fn infer_body(
        &mut self,
        forms: &[Form],
        expected: Option<&Type>,
        path: &mut FormPath,
        offset: usize,
        context: &BoundaryContext,
    ) -> Type {
        let mut result = Type::Nothing;
        for (index, form) in forms.iter().enumerate() {
            path.push(index + offset);
            let body_expected = (index + 1 == forms.len()).then_some(expected).flatten();
            result = self.infer(form, body_expected, path, context.clone());
            path.pop();
        }
        result
    }

    fn infer_call(&mut self, items: &[Form], path: &mut FormPath, context: &BoundaryContext) -> Type {
        path.push(0);
        let callee = self.infer(&items[0], None, path, context.clone());
        path.pop();
        match self.types.resolve(&callee) {
            Type::Fn { args, ret } => {
                if args.len() != items.len() - 1 {
                    self.report_arity(path, context.clone(), Arity::Exact(args.len()), items.len() - 1);
                    return Type::Unknown;
                }
                for (index, (arg, expected)) in items[1..].iter().zip(args).enumerate() {
                    path.push(index + 1);
                    let _ = self.infer(arg, Some(&expected), path, BoundaryContext::argument(call_name(&items[0]), index));
                    path.pop();
                }
                *ret
            }
            Type::Unknown | Type::Var(_) => {
                for (index, arg) in items.iter().enumerate().skip(1) {
                    path.push(index);
                    let _ = self.infer(arg, None, path, context.clone());
                    path.pop();
                }
                self.unchecked(path, context.clone(), "dynamic call retains runtime dispatch".into());
                Type::Unknown
            }
            found => {
                self.report_mismatch(path, Type::function(vec![], Type::Unknown), found, context.clone(), None);
                Type::Unknown
            }
        }
    }

    fn check_signature_call(
        &mut self,
        signature: &Signature,
        args: &[Form],
        path: &mut FormPath,
        _context: &BoundaryContext,
    ) -> Type {
        if !signature.arity.accepts(args.len()) {
            self.report_arity(
                path,
                BoundaryContext::named(BoundaryKind::Argument, signature.name),
                signature.arity,
                args.len(),
            );
            return Type::Unknown;
        }
        let Type::Fn { args: expected_args, ret } = self.instantiate_signature(signature) else { unreachable!() };
        let mut lifted_class = None;
        for (index, arg) in args.iter().enumerate() {
            let expected = expected_args
                .get(index)
                .or_else(|| matches!(signature.arity, Arity::Variadic { .. }).then(|| expected_args.last()).flatten());
            path.push(index + 1);
            let boundary = if signature.name == "entities-where" {
                BoundaryContext::named(BoundaryKind::Query, "entities-where predicate")
            } else if matches!(expected, Some(Type::Action)) {
                BoundaryContext::named(BoundaryKind::Action, signature.name)
            } else {
                BoundaryContext::argument(signature.name, index)
            };
            let found = self.infer(arg, None, path, boundary.clone());
            if let (SignatureKind::Pure, Some(expected), Type::Dyn { class, .. }) =
                (signature.kind, expected, self.types.resolve(&found))
            {
                let dyn_expected = Type::dyn_of(expected.clone());
                let checked = self.apply_expected(found, Some(&dyn_expected), path, boundary, Vec::new());
                if checked != Type::Unknown {
                    lifted_class = merge_dyn_class(lifted_class, class);
                }
            } else {
                let _ = self.infer(arg, expected, path, boundary);
            }
            path.pop();
        }
        let result = *ret;
        match lifted_class {
            Some(class) if result != Type::Unknown => Type::Dyn {
                class: Some(class),
                value: Box::new(result),
            },
            _ => result,
        }
    }

    fn infer_spawn(&mut self, items: &[Form], path: &mut FormPath) -> Type {
        if items.len() < 2 {
            self.report_arity(path, BoundaryContext::named(BoundaryKind::Spawn, "spawn"), Arity::Variadic { min: 1 }, 0);
            return Type::Unknown;
        }
        let spawn_figure_kind = source_figure_kind(&items[1]);
        path.push(1);
        let _ = self.infer(
            &items[1],
            Some(&Type::dyn_of(Type::Figure)),
            path,
            BoundaryContext::named(BoundaryKind::Spawn, "spawn figure"),
        );
        path.pop();
        for (index, slot) in items.iter().enumerate().skip(2) {
            path.push(index);
            match slot {
                Form::Map(_) => {
                    let _ = self.infer(slot, Some(&Type::Meta), path, BoundaryContext::named(BoundaryKind::Meta, "spawn metadata"));
                }
                _ => {
                    let found = self.infer(slot, None, path, BoundaryContext::named(BoundaryKind::Projector, "spawn collider"));
                    let resolved = self.types.resolve(&found);
                    if let (
                        Some(expected_figure),
                        Type::Projector { kind: ProjectorKind::Collider, figure: found_figure, .. },
                    ) = (spawn_figure_kind, &resolved)
                    {
                        if *found_figure != FigureKind::Any && *found_figure != expected_figure {
                            self.report_mismatch(
                                path,
                                Type::Projector {
                                    kind: ProjectorKind::Collider,
                                    figure: expected_figure,
                                    render: None,
                                },
                                resolved.clone(),
                                BoundaryContext::named(BoundaryKind::Projector, "spawn collider figure"),
                                None,
                            );
                        }
                    } else if !matches!(resolved, Type::Unknown | Type::ColliderProjector | Type::Projector { .. } | Type::Record(_) | Type::Dyn { .. } | Type::Array(_) | Type::List(_) | Type::Nothing) {
                        self.report_mismatch(path, Type::ColliderProjector, found, BoundaryContext::named(BoundaryKind::Projector, "spawn collider or metadata"), None);
                    }
                }
            }
            path.pop();
        }
        Type::Action
    }

    fn infer_entities_where(&mut self, items: &[Form], path: &mut FormPath) -> Type {
        if items.len() != 2 {
            self.report_arity(
                path,
                BoundaryContext::named(BoundaryKind::Query, "entities-where"),
                Arity::Exact(1),
                items.len().saturating_sub(1),
            );
            return Type::Unknown;
        }
        path.push(1);
        if matches!(items.get(1), Some(Form::Map(_))) {
            let _ = self.infer(
                &items[1],
                None,
                path,
                BoundaryContext::named(BoundaryKind::Query, "entities-where selector"),
            );
        } else {
            let view = Type::EntityView(EntityShape {
                figure: FigureKind::Any,
                fields: RecordType { fields: BTreeMap::new(), open: true },
            });
            let predicate = Type::function(vec![view], Type::Num);
            let _ = self.infer(
                &items[1],
                Some(&predicate),
                path,
                BoundaryContext::named(BoundaryKind::Query, "entities-where predicate"),
            );
        }
        path.pop();
        Type::EntitySet
    }

    fn infer_emit(&mut self, items: &[Form], path: &mut FormPath) -> Type {
        if items.len() != 3 {
            self.report_arity(path, BoundaryContext::named(BoundaryKind::Action, "emit"), Arity::Exact(2), items.len().saturating_sub(1));
            return Type::Unknown;
        }
        match &items[1] {
            Form::Kw(stream) if stream.as_ref() == "render" => {
                path.push(2);
                self.check_render_row(&items[2], path);
                path.pop();
            }
            Form::Kw(stream) if stream.as_ref() == "events" => {
                path.push(2);
                let _ = self.infer(&items[2], None, path, BoundaryContext::named(BoundaryKind::Action, "event row"));
                path.pop();
            }
            Form::Kw(stream) => {
                path.push(1);
                self.report_mismatch(path, Type::Symbol, Type::Symbol, BoundaryContext::named(BoundaryKind::Action, format!("unknown emit stream :{}", stream)), None);
                path.pop();
            }
            other => {
                path.push(1);
                let _ = self.infer(other, Some(&Type::Symbol), path, BoundaryContext::named(BoundaryKind::Action, "emit stream"));
                path.pop();
            }
        }
        Type::Action
    }

    fn infer_render_call(&mut self, items: &[Form], path: &mut FormPath) -> Type {
        if items.len() != 2 {
            self.report_arity(path, BoundaryContext::named(BoundaryKind::Render, "render"), Arity::Exact(1), items.len().saturating_sub(1));
            return Type::Unknown;
        }
        path.push(1);
        self.check_render_row(&items[1], path);
        path.pop();
        Type::Action
    }

    fn check_render_row(&mut self, form: &Form, path: &mut FormPath) {
        let Form::Map(fields) = form else {
            let _ = self.infer(form, None, path, BoundaryContext::named(BoundaryKind::Render, "render row"));
            self.unchecked(path, BoundaryContext::named(BoundaryKind::Render, "render row"), "computed render rows retain runtime schema checking".into());
            return;
        };
        let kind = fields.iter().find_map(|(key, value)| match (key, value) {
            (Form::Kw(key), Form::Kw(value)) if key.as_ref() == "kind" => Some(value.clone()),
            _ => None,
        }).unwrap_or_else(|| Rc::from("default"));
        self.push_schema(path, SchemaIdentity::RenderKind(kind.clone()));
        let declared = self.schema.render_kinds.iter().find(|decl| decl.name == kind);
        if kind.as_ref() != "default" && declared.is_none() {
            self.diagnostic(
                DiagnosticCategory::UnknownRenderKind,
                CheckConfidence::UncheckedDynamic,
                path,
                None,
                None,
                BoundaryContext::named(BoundaryKind::Render, format!("render kind :{}", kind)),
                None,
                format!("render kind :{} is undeclared; its open runtime schema remains authoritative", kind),
            );
        }
        for (index, (key, value)) in fields.iter().enumerate() {
            let Form::Kw(field) = key else { continue };
            if field.as_ref() == "kind" {
                continue;
            }
            let expected = builtin_render_field_type(field)
                .or_else(|| declared.and_then(|decl| render_field_type(decl, field)));
            path.push(index * 2 + 1);
            match expected {
                Some(expected) => {
                    let _ = self.infer(value, Some(&expected), path, BoundaryContext::field(BoundaryKind::Render, kind.clone(), field.clone()));
                    self.push_schema(path, SchemaIdentity::RenderField { kind: kind.clone(), field: field.clone() });
                }
                None if declared.is_some() => {
                    let found = self.infer(value, None, path, BoundaryContext::field(BoundaryKind::Render, kind.clone(), field.clone()));
                    self.diagnostic(
                        DiagnosticCategory::RenderField,
                        CheckConfidence::ProvenViolation,
                        path,
                        None,
                        Some(found),
                        BoundaryContext::field(BoundaryKind::Render, kind.clone(), field.clone()),
                        None,
                        format!("field :{} is not declared for render kind :{}", field, kind),
                    );
                }
                None => {
                    let _ = self.infer(value, None, path, BoundaryContext::field(BoundaryKind::Render, kind.clone(), field.clone()));
                }
            }
            path.pop();
        }
    }

    fn infer_projector(&mut self, name: &Rc<str>, items: &[Form], path: &mut FormPath) -> Type {
        let signature = projector_signature(name).unwrap();
        self.push_schema(path, SchemaIdentity::Projector(name.clone()));
        if !signature.arity.accepts(items.len().saturating_sub(1)) {
            self.report_arity(
                path,
                BoundaryContext::named(BoundaryKind::Projector, name.clone()),
                signature.arity,
                items.len().saturating_sub(1),
            );
            return Type::Unknown;
        }
        if let Some(Form::Map(fields)) = items.get(1) {
            for (index, (key, value)) in fields.iter().enumerate() {
                let Form::Kw(field) = key else { continue };
                path.extend([1, index * 2 + 1]);
                if let Some(expected) = projector_field_type(name, field) {
                    let _ = self.infer(
                        value,
                        Some(&expected),
                        path,
                        BoundaryContext::field(BoundaryKind::Projector, name.clone(), field.clone()),
                    );
                } else {
                    let _ = self.infer(
                        value,
                        None,
                        path,
                        BoundaryContext::field(BoundaryKind::Projector, name.clone(), field.clone()),
                    );
                    self.unchecked(
                        path,
                        BoundaryContext::field(BoundaryKind::Projector, name.clone(), field.clone()),
                        format!("projector option :{} remains runtime-checked", field),
                    );
                }
                path.truncate(path.len() - 2);
            }
        }
        signature.result
    }

    fn apply_expected(
        &mut self,
        found: Type,
        expected: Option<&Type>,
        path: &FormPath,
        context: BoundaryContext,
        mut coercions: Vec<CoercionKind>,
    ) -> Type {
        let Some(expected) = expected else {
            self.push_node(path, found.clone(), None, coercions, None, None);
            return found;
        };
        let expected = self.types.resolve(expected);
        let mut found = self.types.resolve(&found);
        if found == Type::Unknown || expected == Type::Unknown {
            self.push_node(path, found.clone(), Some(expected), coercions, None, None);
            return found;
        }

        if let Type::Dyn { class: expected_class, value: expected_value } = &expected {
            if let Type::Dyn { class: found_class, value: found_value } = &found {
                if expected_class.is_some() && found_class.is_some() && expected_class != found_class {
                    self.report_mismatch(path, expected.clone(), found.clone(), context, None);
                    return Type::Unknown;
                }
                match self.coerce_structural(
                    (**found_value).clone(),
                    expected_value,
                    &mut coercions,
                ) {
                    Ok(value) => {
                        let ty = Type::Dyn { class: found_class.or(*expected_class), value: Box::new(value) };
                        self.push_node(path, ty.clone(), Some(expected), coercions, None, None);
                        return ty;
                    }
                    Err(error) => {
                        self.report_unify(error, path, context, None);
                        return Type::Unknown;
                    }
                }
            }
            if let Some((structured, class)) = sequence_dyn_structure(&found) {
                found = structured;
                coercions.push(CoercionKind::SequenceDynStructure);
                if let Ok(inner) = self.coerce_structural(found.clone(), expected_value, &mut coercions) {
                    let ty = Type::Dyn { class: Some(class), value: Box::new(inner) };
                    self.push_node(path, ty.clone(), Some(expected), coercions, None, None);
                    return ty;
                }
            }
            match self.coerce_structural(found.clone(), expected_value, &mut coercions) {
                Ok(inner) => {
                    coercions.push(CoercionKind::ConstantToDyn);
                    let ty = Type::Dyn { class: Some(DynClass::Const), value: Box::new(inner) };
                    self.push_node(path, ty.clone(), Some(expected), coercions, None, None);
                    return ty;
                }
                Err(error) => {
                    let failure = (!coercions.is_empty()).then(|| CoercionFailure {
                        path: Vec::new(),
                        attempted: coercions,
                    });
                    self.report_unify(error, path, context, failure);
                    return Type::Unknown;
                }
            }
        }

        match self.coerce_structural(found.clone(), &expected, &mut coercions) {
            Ok(ty) => {
                self.push_node(path, ty.clone(), Some(expected), coercions, None, None);
                ty
            }
            Err(error) => {
                let failure = (!coercions.is_empty()).then(|| CoercionFailure {
                    path: Vec::new(),
                    attempted: coercions,
                });
                self.report_unify(error, path, context, failure);
                Type::Unknown
            }
        }
    }

    fn coerce_structural(
        &mut self,
        found: Type,
        expected: &Type,
        coercions: &mut Vec<CoercionKind>,
    ) -> Result<Type, UnifyError> {
        let found = self.types.resolve(&found);
        let expected = self.types.resolve(expected);
        if found == Type::Pose && expected == Type::Figure {
            coercions.push(CoercionKind::PoseToFigure);
            return Ok(Type::Figure);
        }
        if found == Type::Curve && expected == Type::Figure {
            return Ok(Type::Figure);
        }
        if expected == Type::Action
            && (found == Type::Nothing
                || matches!(&found, Type::Option(value) if **value == Type::Action))
        {
            coercions.push(CoercionKind::NothingToAction);
            return Ok(Type::Action);
        }
        if expected == Type::Figure {
            if let Type::Array(element) | Type::List(element) = &found {
                let element = self.coerce_structural((**element).clone(), &Type::Figure, coercions)?;
                if element == Type::Figure {
                    coercions.push(CoercionKind::HomogeneousList);
                    return Ok(Type::Figure);
                }
            }
        }
        match (&found, &expected) {
            (Type::List(found_elem), Type::Array(expected_elem)) => {
                let elem = self.types.unify(expected_elem, found_elem)?;
                coercions.push(CoercionKind::HomogeneousList);
                return Ok(Type::Array(Box::new(elem)));
            }
            (Type::Array(found_elem), Type::List(expected_elem)) => {
                let elem = self.types.unify(expected_elem, found_elem)?;
                coercions.push(CoercionKind::HomogeneousList);
                return Ok(Type::List(Box::new(elem)));
            }
            (Type::Array(found_elem), Type::Vec { len, elem: expected_elem }) => {
                let elem = self.types.unify(expected_elem, found_elem)?;
                coercions.push(CoercionKind::HomogeneousList);
                return Ok(Type::Vec { len: *len, elem: Box::new(elem) });
            }
            (Type::Record(found_record), Type::Meta) => {
                if found_record.fields.values().all(is_meta_value) {
                    return Ok(Type::Meta);
                }
            }
            _ => {}
        }
        self.types.unify(&expected, &found)
    }

    fn instantiate_signature(&mut self, signature: &Signature) -> Type {
        let ty = Type::function(signature.params.clone(), signature.result.clone());
        self.types.instantiate(&TypeScheme { quantified: vec![TypeVar(0)], ty })
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.env.get(name).is_some() || self.card.defs.contains_key(name)
    }

    fn push_node(
        &mut self,
        path: &[usize],
        inferred: Type,
        expected: Option<Type>,
        coercions: Vec<CoercionKind>,
        schema: Option<SchemaIdentity>,
        resolved: Option<ResolvedIdentity>,
    ) {
        let provenance = self.provenance(path);
        if let Some(existing) = self.report.nodes.iter_mut().find(|node| node.path == path) {
            existing.inferred = inferred;
            if expected.is_some() {
                existing.expected = expected;
            }
            if !coercions.is_empty() {
                existing.coercions = coercions;
            }
            if schema.is_some() {
                existing.schema = schema;
            }
            if resolved.is_some() {
                existing.resolved = resolved;
            }
            return;
        }
        self.report.nodes.push(TypedNode {
            path: path.to_vec(),
            resolved,
            inferred,
            expected,
            coercions,
            schema,
            provenance,
        });
    }

    fn push_schema(&mut self, path: &[usize], schema: SchemaIdentity) {
        self.push_node(path, Type::Unknown, None, Vec::new(), Some(schema), None);
    }

    fn provenance(&self, path: &[usize]) -> Provenance {
        self.provenance
            .get(path)
            .cloned()
            .or_else(|| {
                (1..path.len())
                    .rev()
                    .find_map(|len| self.provenance.get(&path[..len]).cloned())
            })
            .unwrap_or_else(|| Provenance::authored(SourceSpan::synthetic("<generated>")))
    }

    fn report_unify(
        &mut self,
        error: UnifyError,
        path: &[usize],
        context: BoundaryContext,
        failure: Option<CoercionFailure>,
    ) {
        if let UnifyError::Infinite { var, ty } = &error {
            self.diagnostic(
                DiagnosticCategory::RecursiveInference,
                CheckConfidence::CheckerLimitation,
                path,
                None,
                Some(ty.clone()),
                context,
                failure,
                format!("recursive boundary for 't{} remains runtime-checked", var.0),
            );
            return;
        }
        let (expected, found, message) = match error {
            UnifyError::Mismatch { expected, found } => {
                let message = format!("expected {}, found {}", expected, found);
                (Some(expected), Some(found), message)
            }
            UnifyError::Arity { expected, found } => {
                self.report_arity(path, context, Arity::Exact(expected), found);
                return;
            }
            UnifyError::MissingField { field, expected } => {
                let message = format!("missing field :{} of type {}", field, expected);
                (Some(expected), None, message)
            }
            UnifyError::UnexpectedField { field, found } => {
                let message = format!("unexpected field :{} of type {}", field, found);
                (None, Some(found), message)
            }
            UnifyError::Infinite { .. } => unreachable!(),
        };
        let limitation = expected.as_ref().is_some_and(type_contains_unresolved)
            || found.as_ref().is_some_and(type_contains_unresolved);
        self.diagnostic(
            if failure.is_some() { DiagnosticCategory::FailedCoercion } else { category_for(&context) },
            if limitation {
                CheckConfidence::CheckerLimitation
            } else {
                CheckConfidence::ProvenViolation
            },
            path,
            expected,
            found,
            context,
            failure,
            message,
        );
    }

    fn report_mismatch(
        &mut self,
        path: &[usize],
        expected: Type,
        found: Type,
        context: BoundaryContext,
        failure: Option<CoercionFailure>,
    ) {
        let message = format!("expected {}, found {}", expected, found);
        let limitation = type_contains_unresolved(&expected) || type_contains_unresolved(&found);
        self.diagnostic(
            category_for(&context),
            if limitation {
                CheckConfidence::CheckerLimitation
            } else {
                CheckConfidence::ProvenViolation
            },
            path,
            Some(expected),
            Some(found),
            context,
            failure,
            message,
        );
    }

    fn report_arity(&mut self, path: &[usize], context: BoundaryContext, expected: Arity, found: usize) {
        let expected_text = match expected {
            Arity::Exact(count) => count.to_string(),
            Arity::Range { min, max } => format!("{}..={}", min, max),
            Arity::Variadic { min } => format!("at least {}", min),
        };
        self.diagnostic(
            DiagnosticCategory::ArityMismatch,
            CheckConfidence::ProvenViolation,
            path,
            None,
            None,
            context,
            None,
            format!("expected {} arguments, found {}", expected_text, found),
        );
    }

    fn unchecked(&mut self, path: &[usize], context: BoundaryContext, message: String) {
        self.diagnostic(
            DiagnosticCategory::UncheckedForm,
            CheckConfidence::UncheckedDynamic,
            path,
            None,
            None,
            context,
            None,
            message,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn diagnostic(
        &mut self,
        category: DiagnosticCategory,
        confidence: CheckConfidence,
        path: &[usize],
        expected: Option<Type>,
        found: Option<Type>,
        context: BoundaryContext,
        coercion_failure: Option<CoercionFailure>,
        message: String,
    ) {
        let provenance = self.provenance(path);
        self.report.diagnostics.push(TypeDiagnostic {
            category,
            confidence,
            primary_span: provenance.primary_span().clone(),
            expected,
            found,
            context,
            related_spans: provenance
                .expansion_stack
                .iter()
                .filter_map(|frame| frame.definition.clone())
                .collect(),
            expansion_stack: provenance.expansion_stack,
            coercion_failure,
            message,
        });
    }
}

fn category_for(context: &BoundaryContext) -> DiagnosticCategory {
    match context.kind {
        BoundaryKind::Action => DiagnosticCategory::ActionPosition,
        BoundaryKind::Spawn => DiagnosticCategory::SpawnFigure,
        BoundaryKind::Meta => DiagnosticCategory::SpawnMeta,
        BoundaryKind::Query => DiagnosticCategory::QueryPredicate,
        BoundaryKind::Projector => DiagnosticCategory::ProjectorFigure,
        BoundaryKind::Render => DiagnosticCategory::RenderField,
        BoundaryKind::EntityField => DiagnosticCategory::UnknownEntityField,
        BoundaryKind::HostChannel => DiagnosticCategory::HostChannel,
        _ => DiagnosticCategory::TypeMismatch,
    }
}

fn source_figure_kind(form: &Form) -> Option<FigureKind> {
    let Form::List(items) = form else { return None };
    let Some(Form::Sym(head)) = items.first() else { return None };
    match head.as_ref() {
        "curve" => Some(FigureKind::Curve),
        "cart" | "polar" | "pose" | "rot" | "still" | "linear" | "vel" => {
            Some(FigureKind::Pose)
        }
        "in-frame" | "clamp" | "fields" => items.last().and_then(source_figure_kind),
        _ => None,
    }
}

fn no_op_branch_type(expected: Option<&Type>) -> Type {
    if matches!(expected, Some(Type::Action)) {
        Type::Action
    } else {
        Type::Nothing
    }
}

fn param_symbol(form: &Form) -> Option<&Rc<str>> {
    match form {
        Form::Sym(name) => Some(name),
        Form::Vector(items) => items.first().and_then(|item| match item {
            Form::Sym(name) => Some(name),
            _ => None,
        }),
        _ => None,
    }
}

fn call_name(form: &Form) -> Rc<str> {
    match form {
        Form::Sym(name) => name.clone(),
        _ => Rc::from("function"),
    }
}

fn is_generalizable(form: &Form) -> bool {
    match form {
        Form::Num(_) | Form::Str(_) | Form::Kw(_) | Form::Bool(_) | Form::Vector(_) | Form::Map(_) => true,
        Form::List(items) => matches!(items.first(), Some(Form::Sym(head)) if head.as_ref() == "fn"),
        _ => false,
    }
}

fn is_meta_value(ty: &Type) -> bool {
    match ty {
        Type::Num
        | Type::Symbol
        | Type::Kw
        | Type::String
        | Type::Nothing
        | Type::Option(_)
        | Type::Array(_)
        | Type::List(_)
        | Type::Dyn { .. }
        | Type::Unknown
        | Type::Var(_) => true,
        Type::Record(record) => record.fields.values().all(is_meta_value),
        _ => false,
    }
}

fn type_contains_unresolved(ty: &Type) -> bool {
    match ty {
        Type::Unknown | Type::Var(_) => true,
        Type::List(value)
        | Type::Array(value)
        | Type::Option(value)
        | Type::Dyn { value, .. } => type_contains_unresolved(value),
        Type::Vec { elem, .. } | Type::Mat { elem, .. } => type_contains_unresolved(elem),
        Type::Record(record) => record.fields.values().any(type_contains_unresolved),
        Type::Fn { args, ret } => {
            args.iter().any(type_contains_unresolved) || type_contains_unresolved(ret)
        }
        _ => false,
    }
}

fn render_field_type(decl: &RenderKindDecl, field: &str) -> Option<Type> {
    decl.fields.iter().find_map(|(name, kind)| {
        (name.as_ref() == field).then_some(match kind {
            RenderFieldKind::Num => Type::Num,
            RenderFieldKind::Sym => Type::Symbol,
        })
    })
}

fn builtin_render_field_type(field: &str) -> Option<Type> {
    match field {
        "x" | "y" | "theta" | "facing" | "scale" | "alpha" | "opacity" | "hue"
        | "active" => Some(Type::Num),
        "shape" => Some(Type::Unknown),
        "points" | "pts" => Some(Type::Array(Box::new(Type::Pose))),
        _ => None,
    }
}

fn sequence_dyn_structure(ty: &Type) -> Option<(Type, DynClass)> {
    match ty {
        Type::Array(element) => {
            let (element, class) = sequence_dyn_structure(element)?;
            Some((Type::Array(Box::new(element)), class))
        }
        Type::List(element) => {
            let (element, class) = sequence_dyn_structure(element)?;
            Some((Type::List(Box::new(element)), class))
        }
        Type::Record(record) => {
            let mut class = None;
            let mut fields = BTreeMap::new();
            for (name, ty) in &record.fields {
                match ty {
                    Type::Dyn { class: field_class, value } => {
                        class = merge_dyn_class(class, *field_class);
                        fields.insert(name.clone(), (**value).clone());
                    }
                    other => {
                        fields.insert(name.clone(), other.clone());
                    }
                }
            }
            class.map(|class| (Type::Record(RecordType { fields, open: record.open }), class))
        }
        Type::Dyn { class, value } => Some(((**value).clone(), class.unwrap_or(DynClass::Closed))),
        _ => None,
    }
}

fn merge_dyn_class(left: Option<DynClass>, right: Option<DynClass>) -> Option<DynClass> {
    use DynClass::*;
    match (left, right) {
        (None, right) => right,
        (left, None) => left,
        (Some(a), Some(b)) => Some(match (a, b) {
            (Scanned, _) | (_, Scanned) => Scanned,
            (Integrated, _) | (_, Integrated) => Integrated,
            (PiecewiseClosed, _) | (_, PiecewiseClosed) => PiecewiseClosed,
            (Closed, _) | (_, Closed) => Closed,
            _ => Const,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::{read_all, read_one};
    use crate::interp::{collect_card_schema, load_card};

    fn check(src: &str) -> CheckReport {
        let forms = read_all(src).unwrap();
        let card = load_card(&forms).unwrap();
        let schema = collect_card_schema(&card).unwrap();
        let provenance = ProvenanceMap::from_source("fixture.maku", src, &forms);
        check_forms(&forms, &card, &schema, &provenance)
    }

    #[test]
    fn infers_literals_lexical_functions_arrays_records_and_branches() {
        let report = check("(defn id [x] x) (def data [(id 1) (if 1 2 3)]) (def row {:n 1 :tag :ok})");
        assert_eq!(report.violations().count(), 0, "{:?}", report.diagnostics);
        assert!(report.nodes.iter().any(|node| matches!(node.inferred, Type::Fn { .. })));
        assert!(report.nodes.iter().any(|node| matches!(node.inferred, Type::Record(_))));
    }

    #[test]
    fn lexical_shadowing_wins_over_builtin_signature() {
        let report = check("(defn use [+] (+ :symbol))");
        assert_eq!(report.violations().count(), 0, "{:?}", report.diagnostics);
        assert!(report.unchecked().any(|diagnostic| diagnostic.message.contains("dynamic call")));
    }

    #[test]
    fn reports_function_argument_and_action_position_mismatches() {
        let report = check("(defpattern bad [] (seq (wait :later) 3))");
        assert!(report.violations().any(|diagnostic| diagnostic.expected == Some(Type::Num) && diagnostic.found == Some(Type::Symbol)));
        assert!(report.violations().any(|diagnostic| diagnostic.category == DiagnosticCategory::ActionPosition));
    }

    #[test]
    fn runtime_string_atoms_satisfy_symbol_boundaries_in_enforced_mode() {
        let src = r#"
            (def collision-set (collisions "shot" "enemy"))
            (def collider-spec (circle-collider {:layer "shot" :r 1}))
            (defpattern ok [] (wait 0))
        "#;
        let (_sim, report) = crate::sim::Sim::load_with_check_mode(
            src,
            Some("ok"),
            CheckMode::Enforced,
        )
        .unwrap();
        assert_eq!(report.violations().count(), 0, "{:?}", report.diagnostics);
    }

    #[test]
    fn fork_requires_exactly_one_action() {
        let report = check(
            "(defpattern bad [] (seq (fork) (fork (wait 1) (wait 2)) (fork (wait 3))))",
        );
        let fork_arities = report
            .violations()
            .filter(|diagnostic| {
                diagnostic.category == DiagnosticCategory::ArityMismatch
                    && diagnostic.context.name.as_ref() == "fork"
            })
            .collect::<Vec<_>>();
        assert!(fork_arities.iter().any(|diagnostic| diagnostic.message.contains("found 0")));
        assert!(fork_arities.iter().any(|diagnostic| diagnostic.message.contains("found 2")));
        assert!(!fork_arities.iter().any(|diagnostic| diagnostic.message.contains("found 1")));
    }

    #[test]
    fn canonical_pose_then_dyn_coercion_is_recorded() {
        let report = check("(defpattern ok [] (spawn (cart 1 2)))");
        assert_eq!(report.violations().count(), 0, "{:?}", report.diagnostics);
        let node = report.nodes.iter().find(|node| node.coercions.contains(&CoercionKind::ConstantToDyn)).unwrap();
        assert_eq!(node.coercions, [CoercionKind::PoseToFigure, CoercionKind::ConstantToDyn]);
    }

    #[test]
    fn homogeneous_collection_mismatch_names_element_and_coercion() {
        let report = check("(def bad (without 0 [1 :x]))");
        let diagnostic = report.violations().next().unwrap();
        assert_eq!(diagnostic.category, DiagnosticCategory::FailedCoercion);
        assert!(matches!(diagnostic.coercion_failure.as_ref().unwrap().path.as_slice(), [CoercionPathSegment::Element(1)]));
    }

    #[test]
    fn declared_render_schema_checks_fields_but_open_kind_stays_unchecked() {
        let report = check("(defrender-kind :bullet {:geometry :point :fields {:glow :num}}) (deftick (emit :render {:kind :bullet :glow :hot})) (deftick (emit :render {:kind :extension :whatever :ok}))");
        assert!(report.violations().any(|diagnostic| diagnostic.category == DiagnosticCategory::RenderField && diagnostic.expected == Some(Type::Num)));
        assert!(report.unchecked().any(|diagnostic| diagnostic.category == DiagnosticCategory::UnknownRenderKind));
    }

    #[test]
    fn query_predicate_requires_numeric_mask() {
        let report = check("(defpattern bad [] (wait (count-entities (entities-where (fn [e] :yes)))))");
        assert!(report.violations().any(|diagnostic| diagnostic.category == DiagnosticCategory::QueryPredicate || diagnostic.context.name.as_ref().contains("entities-where")));
    }

    #[test]
    fn diagnostics_use_source_types_only() {
        let report = check("(defpattern bad [] (wait :later))");
        let rendered = report.violations().next().unwrap().to_string();
        assert!(rendered.contains("Num"));
        for forbidden in ["Kernel", "register", "F32", "U32", "Rust"] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }

    #[test]
    fn type_correct_unrecognized_form_is_unchecked_not_invalid() {
        let forms = vec![read_one("(mystery 1)").unwrap()];
        let card = load_card(&forms).unwrap();
        let schema = collect_card_schema(&card).unwrap();
        let provenance = ProvenanceMap::synthetic("fixture.maku", &forms);
        let report = check_forms(&forms, &card, &schema, &provenance);
        assert_eq!(report.violations().count(), 0);
        assert!(report.unchecked().count() > 0);
        assert!(report.enforce(CheckMode::Enforced).is_ok());
    }

    #[test]
    fn macro_generated_mismatch_points_to_authored_call_and_definition() {
        let src = "(defmacro bad-num [] `(wait :bad))\n(defpattern bad [] (bad-num))";
        let report = check(src);
        let diagnostic = report
            .violations()
            .find(|diagnostic| diagnostic.found == Some(Type::Symbol))
            .unwrap();
        assert_eq!(diagnostic.primary_span.source.as_ref(), "fixture.maku");
        assert_eq!(
            diagnostic
                .expansion_stack
                .iter()
                .map(|frame| frame.name.as_ref())
                .collect::<Vec<_>>(),
            ["bad-num"]
        );
        assert_eq!(diagnostic.related_spans.len(), 1);
    }

    #[test]
    fn resolved_nested_macro_expansion_records_each_actual_step() {
        let src = "
            (defmacro inner [] `(wait :bad))
            (defmacro outer [] `(inner))
            (defpattern bad [] (outer))
        ";
        let report = check(src);
        let diagnostic = report
            .violations()
            .find(|diagnostic| diagnostic.found == Some(Type::Symbol))
            .unwrap();
        assert_eq!(
            diagnostic
                .expansion_stack
                .iter()
                .map(|frame| frame.name.as_ref())
                .collect::<Vec<_>>(),
            ["outer", "inner"]
        );
        assert_eq!(diagnostic.related_spans.len(), 2);
    }

    #[test]
    fn shadowed_macro_names_do_not_create_expansion_frames() {
        let src = "
            (defmacro local-macro [] `(wait :bad))
            (defmacro def-macro [] `(wait :bad))
            (defn def-macro [] (wait 1))
            (defpattern ok []
              (seq
                (let [local-macro (fn [] (wait 1))] (local-macro))
                (def-macro)))
        ";
        let report = check(src);
        assert_eq!(report.violations().count(), 0, "{:?}", report.diagnostics);
        assert!(report.nodes.iter().all(|node| {
            node.provenance.expansion_stack.iter().all(|frame| {
                !matches!(frame.name.as_ref(), "local-macro" | "def-macro")
            })
        }));
    }

    #[test]
    fn diagnostic_mode_reports_while_enforced_mode_rejects_only_proven_errors() {
        let src = "(defpattern bad [] (wait :later))";
        let (_sim, report) = crate::sim::Sim::load_with_check_mode(
            src,
            Some("bad"),
            CheckMode::Diagnostic,
        )
        .unwrap();
        assert!(report.violations().count() > 0);
        let error = match crate::sim::Sim::load_with_check_mode(
            src,
            Some("bad"),
            CheckMode::Enforced,
        ) {
            Ok(_) => panic!("proven mismatch unexpectedly loaded"),
            Err(error) => error,
        };
        assert!(error.contains("expected Num, found Symbol"), "{error}");
    }

    #[test]
    fn semantically_valid_non_numeric_builtin_runs_interpreted() {
        let src = "(defpattern ok [] (wait (count [1 2])))";
        let mut sim = crate::sim::Sim::load(src, Some("ok")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.tick(), 1);
    }

    #[test]
    #[ignore = "explicit full card corpus checker sweep"]
    fn card_corpus_passes_diagnostic_and_enforced_modes() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(&path, out);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("maku") {
                    out.push(path);
                }
            }
        }

        let mut paths = Vec::new();
        collect(std::path::Path::new("../../cards"), &mut paths);
        paths.sort();
        assert!(!paths.is_empty());
        for path in paths {
            let expanded = crate::edn::expand_card_traced(&path).unwrap();
            let forms = read_all(&expanded.text).unwrap();
            let card = load_card(&forms).unwrap();
            let schema = collect_card_schema(&card).unwrap();
            let provenance = ProvenanceMap::from_expanded(&expanded, &forms);
            let report = check_forms(&forms, &card, &schema, &provenance);
            report.enforce(CheckMode::Diagnostic).unwrap();
            report
                .enforce(CheckMode::Enforced)
                .unwrap_or_else(|error| panic!("{}: {}", path.display(), error));
        }
    }
}
