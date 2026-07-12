# Entity representation flip: spec id + capture vector

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Per-row Rc-laden entities don't fly at 1M rows (scale target: ~10k normal, 100k–1M ceiling, decided 2026-07). Entity should become `(spec id, capture vector, state cells)`. Round 22 landed the first slice (capture vectors + interning for ClosedPt/Vel/RotExpr programs); the full flip — spec id replacing per-row node clones — remains, and is load-bearing on three paths: kernels, memory layout, offline card AOT.

## What Changes

- Replace per-entity `DynNode` clones with a spec id + per-entity capture vector + dense state cells.

## Capabilities

Representation-internal; behavior oracle-gated.

## Impact

- Entity spec store, spawn path, motion/pose walkers, sim SoA stores.
- Natural sequel to round 22; overlaps with `spec-store-dedup` and `group-integrator-dedup` (fold at pick-up if the design wants).
- Governing: `docs/notes/compiled-dyn-design.md`, `openspec/changes/entity-representation-flip/design.md` (SoA row-id target), scale-target consequences in the old TODO (now this stub).
