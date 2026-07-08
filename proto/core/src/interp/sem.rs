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
#[derive(Clone, Debug)]
pub enum ColliderProjectorExpr {
    Stable(Rc<[DynCollider]>),
    DeferredBody { body: Rc<[Form]>, env: Env },
    ColliderSum(Rc<[ColliderProjectorSpec]>),
    ColliderActiveCond { clauses: Rc<[(Option<Form>, ColliderProjectorSpec)]>, env: Env },
}

/// A source-level collider projector expression after dyn-lifting and schema
/// checking for a collider slot.
#[derive(Clone, Debug)]
pub struct ColliderProjectorSpec {
    pub(crate) expr: ColliderProjectorExpr,
}

impl ColliderProjectorSpec {
    pub(crate) fn stable(slots: Vec<DynCollider>) -> ColliderProjectorSpec {
        ColliderProjectorSpec {
            expr: ColliderProjectorExpr::Stable(slots.into()),
        }
    }

    pub(crate) fn deferred_body(body: Rc<[Form]>, env: Env) -> ColliderProjectorSpec {
        ColliderProjectorSpec {
            expr: ColliderProjectorExpr::DeferredBody { body, env },
        }
    }

    pub(crate) fn empty() -> ColliderProjectorSpec {
        ColliderProjectorSpec::stable(Vec::new())
    }

    pub(crate) fn active_cond(
        clauses: Vec<(Option<Form>, ColliderProjectorSpec)>,
        env: Env,
    ) -> ColliderProjectorSpec {
        ColliderProjectorSpec {
            expr: ColliderProjectorExpr::ColliderActiveCond { clauses: clauses.into(), env },
        }
    }

    pub(crate) fn plus(&self, rhs: &ColliderProjectorSpec) -> ColliderProjectorSpec {
        match (&self.expr, &rhs.expr) {
            (ColliderProjectorExpr::Stable(a), ColliderProjectorExpr::Stable(b)) => {
                let mut slots = Vec::with_capacity(a.len() + b.len());
                slots.extend(a.iter().cloned());
                slots.extend(b.iter().cloned());
                ColliderProjectorSpec::stable(slots)
            }
            _ => ColliderProjectorSpec {
                expr: ColliderProjectorExpr::ColliderSum(vec![self.clone(), rhs.clone()].into()),
            },
        }
    }
}

/// Compatibility alias for older internal names while spawn lowering migrates.
pub type ColliderSpecList = ColliderProjectorSpec;

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
    pub specs: Rc<[ColliderProjectorSpec]>,
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
    pub colliders: Vec<ColliderProjectorSpec>,
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
    pub colliders: Vec<ColliderProjectorSpec>,
    pub renderers: Vec<RendererProjectorSpec>,
}
