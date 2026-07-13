# Tasks

Sequencing: implementation touches `interp/schema.rs` and `sim/` — hold
until scoped-channel-overrides' working tree lands.

## 1. Per-kind schema store

- [x] 1.1 Scope the render field schema per kind (`kind → key → RenderFieldKind`); `RenderRow`/`RenderBatch` carry the kind as a distinguished slot (`:default` when unset). Staged batch validation indexes by kind; within-kind conflicts keep the exact abort-and-rerun behavior. Behavior-preserving for single-kind worlds.

## 2. Declaration and negotiation

- [x] 2.1 `defrender-kind` collected by the load-time pass (geometry class, field table, identity); declared-kind tables are fixed at load — emissions check against them, new keys are schema errors with the standard error surface.
- [x] 2.2 `Sim::verify_render_kinds` (next to `verify_host_channels`): host manifest in, load failure naming unsupported declared kinds, lint for undeclared kinds under a strict host. Wire the native player and wasm host.

## 3. Adapter

- [ ] 3.1 `render-adapt`: registration-time kind/key rewriting + field pick over wrapped rules; remap folds into the memoized `RenderSchema` (no per-row compiled cost); downstream sees only the post-adapter world.

## 4. First consumer

- [ ] 4.1 Touhou lib declares its kinds (`:sprite` over point geometry with family/color/variant/scale; the beam polyline kind); mesh-touhou negotiates for them and reads declared schemas instead of probe-and-default (keep ignore/default for undeclared kinds).

## 5. Gates and sync

- [ ] 5.1 Tests: per-kind scoping (same key, two kinds), declared-kind load fixing + new-key error, manifest failure/lint paths, adapter end-to-end (imported kind renders as local), batch/row equivalence under kinds. Full gate: `cargo test --release --manifest-path proto/core/Cargo.toml` + the 4 ignored oracle card suites. Commit each coherent change-set.
- [ ] 5.2 Archive-time spec sync: render-rows and load-time-schema deltas land in `openspec/specs/`; drop the "future work" note from the accretion requirement.
