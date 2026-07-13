## ADDED Requirements

### Requirement: Validated host profile
The Touhou mesh renderer SHALL be constructed from an immutable host-provided profile that declares palettes, sprite styles, beam styles, texture resources, materials, world/pixel scale, explicit sprite/beam/color fallbacks, and unknown-style policy. Profile construction SHALL reject duplicate keys, dangling ids, incompatible material/primitive/instance-layout combinations, non-finite or non-positive geometry sizes, and invalid known-resource regions before rendering.

#### Scenario: Invalid material reference
- **WHEN** a sprite layer references a material id absent from the profile
- **THEN** profile construction fails with a diagnostic identifying that layer and reference

#### Scenario: Profile replacement
- **WHEN** a host replaces a renderer profile
- **THEN** all schema, style, palette, and resource-resolution caches are cleared before the new profile renders a row

### Requirement: Separate authored style axes
For a `:sprite` row, the renderer SHALL resolve `(family, variant)` to a sprite style, resolve `color` independently to a palette entry, apply `hue` and `alpha` as per-row modifiers, and apply `scale` and style orientation policy to geometry. For a `:beam` row it SHALL resolve `(family, variant)` and `color` independently and apply row `width` to the selected beam style. The renderer SHALL NOT concatenate or parse these axes into a composite style string.

#### Scenario: Same family with different colors
- **WHEN** two otherwise identical rows use the same family and variant but different color symbols
- **THEN** they resolve the same geometry/layer recipe and different palette data

#### Scenario: Dynamic unknown style falls back
- **WHEN** a runtime family, variant, or color is absent and the profile selects fallback policy
- **THEN** the renderer uses the profile's explicit corresponding fallback id and records an actionable diagnostic

#### Scenario: Dynamic unknown style is strict
- **WHEN** a runtime family, variant, or color is absent and the profile selects error policy
- **THEN** frame construction fails without silently substituting a built-in radius or color

### Requirement: Palette and declared color channels
A palette entry SHALL support named highlight, light, pure, dark, and outline shades. Each sprite layer SHALL declare fixed color, one-shade tint, or two-endpoint recolor behavior, and its material SHALL declare the matching fixed v1 instance layout. The renderer SHALL NOT populate or transfer a dynamic recolor channel for a material that does not declare it.

#### Scenario: Tint-only style
- **WHEN** a style's layers use only fixed or one-shade tint materials
- **THEN** no two-endpoint recolor instances are emitted for those layers

#### Scenario: Dynamic recolor style
- **WHEN** a layer selects low/high palette shades and a recolor material
- **THEN** its instance contains the hue/alpha-modified low/high colors and references that material

### Requirement: Ordered layered sprite geometry
A sprite style SHALL declare one or more ordered layers with material, texture region, size multiplier, angle offset, alpha multiplier, and color binding. The renderer SHALL emit every layer at that row's position in the frame command stream and SHALL NOT globally regroup layers or rows by material.

#### Scenario: Stock disc and outline
- **WHEN** the stock profile renders one dot
- **THEN** it emits the configured fill and outline layers in profile order with the stock visual dimensions and colors

#### Scenario: Additive glow layer
- **WHEN** a host profile adds a glow layer using an additive material
- **THEN** the renderer emits an additional draw source at that row position with the additive material id

### Requirement: Directional and radial sprite orientation
Each sprite style SHALL explicitly declare whether row `theta` is ignored or applied. Directional styles SHALL apply row `theta` plus the layer angle offset; radial styles SHALL use only their configured fixed offset.

#### Scenario: Directional family rotates
- **WHEN** two directional rows differ only in `theta`
- **THEN** their emitted instance rotations differ by the same angle

#### Scenario: Radial family ignores theta
- **WHEN** two radial rows differ only in `theta`
- **THEN** their emitted instance transforms are identical

### Requirement: Fixed sprite instance layouts
The v1 public frame ABI SHALL provide separate fixed-layout buffers for basic, tinted, and two-endpoint-recolor sprite instances. Every sprite draw command SHALL identify one of those layouts plus a contiguous range and a compatible material id. Arbitrary material-specific instance payloads SHALL NOT be accepted by this API version.

#### Scenario: One million basic sprites
- **WHEN** one million fixed/precolored sprite layers are rendered
- **THEN** their transform/UV data occupies the basic instance buffer without tint or recolor payload bytes

