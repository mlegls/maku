# Collision restructure: per-layer streaming passes

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Collision index capture is ~14% of step (AABB build, memory-bound) and the general AABB index doesn't scale to the 100k–1M ceiling. Player-hit/graze are N-vs-few point tests: ~1–2ms for 1M in f32 SIMD, no index needed.

## What Changes

- Separate collider PROJECTION from contact generation: `ir-unification` lowers fixed figure/meta → collider-column expressions into `ColliderProjectionPlan`s; this change consumes those columns and owns per-layer enumeration, pair tests, fact ordering, and event production.
- Replace general AABB capture with deterministic CPU per-layer streaming passes for the N-vs-few layers.
- If projection or motion runs on GPU, collision-required pose/collider columns must be CPU-visible at the collision tick boundary through explicit readback or a measured mirror. “Only contact events read back” applies only to a future GPU contact-generation tier, which is not part of this change.

## Capabilities

Collision-internal; layer routing semantics unchanged.

## Impact

- `proto/core/src/sim/collision.rs`.
- Benefits from `f32-hot-columns`; a blocking-laser geometry class (`blocking-lasers`) should fit this design.
- `gpu-kernel-backend` may execute bounded fixed collider projection, but initial GPU support does not own collision streaming/contact generation.
