//! Graphics-API-agnostic Touhou-style geometry for maku render frames.
//!
//! The pack is a HOST (render-output-design: "mesh renderers are hosts"):
//! an optional consumer of the public `render_frame()` API with no
//! privileged engine relationship. Frame in, plain f32 buffers out —
//! `MeshFrame { vertices, indices, spans }` in world units plus a 64px
//! two-cell RGBA atlas (antialiased disc + ring outline), uploaded once.
//!
//! Geometry matches the original immediate-mode player's look:
//! - Point rows: two quads per dot — a fill quad (disc cell, size
//!   `dot_radius(family) * scale * 2`, `style_rgb(color)` hue-shifted,
//!   alpha-clamped) and a white outline quad (ring cell, 0.35·alpha).
//! - Polyline rows: ribbon quads per segment (4 verts each), sampling
//!   the disc-center texel; width 6px-equivalent when `:active`, else
//!   1.5px at 0.45·alpha, via `StyleTable.px_per_unit`.
//! - `theta` is unused (dots are radially symmetric); unknown schema
//!   columns are ignored; missing `family`/`color` fall back to
//!   defaults. Draw order is frame order; batches and rows produce
//!   identical geometry for equivalent content (tested).
//!
//! Steady-state costs: schema column indices cached per `Rc::ptr_eq`
//! schema identity, hue shifts memoized per (color, 0.5° bucket),
//! internal buffers reused (the returned `&MeshFrame` borrow lives
//! until the next `build`).

use maku::model::{Column, RenderBatch, RenderData, RenderItem, RenderSchema};
use std::collections::HashMap;
use std::rc::Rc;

const ATLAS_SIDE: u32 = 64;
const CELL: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub count: u32,
}

#[derive(Default, Debug, PartialEq)]
pub struct MeshFrame {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub spans: Vec<Span>,
}

#[derive(Clone, Copy)]
pub struct StyleTable {
    pub palette: fn(&str) -> (u8, u8, u8),
    pub dot_radius: fn(&str) -> f32,
    pub px_per_unit: f32,
}

impl Default for StyleTable {
    fn default() -> Self {
        Self {
            palette: maku::host::style_rgb,
            dot_radius: maku::host::dot_radius,
            px_per_unit: 55.0,
        }
    }
}

#[derive(Clone)]
struct SchemaCache {
    schema: Rc<RenderSchema>,
    family: Option<usize>,
    color: Option<usize>,
}

pub struct TouhouMesh {
    style: StyleTable,
    atlas: Vec<u8>,
    frame: MeshFrame,
    schemas: Vec<SchemaCache>,
    hues: HashMap<(Rc<str>, i32), [u8; 3]>,
}

impl TouhouMesh {
    pub fn new(style: StyleTable) -> Self {
        Self {
            style,
            atlas: make_atlas(),
            frame: MeshFrame::default(),
            schemas: Vec::new(),
            hues: HashMap::new(),
        }
    }

    pub fn atlas(&self) -> (&[u8], u32) {
        (&self.atlas, ATLAS_SIDE)
    }

    pub fn build(&mut self, frame: &[RenderItem]) -> &MeshFrame {
        self.frame.vertices.clear();
        self.frame.indices.clear();
        self.frame.spans.clear();
        for item in frame {
            let start = self.frame.indices.len() as u32;
            match item {
                RenderItem::Batch(batch) => self.push_batch(batch),
                RenderItem::Row(row) => match &row.data {
                    RenderData::Point {
                        x,
                        y,
                        scale,
                        alpha,
                        hue,
                        ..
                    } => {
                        let family = row.sym("family").unwrap_or("");
                        let color = row.sym("color").unwrap_or("");
                        self.push_dot(*x, *y, *scale, *alpha, color, *hue, family);
                    }
                    RenderData::Polyline { points, active } => {
                        let alpha = row.num("alpha").unwrap_or(1.0);
                        let hue = row.num("hue").unwrap_or(0.0);
                        self.push_polyline(
                            points,
                            *active,
                            row.sym("color").unwrap_or(""),
                            hue,
                            alpha,
                        );
                    }
                    RenderData::None => {}
                },
            }
            let count = self.frame.indices.len() as u32 - start;
            if count != 0 {
                self.frame.spans.push(Span { start, count });
            }
        }
        &self.frame
    }

    fn push_batch(&mut self, batch: &RenderBatch) {
        let cache = self.schema_cache(&batch.schema);
        for i in 0..batch.len {
            let family = sym_at(cache.family.and_then(|n| batch.cols.get(n)), i).unwrap_or("");
            let color = sym_at(cache.color.and_then(|n| batch.cols.get(n)), i).unwrap_or("");
            self.push_dot(
                batch.x.at(i),
                batch.y.at(i),
                batch.scale.at(i),
                batch.alpha.at(i),
                color,
                batch.hue.at(i),
                family,
            );
        }
    }

