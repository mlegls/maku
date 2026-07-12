# Per-kind render row schemas

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The current render schema is one global key->kind map. Remaining render-surface work: per-kind registered row schemas with manifest negotiation, the builtin field rename/pick adapter for imported conflicting schemas, and a mesh/sprite-batch kind.

## What Changes

- Per-kind registered row schemas + manifest negotiation.
- Builtin field rename/pick adapter.
- A mesh/sprite-batch render kind.

## Capabilities

Render-row schema semantics (user-facing at the schema-merge boundary).

## Impact

- `proto/core/src/sim/render.rs`, `proto/core/src/model/renderers.rs`, mesh pack consumers.
- Known trade (decided): rule-emitted rows are tick-cadence snapshots; frame-time re-evaluation/interpolation is a host concern.
- The load-time schema pass now EXISTS (`proto/core/src/interp/schema.rs`, `openspec/specs/load-time-schema/spec.md`): per-kind row schemas join that pass as their columns become statically declarable. Governing: `openspec/specs/render-rows/spec.md`, `openspec/changes/entity-representation-flip/design.md`.