#### Scenario: Layout mismatch
- **WHEN** a draw source layout is incompatible with its material declaration
- **THEN** profile validation or frame construction fails before host submission

### Requirement: Styled beam geometry
Beam styles SHALL declare material/texture region, active and warning widths/alpha, and supported join/cap policy. The renderer SHALL multiply the selected profile width by the row's `width` value and SHALL emit variable polyline geometry as indexed vertices/indices.

#### Scenario: Beam row width
- **WHEN** two otherwise identical beams have row widths `1` and `2`
- **THEN** the second beam's strip thickness is exactly twice the first beam's thickness

#### Scenario: Warning beam
- **WHEN** a beam row is inactive
- **THEN** it uses the profile's warning width/alpha policy without changing the authored point path

### Requirement: Stable material and texture contract
The profile SHALL expose stable material ids and texture ids. Each material SHALL declare a host pipeline key, compatible primitive and instance layout, texture id, blend mode, and sampler description. Each texture SHALL provide either immutable builtin RGBA bytes plus dimensions or an opaque external resource key. The renderer SHALL NOT create GPU resources or perform asset I/O.

#### Scenario: External sprite sheet
- **WHEN** a style references a registered external texture resource
- **THEN** the frame contains only its stable material/texture ids and UV region; the host resolves and uploads the resource

#### Scenario: Additive material
- **WHEN** a draw command references an additive material
- **THEN** the host can determine the required blend state from the cold material manifest without inspecting card values

### Requirement: Ordered draw commands and safe coalescing
The frame SHALL contain one ordered command stream over sprite-instance and indexed-geometry ranges. The renderer MAY coalesce only adjacent commands with the same source layout, material, and contiguous backing range. It SHALL preserve the exact command, instance, vertex, and index order produced by equivalent row-at-a-time and batch-expanded inputs.

#### Scenario: Nonadjacent common material
- **WHEN** material A, then material B, then material A layers occur in emission order
- **THEN** the renderer emits three ordered command regions and does not merge the two A regions across B

#### Scenario: Adjacent common material
- **WHEN** consecutive rows emit compatible contiguous layers with the same material and layout
- **THEN** the renderer may represent them as one command covering both ranges

#### Scenario: Batch equivalence
- **WHEN** one render batch and its equivalent sequence of rows are built with the same profile
- **THEN** their instance buffers, indexed geometry, material ids, and command order are byte-for-byte identical

### Requirement: Reused frame storage
Frame construction SHALL clear and reuse renderer-owned buffers and return a borrow valid until the next build. After warm-up, rebuilding frames whose output fits previously reserved capacities SHALL perform no heap allocation for instances, vertices, indices, commands, style lookup, palette lookup, or hue resolution.

#### Scenario: Warm steady state
- **WHEN** the same bounded frame shape is built repeatedly after all caches and capacities are warm
- **THEN** an allocation-counting test observes zero allocations during each build

### Requirement: Native and wasm host integration
Native and wasm hosts SHALL consume the same pack-owned material/resource ids, fixed instance layouts, indexed geometry, and ordered commands. A wasm host wrapper SHALL live above core, depend on both core and the Touhou mesh pack, expose typed-array views into the engine module's linear memory, and expose the cold material/texture manifest. `proto/core` SHALL remain unaware of Touhou resources and SHALL NOT retain a duplicate palette/radius mesh-flattening path.

#### Scenario: Web frame submission
- **WHEN** JavaScript requests a rendered frame from the wasm host wrapper
- **THEN** it can read typed views for each nonempty instance buffer, vertices, indices, and draw commands and resolve every referenced material/resource from the exported manifest without copying geometry through serialization

#### Scenario: Core dependency direction
- **WHEN** the workspace dependency graph is inspected after migration
- **THEN** the host wrapper depends on core and `mesh-touhou`, while core does not depend on either the wrapper or mesh pack

### Requirement: Stock Touhou policy ownership
The default Touhou profile SHALL own the stock palette shades, family radii, generated disc/ring texture resource, sprite/beam styles, and material descriptors needed to reproduce the current prototype look. Core SHALL NOT export Touhou-specific palette, hue, or bullet-radius policy after native and web consumers migrate.

#### Scenario: Default profile visual contract
- **WHEN** existing cards render through the default profile
- **THEN** their stock family sizes, palette colors, fill/outline composition, beam widths, and emission ordering match the pre-migration renderer