    fn schema_cache(&mut self, schema: &Rc<RenderSchema>) -> SchemaCache {
        if let Some(c) = self.schemas.iter().find(|c| Rc::ptr_eq(&c.schema, schema)) {
            return c.clone();
        }
        let find = |name| schema.cols.iter().position(|(key, _)| key.as_ref() == name);
        let cache = SchemaCache {
            schema: schema.clone(),
            family: find("family"),
            color: find("color"),
        };
        self.schemas.push(cache.clone());
        cache
    }

    fn push_dot(
        &mut self,
        x: f64,
        y: f64,
        scale: f64,
        alpha: f64,
        color: &str,
        hue: f64,
        family: &str,
    ) {
        let r = (self.style.dot_radius)(family) * scale as f32;
        let rgb = self.color(color, hue);
        let a = alpha.clamp(0.0, 1.0);
        self.push_quad(
            x as f32,
            y as f32,
            r,
            [0.0, 0.0, 0.5, 0.5],
            [rgb[0], rgb[1], rgb[2], byte(a)],
        );
        self.push_quad(
            x as f32,
            y as f32,
            r,
            [0.5, 0.0, 1.0, 0.5],
            [255, 255, 255, byte(0.35 * a)],
        );
    }

    fn push_polyline(
        &mut self,
        points: &[(f64, f64)],
        active: bool,
        color: &str,
        hue: f64,
        alpha: f64,
    ) {
        let rgb = self.color(color, hue);
        let a = alpha.clamp(0.0, 1.0) * if active { 1.0 } else { 0.45 };
        let half =
            (if active { 6.0 } else { 1.5 }) / self.style.px_per_unit.max(f32::EPSILON) / 2.0;
        let uv = [(0.25, 0.25); 4];
        for segment in points.windows(2) {
            let (ax, ay) = (segment[0].0 as f32, segment[0].1 as f32);
            let (bx, by) = (segment[1].0 as f32, segment[1].1 as f32);
            let (dx, dy) = (bx - ax, by - ay);
            let len = dx.hypot(dy);
            if len <= f32::EPSILON {
                continue;
            }
            let (nx, ny) = (-dy / len * half, dx / len * half);
            self.push_vertices(
                [
                    [ax + nx, ay + ny],
                    [ax - nx, ay - ny],
                    [bx - nx, by - ny],
                    [bx + nx, by + ny],
                ],
                uv,
                [rgb[0], rgb[1], rgb[2], byte(a)],
            );
        }
    }

    fn color(&mut self, sym: &str, hue: f64) -> [u8; 3] {
        if hue.abs() < 1e-9 {
            let (r, g, b) = (self.style.palette)(sym);
            return [r, g, b];
        }
        let bucket = (hue * 2.0).round() as i32;
        let key: (Rc<str>, i32) = (Rc::from(sym), bucket);
        if let Some(rgb) = self.hues.get(&key) {
            return *rgb;
        }
        let (r, g, b) = maku::host::style_rgb_hued(sym, bucket as f64 / 2.0);
        let rgb = [byte(r as f64), byte(g as f64), byte(b as f64)];
        self.hues.insert(key, rgb);
        rgb
    }

    fn push_quad(&mut self, x: f32, y: f32, r: f32, cell: [f32; 4], color: [u8; 4]) {
        self.push_vertices(
            [
                [x - r, y - r],
                [x + r, y - r],
                [x + r, y + r],
                [x - r, y + r],
            ],
            [
                (cell[0], cell[1]),
                (cell[2], cell[1]),
                (cell[2], cell[3]),
                (cell[0], cell[3]),
            ],
            color,
        );
    }

