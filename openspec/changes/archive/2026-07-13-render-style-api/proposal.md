# Host-configurable Touhou render pack API

## Why

`crates/mesh-touhou` currently hardcodes one generated disc/ring atlas, core-owned palette/radius functions, white outlines, alpha-only material behavior, and expanded quad vertices. It also conflates Touhou schema/style policy, reusable sprite/ribbon geometry, and backend submission, making customization difficult and obscuring how another genre pack could reuse the implementation without adopting Touhou vocabulary.

## What Changes

- Recast `mesh-touhou` as a genre-facing render pack: its host API owns Touhou schema bindings, palettes, bullet/beam styles, resources, fallbacks, and validation, while internal batch-aware sprite and ribbon processors own geometry generation.
- Replace callback-based `StyleTable` with an immutable, validated `TouhouProfile`. Keep Maku's existing authored axes separate: `family + variant` select a recipe, `color` selects palette data, and row `hue`/`alpha` modify resolved colors.
- Compile cold semantic configuration into numeric schema bindings, style ids, primitive recipes, materials, and resources. The hot path consumes rows or typed batches without reinterpreting genre configuration or forcing batch expansion.
- Give the pack one shared reusable output arena and one ordered draw-command stream across all kind handlers. Internal module boundaries do not imply separate frames, submissions, or material stores.
- Express sprite and beam looks as ordered primitive-coupled layers. Stock fill/outline, sprite or beam halos, additive glow, shadows, and similar local effects become compiled recipe layers rather than hardcoded primitive features.
- Define capability-based effect composition: primitive processors expose versioned layouts and composition operations; effect adapters declare requirements; profile construction rejects incompatible combinations. Primitives do not enumerate named effects.
- Keep host shader integration material-driven. Opaque pipeline keys may select custom shaders only when they consume a declared standard sprite or strip ABI. Group/offscreen and whole-frame effects remain host compositor policy.
- Replace expanded point quads with compact fixed basic/tint/recolor sprite-instance streams while retaining indexed strip geometry for beams/polylines.
- Replace bare index `Span`s with ordered draw commands over shared sprite-instance or indexed-geometry ranges. Only adjacent compatible commands coalesce.
- Honor point `theta` for directional styles and row `width` for beam styles; radial stock styles may explicitly ignore orientation.
- Make texture upload, shader creation, blend/sampler state, render-target management, and material-id resolution host responsibilities. The pack exposes optional builtin RGBA resources and external resource keys but owns no GPU objects.
- Define a wasm-friendly buffer/resource ABI in a `crates/web` host crate above core. Native and web hosts consume the same pack-owned streams and manifest.
- Preserve row/batch equivalence, zero steady-state allocation, and future execution-plan visibility. The architecture may later lower compatible projection/primitive work to GPU compute without promising one heterogeneous kernel per tick.
- Move stock Touhou palette/radius data out of core host policy into the default profile.
- Do not extract a generic renderer crate in this change. A second concrete pack should establish which primitive/resource contracts are genuinely reusable before extraction.

## Capabilities

### New Capabilities

- `mesh-renderer-api`: Touhou render-pack configuration, schema-handler composition, primitive recipes and local effects, resources/materials, shared sprite/beam geometry output, ordered commands, and native/wasm host contracts.

### Modified Capabilities

None.

## Impact

- `crates/mesh-touhou/src/lib.rs`, its internal module organization, tests, and the macroquad player adapter.
- The existing `crates/web` directory becomes a host-level wasm crate/adapter depending on both core and `mesh-touhou`; `crates/core` remains unaware of the pack.
- Stock palette/radius helpers in `crates/core/src/host.rs` and duplicate web rendering in `crates/core/src/web.rs` migrate to host/profile ownership.
- Touhou render vocabulary remains library/package policy in `crates/core/lib/touhou.maku`; the pack binds its declared `:sprite` and `:beam` schemas to handlers.
- Governing boundaries remain `openspec/specs/render-rows/spec.md`, `openspec/specs/load-time-schema/spec.md`, `openspec/changes/ir-unification/design.md`, `openspec/changes/gpu-kernel-backend/design.md`, and `openspec/changes/gameplay-out-of-core/proposal.md`.
