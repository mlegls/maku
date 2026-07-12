# Scoped channel overrides

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

There is no way to locally override a channel's value for a dynamic extent: `(with {$chan v} body)` is specified as the surface but unimplemented.

## What Changes

- Implement `(with {$chan v} body)` scoped channel overrides.

## Capabilities

To be finalized at pick-up; likely one capability covering channel scoping semantics.

## Impact

- Channel plumbing in `proto/core/src/sim/channels.rs` and interp evaluation.
- Governing design: `openspec/changes/channel-unification/design.md` (converged, not yet ratified) reframes channels as scoped sigiled streams — implement this AFTER or AS PART OF `channel-unification` rather than against the current cell machinery.
