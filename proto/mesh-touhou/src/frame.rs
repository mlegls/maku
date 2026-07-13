use crate::{MaterialId, SourceLayout};

/// Version of the public host-facing buffer and manifest ABI.
pub const FRAME_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BasicSpriteInstance {
    pub center: [f32; 2],
    pub half_size: [f32; 2],
    /// Degrees, matching core's canonical angle convention.
    pub rotation: f32,
    pub uv_rect: [f32; 4],
    /// Per-row opacity remains available without carrying tint/recolor RGB.
    pub alpha: u8,
    pub _pad: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TintedSpriteInstance {
    pub base: BasicSpriteInstance,
    pub tint: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RecolorSpriteInstance {
    pub base: BasicSpriteInstance,
    pub color_lo: [u8; 4],
    pub color_hi: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StripVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawSource {
    BasicSprites { start: u32, count: u32 },
    TintedSprites { start: u32, count: u32 },
    RecolorSprites { start: u32, count: u32 },
    Indexed {
        vertex_start: u32,
        vertex_count: u32,
        index_start: u32,
        index_count: u32,
    },
}

impl DrawSource {
    pub fn layout(self) -> SourceLayout {
        match self {
            DrawSource::BasicSprites { .. } => SourceLayout::BasicSprite,
            DrawSource::TintedSprites { .. } => SourceLayout::TintedSprite,
            DrawSource::RecolorSprites { .. } => SourceLayout::RecolorSprite,
            DrawSource::Indexed { .. } => SourceLayout::IndexedStrip,
        }
    }

    fn merge(self, next: Self) -> Option<Self> {
        match (self, next) {
            (Self::BasicSprites { start, count }, Self::BasicSprites { start: n, count: nc })
                if start.checked_add(count) == Some(n) =>
                Some(Self::BasicSprites { start, count: count + nc }),
            (Self::TintedSprites { start, count }, Self::TintedSprites { start: n, count: nc })
                if start.checked_add(count) == Some(n) =>
                Some(Self::TintedSprites { start, count: count + nc }),
            (Self::RecolorSprites { start, count }, Self::RecolorSprites { start: n, count: nc })
                if start.checked_add(count) == Some(n) =>
                Some(Self::RecolorSprites { start, count: count + nc }),
            (
                Self::Indexed { vertex_start, vertex_count, index_start, index_count },
                Self::Indexed {
                    vertex_start: nv, vertex_count: nvc, index_start: ni, index_count: nic,
                },
            ) if vertex_start.checked_add(vertex_count) == Some(nv)
                && index_start.checked_add(index_count) == Some(ni) =>
                Some(Self::Indexed {
                    vertex_start,
                    vertex_count: vertex_count + nvc,
                    index_start,
                    index_count: index_count + nic,
                }),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawCommand {
    pub material: MaterialId,
    pub source: DrawSource,
}

#[derive(Default, Debug, PartialEq)]
pub struct MeshFrame {
    pub basic_sprites: Vec<BasicSpriteInstance>,
    pub tinted_sprites: Vec<TintedSpriteInstance>,
    pub recolor_sprites: Vec<RecolorSpriteInstance>,
    pub vertices: Vec<StripVertex>,
    pub indices: Vec<u32>,
    pub draws: Vec<DrawCommand>,
}

impl MeshFrame {
    pub(crate) fn clear(&mut self) {
        self.basic_sprites.clear();
        self.tinted_sprites.clear();
        self.recolor_sprites.clear();
        self.vertices.clear();
        self.indices.clear();
        self.draws.clear();
    }

    pub(crate) fn push_draw(&mut self, command: DrawCommand) {
        if let Some(last) = self.draws.last_mut() {
            if last.material == command.material {
                if let Some(source) = last.source.merge(command.source) {
                    last.source = source;
                    return;
                }
            }
        }
        self.draws.push(command);
    }
}
