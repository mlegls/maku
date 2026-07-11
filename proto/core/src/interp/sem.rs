//! Interpreter semantic slot types.
//!
//! This is the narrow boundary between source forms/ordinary evaluated values
//! and backend execution. It is intentionally small for now: spawn is the
//! first caller that needs slot-aware typing/coercion before lowering.

use super::*;
use crate::edn::Form;
use crate::interp::types::Type;
use std::rc::Rc;

/// Projector-local view of entity meta. Today this is implicit in the
/// interpreter's field lookup path; the explicit type names the target boundary
/// where projector adapters can rebind names or select submaps.
#[derive(Clone, Debug, Default)]
pub struct MetaEnv;

/// Per-tick context supplied to collider/render projectors. The current
/// interpreter passes age/tick separately; this type names the target argument
/// for higher-order projector combinators.
#[derive(Clone, Debug)]
pub struct EntityContext {
    pub age: f64,
    pub tick: u64,
    pub entity: Option<EntityRef>,
}

#[derive(Clone, Debug)]
pub struct ProjectorScope {
    pub entity: Rc<str>,
    pub context: Rc<str>,
    pub figure: FigureProjectorKind,
}

/// Collider projector algebra currently supported by the interpreter.
/// Stable constructor slots are the fast path; deferred/composed projectors
/// are evaluated against `(e, ctx)` during collider materialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FigureProjectorKind {
    Pose,
    Parametric,
}

impl FigureProjectorKind {
    pub(crate) fn from_projector_keyword(
        surface: &str,
        name: &str,
    ) -> Result<FigureProjectorKind, String> {
        match name {
            "pose" => Ok(FigureProjectorKind::Pose),
            "parametric" => Ok(FigureProjectorKind::Parametric),
            other => Err(format!(
                "{}: unsupported figure type :{}",
                surface,
                other
            )),
        }
    }

    pub(crate) fn from_defcollider_keyword(name: &str) -> Result<FigureProjectorKind, String> {
        FigureProjectorKind::from_projector_keyword("defcollider", name)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            FigureProjectorKind::Pose => "pose",
            FigureProjectorKind::Parametric => "parametric",
        }
    }
}

/// Concrete numeric expression for a primitive projector override. This is not
/// a dyn slot: any expression is evaluated in the projector's already-bound
/// `e`/`ctx` environment and must produce a number for the current tick.
#[derive(Clone, Debug)]
pub enum ProjectorNum {
    Const(f64),
    Expr(Form),
}

#[derive(Clone, Debug)]
pub struct CircleProjectorSpec {
    pub layer: Symbol,
    pub radius: ProjectorNum,
    pub env: Env,
    pub scope: Option<ProjectorScope>,
}

#[derive(Clone, Debug)]
pub enum ProjectorSampleSet {
    Values(Rc<[f64]>),
    Step(ProjectorNum),
}

#[derive(Clone, Debug)]
pub struct CapsuleChainProjectorSpec {
    pub layer: Symbol,
    pub radius: ProjectorNum,
    pub sample_set: ProjectorSampleSet,
    pub u_max: Option<ProjectorNum>,
    pub width: ProjectorNum,
    pub env: Env,
    pub scope: Option<ProjectorScope>,
}

#[derive(Clone, Debug)]
pub enum ColliderProjectorExpr {
    Stable(Rc<[DynCollider]>),
    Circle(CircleProjectorSpec),
    CapsuleChain(CapsuleChainProjectorSpec),
    Callable { params: Rc<[Rc<str>]>, body: Rc<[Form]>, env: Env },
    Cond { clauses: Rc<[(Option<Form>, Rc<[ColliderProjectorValue]>)]>, env: Env, scope: Option<ProjectorScope> },
}

impl ColliderProjectorExpr {
    pub(crate) fn needs_views(&self) -> bool {
        match self {
            ColliderProjectorExpr::Stable(_) => false,
            ColliderProjectorExpr::Circle(spec) => spec.scope.is_some(),
            ColliderProjectorExpr::CapsuleChain(spec) => spec.scope.is_some(),
            ColliderProjectorExpr::Callable { .. } => true,
            ColliderProjectorExpr::Cond { clauses, scope, .. } => {
                scope.is_some()
                    || clauses.iter().any(|(_, children)| {
                        children.iter().any(|child| child.expr.needs_views())
                    })
            }
        }
    }
}

/// A source-level collider projector expression after dyn-lifting and schema
/// checking for a collider slot.
#[derive(Clone, Debug)]
pub struct ColliderProjectorValue {
    pub(crate) figure: FigureProjectorKind,
    pub(crate) expr: ColliderProjectorExpr,
}

impl ColliderProjectorValue {
    pub(crate) fn stable(slots: Vec<DynCollider>) -> ColliderProjectorValue {
        ColliderProjectorValue::stable_for(FigureProjectorKind::Pose, slots)
    }

