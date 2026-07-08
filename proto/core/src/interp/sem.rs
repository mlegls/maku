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

/// Collider projector algebra currently supported by the interpreter.
/// Stable constructor slots are the fast path; deferred/composed projectors
/// are evaluated against `(e, ctx)` during collider materialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FigureProjectorKind {
    Pose,
}

impl FigureProjectorKind {
    pub(crate) fn from_defcollider_keyword(name: &str) -> Result<FigureProjectorKind, String> {
        match name {
            "pose" => Ok(FigureProjectorKind::Pose),
            other => Err(format!(
                "defcollider: unsupported figure type :{} (only :pose is implemented)",
                other
            )),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            FigureProjectorKind::Pose => "pose",
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
}

#[derive(Clone, Debug)]
pub enum ColliderProjectorExpr {
    Stable(Rc<[DynCollider]>),
    Circle(CircleProjectorSpec),
    CapsuleChain { opts: Option<Form>, env: Env },
    Callable { params: Rc<[Rc<str>]>, body: Rc<[Form]>, env: Env },
    Cond { clauses: Rc<[(Option<Form>, ColliderProjectorValue)]>, env: Env },
    ColliderSum(Rc<[ColliderProjectorValue]>),
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
        ColliderProjectorValue {
            figure: FigureProjectorKind::Pose,
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

    pub(crate) fn capsule_chain(opts: Option<Form>, env: Env) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure: FigureProjectorKind::Pose,
            expr: ColliderProjectorExpr::CapsuleChain { opts, env },
        }
    }

    pub(crate) fn cond(
        figure: FigureProjectorKind,
        clauses: Vec<(Option<Form>, ColliderProjectorValue)>,
        env: Env,
    ) -> ColliderProjectorValue {
        ColliderProjectorValue {
            figure,
            expr: ColliderProjectorExpr::Cond { clauses: clauses.into(), env },
        }
    }

    pub(crate) fn empty() -> ColliderProjectorValue {
        ColliderProjectorValue::stable(Vec::new())
    }

    pub(crate) fn compose(projectors: Vec<ColliderProjectorValue>) -> Result<ColliderProjectorValue, String> {
        let mut flat: Vec<ColliderProjectorValue> = Vec::new();
        let figure = projectors
            .first()
            .map(|p| p.figure)
            .unwrap_or(FigureProjectorKind::Pose);
        for projector in projectors {
            if projector.figure != figure {
                return Err(format!(
                    "collider projector kind mismatch: :{} vs :{}",
                    figure.name(),
                    projector.figure.name()
                ));
            }
            match &projector.expr {
                ColliderProjectorExpr::ColliderSum(items) => {
                    flat.extend(items.iter().cloned());
                }
                _ => flat.push(projector),
            }
        }
        if flat.iter().all(|p| matches!(p.expr, ColliderProjectorExpr::Stable(_))) {
            let mut slots = Vec::new();
            for projector in flat {
                let ColliderProjectorExpr::Stable(items) = projector.expr else { unreachable!() };
                slots.extend(items.iter().cloned());
            }
            Ok(ColliderProjectorValue::stable(slots))
        } else {
            Ok(ColliderProjectorValue {
                figure,
                expr: ColliderProjectorExpr::ColliderSum(flat.into()),
            })
        }
    }
}

/// A source-level renderer projector expression after dyn-lifting and schema
/// checking for a render slot. Dynamic lists are rechecked after per-tick
/// realization until the typed layer can represent their element schema.
#[derive(Clone, Debug)]
pub struct RendererProjectorSpec {
    pub(crate) expr: DynLike,
}

impl RendererProjectorSpec {
    pub(crate) fn checked(expr: DynLike) -> RendererProjectorSpec {
        RendererProjectorSpec { expr }
    }

    pub(crate) fn empty() -> RendererProjectorSpec {
        RendererProjectorSpec::checked(DynLike::List(Vec::new().into()))
    }

    pub(crate) fn eval(
        &self,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
    ) -> Result<Val, String> {
        self.expr.eval(tau, state, sig)
    }
}

/// Compatibility alias for the current `(renderers ...)` bridge surface.
pub type RenderSpecList = RendererProjectorSpec;

/// Semantic collider projector slot carried by a spawned entity. It is still a
/// bridge representation: the interpreter lowers specs against the current
/// figure into realized collider rows during simulation.
#[derive(Clone, Debug)]
pub struct ColliderProjector {
    pub projectors: Rc<[ColliderProjectorValue]>,
}

/// Semantic render projector slot carried by a spawned entity. The style/sigs
/// fields are compatibility host-renderer data and should move behind renderer
/// specs as that boundary becomes explicit.
#[derive(Clone, Debug)]
pub struct RenderProjector {
    pub specs: Rc<[RendererProjectorSpec]>,
    pub style: Style,
    pub sigs: RenderSigs,
}

/// Literal meta forms are kept alongside computed pairs so source-only syntax
/// such as :expose channel designators and signal tags can be handled at the
/// semantic spawn boundary.
pub(crate) struct SpawnMetaInput {
    pub forms: Vec<Form>,
    pub computed_pairs: Vec<(Val, Val)>,
}

/// Source-level spawn directives that are not entity meta values. Today this
/// keeps literal meta maps because legacy signal tags and :expose are still
/// recognized from raw forms; each directive should move here explicitly as
/// the typed boundary is tightened.
pub(crate) struct SpawnDirectives {
    pub raw_meta_forms: Vec<Form>,
}

/// Spawn meta after merging evaluated map values, plus source directives kept
/// out of the eventual `Dyn<Meta>` value.
pub(crate) struct SpawnMetaPlan {
    pub value: Val,
    pub directives: SpawnDirectives,
}

/// Compositional expected types for the low-level spawn API. The current
/// interpreter still stores bridge values in `SpawnSlots`, but these are the
/// semantic targets the future elaborator should use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SpawnSlotTypes {
    pub figure: Type,
    pub colliders: Type,
    pub renderers: Type,
    pub meta: Type,
}

impl SpawnSlotTypes {
    pub(crate) fn low_level() -> SpawnSlotTypes {
        SpawnSlotTypes {
            figure: Type::spawn_figure(),
            colliders: Type::spawn_colliders(),
            renderers: Type::spawn_renderers(),
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
    pub renderers: Vec<RendererProjectorSpec>,
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
    pub renderers: Vec<RendererProjectorSpec>,
}
