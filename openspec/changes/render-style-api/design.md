## Context

`maku_mesh_touhou::TouhouMesh` currently consumes the public ordered render frame and returns reused `MeshFrame { vertices, indices, spans }` buffers plus one generated disc/ring atlas. `StyleTable` is three callbacks/values (`palette`, `dot_radius`, `px_per_unit`); dot geometry always emits a palette-tinted disc plus white outline, nonzero hue bypasses the injected palette through `maku::host::style_rgb_hued`, `variant` and point `theta` are ignored, and beam width is hardcoded. The macroquad host assumes every span uses the same atlas/material. `proto/core/src/web.rs` independently repeats palette/radius rendering instead of consuming the pack.

The engine boundary is already settled by `openspec/specs/render-rows/spec.md`: mesh renderers are optional hosts over an ordered typed frame, style vocabulary is pack/library policy, and engine transport contains no texture/material semantics. The recent per-kind schema work provides stable `:sprite` and `:beam` layouts without requiring another core change.

### Danmokou evidence and lessons

The upstream DMK model is useful evidence, not an API to copy. Sources below refer to `Bagoum/danmokou` master commit `cd21229fbce6deddfad360497ead6263c04ba6cf`; exact prefab, palette, material, and texture assets live in host/game content repositories rather than the renderer engine.

- Official bullet documentation defines shape/color/gradient style names, additive `G*` families, runtime recoloring, and style/color render priority: <https://dmk.bagoum.com/docs/articles/bullets.html>.
- `Assets/Danmokou/Plugins/Danmokou/Core/Scriptables/Colors/Palette.cs` defines named multi-shade palettes (`highlight`, `light`, `pure`, `dark`, `outline`) and a recolorizable flag.
- `.../Danmaku/BulletConfigReader.cs` expands family/palette variant keys, derives most recolored textures at registration/load time, and binds each style to sprite, material, blend mode, shader features, and render priority.
- `.../Descriptors/SimpleBulletEmptyScript.cs` keeps material override, render mode/priority, rotational behavior, sprite sheet, gradient variants, animation, and displacement in style/prefab data.
- `.../SimpleBulletManager.cs` submits compact instanced position/direction/time records with optional tint or two-endpoint recolor buffers and batches by style/material. Its fixed capacities (2047 structured-buffer instances; 127 in the legacy WebGL/WebGPU path) also argue for a host-owned chunking policy.
- `.../Core/Graphics/MaterialUtils.cs` maps normal, additive, soft-additive, and negative modes to concrete blend factors/operations; the evidence does not establish a separate general-purpose “glow shader” API.

Maku should adopt data-owned profiles, palette ramps, material separation, directional style metadata, declared instance channels, and instanced point output. It should not adopt concatenated style strings, Unity prefab/material objects, generated palette cross-products, or global render-queue sorting: Maku already has separate `family`/`color`/`variant` axes and a normative emission-order frame.

## Goals / Non-Goals

**Goals:**

- Host-defined palettes, sprite families/variants, texture regions, beam looks, and alpha/additive materials.
- A stock profile that reproduces current disc/outline and beam behavior as data.
- Compact instanced point output and indexed variable geometry behind one ordered draw-command stream.
- Opaque, stable resource/material ids suitable for native and wasm hosts.
- Directional sprites, palette-ramp recoloring, additive glow layers, and multiple textures without engine changes.
- Explicit profile validation and unknown-style behavior.
- Steady-state buffer reuse and no per-row string/Rc allocation.

**Non-Goals:**

- Changing engine render kinds, schemas, or emission-order semantics.
- Global z/render-queue sorting in the mesh pack.
- GPU texture/shader creation or asset I/O in Rust geometry code.
- Arbitrary material-specific vertex/instance layouts in v1.
- A universal mesh API for non-Touhou packs.
- Moving the pack into `proto/core` or making core depend on it.
- DMK-compatible style-string parsing.

## Decisions

### 1. Replace `StyleTable` with immutable validated `TouhouProfile`

Illustrative public shape:

```rust
pub struct TouhouProfile {
    pub pixels_per_unit: f32,
    pub palettes: Vec<PaletteEntry>,
    pub sprites: Vec<SpriteStyle>,
    pub beams: Vec<BeamStyle>,
    pub textures: Vec<TextureResource>,
    pub materials: Vec<MaterialDesc>,
    pub fallback_sprite: StyleId,
    pub fallback_beam: StyleId,
    pub fallback_color: PaletteId,
    pub unknown: UnknownStylePolicy,
}
```

Construction validates unique keys, id references, finite positive sizes, UV bounds when dimensions are known, material/source compatibility, and required fallbacks. The profile builder defaults to strict `Error`; a fallback profile must opt in and provide all fallback ids. The stock compatibility profile opts into explicit fallbacks to preserve the current dynamic-card behavior. `TouhouMesh` owns an immutable `Rc<TouhouProfile>` (or equivalent shared owner); replacing a profile constructs a new renderer or explicitly clears every schema/style/resource cache.

Alternative: retain function pointers for palette/radius. Rejected because callbacks cannot describe texture regions, materials, layers, web resource manifests, or serializable host configuration, and the hue path already escapes the injected callback.

