## ADDED Requirements

### Requirement: Genre-pack façade and validated profile
The Touhou renderer SHALL expose a genre-facing pack API constructed from an immutable host-provided profile declaring Touhou schema bindings, palettes, sprite styles, beam styles, texture resources, materials, world/pixel scale, explicit sprite/beam/color fallbacks, and unknown-style policy. Profile construction SHALL compile cold semantic configuration into stable ids and primitive recipes and SHALL reject duplicate keys, dangling ids, incompatible material/primitive/layout combinations, non-finite or non-positive geometry sizes, and invalid known-resource regions before rendering.

#### Scenario: Invalid material reference
- **WHEN** a sprite or ribbon layer references a material id absent from the profile
- **THEN** profile construction fails with a diagnostic identifying that layer and reference

#### Scenario: Profile replacement
- **WHEN** a host replaces a renderer profile
- **THEN** all schema, style, palette, primitive-recipe, and resource-resolution caches are cleared before the new profile renders a row

### Requirement: Kind handlers bind declared schemas
The pack SHALL bind each supported render kind's stable schema to a batch-aware kind handler. The pack's supported-kind manifest SHALL be the union of its installed handlers, and each binding SHALL validate required field names and kinds before frame construction. One handler MAY serve multiple render kinds, and one handler MAY compose multiple primitive processors.

#### Scenario: Touhou handler binding
- **WHEN** a loaded card declares compatible `:sprite` and `:beam` schemas
- **THEN** the pack binds them once to its sprite and beam handlers using their stable schema identities

#### Scenario: Incompatible declared field
- **WHEN** a declared `:beam` schema gives `width` a symbol kind rather than the numeric kind required by the pack
- **THEN** binding fails before the first frame with a diagnostic naming the kind and field

### Requirement: Shared pack output and ordered dispatch
All kind handlers in one pack SHALL emit into one pack-owned reusable output arena, resource/material registry, and ordered draw-command stream. Handler or primitive module boundaries SHALL NOT create independent frames, silently reorder frame items, or require independent host submissions.

#### Scenario: Cross-handler ordering
- **WHEN** a frame contains sprite A, beam B, then sprite A in emission order
- **THEN** sprite and beam handlers append commands to one shared stream in A, B, A order without merging across B

### Requirement: Batch-preserving handler execution
A kind handler SHALL consume compatible `RenderBatch` columns directly using a cached schema binding and SHALL NOT require expansion into `RenderRow` values. Row input and batch input SHALL use the same resolved primitive emitters and produce byte-identical output for equivalent content.

#### Scenario: Typed sprite batch
- **WHEN** a compatible compiled `:sprite` batch is rendered
- **THEN** its geometry and style columns are read in place without constructing one `RenderRow` per lane

#### Scenario: Batch equivalence
- **WHEN** one render batch and its equivalent sequence of rows are built with the same profile
- **THEN** their instance buffers, indexed geometry, material ids, and command order are byte-for-byte identical

### Requirement: Separate authored style axes
For a `:sprite` row, the Touhou handler SHALL resolve `(family, variant)` to a sprite recipe, resolve `color` independently to a palette entry, apply `hue` and `alpha` as per-row modifiers, and apply `scale` and style orientation policy to geometry. For a `:beam` row it SHALL resolve `(family, variant)` and `color` independently and apply row `width` to the selected beam recipe. The handler SHALL NOT concatenate or parse these axes into a composite style string.

#### Scenario: Same family with different colors
- **WHEN** two otherwise identical rows use the same family and variant but different color symbols
- **THEN** they resolve the same primitive recipe and different palette data

#### Scenario: Dynamic unknown style falls back
- **WHEN** a runtime family, variant, or color is absent and the profile selects fallback policy
- **THEN** the handler uses the profile's explicit corresponding fallback id and records an actionable diagnostic

#### Scenario: Dynamic unknown style is strict
- **WHEN** a runtime family, variant, or color is absent and the profile selects error policy
- **THEN** frame construction fails without silently substituting a built-in radius or color

### Requirement: Palette and declared color channels
A Touhou palette entry SHALL support named highlight, light, pure, dark, and outline shades. Each local primitive layer SHALL compile to fixed color, one-shade tint, or two-endpoint recolor behavior, and its material SHALL declare the matching fixed v1 layout. The renderer SHALL NOT populate or transfer a dynamic recolor channel for a material that does not declare it.

#### Scenario: Tint-only style
- **WHEN** a style's layers use only fixed or one-shade tint materials
- **THEN** no two-endpoint recolor instances are emitted for those layers

