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
- The schema pass half belongs to the shared load-time schema collection decided in `channel-unification`. Governing: `docs/notes/render-output-design.md`, `docs/notes/data-model.md`.
