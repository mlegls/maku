# EntitySpecStore dedup

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`EntitySpecStore` cold dyn/projector data repeats per spawn site; shared spawn-site/program/archetype storage would cut memory and widen batch lanes across spawns. Milestone B's first slice turned out not to need it (rand-free spawn groups already share node/program Rcs, and ring-sized lanes amortize op decode), but it remains the lever for cross-spawn lane widening and for memory at scale.

## What Changes

- Deduplicate spec-store cold data into shared spawn-site/program/archetype storage where possible.

## Capabilities

Storage-internal.

## Impact

- `proto/core/src/interp/specs.rs`, spawn path.
- Round-22 structural interning already fuses per-ring vel groups; this extends sharing to the rest of the spec. Likely folds into `entity-representation-flip` at pick-up.
