# Split interp/mod.rs

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`crates/core/src/interp/mod.rs` still contains eval plus the specials table and will keep growing with vocabulary work (engineering debt).

## What Changes

- Split `interp/mod.rs` further along its existing seams (eval core vs specials table vs construction sites).

## Capabilities

None (pure code organization).

## Impact

- `crates/core/src/interp/`; no behavior change; cheap to do opportunistically alongside another interp round.
