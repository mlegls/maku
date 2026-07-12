# Group integrator dedup

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Ring lanes often carry bit-identical integrator state (the per-bullet angle lives in the ConstFrame wrapper): thousands of bullets, dozens of distinct folds. One integrator per (program, captures, birth) group with per-lane frame transforms collapses the redundant integration work and memory.

## What Changes

- Deduplicate integrator state to one fold per (program, captures, birth-tick) group; lanes apply their frame transforms on read.

## Capabilities

Representation-internal; oracle-gated.

## Impact

- Vel batch machinery in `proto/core/src/sim/mod.rs` + motion state.
- Natural companion to `entity-representation-flip` (fold at pick-up if the design wants). Governing: scale-target consequences, `docs/notes/compiled-dyn-design.md`.
