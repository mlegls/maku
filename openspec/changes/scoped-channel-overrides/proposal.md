# Scoped channel overrides

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

There is no way to locally override a channel's value for a dynamic extent: `(with {$chan v} body)` is specified as the surface but unimplemented.

## What Changes

- Implement `(with {$chan v} body)` scoped channel overrides.

## Capabilities

To be finalized at pick-up; likely one capability covering channel scoping semantics.

## Impact

- Stream plumbing in `proto/core/src/sim/channels.rs` and interp evaluation.
- channel-unification has LANDED: channels/cells are unified sigiled streams (`openspec/specs/language/spec.md`; rationale in `openspec/changes/archive/2026-07-12-channel-unification/design.md`). Build `with` against the stream store and producer refresh, honoring the distribution-law semantics in the language spec Reference (§3).
