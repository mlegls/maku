# Move gameplay-domain behavior out of core

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Core should remain a 2D graphing + collision/rule/render-row engine; residual gameplay-domain behavior lives in it.

## What Changes

- Move bare hostile `(cull)` out of core.
- Host palette tables (`style_rgb`, `dot_radius`) remain stock host policy in `host.rs` for now; move them behind host/profile config when a second frontend needs different vocabulary (the mesh pack already takes a StyleTable, defaulting to the host tables).

## Capabilities

Core/lib boundary cleanup.

## Impact

- `proto/core/src/interp/engine.rs`, `host.rs`, `cards/lib/touhou.maku`.
- Related stance: `docs/notes/intrinsics.md` (stdlib section).
