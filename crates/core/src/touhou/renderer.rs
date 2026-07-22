use super::color::{alpha_byte, with_hue_alpha};
use super::*;
use crate::render::{Column, RenderBatch, RenderData, RenderFieldKind, RenderItem, RenderRow, RenderSchema};
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderDiagnostic {
    pub kind: &'static str,
    pub family_or_color: Rc<str>,
    pub variant: Rc<str>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    Schema(String),
    UnknownStyle { domain: &'static str, family_or_color: String, variant: String },
    InvalidRow(String),
    OutputOverflow,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(v) | Self::InvalidRow(v) => f.write_str(v),
            Self::UnknownStyle { domain, family_or_color, variant } =>
                write!(f, "unknown {domain} '{}:{}'", family_or_color, variant),
            Self::OutputOverflow => f.write_str("mesh frame exceeds the v1 u32 range"),
        }
    }
}
impl std::error::Error for RenderError {}

#[derive(Clone)]
struct SpriteBinding {
    schema: Rc<RenderSchema>,
    family: usize,
    color: usize,
    variant: usize,
}

#[derive(Clone)]
struct BeamBinding {
    schema: Rc<RenderSchema>,
    _width: usize,
    _hue: Option<usize>,
    _family: usize,
    _color: usize,
    _variant: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundBatchPlan {
    pub rows: usize,
    pub total_instances: usize,
    /// Present when every lane resolves the same recipe cardinality.
    pub fixed_layers_per_row: Option<usize>,
    pub instances_by_layout: [usize; 3],
    pub source_layouts: u32,
}

/// Genre-facing façade over shared sprite/ribbon processors.
pub struct TouhouMesh {
    profile: Rc<TouhouProfile>,
    frame: MeshFrame,
    sprite_bindings: Vec<SpriteBinding>,
    beam_bindings: Vec<BeamBinding>,
    diagnostics: Vec<RenderDiagnostic>,
}

impl Default for TouhouMesh {
    fn default() -> Self { Self::new(Rc::new(TouhouProfile::stock())) }
}

impl TouhouMesh {
    pub const RENDER_KINDS: [&'static str; 2] = ["sprite", "beam"];

    pub fn new(profile: Rc<TouhouProfile>) -> Self {
        Self {
            profile,
            frame: MeshFrame::default(),
            sprite_bindings: Vec::new(),
            beam_bindings: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn profile(&self) -> &TouhouProfile { &self.profile }
    pub fn frame(&self) -> &MeshFrame { &self.frame }
    pub fn supported_kinds(&self) -> &[&'static str] { &Self::RENDER_KINDS }
    pub fn diagnostics(&self) -> &[RenderDiagnostic] { &self.diagnostics }

    pub fn replace_profile(&mut self, profile: Rc<TouhouProfile>) {
        self.profile = profile;
        self.frame = MeshFrame::default();
        self.sprite_bindings.clear();
        self.beam_bindings.clear();
        self.diagnostics.clear();
    }

    /// Preflight a declared schema. Batches are also lazily bound on first use
    /// for hosts that cannot obtain the declaration separately.
    pub fn bind_schema(&mut self, kind: &str, schema: Rc<RenderSchema>) -> Result<(), RenderError> {
        match kind {
            "sprite" => { self.sprite_binding(&schema)?; Ok(()) }
            "beam" => { self.beam_binding(&schema)?; Ok(()) }
            other => Err(RenderError::Schema(format!("unsupported render kind '{other}'"))),
        }
    }

    pub fn plan_batch(&mut self, batch: &RenderBatch) -> Result<BoundBatchPlan, RenderError> {
        if batch.kind.as_ref() != "sprite" {
            return Err(RenderError::Schema(format!("kind '{}' has no point-batch handler", batch.kind)));
        }
        let binding = self.sprite_binding(&batch.schema)?;
        let mut layouts = 0;
        let mut instances_by_layout = [0; 3];
        let mut total_instances = 0;
        let mut first_layer_count = None;
        let mut homogeneous_cardinality = true;
        for lane in 0..batch.len {
            let family = sym_at(batch.cols.get(binding.family), lane).unwrap_or("default");
            let variant = sym_at(batch.cols.get(binding.variant), lane).unwrap_or("");
            let id = self.resolve_sprite(family, variant)?;
            let style = self.profile.sprite(id);
            match first_layer_count {
                None => first_layer_count = Some(style.layers.len()),
                Some(count) if count != style.layers.len() => homogeneous_cardinality = false,
                Some(_) => {}
            }
            total_instances += style.layers.len();
            for layer in &style.layers {
                let layout = layer.color.layout();
                layouts |= layout_bit(layout);
                instances_by_layout[match layout {
                    SourceLayout::BasicSprite => 0,
                    SourceLayout::TintedSprite => 1,
                    SourceLayout::RecolorSprite => 2,
                    SourceLayout::IndexedStrip => unreachable!(),
                }] += 1;
            }
        }
        Ok(BoundBatchPlan {
            rows: batch.len,
            total_instances,
            fixed_layers_per_row: homogeneous_cardinality.then_some(first_layer_count).flatten(),
            instances_by_layout,
            source_layouts: layouts,
        })
    }

    /// Deterministic sizing seam for variable ribbon geometry.
    pub fn ribbon_segment_count(points: &[(f64, f64)]) -> usize {
        points.windows(2).filter(|p| {
            let dx = p[1].0 - p[0].0;
            let dy = p[1].1 - p[0].1;
            dx.is_finite() && dy.is_finite() && dx.hypot(dy) > f64::EPSILON
        }).count()
    }

    pub fn build(&mut self, items: &[RenderItem]) -> Result<&MeshFrame, RenderError> {
        self.frame.clear();
        for item in items {
            let result = match item {
                RenderItem::Batch(batch) => self.push_batch(batch),
                RenderItem::Row(row) => self.push_row(row),
            };
            if let Err(error) = result {
                self.frame.clear();
                return Err(error);
            }
        }
        Ok(&self.frame)
    }

    fn push_row(&mut self, row: &RenderRow) -> Result<(), RenderError> {
        match (&*row.kind, &row.data) {
            ("sprite", RenderData::Point { x, y, theta, scale, alpha, hue }) => {
                if self.sprite_bindings.is_empty() {
                    return Err(RenderError::Schema("render kind 'sprite' was not bound before row emission".into()));
                }
                self.push_sprite(*x, *y, *theta, *scale, *alpha, *hue,
                    row.sym("family").unwrap_or("default"), row.sym("variant").unwrap_or(""),
                    row.sym("color").unwrap_or("white"))
            }
            ("beam", RenderData::Polyline { points, active }) => {
                if self.beam_bindings.is_empty() {
                    return Err(RenderError::Schema("render kind 'beam' was not bound before row emission".into()));
                }
                self.push_ribbon(points, *active, row.num("width").unwrap_or(1.0),
                    row.num("alpha").unwrap_or(1.0), row.num("hue").unwrap_or(0.0),
                    row.sym("family").unwrap_or("default"), row.sym("variant").unwrap_or(""),
                    row.sym("color").unwrap_or("white"))
            }
            (_, RenderData::None) => Ok(()),
            ("sprite" | "beam", _) => Err(RenderError::InvalidRow(format!(
                "render kind '{}' uses geometry incompatible with its Touhou handler", row.kind))),
            // Undeclared/foreign kinds remain legal core transport. This pack
            // is a controlled façade and does not blindly reinterpret them.
            _ => Ok(()),
        }
    }

    fn push_batch(&mut self, batch: &RenderBatch) -> Result<(), RenderError> {
        if batch.kind.as_ref() != "sprite" {
            return Ok(());
        }
        let binding = self.sprite_binding(&batch.schema)?;
        for i in 0..batch.len {
            self.push_sprite(
                batch.x.at(i), batch.y.at(i), batch.theta.at(i), batch.scale.at(i),
                batch.alpha.at(i), batch.hue.at(i),
                sym_at(batch.cols.get(binding.family), i).unwrap_or("default"),
                sym_at(batch.cols.get(binding.variant), i).unwrap_or(""),
                sym_at(batch.cols.get(binding.color), i).unwrap_or("white"),
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_sprite(&mut self, x: f64, y: f64, theta: f64, scale: f64, alpha: f64,
        hue: f64, family: &str, variant: &str, color: &str) -> Result<(), RenderError> {
        if ![x, y, theta, scale, alpha, hue].iter().all(|v| fits_f32(*v)) || scale <= 0.0 {
            return Err(RenderError::InvalidRow("sprite transform must fit finite f32 with positive scale".into()));
        }
        let style_id = self.resolve_sprite(family, variant)?;
        let palette_id = self.resolve_palette(color)?;
        let profile = &self.profile;
        let style = profile.sprite(style_id);
        let palette = profile.palette(palette_id);
        for layer in &style.layers {
            let base_alpha = alpha.clamp(0.0, 1.0) * layer.alpha_mul as f64;
            let rotation = layer.angle_offset + match style.orientation {
                OrientationPolicy::Radial => 0.0,
                OrientationPolicy::Directional => theta as f32,
            };
            let radius = style.radius_world * scale as f32;
            if !rotation.is_finite() || !radius.is_finite()
                || !layer.size_mul.iter().all(|v| (radius * *v).is_finite()) {
                return Err(RenderError::InvalidRow("sprite recipe transform overflows f32 output".into()));
            }
            let base = BasicSpriteInstance {
                center: [x as f32, y as f32],
                half_size: [radius * layer.size_mul[0], radius * layer.size_mul[1]],
                rotation,
                uv_rect: layer.region.uv,
                alpha: alpha_byte(base_alpha),
                _pad: [0; 3],
            };
            let material = layer.material;
            let source = match layer.color {
                LayerColor::Fixed(_) => {
                    let start = checked_u32(self.frame.basic_sprites.len())?;
                    self.frame.basic_sprites.push(base);
                    DrawSource::BasicSprites { start, count: 1 }
                }
                LayerColor::Tint(shade) => {
                    let start = checked_u32(self.frame.tinted_sprites.len())?;
                    self.frame.tinted_sprites.push(TintedSpriteInstance {
                        base,
                        tint: with_hue_alpha(palette.shade(shade), hue, base_alpha),
                    });
                    DrawSource::TintedSprites { start, count: 1 }
                }
                LayerColor::Recolor { low, high } => {
                    let start = checked_u32(self.frame.recolor_sprites.len())?;
                    self.frame.recolor_sprites.push(RecolorSpriteInstance {
                        base,
                        color_lo: with_hue_alpha(palette.shade(low), hue, base_alpha),
                        color_hi: with_hue_alpha(palette.shade(high), hue, base_alpha),
                    });
                    DrawSource::RecolorSprites { start, count: 1 }
                }
            };
            self.frame.push_draw(DrawCommand { material, source });
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_ribbon(&mut self, points: &[(f64, f64)], active: bool, width: f64, alpha: f64,
        hue: f64, family: &str, variant: &str, color: &str) -> Result<(), RenderError> {
        if ![width, alpha, hue].iter().all(|v| fits_f32(*v)) || width <= 0.0
            || !points.iter().all(|(x, y)| fits_f32(*x) && fits_f32(*y)) {
            return Err(RenderError::InvalidRow("beam points/modifiers must fit finite f32 with positive width".into()));
        }
        let style_id = self.resolve_beam(family, variant)?;
        let palette_id = self.resolve_palette(color)?;
        let profile = &self.profile;
        let style = profile.beam(style_id);
        let palette = profile.palette(palette_id);
        for layer in &style.layers {
            let appearance = if active { layer.active } else { layer.warning };
            let half = appearance.width_px * layer.width_mul * width as f32
                / profile.pixels_per_unit() / 2.0;
            if !half.is_finite() {
                return Err(RenderError::InvalidRow("beam width overflows f32 output".into()));
            }
            let layer_alpha = alpha.clamp(0.0, 1.0) * appearance.alpha_mul as f64;
            let color = match layer.color {
                LayerColor::Fixed(c) => with_hue_alpha(c, 0.0, layer_alpha),
                LayerColor::Tint(shade) => with_hue_alpha(palette.shade(shade), hue, layer_alpha),
                LayerColor::Recolor { .. } => unreachable!("profile validation rejects indexed recolor"),
            };
            let vertex_start = checked_u32(self.frame.vertices.len())?;
            let index_start = checked_u32(self.frame.indices.len())?;
            for pair in points.windows(2) {
                let (ax, ay) = (pair[0].0 as f32, pair[0].1 as f32);
                let (bx, by) = (pair[1].0 as f32, pair[1].1 as f32);
                let (dx, dy) = (bx - ax, by - ay);
                let len = dx.hypot(dy);
                if !len.is_finite() {
                    return Err(RenderError::InvalidRow("beam segment overflows f32 output".into()));
                }
                if len <= f32::EPSILON { continue; }
                let (nx, ny) = (-dy / len * half, dx / len * half);
                let base = checked_u32(self.frame.vertices.len())?;
                let [u0, v0, u1, v1] = layer.region.uv;
                self.frame.vertices.extend_from_slice(&[
                    StripVertex { pos: [ax + nx, ay + ny], uv: [u0, v0], color },
                    StripVertex { pos: [ax - nx, ay - ny], uv: [u0, v1], color },
                    StripVertex { pos: [bx - nx, by - ny], uv: [u1, v1], color },
                    StripVertex { pos: [bx + nx, by + ny], uv: [u1, v0], color },
                ]);
                self.frame.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            let vertex_count = checked_u32(self.frame.vertices.len())? - vertex_start;
            let index_count = checked_u32(self.frame.indices.len())? - index_start;
            if index_count != 0 {
                self.frame.push_draw(DrawCommand {
                    material: layer.material,
                    source: DrawSource::Indexed { vertex_start, vertex_count, index_start, index_count },
                });
            }
        }
        Ok(())
    }

    fn sprite_binding(&mut self, schema: &Rc<RenderSchema>) -> Result<SpriteBinding, RenderError> {
        if let Some(binding) = self.sprite_bindings.iter().find(|v| Rc::ptr_eq(&v.schema, schema)) {
            return Ok(binding.clone());
        }
        let binding = SpriteBinding {
            schema: schema.clone(),
            family: require_field(schema, "sprite", "family", RenderFieldKind::Sym)?,
            color: require_field(schema, "sprite", "color", RenderFieldKind::Sym)?,
            variant: require_field(schema, "sprite", "variant", RenderFieldKind::Sym)?,
        };
        self.sprite_bindings.push(binding.clone());
        Ok(binding)
    }

    fn beam_binding(&mut self, schema: &Rc<RenderSchema>) -> Result<BeamBinding, RenderError> {
        if let Some(binding) = self.beam_bindings.iter().find(|v| Rc::ptr_eq(&v.schema, schema)) {
            return Ok(binding.clone());
        }
        let binding = BeamBinding {
            schema: schema.clone(),
            _width: require_field(schema, "beam", "width", RenderFieldKind::Num)?,
            _hue: optional_field(schema, "beam", "hue", RenderFieldKind::Num)?,
            _family: require_field(schema, "beam", "family", RenderFieldKind::Sym)?,
            _color: require_field(schema, "beam", "color", RenderFieldKind::Sym)?,
            _variant: require_field(schema, "beam", "variant", RenderFieldKind::Sym)?,
        };
        self.beam_bindings.push(binding.clone());
        Ok(binding)
    }

    fn resolve_sprite(&mut self, family: &str, variant: &str) -> Result<SpriteStyleId, RenderError> {
        if let Some(id) = self.profile.sprite_id(family, variant) { return Ok(id); }
        self.unknown("sprite", family, variant)?;
        Ok(self.profile.fallback_sprite())
    }

    fn resolve_beam(&mut self, family: &str, variant: &str) -> Result<BeamStyleId, RenderError> {
        if let Some(id) = self.profile.beam_id(family, variant) { return Ok(id); }
        self.unknown("beam", family, variant)?;
        Ok(self.profile.fallback_beam())
    }

    fn resolve_palette(&mut self, color: &str) -> Result<PaletteId, RenderError> {
        if let Some(id) = self.profile.palette_id(color) { return Ok(id); }
        self.unknown("palette", color, "")?;
        Ok(self.profile.fallback_color())
    }

    fn unknown(&mut self, name: &'static str, key: &str, variant: &str) -> Result<(), RenderError> {
        if self.profile.unknown_policy() == UnknownStylePolicy::Error {
            return Err(RenderError::UnknownStyle {
                domain: name, family_or_color: key.to_owned(), variant: variant.to_owned(),
            });
        }
        if !self.diagnostics.iter().any(|d| {
            d.kind == name && d.family_or_color.as_ref() == key && d.variant.as_ref() == variant
        }) {
            self.diagnostics.push(RenderDiagnostic {
                kind: name,
                family_or_color: Rc::from(key),
                variant: Rc::from(variant),
                message: format!("unknown {name} '{}:{}'; using explicit profile fallback", key, variant),
            });
        }
        Ok(())
    }
}

fn checked_u32(value: usize) -> Result<u32, RenderError> { u32::try_from(value).map_err(|_| RenderError::OutputOverflow) }
fn fits_f32(value: f64) -> bool { value.is_finite() && value.abs() <= f32::MAX as f64 }

fn require_field(schema: &RenderSchema, kind: &str, name: &str, expected: RenderFieldKind) -> Result<usize, RenderError> {
    match schema.cols.iter().position(|(key, _)| key.as_ref() == name) {
        Some(index) if schema.cols[index].1 == expected => Ok(index),
        Some(_) => Err(RenderError::Schema(format!("render kind '{kind}' field '{name}' has incompatible kind"))),
        None => Err(RenderError::Schema(format!("render kind '{kind}' is missing required field '{name}'"))),
    }
}

fn optional_field(
    schema: &RenderSchema,
    kind: &str,
    name: &str,
    expected: RenderFieldKind,
) -> Result<Option<usize>, RenderError> {
    match schema.cols.iter().position(|(key, _)| key.as_ref() == name) {
        Some(index) if schema.cols[index].1 == expected => Ok(Some(index)),
        Some(_) => Err(RenderError::Schema(format!("render kind '{kind}' field '{name}' has incompatible kind"))),
        None => Ok(None),
    }
}

fn sym_at(column: Option<&Column>, index: usize) -> Option<&str> {
    match column? {
        Column::SymConst(value) => Some(value),
        Column::Syms(values) => values.get(index)?.as_deref(),
        _ => None,
    }
}

fn layout_bit(layout: SourceLayout) -> u32 {
    match layout {
        SourceLayout::BasicSprite => PrimitiveCapabilities::LAYOUT_BASIC,
        SourceLayout::TintedSprite => PrimitiveCapabilities::LAYOUT_TINT,
        SourceLayout::RecolorSprite => PrimitiveCapabilities::LAYOUT_RECOLOR,
        SourceLayout::IndexedStrip => PrimitiveCapabilities::LAYOUT_INDEXED,
    }
}
