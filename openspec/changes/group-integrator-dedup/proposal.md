# Group integrator dedup

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Ring lanes often carry bit-identical integrator state (the per-bullet angle lives in the ConstFrame wrapper): thousands of bullets, dozens of distinct folds. One integrator per (program, captures, birth) group with per-lane frame transforms collapses the redundant integration work and memory.

## What Changes

- Deduplicate integrator state to one fold per (program, captures, birth-tick) group; lanes apply their frame transforms on read.

## Capabilities

Representation-internal; oracle-gated.

## Impact

- Vel batch machinery in `crates/core/src/sim/mod.rs` + motion state.
- `entity-representation-flip` landed 2026-08: rows are now
  `spec_id` (generational, refcounted) + capture vector + `RowFrame`
  pose, batch keys dropped capture values (lanes already fuse across
  draws — the lane-widening half of this stub's observation), and
  per-spec identity is the natural group key for the fold dedup this
  stub still wants. What remains here is the state half: one integrator
  fold per (program, captures, birth-tick) group instead of per row.
  Governing: scale-target consequences, `openspec/specs/lowering/spec.md`
  ("Entity storage is a cold spec table plus dense rows").
