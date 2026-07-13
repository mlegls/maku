## Context

`maku_mesh_touhou::TouhouMesh` currently consumes the public ordered render frame and returns reused `MeshFrame { vertices, indices, spans }` buffers plus one generated disc/ring atlas. `StyleTable` is three callbacks/values (`palette`, `dot_radius`, `px_per_unit`); dot geometry always emits a palette-tinted disc plus white outline, nonzero hue bypasses the injected palette through `maku::host::style_rgb_hued`, `variant` and point `theta` are ignored, and beam width is hardcoded. The macroquad host assumes every span uses the same atlas/material. `proto/core/src/web.rs` independently repeats palette/radius rendering instead of consuming the pack.

The current package also collapses three concerns: Touhou's semantic schema/style vocabulary, reusable sprite/ribbon geometry algorithms, and backend submission contracts. This change keeps one vertical `mesh-touhou` package but makes those boundaries explicit. The host-facing API is a controlled Touhou façade; internal kind handlers compile semantic rows/batches into primitive recipes and append to one shared output. This is not evidence yet for a separately versioned universal renderer crate.

The engine boundary is already settled by `openspec/specs/render-rows/spec.md`: mesh renderers are optional hosts over an ordered typed frame, style vocabulary is pack/library policy, and engine transport contains no texture/material semantics. The recent per-kind schema work provides stable `:sprite` and `:beam` layouts without requiring another core change. `openspec/changes/ir-unification/design.md` and `openspec/changes/gpu-kernel-backend/design.md` additionally require fixed render projection to remain visible as typed plans/columns; pack modularity must not force semantic row reconstruction or make one module equal one physical GPU dispatch.

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

- A genre-facing Touhou pack API for palettes, sprite families/variants, texture regions, layered beam looks, and alpha/additive materials.
- Batch-aware kind handlers over stable `:sprite` and `:beam` schemas, backed by reusable sprite/ribbon processors and one shared output arena/command stream.
- A stock profile that reproduces current disc/outline and beam behavior as compiled data.
- Compact instanced point output and indexed variable geometry behind one ordered draw-command stream.
- Opaque, stable resource/material ids suitable for native and wasm hosts.
- Primitive effect surfaces and validated adapters for local effects such as sprite or beam halos without hardcoding named effects into primitive processors.
- Explicit profile/schema validation, unknown-style behavior, and custom-pipeline compatibility against fixed layouts.
- Steady-state buffer reuse, no per-row string/Rc allocation, and retained batch/plan structure for future CPU/SIMD/GPU execution.

**Non-Goals:**

- Changing engine render kinds, schemas, or emission-order semantics.
- One package or physical dispatch per render kind; handlers are logical composition boundaries over shared execution/output state.
- Global z/render-queue sorting in the mesh pack.
- Group/offscreen effects, screen-space bloom, color grading, or render-target orchestration; those belong to a host compositor.
- GPU texture/shader creation, asset I/O, or GPU mesh-kernel implementation in this change.
- A guarantee that a whole tick compiles to one heterogeneous GPU kernel.
- Arbitrary material-specific vertex/instance layouts in v1.
- Extracting a universal mesh crate before a second concrete render pack establishes demonstrated commonality.
- Moving the pack into `proto/core` or making core depend on it.
- DMK-compatible style-string parsing.

## Decisions

### Treat `mesh-touhou` as a pack façade over internal primitive processors

The public cold API speaks Touhou policy: schema bindings, `(family, variant, color)` styles, palette ramps, beam active/warning state, fallbacks, and resources. Profile construction compiles that configuration to numeric ids, cached schema bindings, and ordinary sprite/ribbon recipes. The hot API consumes the engine frame and exposes only fixed buffers, resource/material manifests, and ordered commands.

Internally, a sprite handler and beam handler bind compatible stable schemas and call sprite-instance and ribbon processors. The pack, not either processor, owns the frame buffers, resource registry, and command builder. A handler may support multiple kinds and may compose several processors; a primitive processor may be reused by several handlers. Module boundaries therefore do not imply separate frames or submissions.

```text
RenderFrame
    -> bound kind handler
    -> compiled primitive recipe
    -> shared sprite/ribbon emitters
    -> one MeshFrame + ordered command stream
```

Alternative: expose the primitive API directly as the normal host API. Rejected because hosts using the Touhou pack should configure Touhou semantics and receive Touhou validation rather than manually reproduce its schema mapping. Alternative: extract a generic crate now. Rejected because one pack cannot establish which palette, effect, recipe, or ABI concepts are genuinely cross-pack.

