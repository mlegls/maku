# Host-configurable Touhou mesh renderer API

## Why

`proto/mesh-touhou` currently hardcodes one generated disc/ring atlas, core-owned palette/radius functions, white outlines, alpha-only material behavior, and expanded quad vertices. That preserves the prototype look but cannot express custom sprite sheets, palette families, directional sprites, additive glow, or efficient web/native instancing without host-specific forks.

## What Changes

- Replace callback-based `StyleTable` with an immutable, validated `TouhouProfile` containing palettes, `(family, variant)` sprite styles, beam styles, texture/resource descriptors, material descriptors, explicit fallbacks, and world/pixel scale policy.
- Keep Maku's existing style axes separate: `family + variant` select geometry/layers, `color` selects a profile palette entry, and per-row `hue`/`alpha` modify the resolved colors. Do not adopt DMK's concatenated style-string grammar.
- Express sprite looks as ordered layers referencing texture regions and opaque host material ids. The stock disc plus white outline becomes ordinary profile data; additive glow is another layer/material rather than an engine feature.
- Replace expanded point quads with compact fixed basic/tint/recolor sprite-instance streams while retaining indexed strip geometry for beams/polylines; materials declare which fixed layout they consume.
- Replace bare index `Span`s with an ordered draw-command stream over sprite-instance or indexed-geometry ranges plus material/resource ids. Commands preserve render-frame emission order and only adjacent compatible commands coalesce.
- Honor point `theta` for directional styles and row `width` for beam styles; radial stock styles may explicitly ignore orientation.
- Make texture upload, shader creation, blend state, sampler state, and material-id resolution host responsibilities. The pack exposes optional builtin RGBA resources and external resource keys but never owns GPU objects.
- Move stock Touhou palette/radius data out of core host policy into the default mesh profile, resolving the overlapping palette slice in `gameplay-out-of-core`.
- Define a wasm-friendly buffer/resource ABI in a `proto/web` host crate above core: Rust geometry remains in the engine host module, while JavaScript consumes typed-array views and maps material/resource ids to its pipelines. The core engine remains unaware of the pack.
- Preserve row/batch geometry equivalence, zero steady-state allocation through buffer reuse, and the ordered render-frame contract.
- Exclude global z/render-queue sorting, arbitrary material-specific vertex layouts, asset loading, engine render-schema changes, and styling APIs for non-Touhou renderer packs.

## Capabilities

### New Capabilities

- `mesh-renderer-api`: Touhou profile configuration, resources/materials, sprite/beam geometry output, ordered draw commands, fallback behavior, and native/wasm host integration contracts.

### Modified Capabilities

None.

## Impact

- `proto/mesh-touhou/src/lib.rs`, its tests, and the macroquad player adapter.
- The existing `proto/web` directory becomes a host-level wasm crate/adapter that depends on both core and `mesh-touhou`; `proto/core` must not depend on the pack.
- Stock palette/radius helpers currently in `proto/core/src/host.rs` and duplicate web rendering in `proto/core/src/web.rs` migrate to host/profile ownership through a clean cutover.
- Touhou render vocabulary remains library policy in `cards/lib/touhou.maku`; no `openspec/specs/render-rows/spec.md` requirement changes are needed because the pack consumes the existing ordered typed frame.
- Governing boundaries: `openspec/specs/render-rows/spec.md`, `openspec/specs/load-time-schema/spec.md`, and `openspec/changes/gameplay-out-of-core/proposal.md`.
