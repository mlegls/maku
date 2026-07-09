//! Built-in renderer cases and compatibility style bridge.

use super::*;
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
