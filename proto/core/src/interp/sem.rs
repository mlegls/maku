//! Interpreter semantic slot types.
//!
//! This is the narrow boundary between source forms/ordinary evaluated values
//! and backend execution. It is intentionally small for now: spawn is the
//! first caller that needs slot-aware typing/coercion before lowering.

use super::*;
use crate::edn::Form;
use crate::interp::types::Type;
use std::rc::Rc;

/// A source-level collider slot expression after dyn-lifting and schema
/// checking for a collider slot. Dynamic lists are rechecked after per-tick
/// realization until the typed layer can represent their element schema.
#[derive(Clone, Debug)]
pub struct ColliderSpecList {
    pub(crate) expr: DynLike,
}

impl ColliderSpecList {
    pub(crate) fn checked(expr: DynLike) -> ColliderSpecList {
        ColliderSpecList { expr }
    }

    pub(crate) fn empty() -> ColliderSpecList {
        ColliderSpecList::checked(DynLike::List(Vec::new().into()))
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

/// A source-level render slot expression after dyn-lifting and schema checking
/// for a render slot. Dynamic lists are rechecked after per-tick realization
/// until the typed layer can represent their element schema.
#[derive(Clone, Debug)]
pub struct RenderSpecList {
    pub(crate) expr: DynLike,
}

impl RenderSpecList {
    pub(crate) fn checked(expr: DynLike) -> RenderSpecList {
        RenderSpecList { expr }
    }

    pub(crate) fn empty() -> RenderSpecList {
        RenderSpecList::checked(DynLike::List(Vec::new().into()))
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

/// Semantic collider projector slot carried by a spawned entity. It is still a
/// bridge representation: the interpreter lowers specs against the current
/// figure into realized collider rows during simulation.
#[derive(Clone, Debug)]
pub struct ColliderProjector {
    pub specs: Rc<[ColliderSpecList]>,
}

/// Semantic render projector slot carried by a spawned entity. The style/sigs
/// fields are compatibility host-renderer data and should move behind renderer
/// specs as that boundary becomes explicit.
#[derive(Clone, Debug)]
pub struct RenderProjector {
    pub specs: Rc<[RenderSpecList]>,
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
    pub colliders: Vec<ColliderSpecList>,
    pub renderers: Vec<RenderSpecList>,
    pub meta: SpawnMetaInput,
}

/// Lowered spawn plan after slot normalization, meta/directive preservation,
/// figure flattening, and per-element rand instantiation. This is still
/// interpreter-facing, but it separates semantic slot planning from final
/// `EntitySpec` construction.
pub(crate) struct SpawnPlan {
    pub elems: Vec<SpawnElem>,
    pub meta: Val,
    pub meta_forms: Vec<Form>,
    pub colliders: Vec<ColliderSpecList>,
    pub renderers: Vec<RenderSpecList>,
}
