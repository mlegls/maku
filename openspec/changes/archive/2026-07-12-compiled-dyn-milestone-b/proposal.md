# Compiled-dyn milestone B remainder

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Milestone B (group evaluation of shared programs) is partly landed: round 19 landed batched Vel steps (`run_lanes`, `VelBatchScratch`) plus the pos_only pose fast path; round 22 landed input slots (capture vectors over `(%capture i)` marker programs) and structural interning (cross-site batch fusion, −8% on the scaled rig). The remainder is now JIT prep more than wall win on the current rig, but it completes the "one interned, input-slotted IR runs all surfaces" stopping point before native codegen.

## What Changes

- ClosedPt group pose evaluation (closed-shape pose fill as lanes, like Vel).
- AxisSel lane scatter: array-valued dyn meta binds per spawn element (`NumDynRepr::AxisSel`) but each entity still evaluates the full shared array per tick and keeps its lane — recognize the shared program, evaluate once per group, scatter lanes (SS5 array-of-signals/signal-of-array interchange).
- The bail census's homing-slew nodes need ReadScan + Channel ops to lower.
- Cheap win: motion readers closing over SoA columns + row index instead of building per-row snapshots (readers are constructed per entity per phase).
- Candidate lever (needs a rule-effect audit first): cull-time reuse of the collide-phase `fast_pos_pose` (~11% of step, called 2x/row/tick) — exact for Vel chains ONLY if nothing between the phases mutates n2 state or figures.

## Capabilities

Lowering-internal; likely no user-facing spec changes (oracle-gated equivalence).

## Impact

- `crates/core/src/interp/{lower,motion,spawn}.rs`, `crates/core/src/sim/mod.rs`.
- Governing design + status: `openspec/specs/lowering/spec.md` ("JIT readiness" gap list). Methodology: `openspec/specs/perf/spec.md`.
