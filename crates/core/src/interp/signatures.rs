//! Authoritative source signatures for interpreter-dispatched vocabulary.
//!
//! Runtime dispatch continues to live in the evaluator. Dispatch membership is
//! derived from this registry so the checker cannot silently acquire a second,
//! drifting list of builtins or engine forms.

use super::types::{DynClass, FigureKind, ProjectorKind, RenderKind, Type, TypeVar};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arity {
    Exact(usize),
    Range { min: usize, max: usize },
    Variadic { min: usize },
}

impl Arity {
    pub fn accepts(self, count: usize) -> bool {
        match self {
            Arity::Exact(expected) => count == expected,
            Arity::Range { min, max } => (min..=max).contains(&count),
            Arity::Variadic { min } => count >= min,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureKind {
    Pure,
    Special,
    Action,
    Projector,
    Schema,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub name: &'static str,
    pub arity: Arity,
    pub params: Vec<Type>,
    pub result: Type,
    pub kind: SignatureKind,
}

impl Signature {
    pub fn parameter(&self, index: usize) -> Option<&Type> {
        self.params.get(index).or_else(|| match self.arity {
            Arity::Variadic { .. } => self.params.last(),
            _ => None,
        })
    }
}

const BUILTIN_NAMES: &[&str] = &[
    "+", "-", "*", "/", "mod", "pow", "=", "<", ">", "<=", ">=", "min", "max",
    "abs", "floor", "ceil", "round", "sqrt", "quot", "ticks", "not", "sin", "cos",
    "sine", "lssht", "lerp", "lerp3", "lerpsmooth", "einsine", "eoutsine", "eiosine",
    "iota", "range", "nth", "without", "stutter", "count", "first", "rest", "drop",
    "take", "concat", "cart", "polar", "pose", "rot", "still", "linear", "angle-of",
    "mag", "forms", "get", "form-type", "form-name", "nothing?", "num?", "seq?",
];

const ENGINE_SPECIAL_NAMES: &[&str] = &[
    "matches", "manip", "remat", "change-col", "cull", "pos", "on-curve",
    "count-entities", "sum-entities", "entities-where", "collisions", "curve-samples",
    "emit", "entity-col", "nearest-entity",
];

pub fn builtin_names() -> &'static [&'static str] {
    BUILTIN_NAMES
}

pub fn engine_special_names() -> &'static [&'static str] {
    ENGINE_SPECIAL_NAMES
}

pub fn builtin_signature(name: &str) -> Option<Signature> {
    let num1 = || signature(name, Arity::Exact(1), vec![Type::Num], Type::Num, SignatureKind::Pure);
    let num2 = || signature(name, Arity::Exact(2), vec![Type::Num, Type::Num], Type::Num, SignatureKind::Pure);
    let num = match name {
        "+" | "-" | "*" | "/" => signature(
            name,
            Arity::Variadic { min: 0 },
            vec![Type::Unknown],
            Type::Unknown,
            SignatureKind::Pure,
        ),
        "mod" | "pow" | "<" | ">" | "<=" | ">=" | "min" | "max" | "quot" => num2(),
        "abs" | "floor" | "ceil" | "round" | "sqrt" | "ticks" | "not" | "sin" | "cos"
        | "einsine" | "eoutsine" | "eiosine" => num1(),
        "=" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Exact(2), vec![value.clone(), value], Type::Num, SignatureKind::Pure)
        }
        "sine" => signature(name, Arity::Exact(3), vec![Type::Num; 3], Type::Num, SignatureKind::Pure),
        "lssht" => signature(name, Arity::Exact(4), vec![Type::Num; 4], Type::Num, SignatureKind::Pure),
        "lerp" => signature(name, Arity::Exact(5), vec![Type::Num; 5], Type::Num, SignatureKind::Pure),
        "lerp3" => signature(name, Arity::Exact(8), vec![Type::Num; 8], Type::Num, SignatureKind::Pure),
        "lerpsmooth" => signature(
            name,
            Arity::Exact(6),
            vec![Type::function(vec![Type::Num], Type::Num), Type::Num, Type::Num, Type::Num, Type::Num, Type::Num],
            Type::Num,
            SignatureKind::Pure,
        ),
        "iota" => signature(name, Arity::Exact(1), vec![Type::Num], Type::Array(Box::new(Type::Num)), SignatureKind::Pure),
        "range" => signature(name, Arity::Range { min: 2, max: 3 }, vec![Type::Num; 3], Type::Array(Box::new(Type::Num)), SignatureKind::Pure),
        "nth" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Exact(2), vec![Type::Array(Box::new(value.clone())), Type::Unknown], value, SignatureKind::Pure)
        }
        "without" => signature(name, Arity::Exact(2), vec![Type::Num, Type::Array(Box::new(Type::Num))], Type::Array(Box::new(Type::Num)), SignatureKind::Pure),
        "stutter" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Exact(2), vec![Type::Num, Type::Array(Box::new(value.clone()))], Type::Array(Box::new(value)), SignatureKind::Pure)
        }
        "count" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::Num, SignatureKind::Pure),
        "first" => signature(
            name,
            Arity::Exact(1),
            vec![Type::Unknown],
            Type::Unknown,
            SignatureKind::Pure,
        ),
        "rest" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Exact(1), vec![Type::Array(Box::new(value.clone()))], Type::Array(Box::new(value)), SignatureKind::Pure)
        }
        "drop" | "take" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Exact(2), vec![Type::Num, Type::Array(Box::new(value.clone()))], Type::Array(Box::new(value)), SignatureKind::Pure)
        }
        "concat" => {
            let value = Type::Var(TypeVar(0));
            signature(name, Arity::Variadic { min: 0 }, vec![Type::Array(Box::new(value.clone()))], Type::Array(Box::new(value)), SignatureKind::Pure)
        }
        "cart" | "polar" => signature(name, Arity::Exact(2), vec![Type::Num, Type::Num], Type::Pose, SignatureKind::Pure),
        "pose" => signature(name, Arity::Range { min: 1, max: 2 }, vec![Type::Pose, Type::Meta], Type::Pose, SignatureKind::Pure),
        "rot" => signature(name, Arity::Exact(1), vec![Type::Num], Type::Pose, SignatureKind::Pure),
        "still" => signature(name, Arity::Exact(0), vec![], Type::Pose, SignatureKind::Pure),
        "linear" => signature(name, Arity::Range { min: 1, max: 2 }, vec![Type::Pose, Type::Meta], Type::Dyn { class: Some(DynClass::Closed), value: Box::new(Type::Pose) }, SignatureKind::Pure),
        "angle-of" | "mag" => signature(name, Arity::Exact(1), vec![Type::Pose], Type::Num, SignatureKind::Pure),
        "forms" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::Array(Box::new(Type::Unknown)), SignatureKind::Pure),
        "get" => signature(name, Arity::Range { min: 2, max: 3 }, vec![Type::Unknown; 3], Type::Unknown, SignatureKind::Pure),
        "form-type" | "form-name" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::Symbol, SignatureKind::Pure),
        "nothing?" | "num?" | "seq?" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::Num, SignatureKind::Pure),
        _ => return None,
    };
    Some(num)
}