### Bind schemas once and preserve batches and planning information

At load/profile binding, each supported kind validates required field names/kinds and records column indices against stable `RenderSchema` identity. A compatible `RenderBatch` is read in place; it is never expanded merely to cross a handler boundary. Rows and batches call the same resolved emitters.

Handlers expose fixed recipe cardinality and destination layouts where known; ribbons retain an explicit sizing/emission seam for variable path output. This permits later CPU, SIMD, or GPU planners to fuse compatible work or schedule specialized passes. It does not require one kernel per module or one heterogeneous kernel per tick. The useful GPU objective is resident typed buffers and one planned command submission, not an artificially universal shader.

Alternative: opaque row-at-a-time renderer traits. Rejected because they discard the column transport, stable schema bindings, fixed-output planning, and GPU-residency opportunities established by `render-rows` and the kernel changes.

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

`OrientationPolicy` is radial/ignore-theta or directional/use-theta. The stock disc and white outline are two layers over the generated atlas. A glow, shadow, or outline is an additional layer referencing a compatible material. A custom sprite sheet is another texture region. Layer order is preserved within the row; no global style sorting occurs.

The sprite processor exports an effect surface—standard source layouts plus layer duplication, scale/offset, color/alpha binding, material replacement, and insertion order—not a list of named effects. A pack-level effect adapter such as a soft halo declares the capabilities it needs and compiles to ordinary layers during profile construction. Missing channels require rejection or a later versioned standard layout, never an arbitrary payload.

Alternative: one style equals one texture/material. Rejected because the current look already needs fill plus outline and glow/additive composition should not require card-level duplicate rows. Alternative: `supports_glow`/`supports_shadow` flags on the primitive. Rejected because named-effect enumeration couples an open effect vocabulary to the geometry implementation.

### 5. Beam styles are separate layered profile data

`BeamStyle` declares ordered `RibbonLayer`s. Each layer owns a material/texture region, width and alpha multipliers, active and warning base policy, and supported join/cap policy. The row's schema-provided `width` multiplies every resolved layer width; `active` chooses active versus warning appearance. The existing hardcoded 6px/1.5px values become stock-profile defaults.

This gives ribbons the same compositional local-effect model as sprites. A soft beam glow can compile to a wider additive ribbon followed by the body ribbon. The ribbon processor exports duplication, width scaling, material replacement, tint/alpha, UV, and insertion operations; an effect adapter declares which it requires. It does not claim that a sprite shader automatically consumes strip vertices.

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

Exact owned/borrowed storage may vary, but the contract is: the pack exposes stable ids and optional builtin bytes; hosts load/upload external resources and create pipelines. `MaterialId` in a frame is an index into the immutable profile manifest. Alpha and additive blend modes are standard descriptors; custom pipeline keys let a host substitute shaders that consume the declared standard v1 instance/vertex ABI. Compatibility is checked from the primitive/source-layout contract, not from a claim that one shader applies to every topology.

Effects have explicit scope. Material/layer effects and primitive-coupled geometry effects compile into recipes before the hot path. Group/offscreen effects need bounds, intermediate targets, and command grouping; whole-frame effects such as bloom need a render graph. Those scopes remain host compositor policy rather than primitive flags or hidden draw queues in this pack.

Materials requiring additional per-instance attributes are out of v1. Add a versioned standard stream or a separate renderer pack after a concrete schema requires it; do not reserve a permanently bloated generic payload.

### 8. Preserve zero-allocation steady state

`TouhouMesh::build` clears and reuses every shared frame vector. Schema caches resolve relevant column indices once per stable `Rc<RenderSchema>`. Symbol-to-style/palette caches retain interned/shared keys without `Rc::from` per row. Hue/ramp results memoize on resolved ids and hue bucket. Effect adapters have already compiled to immutable primitive recipes. Building returns a borrow valid until the next build, as today.

Strict unknown-style errors occur only on first resolution; fallback mode resolves once to explicit fallback ids. Profile/resource mutation during build is prohibited.

### 9. Wasm integration lives in the existing `proto/web` host directory above core

`mesh-touhou` already depends on core, so core cannot depend back on the pack. `proto/web` becomes a workspace crate in addition to retaining its static/editor assets and build script; its Rust library depends on both core and `mesh-touhou`, owns `Instance` plus `TouhouMesh`, builds once per render frame, and exports typed-array views over sprites, strip vertices, indices, and packed draw commands in the same wasm linear memory. It also exports the cold material/texture manifest and builtin resource bytes. `proto/web/build.sh` builds this host crate rather than core directly.

