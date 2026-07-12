# Channel/cell unification and load-time schema pass

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Missing host channels such as `$wind` fail mid-run instead of at load. Decided: channel manifests, per-kind render row schemas, and entity field tables are ONE load-time schema collection pass — shared machinery, separate tables where the columns differ. The converged (not yet ratified) design in `docs/notes/channel-unification.md` makes the manifest check fall out of scoping: a free `$name` neither bound nor def'd is a load error, and the manifest is the set of `(from-host :name)` sites.

## What Changes

- Ratify and implement `docs/notes/channel-unification.md`: cells dissolve into let-bound sigiled streams; the dynamic cell scope (CELLS_KEY/cell_scope/adapter caller-cells) becomes deletable kernel surface.
- Channel manifest checking at load time as a scoping consequence.
- The shared load-time schema collection pass (channels + render row schemas + entity field tables).

## Capabilities

To be finalized at pick-up; likely channel-scoping and load-time-schema capabilities.

## Impact

- `proto/core/src/sim/channels.rs`, interp scoping, card loading.
- Unblocks/subsumes: `scoped-channel-overrides`, the cell half of `pattern-embedding-adapters`, and the schema-pass half of `render-schema-per-kind`.
