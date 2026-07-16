# Channel/cell unification and load-time schema pass

## Why

Missing host channels such as `$wind` fail mid-run instead of at load. Decided: channel manifests, per-kind render row schemas, and entity field tables are ONE load-time schema collection pass — shared machinery, separate tables where the columns differ. The converged (not yet ratified) design in `openspec/changes/channel-unification/design.md` makes the manifest check fall out of scoping: a free `$name` neither bound nor def'd is a load error, and the manifest is the set of `(from-host :name)` sites.

## What Changes

- Ratify and implement `openspec/changes/channel-unification/design.md`: cells dissolve into let-bound sigiled streams; the dynamic cell scope (CELLS_KEY/cell_scope/adapter caller-cells) becomes deletable kernel surface.
- Channel manifest checking at load time as a scoping consequence.
- The shared load-time schema collection pass (channels + render row schemas + entity field tables).

## Capabilities

- `language` (modified): stream unification — sigiled bindings, `bind!`/`export!`/`from-host`, free-`$name` load errors, producer/set! interplay, export collisions.
- `load-time-schema` (new): the shared load-time schema collection pass and host-manifest checking.

## Impact

- `crates/core/src/sim/channels.rs`, interp scoping, card loading.
- Unblocks/subsumes: `scoped-channel-overrides`, the cell half of `pattern-embedding-adapters`, and the schema-pass half of `render-schema-per-kind`.
