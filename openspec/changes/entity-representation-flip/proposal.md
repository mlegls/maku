# Entity representation flip: spec id + capture vector

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Per-row Rc-laden entities don't fly at 1M rows (scale target: ~10k normal, 100k–1M ceiling, decided 2026-07). Entity should become `(spec id, capture vector, state cells)`. Round 22 landed the first slice (capture vectors + interning for ClosedPt/Vel/RotExpr programs); the full flip — spec id replacing per-row node clones — remains, and is load-bearing on three paths: kernels, memory layout, offline card AOT.

## What Changes

- Replace per-entity `DynNode` clones with a spec id + per-entity capture vector + dense state cells.
- Make the spec table the owner of stable `KernelPlanId`/`KernelProgramId` references, state schemas, projector/render schemas, and cache policy; rows retain only `spec_id`, captures, typed fields/state, epochs, and generation identity.
- Replace pointer identity in batch grouping, motion state lookup, and projector memoization with explicit spec/program/plan identity.

## Capabilities

Representation-internal; behavior oracle-gated.

## Impact

- Entity spec store, spawn path, motion/pose walkers, sim SoA stores.
- Natural sequel to round 22; overlaps with `spec-store-dedup` and `group-integrator-dedup` (fold at pick-up if the design wants).
- `spec-store-dedup` folds into this change: deduplicated cold specs are the spec-id table, not a parallel sharing mechanism. `group-integrator-dedup` coordinates with the new state/group identity but remains an optional later execution optimization.
- `ir-unification` owns program execution and plan bindings; this change owns entity/spec storage identity. Neither requires a universal typed semantic IR.
- Governing: `openspec/specs/lowering/spec.md`, `openspec/changes/entity-representation-flip/design.md` (SoA row-id target), scale-target consequences in the old TODO (now this stub).