JavaScript maps material/pipeline/resource ids to WebGL/WebGPU objects and performs draw calls. Rust geometry remains in the same wasm module as the engine; a second wasm module would add memory sharing/copying without an ownership benefit. Existing direct palette/radius flattening in `proto/core/src/web.rs` moves through this host-level clean cutover; core keeps only pack-neutral APIs and the host crate owns wasm-bindgen surface types.

The buffer layout is public and versioned for this pack, but is not prematurely extracted into an engine-owned universal mesh crate. A second renderer pack may justify that extraction later.

### 10. Stock policy leaves core

The default `TouhouProfile` owns stock family radii, palette shades, generated disc/ring resource, sprite/beam styles, and material descriptors. `maku::host::style_rgb`, `style_rgb_hued`, and `dot_radius` cease to be core policy after native and web consumers migrate. This folds the palette portion of `gameplay-out-of-core` into this change; generic host primitives unrelated to Touhou remain in core.

## Risks / Trade-offs

- **[Risk] Logical handler modules become per-row callbacks or physical dispatch boundaries.** → Bind schemas once, consume batches directly, share output/planning state, and keep module composition independent from CPU/GPU scheduling.
- **[Risk] Capability descriptors become a generic effect engine.** → Limit v1 to concrete sprite/ribbon operations and fixed layouts needed by this pack; named semantic effects compile in pack policy and unsupported channels fail validation.
- **[Risk] Multiple fixed sprite layouts complicate hosts.** → Three typed buffers avoid forcing recolor bandwidth onto fixed/tint-only bullets and remain directly representable as native slices and wasm typed-array views; v1 admits no arbitrary layouts.
- **[Risk] Layered ribbons and materials create many draw commands.** → Preserve correctness first, coalesce adjacent compatible commands, and design stock recipes/atlases so common variation stays on few materials.
- **[Risk] Custom textures use incompatible dimensions/UVs.** → Validate known resources at profile construction and make external resource metadata part of host registration.
- **[Risk] Strict unknown-style failures occur mid-frame.** → Resolve/cache at first occurrence, offer explicit fallback policy, and expose diagnostics so hosts can preflight representative cards.
- **[Risk] Macroquad lacks efficient instancing.** → Keep the pack output instanced; the macroquad adapter may expand/chunk as a compatibility host without forcing every host to pay that cost.
- **[Risk] Web wrapper relocation breaks consumers.** → Perform a clean host-level cutover with focused wasm API tests; do not leave duplicate palette/render paths.
- **[Risk] Internal style/effect layers appear to reorder rows.** → Layers stay inside each row's command position and no global sort occurs.
- **[Risk] Future GPU work is constrained by the host façade.** → Compile semantics to numeric recipes, retain typed batch bindings and explicit output cardinality/layouts, and permit multiple specialized resident dispatches rather than requiring one universal kernel.
- **[Risk] The API grows into a generic material engine.** → Keep a fixed Touhou sprite/strip ABI and opaque host pipeline keys; new arbitrary attributes or render-graph scopes require concrete evidence and a versioned follow-up.

## Migration Plan

1. Establish the pack/profile, schema-binding, primitive-recipe, capability, resource/material, and shared-output types inside `mesh-touhou`; do not extract a generic crate.
2. Build a stock profile that compiles current dots and beams to ordered sprite/ribbon layers and reproduces current outputs.
3. Replace `StyleTable` lookup, fix hue resolution, and bind `:sprite`/`:beam` rows and batches to cached handlers.
4. Add layered sprite/ribbon resolution and compatible local-effect fixtures while retaining the old expanded-output path under tests.
5. Introduce sprite instances, indexed ribbons, and one shared ordered command stream; migrate macroquad through a compatibility expansion adapter.
6. Add custom texture, tint, dynamic-recolor, additive material, effect-capability, fallback, schema-binding, and profile-validation fixtures.
7. Add the host-level wasm wrapper and typed-array/resource manifest, then delete core's duplicate Touhou web flattening.
8. Move stock palette/radius policy out of core and remove `StyleTable`, the single-atlas API, and bare `Span` after all consumers cut over.

## Open Questions

- Whether the eventual second render pack demonstrates enough identical sprite/ribbon/resource contracts to justify extraction into a shared crate; this change deliberately leaves the seam internal.
- Which host compositor API should later represent grouped offscreen effects and emissive/bloom passes; primitive recipe flags are explicitly not the answer.
- Whether future GPU mesh compilation consumes GPU-resident render columns directly or uses host-provided device handles; preserve planning information here and decide orchestration with measured backend work.

