# Scoped channel overrides

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

There is no way to locally override a channel's value for a dynamic extent: `(with {$chan v} body)` is specified as the surface but unimplemented.

Motivating uses (recorded in cards/translations/NOTES.md — the ambient-context adoption note and finding F19): difficulty modulation of a subtree ("this card at half rank" = wrap in `with`; the write-side complement of reclassifying DMK's `dl` as the `$rank` channel, which killed parameter-threading noise in the Spell 2 translation), and card algebra — embedding a foreign card under retuned ambient (half rank, a pinned `$player` decoy for aimed patterns) without editing it. Provenance: Clojure `binding` / React context, as card-visible tree nodes (spec §14).

## What Changes

- Implement `(with {$chan v} body)` scoped channel overrides.

## Capabilities

To be finalized at pick-up; likely one capability covering channel scoping semantics.

## Impact

- Stream plumbing in `crates/core/src/sim/channels.rs` and interp evaluation.
- channel-unification has LANDED: channels/cells are unified sigiled streams (`openspec/specs/language/spec.md`; rationale in `openspec/changes/archive/2026-07-12-channel-unification/design.md`). Build `with` against the stream store and producer refresh, honoring the distribution-law semantics in the language spec Reference (§3).