pub fn engine_signature(name: &str) -> Option<Signature> {
    let signature = match name {
        "matches" => signature(name, Arity::Variadic { min: 1 }, vec![Type::Unknown], Type::Unknown, SignatureKind::Special),
        "manip" => signature(name, Arity::Exact(2), vec![Type::Unknown, Type::function(vec![Type::Handle], Type::Action)], Type::Action, SignatureKind::Action),
        "remat" => signature(name, Arity::Exact(2), vec![Type::Handle, Type::Unknown], Type::Action, SignatureKind::Action),
        "change-col" => {
            let value = Type::Var(TypeVar(0));
            signature(
                name,
                Arity::Exact(3),
                vec![
                    Type::Handle,
                    Type::Symbol,
                    Type::function(vec![value.clone()], value),
                ],
                Type::Action,
                SignatureKind::Action,
            )
        }
        "cull" => signature(name, Arity::Range { min: 0, max: 1 }, vec![Type::Handle], Type::Action, SignatureKind::Action),
        "pos" => signature(name, Arity::Exact(1), vec![Type::Handle], Type::Pose, SignatureKind::Special),
        "on-curve" => signature(name, Arity::Exact(2), vec![Type::Handle, Type::Num], Type::Pose, SignatureKind::Special),
        "count-entities" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::Num, SignatureKind::Special),
        "sum-entities" => signature(name, Arity::Exact(2), vec![Type::Unknown, Type::Symbol], Type::Num, SignatureKind::Special),
        "entities-where" => signature(name, Arity::Exact(1), vec![Type::Unknown], Type::EntitySet, SignatureKind::Special),
        "collisions" => signature(name, Arity::Exact(2), vec![Type::Symbol, Type::Symbol], Type::EntitySet, SignatureKind::Special),
        "curve-samples" => signature(name, Arity::Range { min: 1, max: 2 }, vec![Type::Handle, Type::Unknown], Type::Unknown, SignatureKind::Special),
        "emit" => signature(name, Arity::Exact(2), vec![Type::Symbol, Type::Unknown], Type::Action, SignatureKind::Action),
        "entity-col" => signature(name, Arity::Exact(2), vec![Type::Handle, Type::Symbol], Type::Num, SignatureKind::Special),
        "nearest-entity" => signature(name, Arity::Exact(2), vec![Type::Unknown, Type::Pose], Type::Option(Box::new(Type::Pose)), SignatureKind::Special),
        _ => return None,
    };
    Some(signature)
}

