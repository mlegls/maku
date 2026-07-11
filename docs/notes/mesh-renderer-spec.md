# Touhou mesh renderer pack — implementation spec

Status: WORK ORDER 2026-07 (round 22, track B). Architecture decided in
render-output-design.md "Mesh renderers are hosts": the pack is a HOST —
a separate optional crate consuming the public `render_frame()` API with
no privileged engine relationship. This spec is the concrete build plan.

## Deliverables

1. **Extract the player bin into its own crate** `proto/player`
   (`maku-player`), workspace member. Today the bin lives inside the
   `maku` crate (`proto/core/Cargo.toml` `[[bin]] path =
   "../player/src/main.rs"`, feature `player`); that makes a
   maku-dependent mesh crate a dependency cycle. After extraction:
   - `proto/player/Cargo.toml`: `maku = { path = "../core" }`,
     `macroquad = "0.4"`, `maku-mesh-touhou = { path = "../mesh-touhou" }`.
   - Remove the `[[bin]]`, `player` feature, and optional macroquad dep
     from `proto/core/Cargo.toml`. Keep the `web` feature as is.
   - Workspace members: `["core", "player", "mesh-touhou"]`.
   - Update build/run instructions everywhere they appear:
     `cargo run --release --features player` (and `--bin maku` variants)
     → `cargo run --release -p maku-player`. Files: README.md,
     docs/player.md, docs/tutorials/*.md,
     proto/editors/danmaku.nvim/README.md. Search for `--features
     player` and `features = ["player"]` to catch all.

2. **New crate `proto/mesh-touhou`** (`maku-mesh-touhou`): frame →
   geometry, graphics-API-agnostic (NO macroquad/wgpu dependency; output
   is plain f32 buffers any GPU host can upload). Depends only on `maku`.

3. **Player adoption**: the player's per-row immediate-mode draw loop
   (`draw_circle`/`draw_line` over `inst.render()`) is replaced by
   `inst.render_frame()` → mesh pack → one `draw_mesh` call (or a few).
   Everything else in the player (markers, event flashes, UI, timeline)
   stays exactly as is.

## Pack API (shape, not letter — adjust as implementation demands)

```rust
pub struct TouhouMesh {
    style: StyleTable,
    // internal: schema column-index caches keyed on Rc::ptr_eq,
    // reusable vertex/index Vecs, sprite atlas
}

pub struct StyleTable {
    /// color sym -> sRGB; default table = maku::host::style_rgb palette.
    pub palette: fn(&str) -> (u8, u8, u8), // or a map — impl choice
    /// family sym -> dot radius in world units; default =
    /// maku::host::dot_radius.
    pub dot_radius: fn(&str) -> f32,
}

/// One frame's geometry. World-space coordinates, f32. The host applies
/// its own world->screen transform (scale/offset or a matrix) at draw.
pub struct MeshFrame {
    pub vertices: Vec<Vertex>,   // pos: [f32; 2], uv: [f32; 2], color: [u8; 4] (premult-free sRGB + alpha)
    pub indices: Vec<u32>,
    /// Contiguous index ranges in draw order. All spans share the one
    /// atlas texture; the split exists so hosts can interleave their own
    /// draws if they ever need to (draw order = frame order).
    pub spans: Vec<Span>,        // { start: u32, count: u32 }
}

impl TouhouMesh {
    pub fn new(style: StyleTable) -> Self;
    /// 2^n square RGBA8 sprite atlas, built once: a filled antialiased
    /// disc (soft ~1.5px edge) and a thin circle outline, each in its
    /// own atlas cell. The host uploads this as a texture at init.
    pub fn atlas(&self) -> (&[u8], u32 /* side px */);
    /// Build the frame's geometry. Reuses internal buffers; the returned
    /// borrow is valid until the next build call.
    pub fn build(&mut self, frame: &[maku::model::RenderItem]) -> &MeshFrame;
}
```

## Geometry rules (match today's player output visually)

Consume both `RenderItem::Batch` (column reads, the fast path) and
`RenderItem::Row` (per-row fallback; also handles interpreted rules like
beams). Draw order is frame order — batches and rows interleave exactly
as the frame streams them.

- **Point rows** (RenderData::Point / batch geometry columns): two quads
  per dot, both UV-mapped into the atlas:
  1. fill quad: disc cell, size `dot_radius(family) * scale * 2`,
     color = `style_rgb(color)` hue-shifted by `hue` (see below),
     alpha = `alpha.clamp(0,1)`;
  2. outline quad: ring cell, same size, white at `0.35 * alpha` —
     matches today's `draw_circle_lines(..., 1.5, white 0.35*a)`.
  Batch column access: `family`/`color` are sym columns, `hue`/`alpha`/
  `scale`/`x`/`y` come from the geometry NumColumns (`NumColumn::Const`
  vs `Rows` — handle both; `Const` means one lookup for the whole
  batch). Missing sym field on a row → same defaults as the player today
  (`family`→"", `color`→"" → white). Cache per-schema column indices
  keyed on `Rc::ptr_eq(&batch.schema, cached)` — schema identity is
  stable at steady state (render-output-design.md).
- **Hue shift**: reuse `maku::host::style_rgb_hued` for correctness, but
  since batches often carry `hue` as a per-row column, memoize
  `(color-sym, hue-bucket)` → rgb with hue bucketed to 0.5° — visually
  identical, avoids per-row HSL round-trips. `hue == 0` short-circuits.
- **Polyline rows** (RenderData::Polyline): tessellate each segment
  chain as a ribbon (triangle strip as indexed quads per segment, using
  the atlas disc-center texel region so it renders as solid color).
  Width: active ⇒ 6px-equivalent, else 1.5px-equivalent at 0.45 alpha —
  BUT the pack works in world units; take a `px_per_unit: f32` hint in
  StyleTable (player passes its PIXELS_PER_UNIT) to convert. Match
  today's per-segment `draw_line` look; miter/joins can be naive
  (segment quads with round caps via small disc quads at joints is
  optional polish, not required).
- **theta** is currently unused by the player's dot draw; ignore it
  (dots are radially symmetric). Do not error on extra schema columns —
  unknown columns are simply unused (schemas are user-defined; this pack
  styles the touhou schema's vocabulary: family, color, hue, alpha).

## Player integration details

- Upload the atlas once (`Texture2D::from_rgba8`); set FilterMode::Linear.
- Per frame: `let frame = app.inst.render_frame();` →
  `mesh.build(&frame)` → convert to macroquad `Mesh { vertices, indices,
  texture }` and `draw_mesh`. macroquad's `Vertex` has pos3/uv/color —
  map x,y through the existing `to_screen` transform at vertex-build
  time is NOT allowed (pack is host-agnostic, world units); instead the
  player either (a) applies the camera via `set_camera` with a Camera2D
  whose zoom/offset reproduce `to_screen`, drawing the mesh in world
  space (mind the y-flip), then restores the default camera for UI, or
  (b) transforms vertices while converting to macroquad Vertex (simple
  loop it already pays). (b) is fine and simpler; pick it unless (a)
  turns out cleaner.
- macroquad `draw_mesh` with u16 indices only? Check: macroquad 0.4
  Mesh uses u16 indices. If so, chunk MeshFrame spans into ≤65535-vertex
  draws during conversion (the pack keeps u32; the player chunks).
- Keep `inst.render()` unused in the player afterward (do not delete the
  core API — web.rs and tests use frame/rows as before).
- The player-marker, graze rings, iframe flash, pattern menu, timeline:
  untouched, drawn after the mesh (same z order as today: bullets under
  UI overlays).

## Testing / acceptance gates (ALL must pass)

1. `cargo test --release --manifest-path proto/core/Cargo.toml` — 236
   tests, must stay green (core is untouched except Cargo.toml).
2. `MAKU_LOWER_ORACLE=1 cargo test --release --manifest-path
   proto/core/Cargo.toml -- --ignored` — 4 card suites green.
3. `cargo build --release -p maku-player` and `cargo test -p
   maku-mesh-touhou` build/pass.
4. Pack unit tests (in mesh-touhou): (a) a hand-built batch (use the
   public model types) produces 2 quads per row with expected positions/
   sizes/colors, Const and Rows NumColumns both covered; (b) sym-column
   absence falls back to defaults; (c) a Row item and a Batch item
   produce identical geometry for equivalent content; (d) polyline
   ribbon vertex count = 4 per segment (+caps if implemented); (e) atlas
   is generated, square, 2^n.
5. Visual smoke: run `cargo run --release -p maku-player
   cards/tutorials/t03.maku ex3-fruit-colors` manually is NOT possible
   in automation — instead add a pack test that builds geometry from a
   REAL sim: boot `maku::host::Instance` with
   `cards/tutorials/t03.maku`, advance ~300 ticks, `render_frame()`,
   assert build() output is non-empty, vertex count == 8 × dot rows +
   ribbon vertices, and all vertex positions finite and within ±20
   world units.
6. Doc updates from deliverable 1 complete (`grep -rn "features player\|--features player"` finds nothing stale outside docs/notes history).

Commit in coherent change-sets (player extraction; pack crate; player
adoption; docs) with clear messages. Do not modify anything under
proto/core/src, cards/, or docs/notes (other than what this spec
requires: proto/core/Cargo.toml only).
