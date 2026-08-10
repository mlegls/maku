# Entity representation flip: spec id + capture vector

## Why

Per-row Rc-laden entities don't fly at 1M rows (scale target: ~10k
normal, 100k–1M ceiling, decided 2026-07). Every spawned element today
clones its figure's node spine (`instantiate_rand`), allocates a
content-identical `MotionStateSchema` (four hashmaps + an Rc per row),
and gets a fresh `dyn_cols` Rc — even when the whole group is one
template. Motion state identity is minted from `Rc::as_ptr` interning,
which also leaves a cross-tick raw-pointer row-class cache (latent ABA)
and an O(live-entities) node-carrier scan in the executor. Round 22
landed the program half (capture-slot programs, structural interning,
explicit `MotionProgramIdentity` batch keys); the storage half — spec id
replacing per-row node clones — is the remaining prerequisite for the
1M-row layout, f32 hot columns, and JIT data-parallel entity loops.

## What Changes

- Introduce a world-owned, refcounted `EntitySpec` table: cold per-group
  data (figure template, motion state schema, dyn cols, collider
  projectors, cache policy, capture layout) stored once per spec; rows
  hold a generational `spec_id` plus one per-row capture vector and the
  existing dense state columns. The six per-row `EntitySpecStore`
  columns collapse into that one column.
- Motion state schemas become per-spec: node-id maps are built once over
  the spec's shared tree, not per entity. `instantiate_rand` stops
  cloning node spines on the compiled path — rand draws fill the row's
  capture vector; the ambient spawn frame becomes per-row data instead
  of a per-spawn wrapper node.
- Batch grouping, row classification, motion state lookup, and
  dyn-field/projector memoization key explicit spec/program/plan
  identity instead of `Rc<DynNode>` / `Rc<[..]>` pointer identity.
- Culled rows release their spec reference (today they pin the tree
  until slot reuse); spec entries free at refcount zero with
  generation-bumped id reuse, so spawn-loop cards stay bounded.
- `spec-store-dedup` folds in: the spec table IS the dedup mechanism
  (front-end template memo per site; structural cross-site fusing
  remains an optional later widening lever, since batch-lane fusing
  already comes from program interning). `group-integrator-dedup` stays
  a separate optional follow-up coordinating with the new identity.

Representation-internal; behavior oracle-gated, bit-exact — including
RNG: capture draws keep the exact per-leaf keying and site numbering
landed in `rng-spawn-order-independence`.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `lowering`: adds the entity storage identity requirements — cold spec
  table vs dense rows, explicit-id keying for grouping/state/memoization,
  bounded spec lifetime. (The program-identity requirements already
  landed; this extends the same spec to storage.)

## Impact

- `crates/core/src/interp/{world,spawn,motion}.rs`,
  `crates/core/src/sim/{mod,exec,slots,collision,render}.rs` — entity
  store, spawn path, motion walkers, batch scratch, memo keys. No other
  crate touches entity layout (verified: web/js/player/bench go through
  `render_frame`/host only; no serialization of entity layout exists).
- Host-visible pointer contracts preserved untouched: `Rc::ptr_eq` on
  `RenderSchema` (model/renderers, touhou renderer) and the shared
  `Rc<EventLog>` across snapshots.
- Unblocked downstream: `f32-hot-columns`, `jit-native-codegen`
  data-parallel loops, `gpu-kernel-backend`, offline card AOT.
- Governing: `openspec/specs/lowering/spec.md`,
  `openspec/specs/determinism/spec.md`, this change's `design.md`.
