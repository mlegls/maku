use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

macro_rules! id_type {
    ($name:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);
        impl $name { pub(crate) fn index(self) -> usize { self.0 as usize } }
    };
}

id_type!(PaletteId);
id_type!(SpriteStyleId);
id_type!(BeamStyleId);
id_type!(TextureId);
id_type!(MaterialId);

pub type SymbolKey = Rc<str>;
pub type ResourceKey = Rc<str>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8(pub [u8; 4]);

impl Rgba8 {
    pub const WHITE: Self = Self([255, 255, 255, 255]);
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self([r, g, b, 255]) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteShade { Highlight, Light, Pure, Dark, Outline }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    pub key: SymbolKey,
    pub highlight: Rgba8,
    pub light: Rgba8,
    pub pure: Rgba8,
    pub dark: Rgba8,
    pub outline: Rgba8,
}

impl PaletteEntry {
    pub fn solid(key: impl Into<SymbolKey>, color: Rgba8) -> Self {
        Self { key: key.into(), highlight: color, light: color, pure: color, dark: color, outline: color }
    }

    pub fn shade(&self, shade: PaletteShade) -> Rgba8 {
        match shade {
            PaletteShade::Highlight => self.highlight,
            PaletteShade::Light => self.light,
            PaletteShade::Pure => self.pure,
            PaletteShade::Dark => self.dark,
            PaletteShade::Outline => self.outline,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFilter { Nearest, Linear }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressMode { Clamp, Repeat, Mirror }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplerDesc {
    pub min_filter: TextureFilter,
    pub mag_filter: TextureFilter,
    pub address_u: AddressMode,
    pub address_v: AddressMode,
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            min_filter: TextureFilter::Linear,
            mag_filter: TextureFilter::Linear,
            address_u: AddressMode::Clamp,
            address_v: AddressMode::Clamp,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode { Opaque, Alpha, Additive, SoftAdditive }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveClass { Sprites, IndexedStrips }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceLayout { BasicSprite, TintedSprite, RecolorSprite, IndexedStrip }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSource {
    BuiltinRgba8 { width: u32, height: u32, bytes: Box<[u8]> },
    External { key: ResourceKey },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureResource {
    pub key: ResourceKey,
    pub source: TextureSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureRegion {
    pub texture: TextureId,
    /// Normalized `(u0, v0, u1, v1)` coordinates.
    pub uv: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialDesc {
    pub key: ResourceKey,
    pub primitive: PrimitiveClass,
    pub layout: SourceLayout,
    pub texture: TextureId,
    pub pipeline: ResourceKey,
    pub blend: BlendMode,
    pub sampler: SamplerDesc,
    /// Pipeline/material constant used by the basic precolored layout.
    pub fixed_color: Option<Rgba8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrientationPolicy { Radial, Directional }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerColor {
    Fixed(Rgba8),
    Tint(PaletteShade),
    Recolor { low: PaletteShade, high: PaletteShade },
}

impl LayerColor {
    pub fn layout(self) -> SourceLayout {
        match self {
            LayerColor::Fixed(_) => SourceLayout::BasicSprite,
            LayerColor::Tint(_) => SourceLayout::TintedSprite,
            LayerColor::Recolor { .. } => SourceLayout::RecolorSprite,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteLayer {
    pub material: MaterialId,
    pub region: TextureRegion,
    pub size_mul: [f32; 2],
    pub angle_offset: f32,
    pub alpha_mul: f32,
    pub color: LayerColor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteStyle {
    pub family: SymbolKey,
    pub variant: SymbolKey,
    pub radius_world: f32,
    pub orientation: OrientationPolicy,
    pub layers: Vec<SpriteLayer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinPolicy { Segment, Bevel }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapPolicy { Butt, Square }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RibbonAppearance {
    pub width_px: f32,
    pub alpha_mul: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RibbonLayer {
    pub material: MaterialId,
    pub region: TextureRegion,
    pub width_mul: f32,
    pub active: RibbonAppearance,
    pub warning: RibbonAppearance,
    pub color: LayerColor,
    pub join: JoinPolicy,
    pub cap: CapPolicy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BeamStyle {
    pub family: SymbolKey,
    pub variant: SymbolKey,
    pub layers: Vec<RibbonLayer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnknownStylePolicy { Error, Fallback }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleKey {
    pub family: SymbolKey,
    pub variant: SymbolKey,
}

impl StyleKey {
    pub fn new(family: impl Into<SymbolKey>, variant: impl Into<SymbolKey>) -> Self {
        Self { family: family.into(), variant: variant.into() }
    }
}

/// Versioned capabilities exposed by a primitive processor. Bits represent
/// layouts/channels/operations, not semantic effect names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimitiveCapabilities {
    pub layouts: u32,
    pub channels: u32,
    pub operations: u32,
}

impl PrimitiveCapabilities {
    pub const LAYOUT_BASIC: u32 = 1 << 0;
    pub const LAYOUT_TINT: u32 = 1 << 1;
    pub const LAYOUT_RECOLOR: u32 = 1 << 2;
    pub const LAYOUT_INDEXED: u32 = 1 << 3;
    pub const CHANNEL_TRANSFORM: u32 = 1 << 0;
    pub const CHANNEL_UV: u32 = 1 << 1;
    pub const CHANNEL_TINT: u32 = 1 << 2;
    pub const CHANNEL_RECOLOR: u32 = 1 << 3;
    pub const CHANNEL_ALPHA: u32 = 1 << 4;
    pub const CHANNEL_WIDTH: u32 = 1 << 5;
    pub const OP_DUPLICATE: u32 = 1 << 0;
    pub const OP_SCALE: u32 = 1 << 1;
    pub const OP_REPLACE_MATERIAL: u32 = 1 << 2;
    pub const OP_INSERT_ORDERED: u32 = 1 << 3;

    pub const SPRITE_V1: Self = Self {
        layouts: Self::LAYOUT_BASIC | Self::LAYOUT_TINT | Self::LAYOUT_RECOLOR,
        channels: Self::CHANNEL_TRANSFORM | Self::CHANNEL_UV | Self::CHANNEL_TINT
            | Self::CHANNEL_RECOLOR | Self::CHANNEL_ALPHA,
        operations: Self::OP_DUPLICATE | Self::OP_SCALE | Self::OP_REPLACE_MATERIAL
            | Self::OP_INSERT_ORDERED,
    };
    pub const RIBBON_V1: Self = Self {
        layouts: Self::LAYOUT_INDEXED,
        channels: Self::CHANNEL_UV | Self::CHANNEL_TINT | Self::CHANNEL_RECOLOR
            | Self::CHANNEL_ALPHA | Self::CHANNEL_WIDTH,
        operations: Self::OP_DUPLICATE | Self::OP_SCALE | Self::OP_REPLACE_MATERIAL
            | Self::OP_INSERT_ORDERED,
    };

    pub fn contains(self, required: Self) -> bool {
        self.layouts & required.layouts == required.layouts
            && self.channels & required.channels == required.channels
            && self.operations & required.operations == required.operations
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerInsertion { Before, After }

#[derive(Clone, Debug, PartialEq)]
pub enum LocalEffect {
    SpriteLayers {
        target: StyleKey,
        required: PrimitiveCapabilities,
        insertion: LayerInsertion,
        layers: Vec<SpriteLayer>,
    },
    RibbonLayers {
        target: StyleKey,
        required: PrimitiveCapabilities,
        insertion: LayerInsertion,
        layers: Vec<RibbonLayer>,
    },
}

#[derive(Clone, Debug)]
pub struct ProfileDesc {
    pub pixels_per_unit: f32,
    pub palettes: Vec<PaletteEntry>,
    pub sprites: Vec<SpriteStyle>,
    pub beams: Vec<BeamStyle>,
    pub textures: Vec<TextureResource>,
    pub materials: Vec<MaterialDesc>,
    pub effects: Vec<LocalEffect>,
    pub fallback_sprite: StyleKey,
    pub fallback_beam: StyleKey,
    pub fallback_color: SymbolKey,
    pub unknown: UnknownStylePolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileError {
    pub diagnostics: Vec<String>,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Touhou profile: {}", self.diagnostics.join("; "))
    }
}
impl std::error::Error for ProfileError {}

#[derive(Debug)]
pub struct TouhouProfile {
    pixels_per_unit: f32,
    palettes: Vec<PaletteEntry>,
    sprites: Vec<SpriteStyle>,
    beams: Vec<BeamStyle>,
    textures: Vec<TextureResource>,
    materials: Vec<MaterialDesc>,
    palette_ids: HashMap<SymbolKey, PaletteId>,
    sprite_ids: HashMap<SymbolKey, HashMap<SymbolKey, SpriteStyleId>>,
    beam_ids: HashMap<SymbolKey, HashMap<SymbolKey, BeamStyleId>>,
    fallback_sprite: SpriteStyleId,
    fallback_beam: BeamStyleId,
    fallback_color: PaletteId,
    unknown: UnknownStylePolicy,
}

impl TouhouProfile {
    pub fn new(mut desc: ProfileDesc) -> Result<Self, ProfileError> {
        let mut errors = Vec::new();
        apply_effects(&mut desc, &mut errors);
        if !desc.pixels_per_unit.is_finite() || desc.pixels_per_unit <= 0.0 {
            errors.push("pixels_per_unit must be finite and positive".into());
        }
        unique_keys("texture", desc.textures.iter().map(|v| v.key.as_ref()), &mut errors);
        unique_keys("material", desc.materials.iter().map(|v| v.key.as_ref()), &mut errors);
        unique_keys("palette", desc.palettes.iter().map(|v| v.key.as_ref()), &mut errors);
        unique_style_keys("sprite", desc.sprites.iter().map(|v| (&*v.family, &*v.variant)), &mut errors);
        unique_style_keys("beam", desc.beams.iter().map(|v| (&*v.family, &*v.variant)), &mut errors);

        for (i, texture) in desc.textures.iter().enumerate() {
            if let TextureSource::BuiltinRgba8 { width, height, bytes } = &texture.source {
                let expected = (*width as usize).checked_mul(*height as usize).and_then(|n| n.checked_mul(4));
                if *width == 0 || *height == 0 || expected != Some(bytes.len()) {
                    errors.push(format!("texture {i} '{}' has invalid RGBA8 dimensions/byte length", texture.key));
                }
            }
        }
        for (i, material) in desc.materials.iter().enumerate() {
            if material.texture.index() >= desc.textures.len() {
                errors.push(format!("material {i} '{}' references missing texture {}", material.key, material.texture.0));
            }
            let valid = matches!((material.primitive, material.layout),
                (PrimitiveClass::Sprites, SourceLayout::BasicSprite | SourceLayout::TintedSprite | SourceLayout::RecolorSprite)
                | (PrimitiveClass::IndexedStrips, SourceLayout::IndexedStrip));
            if !valid {
                errors.push(format!("material {i} '{}' has incompatible primitive/layout", material.key));
            }
            if (material.layout == SourceLayout::BasicSprite) != material.fixed_color.is_some() {
                errors.push(format!("material {i} '{}' must declare fixed_color exactly for the basic sprite layout", material.key));
            }
        }
        for (i, style) in desc.sprites.iter().enumerate() {
            if !style.radius_world.is_finite() || style.radius_world <= 0.0 {
                errors.push(format!("sprite {i} has non-positive or non-finite radius"));
            }
            if style.layers.is_empty() { errors.push(format!("sprite {i} has no layers")); }
            for (j, layer) in style.layers.iter().enumerate() {
                validate_sprite_layer(layer, i, j, &desc, &mut errors);
            }
        }
        for (i, style) in desc.beams.iter().enumerate() {
            if style.layers.is_empty() { errors.push(format!("beam {i} has no layers")); }
            for (j, layer) in style.layers.iter().enumerate() {
                validate_ribbon_layer(layer, i, j, &desc, &mut errors);
            }
        }

        let palette_ids = desc.palettes.iter().enumerate()
            .map(|(i, v)| (v.key.clone(), PaletteId(i as u32))).collect::<HashMap<_, _>>();
        let sprite_ids = style_map(desc.sprites.iter().enumerate().map(|(i, v)|
            (v.family.clone(), v.variant.clone(), SpriteStyleId(i as u32))));
        let beam_ids = style_map(desc.beams.iter().enumerate().map(|(i, v)|
            (v.family.clone(), v.variant.clone(), BeamStyleId(i as u32))));
        let fallback_sprite = lookup_style(&sprite_ids, &desc.fallback_sprite)
            .unwrap_or_else(|| { errors.push("fallback_sprite does not reference a declared sprite".into()); SpriteStyleId(0) });
        let fallback_beam = lookup_style(&beam_ids, &desc.fallback_beam)
            .unwrap_or_else(|| { errors.push("fallback_beam does not reference a declared beam".into()); BeamStyleId(0) });
        let fallback_color = palette_ids.get(desc.fallback_color.as_ref()).copied()
            .unwrap_or_else(|| { errors.push("fallback_color does not reference a declared palette".into()); PaletteId(0) });
        if !errors.is_empty() { return Err(ProfileError { diagnostics: errors }); }

        Ok(Self {
            pixels_per_unit: desc.pixels_per_unit,
            palettes: desc.palettes,
            sprites: desc.sprites,
            beams: desc.beams,
            textures: desc.textures,
            materials: desc.materials,
            palette_ids,
            sprite_ids,
            beam_ids,
            fallback_sprite,
            fallback_beam,
            fallback_color,
            unknown: desc.unknown,
        })
    }

    pub fn stock() -> Self { Self::new(crate::stock::stock_desc()).expect("stock profile is valid") }
    pub fn stock_desc() -> ProfileDesc { crate::stock::stock_desc() }
    pub fn pixels_per_unit(&self) -> f32 { self.pixels_per_unit }
    pub fn palettes(&self) -> &[PaletteEntry] { &self.palettes }
    pub fn sprites(&self) -> &[SpriteStyle] { &self.sprites }
    pub fn beams(&self) -> &[BeamStyle] { &self.beams }
    pub fn textures(&self) -> &[TextureResource] { &self.textures }
    pub fn materials(&self) -> &[MaterialDesc] { &self.materials }
    pub fn material(&self, id: MaterialId) -> &MaterialDesc { &self.materials[id.index()] }
    pub fn texture(&self, id: TextureId) -> &TextureResource { &self.textures[id.index()] }
    pub fn unknown_policy(&self) -> UnknownStylePolicy { self.unknown }
    pub fn sprite_capabilities(&self) -> PrimitiveCapabilities { PrimitiveCapabilities::SPRITE_V1 }
    pub fn ribbon_capabilities(&self) -> PrimitiveCapabilities { PrimitiveCapabilities::RIBBON_V1 }
    pub fn fixed_sprite_layers(&self, id: SpriteStyleId) -> usize { self.sprites[id.index()].layers.len() }

    pub(crate) fn palette_id(&self, key: &str) -> Option<PaletteId> { self.palette_ids.get(key).copied() }
    pub(crate) fn sprite_id(&self, family: &str, variant: &str) -> Option<SpriteStyleId> {
        self.sprite_ids.get(family).and_then(|m| m.get(variant)).copied()
    }
    pub(crate) fn beam_id(&self, family: &str, variant: &str) -> Option<BeamStyleId> {
        self.beam_ids.get(family).and_then(|m| m.get(variant)).copied()
    }
    pub(crate) fn palette(&self, id: PaletteId) -> &PaletteEntry { &self.palettes[id.index()] }
    pub(crate) fn sprite(&self, id: SpriteStyleId) -> &SpriteStyle { &self.sprites[id.index()] }
    pub(crate) fn beam(&self, id: BeamStyleId) -> &BeamStyle { &self.beams[id.index()] }
    pub(crate) fn fallback_sprite(&self) -> SpriteStyleId { self.fallback_sprite }
    pub(crate) fn fallback_beam(&self) -> BeamStyleId { self.fallback_beam }
    pub(crate) fn fallback_color(&self) -> PaletteId { self.fallback_color }
}

fn style_map<I, Id>(iter: I) -> HashMap<SymbolKey, HashMap<SymbolKey, Id>>
where I: Iterator<Item=(SymbolKey, SymbolKey, Id)>, Id: Copy {
    let mut out = HashMap::new();
    for (family, variant, id) in iter {
        out.entry(family).or_insert_with(HashMap::new).insert(variant, id);
    }
    out
}

fn lookup_style<Id: Copy>(map: &HashMap<SymbolKey, HashMap<SymbolKey, Id>>, key: &StyleKey) -> Option<Id> {
    map.get(key.family.as_ref()).and_then(|m| m.get(key.variant.as_ref())).copied()
}

fn unique_keys<'a>(what: &str, keys: impl Iterator<Item=&'a str>, errors: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for key in keys {
        if key.is_empty() { errors.push(format!("{what} key must not be empty")); }
        if !seen.insert(key) { errors.push(format!("duplicate {what} key '{key}'")); }
    }
}

fn unique_style_keys<'a>(what: &str, keys: impl Iterator<Item=(&'a str, &'a str)>, errors: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for key in keys {
        if key.0.is_empty() { errors.push(format!("{what} family must not be empty")); }
        if !seen.insert(key) { errors.push(format!("duplicate {what} key '{}:{}'", key.0, key.1)); }
    }
}

fn valid_region(region: TextureRegion, desc: &ProfileDesc) -> bool {
    if region.texture.index() >= desc.textures.len() { return false; }
    let [u0, v0, u1, v1] = region.uv;
    [u0, v0, u1, v1].iter().all(|v| v.is_finite())
        && u0 >= 0.0 && v0 >= 0.0 && u1 <= 1.0 && v1 <= 1.0 && u0 < u1 && v0 < v1
}

fn finite_positive(values: &[f32]) -> bool { values.iter().all(|v| v.is_finite() && *v > 0.0) }
fn finite_nonnegative(values: &[f32]) -> bool { values.iter().all(|v| v.is_finite() && *v >= 0.0) }

fn validate_sprite_layer(layer: &SpriteLayer, style: usize, layer_i: usize, desc: &ProfileDesc, errors: &mut Vec<String>) {
    let label = format!("sprite {style} layer {layer_i}");
    let Some(material) = desc.materials.get(layer.material.index()) else {
        errors.push(format!("{label} references missing material {}", layer.material.0)); return;
    };
    if material.primitive != PrimitiveClass::Sprites || material.layout != layer.color.layout() {
        errors.push(format!("{label} material/layout is incompatible with layer color"));
    }
    if let LayerColor::Fixed(color) = layer.color {
        if material.fixed_color != Some(color) {
            errors.push(format!("{label} fixed color differs from its material constant"));
        }
    }
    if material.texture != layer.region.texture { errors.push(format!("{label} texture differs from its material")); }
    if !valid_region(layer.region, desc) { errors.push(format!("{label} has invalid texture region")); }
    if !finite_positive(&layer.size_mul) || !finite_nonnegative(&[layer.alpha_mul]) || !layer.angle_offset.is_finite() {
        errors.push(format!("{label} has invalid dimensions/alpha/angle"));
    }
}

fn validate_ribbon_layer(layer: &RibbonLayer, style: usize, layer_i: usize, desc: &ProfileDesc, errors: &mut Vec<String>) {
    let label = format!("beam {style} layer {layer_i}");
    let Some(material) = desc.materials.get(layer.material.index()) else {
        errors.push(format!("{label} references missing material {}", layer.material.0)); return;
    };
    if material.primitive != PrimitiveClass::IndexedStrips || material.layout != SourceLayout::IndexedStrip {
        errors.push(format!("{label} material/layout is incompatible with ribbon geometry"));
    }
    if material.texture != layer.region.texture { errors.push(format!("{label} texture differs from its material")); }
    if !valid_region(layer.region, desc) { errors.push(format!("{label} has invalid texture region")); }
    if !finite_positive(&[layer.width_mul, layer.active.width_px, layer.warning.width_px])
        || !finite_nonnegative(&[layer.active.alpha_mul, layer.warning.alpha_mul]) {
        errors.push(format!("{label} has invalid width/alpha"));
    }
    if matches!(layer.color, LayerColor::Recolor { .. }) {
        errors.push(format!("{label} requests recolor but indexed v1 exposes one vertex tint"));
    }
    if layer.join != JoinPolicy::Segment || layer.cap != CapPolicy::Butt {
        errors.push(format!("{label} requests a join/cap policy unsupported by ribbon v1"));
    }
}

fn apply_effects(desc: &mut ProfileDesc, errors: &mut Vec<String>) {
    for effect in std::mem::take(&mut desc.effects) {
        match effect {
            LocalEffect::SpriteLayers { target, required, insertion, mut layers } => {
                if !PrimitiveCapabilities::SPRITE_V1.contains(required) {
                    errors.push(format!("sprite effect for '{}:{}' requires unsupported capabilities", target.family, target.variant));
                    continue;
                }
                match desc.sprites.iter_mut().find(|s| s.family == target.family && s.variant == target.variant) {
                    Some(style) => match insertion {
                        LayerInsertion::Before => { layers.append(&mut style.layers); style.layers = layers; }
                        LayerInsertion::After => style.layers.extend(layers),
                    },
                    None => errors.push(format!("sprite effect target '{}:{}' is missing", target.family, target.variant)),
                }
            }
            LocalEffect::RibbonLayers { target, required, insertion, mut layers } => {
                if !PrimitiveCapabilities::RIBBON_V1.contains(required) {
                    errors.push(format!("ribbon effect for '{}:{}' requires unsupported capabilities", target.family, target.variant));
                    continue;
                }
                match desc.beams.iter_mut().find(|s| s.family == target.family && s.variant == target.variant) {
                    Some(style) => match insertion {
                        LayerInsertion::Before => { layers.append(&mut style.layers); style.layers = layers; }
                        LayerInsertion::After => style.layers.extend(layers),
                    },
                    None => errors.push(format!("ribbon effect target '{}:{}' is missing", target.family, target.variant)),
                }
            }
        }
    }
}