    fn push_vertices(&mut self, pos: [[f32; 2]; 4], uv: [(f32, f32); 4], color: [u8; 4]) {
        let base = self.frame.vertices.len() as u32;
        self.frame.vertices.extend((0..4).map(|i| Vertex {
            pos: pos[i],
            uv: [uv[i].0, uv[i].1],
            color,
        }));
        self.frame
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn sym_at(column: Option<&Column>, i: usize) -> Option<&str> {
    match column? {
        Column::SymConst(v) => Some(v),
        Column::Syms(v) => v.get(i)?.as_deref(),
        _ => None,
    }
}

fn byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
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
            out[p..p + 4].copy_from_slice(&[255, 255, 255, byte(alpha as f64)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use maku::host::Instance;
    use maku::model::{NumColumn, RenderBatch, RenderFieldKind, RenderRow};
    use maku::sim::Inputs;

    fn schema(with_syms: bool) -> Rc<RenderSchema> {
        Rc::new(RenderSchema {
            cols: if with_syms {
                vec![
                    (Rc::from("family"), RenderFieldKind::Sym),
                    (Rc::from("color"), RenderFieldKind::Sym),
                ]
            } else {
                vec![]
            },
        })
    }

    fn batch(with_syms: bool) -> Rc<RenderBatch> {
        Rc::new(RenderBatch {
            schema: schema(with_syms),
            len: 2,
            x: NumColumn::Rows(vec![1.0, 3.0]),
            y: NumColumn::Const(2.0),
            theta: NumColumn::Const(0.0),
            scale: NumColumn::Rows(vec![1.0, 2.0]),
            alpha: NumColumn::Const(0.5),
            hue: NumColumn::Rows(vec![0.0, 0.0]),
            cols: if with_syms {
                vec![
                    Column::SymConst(Rc::from("star")),
                    Column::Syms(vec![Some(Rc::from("red")), Some(Rc::from("blue"))]),
                ]
            } else {
                vec![]
            },
        })
    }

    #[test]
    fn batch_const_and_rows_make_two_quads_per_dot() {
        let mut mesh = TouhouMesh::new(StyleTable::default());
        let out = mesh.build(&[RenderItem::Batch(batch(true))]);
        assert_eq!((out.vertices.len(), out.indices.len()), (16, 24));
        assert_eq!(out.vertices[0].pos, [1.0 - 5.0 / 55.0, 2.0 - 5.0 / 55.0]);
        assert_eq!(out.vertices[0].color, [0xff, 0x4d, 0x5e, 128]);
        assert_eq!(out.vertices[8].pos, [3.0 - 10.0 / 55.0, 2.0 - 10.0 / 55.0]);
        assert_eq!(out.vertices[8].color, [0x5c, 0x9d, 0xff, 128]);
    }

    #[test]
    fn missing_symbols_use_default_style() {
        let mut mesh = TouhouMesh::new(StyleTable::default());
        let out = mesh.build(&[RenderItem::Batch(batch(false))]);
        assert_eq!(out.vertices[0].color, [255, 255, 255, 128]);
        assert_eq!(out.vertices[0].pos, [1.0 - 6.0 / 55.0, 2.0 - 6.0 / 55.0]);
    }

    #[test]
    fn row_and_batch_geometry_match() {
        let row = RenderRow {
            data: RenderData::Point {
                x: 1.0,
                y: 2.0,
                theta: 0.0,
                scale: 1.0,
                alpha: 0.5,
                hue: 0.0,
            },
            nums: vec![],
            syms: vec![
                (Rc::from("family"), Rc::from("star")),
                (Rc::from("color"), Rc::from("red")),
            ],
        };
        let one = RenderBatch {
            schema: schema(true),
            len: 1,
            x: NumColumn::Const(1.0),
            y: NumColumn::Const(2.0),
            theta: NumColumn::Const(0.0),
            scale: NumColumn::Const(1.0),
            alpha: NumColumn::Const(0.5),
            hue: NumColumn::Const(0.0),
            cols: vec![
                Column::SymConst(Rc::from("star")),
                Column::SymConst(Rc::from("red")),
            ],
        };
        let mut mesh = TouhouMesh::new(StyleTable::default());
        let a = mesh
            .build(&[RenderItem::Row(Rc::new(row))])
            .vertices
            .clone();
        let b = mesh
            .build(&[RenderItem::Batch(Rc::new(one))])
            .vertices
            .clone();
        assert_eq!(a, b);
    }

    #[test]
    fn polyline_has_four_vertices_per_segment() {
        let row = RenderRow::plain(RenderData::Polyline {
            points: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
            active: true,
        });
        let mut mesh = TouhouMesh::new(StyleTable::default());
        assert_eq!(
            mesh.build(&[RenderItem::Row(Rc::new(row))]).vertices.len(),
            8
        );
    }

    #[test]
    fn atlas_is_generated_square_power_of_two() {
        let mesh = TouhouMesh::new(StyleTable::default());
        let (rgba, side) = mesh.atlas();
        assert!(side.is_power_of_two());
        assert_eq!(rgba.len(), (side * side * 4) as usize);
        assert!(rgba.chunks_exact(4).any(|p| p[3] != 0));
    }

    #[test]
    fn real_sim_builds_finite_bounded_geometry() {
        let card = format!(
            "{}/../../cards/tutorials/t03.maku",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut inst = Instance::new(None);
        inst.boot(card, Some("ex3-fruit-colors".into()));
        for _ in 0..300 {
            inst.advance(Inputs::default());
        }
        assert!(
            inst.running(),
            "real tutorial sim failed: {}",
            inst.status()
        );
        let frame = inst.render_frame();
        let mut dots = 0;
        let mut ribbon_vertices = 0;
        for item in &frame {
            match item {
                RenderItem::Batch(batch) => dots += batch.len,
                RenderItem::Row(row) => match &row.data {
                    RenderData::Point { .. } => dots += 1,
                    RenderData::Polyline { points, .. } => {
                        ribbon_vertices += points.windows(2).filter(|p| p[0] != p[1]).count() * 4;
                    }
                    RenderData::None => {}
                },
            }
        }
        let mut mesh = TouhouMesh::new(StyleTable::default());
        let out = mesh.build(&frame);
        assert!(!out.vertices.is_empty());
        assert_eq!(out.vertices.len(), 8 * dots + ribbon_vertices);
        assert!(out.vertices.iter().all(|v| {
            v.pos[0].is_finite()
                && v.pos[1].is_finite()
                && v.pos[0].abs() <= 20.0
                && v.pos[1].abs() <= 20.0
        }));
    }
}
