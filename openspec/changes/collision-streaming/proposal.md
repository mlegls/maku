# Collision restructure: per-layer streaming passes

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Collision index capture is ~14% of step (AABB build, memory-bound) and the general AABB index doesn't scale to the 100k–1M ceiling. Player-hit/graze are N-vs-few point tests: ~1–2ms for 1M in f32 SIMD, no index needed.

## What Changes

- Replace general AABB capture with per-layer streaming passes for the N-vs-few layers.
- Keep gameplay collision on the deterministic CPU tier even if positions/render move to GPU (only contact EVENTS read back).

## Capabilities

Collision-internal; layer routing semantics unchanged.

## Impact

- `proto/core/src/sim/collision.rs`.
- Benefits from `f32-hot-columns`; a blocking-laser geometry class (`blocking-lasers`) should fit this design.