    pub(crate) fn stable_for(
        figure: FigureProjectorKind,
        slots: Vec<DynCollider>,
    ) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure,
            expr: ColliderProjectorExpr::Stable(slots.into()),
        }
    }

    pub(crate) fn callable(
        figure: FigureProjectorKind,
        params: Vec<Rc<str>>,
        body: Rc<[Form]>,
        env: Env,
    ) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure,
            expr: ColliderProjectorExpr::Callable { params: params.into(), body, env },
        }
    }

    pub(crate) fn circle(spec: CircleProjectorSpec) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure: FigureProjectorKind::Pose,
            expr: ColliderProjectorExpr::Circle(spec),
        }
    }

    pub(crate) fn capsule_chain(
        figure: FigureProjectorKind,
        spec: CapsuleChainProjectorSpec,
    ) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure,
            expr: ColliderProjectorExpr::CapsuleChain(spec),
        }
    }

    pub(crate) fn cond(
        figure: FigureProjectorKind,
        clauses: Vec<(Option<Form>, Rc<[ColliderProjectorValue]>)>,
        env: Env,
        scope: Option<ProjectorScope>,
    ) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure,
            expr: ColliderProjectorExpr::Cond { clauses: clauses.into(), env, scope },
        }
    }

    pub(crate) fn empty() -> ColliderProjectorValue {
        ColliderProjectorValue::stable(Vec::new())
    }
}

/// Semantic collider projector slot carried by a spawned entity. It is still a
/// bridge representation: the interpreter lowers specs against the current
/// figure into realized collider rows during simulation.
#[derive(Clone, Debug)]
pub struct ColliderProjector {
    pub projectors: Rc<[ColliderProjectorValue]>,
}

impl ColliderProjector {
    pub(crate) fn needs_views(&self) -> bool {
        self.projectors.iter().any(|value| value.expr.needs_views())
    }
}

#[cfg(test)]
mod collider_projector_tests {
    use super::*;

    #[test]
    fn needs_views_classifies_projector_algebra() {
        let stable = ColliderProjectorValue::stable(Vec::new());
        assert!(!stable.expr.needs_views());

        let scoped_circle = ColliderProjectorValue::circle(CircleProjectorSpec {
            layer: Symbol(0),
            radius: ProjectorNum::Const(1.0),
            env: Env::empty(),
            scope: Some(ProjectorScope {
                entity: "e".into(),
                context: "ctx".into(),
                figure: FigureProjectorKind::Pose,
            }),
        });
        assert!(scoped_circle.expr.needs_views());

        let callable = ColliderProjectorValue::callable(
            FigureProjectorKind::Pose,
            vec!["e".into(), "ctx".into()],
            Vec::<Form>::new().into(),
            Env::empty(),
        );
        assert!(callable.expr.needs_views());

        let cond = ColliderProjectorValue::cond(
            FigureProjectorKind::Pose,
            vec![(None, vec![stable].into())],
            Env::empty(),
            None,
        );
        assert!(!cond.expr.needs_views());
    }
}

/// Literal meta forms are lifted through DynLike before merging so static keys
/// may carry dyn-valued field initializers.
pub(crate) struct SpawnMetaInput {
    pub forms: Vec<Form>,
    pub computed_pairs: Vec<(Val, Val)>,
}

/// Spawn meta after merging evaluated map values.
pub(crate) struct SpawnMetaPlan {
    pub value: Val,
}

/// Compositional expected types for the low-level spawn API. The current
/// interpreter still stores bridge values in `SpawnSlots`, but these are the
/// semantic targets the future elaborator should use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SpawnSlotTypes {
    pub figure: Type,
    pub colliders: Type,
    pub meta: Type,
}

impl SpawnSlotTypes {
    pub(crate) fn low_level() -> SpawnSlotTypes {
        SpawnSlotTypes {
            figure: Type::spawn_figure(),
            colliders: Type::spawn_colliders(),
            meta: Type::spawn_meta(),
        }
    }
}

/// Slot-aware spawn inputs after ordinary argument evaluation, before entity
/// flattening and backend lowering.
pub(crate) struct SpawnSlots {
    pub targets: SpawnSlotTypes,
    pub figure: Val,
    pub colliders: Vec<ColliderProjectorValue>,
    pub meta: SpawnMetaInput,
}

/// Lowered spawn plan after slot normalization, meta/directive preservation,
/// figure flattening, and per-element rand instantiation. This is still
/// interpreter-facing, but it separates semantic slot planning from final
/// `EntitySpec` construction.
pub(crate) struct SpawnPlan {
    pub elems: Vec<SpawnElem>,
    pub meta: SpawnMetaPlan,
    pub colliders: Vec<ColliderProjectorValue>,
}
