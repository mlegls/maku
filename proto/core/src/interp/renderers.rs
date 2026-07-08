//! Built-in renderer cases and compatibility style bridge.

use super::*;
use crate::edn::Form;

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub family: String,
    pub color: String,
    pub variant: String,
}

#[derive(Clone, Debug)]
pub enum RenderDynRepr {
    Polyline(CurveRenderSlot),
}

impl DynKind for RenderData {
    type Repr = RenderDynRepr;
}

pub type DynRender = Dyn<RenderData>;

/// Source-level collider/render metadata carried as generic dyn-like data.
/// Typed projection happens at the collision/render boundary after this data
/// is realized for the current tick.
pub type ColliderSpecList = DynLike;
pub type RenderSpecList = DynLike;

/// Bridge representation of a collider projector: source-level spec lists
/// that lower against the entity's current figure into realized collider rows.
#[derive(Clone, Debug)]
pub struct ColliderProjector {
    pub specs: Rc<[ColliderSpecList]>,
}

/// Bridge representation of a render projector: source-level spec lists
/// that lower against the entity's current figure into realized render rows.
#[derive(Clone, Debug)]
pub struct RenderProjector {
    pub specs: Rc<[RenderSpecList]>,
    /// Compatibility host style. This belongs to the current default renderer,
    /// not to entity semantics.
    pub style: Style,
    /// Compatibility render/collider modifier signals from legacy meta tags.
    pub sigs: RenderSigs,
}

/// A signal-valued meta tag sampled at render time (e.g. :hue).
#[derive(Debug, Clone)]
pub struct MetaSig {
    pub form: Form,
    pub env: Env,
    pub idx: usize, // element index for array-valued tag signals
}

/// The render-affecting signal tags (§7): each is an optional signal over
/// entity-local t, sampled at render time (scale also at collision time —
/// a scaled sprite scales its colliders). DMK's simple-bullet modifiers
/// (scale/dir/opacity), dissolved into meta tags like :hue.
#[derive(Debug, Clone, Default)]
pub struct RenderSigs {
    pub hue: Option<MetaSig>,
    /// Sprite + collider size multiplier (default 1).
    pub scale: Option<MetaSig>,
    /// Sprite rotation in degrees, overriding the motion direction.
    pub facing: Option<MetaSig>,
    /// Alpha multiplier (default 1).
    pub opacity: Option<MetaSig>,
}

impl Dyn<RenderData> {
    pub fn render_polyline(slot: CurveRenderSlot) -> DynRender {
        Dyn { repr: RenderDynRepr::Polyline(slot) }
    }

    pub fn repr(&self) -> &RenderDynRepr {
        &self.repr
    }

    pub fn polyline(&self) -> &CurveRenderSlot {
        match &self.repr {
            RenderDynRepr::Polyline(r) => r,
        }
    }
}
