# lowering — delta for entity-representation-flip

## ADDED Requirements

### Requirement: Entity storage is a cold spec table plus dense rows

Per-group cold data — figure template, motion state schema, dyn
columns, collider projectors, cache policy, capture layout — SHALL be
stored once in a world-owned spec table; entity rows SHALL hold a
generational spec id, one per-row capture vector, and dense typed
state/field columns, never per-row clones of node trees or schemas.
Spec entries SHALL be reference-counted by their rows and free at
refcount zero with generation-bumped id reuse, so long-running cards
that repeatedly construct fresh figures keep a bounded table. Culling a
row SHALL release its spec reference immediately.

#### Scenario: Grouped spawn shares one spec

- **WHEN** a spawn instantiates a group of elements from one figure
  template, including rand-bearing signals on the compiled path
- **THEN** all rows reference the same spec entry, node trees and
  motion state schemas are allocated once, and only capture vectors,
  rng keys, and state cells are per-row

#### Scenario: Spawn-loop card stays bounded

- **WHEN** a task constructs a fresh figure value on every spawn call
  and the spawned entities later die
- **THEN** their spec entries are freed and reused, and the spec table
  size tracks the live population rather than total spawns

### Requirement: Row-level caches and grouping key explicit spec identity

Cross-tick row classification, batch grouping, motion state-slot
resolution, and per-group memoization (dyn-field plans, projector
plans) SHALL key explicit spec/program/plan identity — including the
spec generation — rather than the addresses of per-row `Rc` payloads.
Pointer-keyed node-id maps MAY exist only at spec scope over the spec's
own immutable tree. Memoization tables SHALL be bounded by the number
of live specs, not by the number of entities ever spawned.

#### Scenario: Reused spec slot cannot alias a stale class

- **WHEN** a spec entry is freed and its slot is reused for a different
  figure while a cross-tick row-classification cache still holds the
  old id
- **THEN** the generation mismatch prevents the stale classification
  from applying to rows of the new spec

#### Scenario: Per-entity memo growth is gone

- **WHEN** a card spawns many entities from few templates over a long
  session
- **THEN** dyn-field and projector plan memos hold one entry per spec,
  not one per entity

## MODIFIED Requirements

(none — the program-identity requirements from round 22 stand as
written; this delta extends the same capability to storage identity.)