### 2. Keep authored style axes separate

For a `:sprite` row:

```text
(family, variant) -> SpriteStyle
color             -> PaletteEntry
hue, alpha         -> per-instance color/opacity modification
scale, theta       -> geometry transform
```

For `:beam`, `(family, variant)` selects `BeamStyle`, `color` selects the palette, and row `width` scales the profile width. Profiles may use empty/default variants explicitly. No runtime concatenation or parsing such as DMK's `circle-red/w` occurs.

Resolved style/palette ids are cached by existing interned/Rc symbol identity where possible. Dynamic unknowns follow `UnknownStylePolicy::{Fallback, Error}`; fallback ids are explicit profile data, never hidden white/radius defaults.

### 3. Palettes are shade ramps, not one RGB callback

```rust
pub struct PaletteEntry {
    pub key: SymbolKey,
    pub highlight: Rgba8,
    pub light: Rgba8,
    pub pure: Rgba8,
    pub dark: Rgba8,
    pub outline: Rgba8,
}
```

A layer selects a fixed color, one palette shade, or two palette shades/endpoints. `MaterialDesc` declares whether its standard instance layout consumes no dynamic color, one tint, or a low/high recolor pair. Most fixed recolors may instead be represented by profile resources prepared once at load time, matching DMK's useful cold-data pattern without requiring runtime texture construction inside the geometry pass. Per-row hue/alpha promotes a draw to the needed dynamic layout and resolves the selected palette shades through one pack-owned deterministic color transform.

This captures the useful part of DMK's gradient palette without coupling to Unity gradient textures or paying two colors for every instance. The stock profile derives its current colors and white outline from these entries.

### 4. Sprite styles are ordered layers

```rust
pub struct SpriteStyle {
    pub family: SymbolKey,
    pub variant: SymbolKey,
    pub radius_world: f32,
    pub orientation: OrientationPolicy,
    pub layers: Box<[SpriteLayer]>,
}

pub struct SpriteLayer {
    pub material: MaterialId,
    pub region: TextureRegion,
    pub size_mul: [f32; 2],
    pub angle_offset: f32,
    pub alpha_mul: f32,
    pub color: LayerColor,
}
```

`OrientationPolicy` is radial/ignore-theta or directional/use-theta. The stock disc and white outline are two layers over the generated atlas. A glow is an additional layer referencing an additive material. A custom sprite sheet is another texture region. Layer order is preserved within the row; no global style sorting occurs.

Alternative: one style equals one texture/material. Rejected because the current look already needs fill plus outline and glow/additive composition should not require card-level duplicate rows.

### 5. Beam styles are separate profile data

`BeamStyle` declares material/texture region, active and warning base widths, alpha multipliers, and join/cap policy supported by the pack. The row's schema-provided `width` multiplies the profile width; `active` chooses active versus warning appearance. The existing hardcoded 6px/1.5px values become stock-profile defaults.

Variable polyline points remain indexed strip geometry. More advanced tiled/animated lasers require a later bounded beam-layout extension, not an arbitrary shader callback.

### 6. Output fixed-layout sprite instance streams plus indexed geometry

```rust
pub struct BasicSpriteInstance {
    pub center: [f32; 2],
    pub half_size: [f32; 2],
    pub rotation: f32,
    pub uv_rect: [f32; 4],
}

pub struct TintedSpriteInstance {
    pub base: BasicSpriteInstance,
    pub tint: [u8; 4],
}

pub struct RecolorSpriteInstance {
    pub base: BasicSpriteInstance,
    pub color_lo: [u8; 4],
    pub color_hi: [u8; 4],
}

pub struct StripVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

pub struct MeshFrame {
    pub basic_sprites: Vec<BasicSpriteInstance>,
    pub tinted_sprites: Vec<TintedSpriteInstance>,
    pub recolor_sprites: Vec<RecolorSpriteInstance>,
    pub vertices: Vec<StripVertex>,
    pub indices: Vec<u32>,
    pub draws: Vec<DrawCommand>,
}
```

These three sprite layouts are the complete v1 ABI: no color for fixed/precolored assets, one tint for the common palette/hue/alpha case, and two endpoints only for materials that declare dynamic recolor. `DrawCommand` selects the matching `Sprites { layout, start, count }` or `Indexed { start, count }` source plus one compatible `MaterialId`. A material's declared layout must match the source. Texture/pipeline/blend/sampler state are resolved through the material table.

Commands are appended in render-frame order; only adjacent commands with the same source layout/material and contiguous range coalesce. Expanding a render batch versus equivalent rows produces identical instance/geometry/command order.

Alternative: continue expanding point quads. Rejected because it multiplies CPU writes and wasm transfer by four vertices plus six indices per layer at the exact scale where instancing is required.

Alternative: bucket globally by material. Rejected because `render-rows` makes emission order observable. DMK can pool/sort styles by render queue; Maku cannot silently adopt that ordering model.

### 7. Materials/resources are cold host contracts

