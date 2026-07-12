# Model split: backend-parametric dyn kernel

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The dyn kernel (and entity spec / state-schema semantic halves) should move to model/ as a backend-parametric `Dyn<E>` — but AFTER the remaining evolve re-expression (vel/stages) shrinks the kernel; moving now would enshrine Vel/Stages, which become lib shapes.

## What Changes

- Move the dyn kernel and entity spec/state-schema semantic halves under `proto/core/src/model/` as backend-parametric types.
- Finish shared model extraction: built-in collider/render projector cases still live under `interp` until their specs no longer depend directly on interpreter `Dyn`/`DynLike`/`Env` types.

## Capabilities

Code-structure refactor; semantics unchanged.

## Impact

- Sequenced AFTER the vel/stages re-expression in `evolve-followups` (which is itself deferred TO this split for `vel` — the design note resolves the ordering).
- Governing: `openspec/changes/model-split/design.md` (direction + sequencing).