pub fn projector_signature(name: &str) -> Option<Signature> {
    match name {
        "circle-collider" | "capsule-chain-collider" => Some(signature(
            name,
            Arity::Exact(1),
            vec![Type::open_record(std::iter::empty::<(String, Type)>())],
            Type::Projector {
                kind: ProjectorKind::Collider,
                figure: if name == "circle-collider" { FigureKind::Pose } else { FigureKind::Curve },
                render: None,
            },
            SignatureKind::Projector,
        )),
        _ => None,
    }
}

pub fn projector_field_type(constructor: &str, field: &str) -> Option<Type> {
    match (constructor, field) {
        ("circle-collider", "layer") | ("capsule-chain-collider", "layer") => {
            Some(Type::Symbol)
        }
        ("circle-collider", "r" | "radius")
        | ("capsule-chain-collider", "r" | "radius" | "u-max" | "resolution" | "width") => {
            Some(Type::Num)
        }
        _ => None,
    }
}

pub fn action_slot_signature(name: &str) -> Option<Signature> {
    let signature = match name {
        "wait" => signature(name, Arity::Exact(1), vec![Type::Num], Type::Action, SignatureKind::Action),
        "seq" => signature(name, Arity::Variadic { min: 0 }, vec![Type::Action], Type::Action, SignatureKind::Action),
        "fork" => signature(name, Arity::Exact(1), vec![Type::Action], Type::Action, SignatureKind::Action),
        "spawn" => signature(
            name,
            Arity::Variadic { min: 1 },
            vec![Type::dyn_of(Type::Figure), Type::Unknown],
            Type::Action,
            SignatureKind::Action,
        ),
        "render" => signature(
            name,
            Arity::Exact(1),
            vec![Type::RenderData(RenderKind::Any)],
            Type::Action,
            SignatureKind::Schema,
        ),
        _ => return None,
    };
    Some(signature)
}

fn signature(
    name: &str,
    arity: Arity,
    params: Vec<Type>,
    result: Type,
    kind: SignatureKind,
) -> Signature {
    let name = BUILTIN_NAMES
        .iter()
        .chain(ENGINE_SPECIAL_NAMES)
        .copied()
        .chain(["circle-collider", "capsule-chain-collider", "wait", "seq", "fork", "spawn", "render"])
        .find(|known| *known == name)
        .expect("signature name must have static registry storage");
    Signature { name, arity, params, result, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dispatch_name_has_one_signature() {
        for name in builtin_names() {
            assert_eq!(builtin_signature(name).as_ref().map(|signature| signature.name), Some(*name));
        }
        for name in engine_special_names() {
            assert_eq!(engine_signature(name).as_ref().map(|signature| signature.name), Some(*name));
        }
    }
}