#### Scenario: Dynamic recolor style
- **WHEN** a layer selects low/high palette shades and a recolor material
- **THEN** its emitted source contains the hue/alpha-modified low/high colors and references that material

### Requirement: Primitive effect surfaces and compatibility
Each primitive processor SHALL declare a versioned effect surface consisting of its accepted source layouts, available channels, and supported composition operations. A primitive-coupled effect adapter SHALL declare its required capabilities and the primitive recipe layers or replacement operation it produces. Profile construction SHALL reject an effect whose requirements are not met. Primitive processors SHALL NOT maintain an enumeration of named effects.

#### Scenario: Compatible sprite halo
- **WHEN** a halo adapter requires sprite-layer duplication, size multiplication, tint, alpha-mask sampling, and an additive-compatible material and the sprite processor provides those capabilities
- **THEN** profile construction compiles the halo into ordinary ordered sprite layers

#### Scenario: Missing effect channel
- **WHEN** an effect requires a per-instance progress channel absent from every declared v1 layout
- **THEN** profile construction rejects the effect rather than appending an arbitrary custom payload

### Requirement: Ordered layered sprite geometry
A sprite recipe SHALL declare one or more ordered layers with material, texture region, size multiplier, angle offset, alpha multiplier, and color binding. The sprite processor SHALL emit every layer at that row's position in the shared command stream and SHALL NOT globally regroup layers or rows by material.

#### Scenario: Stock disc and outline
- **WHEN** the stock profile renders one dot
- **THEN** it emits the configured fill and outline layers in profile order with the stock visual dimensions and colors

#### Scenario: Additive glow layer
- **WHEN** a host profile adds a compatible sprite halo using an additive material
- **THEN** the compiled recipe emits an additional sprite source at that row position with the additive material id

### Requirement: Directional and radial sprite orientation
Each sprite recipe SHALL explicitly declare whether row `theta` is ignored or applied. Directional recipes SHALL apply row `theta` plus the layer angle offset; radial recipes SHALL use only their configured fixed offset.

#### Scenario: Directional family rotates
- **WHEN** two directional rows differ only in `theta`
- **THEN** their emitted instance rotations differ by the same angle

#### Scenario: Radial family ignores theta
- **WHEN** two radial rows differ only in `theta`
- **THEN** their emitted instance transforms are identical

### Requirement: Fixed sprite instance layouts
The v1 public frame ABI SHALL provide separate fixed-layout buffers for basic, tinted, and two-endpoint-recolor sprite instances. Every sprite draw command SHALL identify one of those layouts plus a contiguous range and a compatible material id. Arbitrary material-specific instance payloads SHALL NOT be accepted by this API version.

#### Scenario: One million basic sprites
- **WHEN** one million fixed or precolored sprite layers are rendered
- **THEN** their transform and UV data occupies the basic instance buffer without tint or recolor payload bytes

#### Scenario: Layout mismatch
- **WHEN** a sprite source layout is incompatible with its material declaration
- **THEN** profile validation or frame construction fails before host submission

### Requirement: Ordered layered ribbon geometry
A beam recipe SHALL declare one or more ordered ribbon layers. Each layer SHALL declare material and texture region, width multiplier, alpha multiplier, active/warning policy, and supported join/cap policy. The ribbon processor SHALL multiply each selected profile width by the row's `width` value and emit variable polyline geometry as indexed vertices and indices at that row's position in the shared command stream.

#### Scenario: Beam row width
- **WHEN** two otherwise identical beams have row widths `1` and `2`
- **THEN** every corresponding ribbon layer of the second beam is exactly twice the first beam's thickness

#### Scenario: Warning beam
- **WHEN** a beam row is inactive
- **THEN** every layer uses its compiled warning width and alpha policy without changing the authored point path

#### Scenario: Beam halo
- **WHEN** a compatible beam-glow adapter compiles to a wider additive ribbon followed by the body ribbon
- **THEN** both indexed layers are emitted in recipe order at the beam row's command position

### Requirement: Stable material and texture contract
The profile SHALL expose stable material ids and texture ids. Each material SHALL declare a host pipeline key, compatible primitive and fixed source layout, texture id, blend mode, and sampler description. Each texture SHALL provide either immutable builtin RGBA bytes plus dimensions or an opaque external resource key. The renderer SHALL NOT create GPU resources or perform asset I/O.

#### Scenario: External sprite sheet
- **WHEN** a style references a registered external texture resource
- **THEN** the frame contains only its stable material and texture ids plus UV data and the host resolves and uploads the resource

