# Rendering style API redesign (post-DMK-study)

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The mesh pack (`proto/mesh-touhou`, landed round 22) is a direct port of the old immediate-mode player's look: stock `style_rgb` palette + hue shift, one hardcoded disc/ring atlas, fixed white-outline treatment, `StyleTable` as the only configuration surface. That was the right first step (visual parity, batched), but it was never a designed styling API. Before designing one, study how DMK/Danmokou actually structures its style/rendering schema (bullet style families and recolor palettes, materials/shaders, additive glow, z-layering) — the current touhou schema vocabulary (`family`/`color`/`hue`/`alpha`) was also basically inherited rather than designed (2026-07).

## What Changes

- Research phase: a written survey of DMK's style/rendering model (style naming, recolorable palettes, texture/material pipeline, glow/additive blending, layers) and what of it Maku wants; extend or contradict `docs/from-dmk.md` as needed.
- API redesign candidates for the pack (and schema where needed):
  - custom palettes and shapes (user/host-defined, not the stock table);
  - host-provided textures (atlas injection or per-family sprites instead of the generated disc/ring);
  - shader effects — glow/additive blending, which likely means the pack emitting material/blend metadata per span rather than plain quads;
  - whatever layering/blend vocabulary the survey justifies.
- Decide what is pack configuration vs render-schema vocabulary vs engine frame semantics.
- Web packaging (decided direction 2026-07): a renderer pack on wasm is two-part — the Rust geometry crate compiles INTO the engine wasm module (MeshFrame buffers exposed zero-copy as typed-array views over linear memory; a separate wasm module would need shared-memory gymnastics for nothing), plus a JS pipeline module (shader, blend state, texture upload, draw-per-span). Shader effects like glow/additive live in the JS pipeline half; the Rust half emits per-span material/blend metadata. Open sub-question: make the wasm export surface GENERIC (one engine-owned MeshFrame export + span material tags, packs = geometry crate + JS pipeline + config) vs per-pack bespoke bindings — generic is the frame-boundary-consistent default unless per-instance shader data forces bespoke layouts.

## Capabilities

To be finalized at pick-up; likely a `render-styling` capability, plus MODIFIED deltas against `render-rows` only if frame semantics (layers/blend) change.

## Impact

- `proto/mesh-touhou`, `proto/player`, possibly web host; schema vocabulary in `cards/lib/touhou.maku`.
- Architecture constraint (decided): mesh renderers are hosts — styling stays pack/host policy; the engine's obligation ends at the typed frame (`docs/notes/render-output-design.md`). Palette tables moving behind host/profile config is tracked in `gameplay-out-of-core`; per-kind schemas and a mesh/sprite-batch kind in `render-schema-per-kind` — this change should sequence with those rather than duplicate them.
- Current pack behavior/geometry rules: `docs/notes/mesh-renderer-spec.md`.
