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
pub struct PointRenderSlot {
    /// Optional sprite rotation in degrees. Defaults to the figure pose angle.
    pub facing: Option<DynNum>,
    /// Sprite size multiplier.
    pub scale: DynNum,
    /// Hue offset / palette phase for the stock host renderer.
    pub hue: DynNum,
    /// Alpha multiplier.
    pub opacity: DynNum,
}

#[derive(Clone, Debug)]
pub enum RenderDynRepr {
    Point(PointRenderSlot),
    Polyline(CurveRenderSlot),
}

impl DynKind for RenderData {
    type Repr = RenderDynRepr;
}

pub type DynRender = Dyn<RenderData>;

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
    pub fn render_point(slot: PointRenderSlot) -> DynRender {
        Dyn { repr: RenderDynRepr::Point(slot) }
    }

    pub fn render_polyline(slot: CurveRenderSlot) -> DynRender {
        Dyn { repr: RenderDynRepr::Polyline(slot) }
    }

    pub fn repr(&self) -> &RenderDynRepr {
        &self.repr
    }
}