#### Scenario: Custom compatible shader
- **WHEN** a material names a custom host pipeline compatible with its declared standard source layout
- **THEN** the host can resolve that pipeline without the renderer accepting custom per-instance attributes

### Requirement: Effect scope boundary
Primitive-coupled layer and geometry effects SHALL compile into primitive recipes before hot frame construction. Group/offscreen and whole-frame effects, including screen-space bloom and color grading, SHALL remain host render-graph policy and SHALL NOT be represented as primitive flags or silently inserted draw queues by this pack.

#### Scenario: Local additive glow
- **WHEN** a bullet glow is expressible as a duplicate scaled sprite layer
- **THEN** the pack compiles it into the sprite recipe and ordinary material commands

#### Scenario: Screen-space bloom
- **WHEN** a host wants screen-space bloom over emissive materials
- **THEN** the host compositor owns render targets, blur passes, and composition without changing the mesh pack's geometry ABI

### Requirement: Ordered draw commands and safe coalescing
The frame SHALL contain one ordered command stream over all sprite-instance and indexed-geometry ranges. The renderer MAY coalesce only adjacent commands with the same source layout, material, and contiguous backing range. It SHALL preserve exact command and source order across handlers and equivalent row-at-a-time and batch inputs.

#### Scenario: Nonadjacent common material
- **WHEN** material A, then material B, then material A layers occur in emission order
- **THEN** the renderer emits three ordered command regions and does not merge the two A regions across B

#### Scenario: Adjacent common material
- **WHEN** consecutive rows emit compatible contiguous layers with the same material and layout
- **THEN** the renderer may represent them as one command covering both ranges

### Requirement: Reused frame storage
Frame construction SHALL clear and reuse all pack-owned buffers and return a borrow valid until the next build. After warm-up, rebuilding frames whose output fits reserved capacities SHALL perform no heap allocation for instances, vertices, indices, commands, schema binding, style lookup, palette lookup, hue resolution, or compiled recipe dispatch.

#### Scenario: Warm steady state
- **WHEN** the same bounded mixed-kind frame shape is built repeatedly after caches and capacities are warm
- **THEN** an allocation-counting test observes zero allocations during each build

### Requirement: Execution-plan visibility
Kind handlers and primitive processors SHALL retain batch column bindings, fixed output relationships where applicable, and explicit output source layouts so a later executor can plan compatible CPU, SIMD, or GPU work without reconstructing semantic rows. This API SHALL NOT require one heterogeneous GPU kernel per tick or make GPU support necessary for valid profiles.

#### Scenario: Fixed sprite planning
- **WHEN** a bound sprite batch uses a recipe with a fixed number of layers
- **THEN** an execution planner can determine its instance counts and destination layouts from the binding and compiled recipe without expanding rows

#### Scenario: Variable ribbon planning
- **WHEN** a beam batch or row has variable path cardinality
- **THEN** it may use a distinct ribbon sizing and emission pass without changing shared command ordering

### Requirement: Native and wasm host integration
Native and wasm hosts SHALL consume the same pack-owned material/resource ids, fixed instance layouts, indexed geometry, and ordered commands. A wasm host wrapper SHALL live above core, depend on both core and the Touhou pack, expose typed-array views into the engine module's linear memory, and expose the cold material/texture manifest. `crates/core` SHALL remain unaware of Touhou resources and SHALL NOT retain a duplicate palette/radius mesh-flattening path.

#### Scenario: Web frame submission
- **WHEN** JavaScript requests a rendered frame from the wasm host wrapper
- **THEN** it can read typed views for each nonempty instance buffer, vertices, indices, and draw commands and resolve every referenced material/resource without copying geometry through serialization

#### Scenario: Core dependency direction
- **WHEN** the workspace dependency graph is inspected after migration
- **THEN** the host wrapper depends on core and `mesh-touhou`, while core does not depend on either the wrapper or mesh pack

### Requirement: Stock Touhou policy ownership
The default Touhou profile SHALL own the stock palette shades, family radii, generated disc/ring texture resource, sprite and layered beam recipes, and material descriptors needed to reproduce the current prototype look. Core SHALL NOT export Touhou-specific palette, hue, or bullet-radius policy after native and web consumers migrate.

#### Scenario: Default profile visual contract
- **WHEN** existing cards render through the default profile
- **THEN** their stock family sizes, palette colors, fill/outline composition, beam widths, and emission ordering match the pre-migration renderer
