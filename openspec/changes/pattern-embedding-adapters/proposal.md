# Pattern embedding scope adapters

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Callable patterns currently embed bare defaults only: there is no argument passing and no shared-cell adapters, so a pattern cannot be embedded into another card with its knobs rebound.

## What Changes

- Add argument passing and shared-cell scope adapters for pattern embedding.

## Capabilities

To be finalized at pick-up.

## Impact

- Pattern/card loading in `proto/core/src/interp/card.rs`.
- The cell half interacts with `docs/notes/channel-unification.md` (cells dissolve into let-bound sigiled streams there); sequence against `channel-unification` to avoid building adapters on deletable kernel surface.
