use crate::*;
use std::rc::Rc;

const ATLAS_SIDE: u32 = 64;
const CELL: f32 = 32.0;

pub(crate) fn stock_desc() -> ProfileDesc {
    let texture = TextureResource {
        key: Rc::from("touhou.disc-ring"),
        source: TextureSource::BuiltinRgba8 {
            width: ATLAS_SIDE,
            height: ATLAS_SIDE,
            bytes: make_atlas().into_boxed_slice(),
        },
    };
    let materials = vec![
        material("touhou.sprite.tint", PrimitiveClass::Sprites, SourceLayout::TintedSprite, BlendMode::Alpha),
        material("touhou.sprite.fixed", PrimitiveClass::Sprites, SourceLayout::BasicSprite, BlendMode::Alpha),
        material("touhou.sprite.recolor", PrimitiveClass::Sprites, SourceLayout::RecolorSprite, BlendMode::Alpha),
        material("touhou.ribbon.tint", PrimitiveClass::IndexedStrips, SourceLayout::IndexedStrip, BlendMode::Alpha),
        material("touhou.sprite.additive", PrimitiveClass::Sprites, SourceLayout::TintedSprite, BlendMode::Additive),
        material("touhou.ribbon.additive", PrimitiveClass::IndexedStrips, SourceLayout::IndexedStrip, BlendMode::Additive),
    ];
    let palettes = [
        ("white", 0xff, 0xff, 0xff),
        ("red", 0xff, 0x4d, 0x5e),
        ("orange", 0xff, 0x9d, 0x3c),
        ("yellow", 0xff, 0xe0, 0x66),
        ("green", 0x66, 0xe0, 0x85),
        ("teal", 0x4d, 0xd8, 0xd0),
        ("blue", 0x5c, 0x9d, 0xff),
        ("purple", 0xb2, 0x7d, 0xff),
        ("pink", 0xff, 0x85, 0xc2),
        ("black", 0x60, 0x60, 0x70),
        ("blueteal", 0x4d, 0xbc, 0xe8),
    ].into_iter().map(|(key, r, g, b)| palette(key, [r, g, b])).collect();

    let fill = SpriteLayer {
        material: MaterialId(0),
        region: TextureRegion { texture: TextureId(0), uv: [0.0, 0.0, 0.5, 0.5] },
        size_mul: [1.0, 1.0],
        angle_offset: 0.0,
        alpha_mul: 1.0,
        color: LayerColor::Tint(PaletteShade::Pure),
    };
    let outline = SpriteLayer {
        material: MaterialId(1),
        region: TextureRegion { texture: TextureId(0), uv: [0.5, 0.0, 1.0, 0.5] },
        size_mul: [1.0, 1.0],
        angle_offset: 0.0,
        alpha_mul: 0.35,
        color: LayerColor::Fixed(Rgba8::WHITE),
    };
    let families = [
        "default", "amulet", "arrow", "arrowlaser", "circle", "ellipse", "fireball",
        "gcircle", "gdcircle", "gdlaser", "gem", "gglcircle", "glaser", "gpather",
        "keine", "laser", "lellipse", "lightning", "lstar", "pather", "sakura",
        "scircle", "star", "triangle",
    ];
    let variants = ["", "w", "b", "c"];
    let mut sprites = Vec::with_capacity(families.len() * variants.len());
    for family in families {
        let radius_px = match family { "lstar" | "gglcircle" => 10.0, "gem" | "star" => 5.0, _ => 6.0 };
        for variant in variants {
            sprites.push(SpriteStyle {
                family: Rc::from(family),
                variant: Rc::from(variant),
                radius_world: radius_px / 55.0,
                orientation: if directional_family(family) { OrientationPolicy::Directional } else { OrientationPolicy::Radial },
                layers: vec![fill.clone(), outline.clone()],
            });
        }
    }
    let ribbon = RibbonLayer {
        material: MaterialId(3),
        region: TextureRegion { texture: TextureId(0), uv: [0.24, 0.24, 0.26, 0.26] },
        width_mul: 1.0,
        active: RibbonAppearance { width_px: 6.0, alpha_mul: 1.0 },
        warning: RibbonAppearance { width_px: 1.5, alpha_mul: 0.45 },
        color: LayerColor::Tint(PaletteShade::Pure),
        join: JoinPolicy::Segment,
        cap: CapPolicy::Butt,
    };
    let mut beams = Vec::with_capacity(families.len() * variants.len());
    for family in families {
        for variant in variants {
            beams.push(BeamStyle { family: Rc::from(family), variant: Rc::from(variant), layers: vec![ribbon.clone()] });
        }
    }

    ProfileDesc {
        pixels_per_unit: 55.0,
        palettes,
        sprites,
        beams,
        textures: vec![texture],
        materials,
        effects: vec![],
        fallback_sprite: StyleKey::new("default", ""),
        fallback_beam: StyleKey::new("default", ""),
        fallback_color: Rc::from("white"),
        unknown: UnknownStylePolicy::Fallback,
    }
}

fn material(key: &str, primitive: PrimitiveClass, layout: SourceLayout, blend: BlendMode) -> MaterialDesc {
    MaterialDesc {
        key: Rc::from(key), primitive, layout, texture: TextureId(0),
        pipeline: Rc::from(match primitive { PrimitiveClass::Sprites => "touhou.sprite.v1", PrimitiveClass::IndexedStrips => "touhou.ribbon.v1" }),
        blend, sampler: SamplerDesc::default(),
        fixed_color: (layout == SourceLayout::BasicSprite).then_some(Rgba8::WHITE),
    }
}

fn palette(key: &str, pure: [u8; 3]) -> PaletteEntry {
    let shade = |factor: f32| Rgba8::rgb(
        (pure[0] as f32 * factor).clamp(0.0, 255.0).round() as u8,
        (pure[1] as f32 * factor).clamp(0.0, 255.0).round() as u8,
        (pure[2] as f32 * factor).clamp(0.0, 255.0).round() as u8,
    );
    PaletteEntry {
        key: Rc::from(key),
        highlight: shade(1.18), light: shade(1.08), pure: Rgba8::rgb(pure[0], pure[1], pure[2]),
        dark: shade(0.60), outline: Rgba8::WHITE,
    }
}

fn directional_family(family: &str) -> bool {
    matches!(family, "amulet" | "arrow" | "arrowlaser" | "ellipse" | "gem" | "keine" | "laser" | "lellipse" | "lightning" | "pather" | "sakura" | "triangle")
}

fn make_atlas() -> Vec<u8> {
    let mut out = vec![0; (ATLAS_SIDE * ATLAS_SIDE * 4) as usize];
    for y in 0..ATLAS_SIDE {
        for x in 0..ATLAS_SIDE {
            let cell_x = (x % CELL as u32) as f32 + 0.5;
            let d = ((cell_x - CELL / 2.0).powi(2) + (y as f32 + 0.5 - CELL / 2.0).powi(2)).sqrt();
            let alpha = if x < CELL as u32 {
                (CELL / 2.0 - d + 0.75).clamp(0.0, 1.0)
            } else {
                (1.5 - (d - (CELL / 2.0 - 1.5)).abs()).clamp(0.0, 1.0)
            };
            let p = ((y * ATLAS_SIDE + x) * 4) as usize;
            out[p..p + 4].copy_from_slice(&[255, 255, 255, byte(alpha)]);
        }
    }
    out
}

fn byte(value: f32) -> u8 { (value.clamp(0.0, 1.0) * 255.0).round() as u8 }