```rust
pub struct MaterialDesc {
    pub key: ResourceKey,
    pub primitive: PrimitiveClass, // sprites or indexed strips
    pub texture: TextureId,
    pub pipeline: ResourceKey,
    pub blend: BlendMode,
    pub sampler: SamplerDesc,
}

pub enum TextureSource {
    BuiltinRgba8 { width: u32, height: u32, bytes: Box<[u8]> },
    External { key: ResourceKey },
}
```

Exact owned/borrowed storage may vary, but the contract is: the pack exposes stable ids and optional builtin bytes; hosts load/upload external resources and create pipelines. `MaterialId` in a frame is an index into the immutable profile manifest. Alpha and additive blend modes are standard descriptors; custom pipeline keys let a host substitute shaders that consume the standard v1 instance/vertex ABI.

Materials requiring additional per-instance attributes are out of v1. Add a versioned standard stream or a separate renderer pack after a concrete schema requires it; do not reserve a permanently bloated generic payload.

### 8. Preserve zero-allocation steady state

`TouhouMesh::build` clears and reuses every frame vector. Schema caches resolve relevant column indices once per stable `Rc<RenderSchema>`. Symbol-to-style/palette caches retain interned/shared keys without `Rc::from` per row. Hue/ramp results memoize on resolved ids and hue bucket. Building returns a borrow valid until the next build, as today.

Strict unknown-style errors occur only on first resolution; fallback mode resolves once to explicit fallback ids. Profile/resource mutation during build is prohibited.

### 9. Wasm integration lives in the existing `proto/web` host directory above core

`mesh-touhou` already depends on core, so core cannot depend back on the pack. `proto/web` becomes a workspace crate in addition to retaining its static/editor assets and build script; its Rust library depends on both core and `mesh-touhou`, owns `Instance` plus `TouhouMesh`, builds once per render frame, and exports typed-array views over sprites, strip vertices, indices, and packed draw commands in the same wasm linear memory. It also exports the cold material/texture manifest and builtin resource bytes. `proto/web/build.sh` builds this host crate rather than core directly.

JavaScript maps material/pipeline/resource ids to WebGL/WebGPU objects and performs draw calls. Rust geometry remains in the same wasm module as the engine; a second wasm module would add memory sharing/copying without an ownership benefit. Existing direct palette/radius flattening in `proto/core/src/web.rs` moves through this host-level clean cutover; core keeps only pack-neutral APIs and the host crate owns wasm-bindgen surface types.

The buffer layout is public and versioned for this pack, but is not prematurely extracted into an engine-owned universal mesh crate. A second renderer pack may justify that extraction later.

### 10. Stock policy leaves core

The default `TouhouProfile` owns stock family radii, palette shades, generated disc/ring resource, sprite/beam styles, and material descriptors. `maku::host::style_rgb`, `style_rgb_hued`, and `dot_radius` cease to be core policy after native and web consumers migrate. This folds the palette portion of `gameplay-out-of-core` into this change; generic host primitives unrelated to Touhou remain in core.

## Risks / Trade-offs

- **[Risk] Multiple fixed sprite layouts complicate hosts.** → Three typed buffers avoid forcing recolor bandwidth onto fixed/tint-only bullets and remain directly representable as native slices and wasm typed-array views; v1 admits no arbitrary layouts.
- **[Risk] Material changes can create many draw commands.** → Preserve correctness first, coalesce adjacent compatible commands, and design stock profiles/atlases so common family/color variation stays one material.
- **[Risk] Custom textures use incompatible dimensions/UVs.** → Validate known resources at profile construction and make external resource metadata part of host registration.
- **[Risk] Strict unknown-style failures occur mid-frame.** → Resolve/cache at first occurrence, offer explicit fallback policy, and expose diagnostics so hosts can preflight representative cards.
- **[Risk] Macroquad lacks efficient instancing.** → Keep the pack output instanced; the macroquad adapter may expand/chunk as a compatibility host without forcing every host to pay that cost.
- **[Risk] Web wrapper relocation breaks consumers.** → Perform a clean host-level cutover with focused wasm API tests; do not leave duplicate palette/render paths.
- **[Risk] Internal style layers appear to reorder rows.** → Layers stay inside each row's command position and no global sort occurs.
- **[Risk] The API grows into a generic material engine.** → Keep a fixed Touhou sprite/strip ABI and opaque host pipeline keys; new arbitrary attributes require concrete evidence and a versioned follow-up.

## Migration Plan

1. Add profile/resource/material types and a stock profile that reproduces current outputs.
2. Replace `StyleTable` lookup and fix hue resolution to use the selected profile.
3. Add layered sprite resolution and directional transforms while retaining the old expanded-output path under tests.
4. Introduce sprite instances and ordered draw commands; migrate macroquad through a compatibility expansion adapter.
5. Migrate beam width/style configuration and indexed draw commands.
6. Add custom texture, tint, dynamic-recolor, additive material, fallback, and profile-validation fixtures.
7. Add the host-level wasm wrapper and typed-array/resource manifest, then delete core's duplicate Touhou web flattening.
8. Move stock palette/radius policy out of core and remove `StyleTable`, the single-atlas API, and bare `Span` after all consumers cut over.

