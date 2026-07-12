# Extract the session contract from player.md

## Why

`docs/player.md` should be user-facing tool documentation, but its "The session (core::session)" section is the internal session/replay contract (two-tape deterministic fold, snapshot+re-step reachability, branch-on-resume) — the last internal-facing content left under `docs/` after dissolve-design-notes, plus one dangling `design.md §11` reference.

## What Changes

- New `session` capability spec: the two-tape model, tick reachability, branch-on-resume, bindings-as-host-config (values taped, not keys), snapshots-as-cache semantics.
- `docs/player.md` keeps user-visible scrubbing behavior, cites the spec, loses the internals and the dangling reference.

## Capabilities

### New Capabilities
- `session`: the deterministic session/replay/scrub contract.

### Modified Capabilities

_None._

## Impact

- `openspec/specs/session/`, `docs/player.md`. No code changes.
