# Touhou stdlib growth

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Recurring Touhou/DMK vocabulary is repeated at call sites or missing from `cards/lib/touhou.maku`.

## What Changes

- Move family->hitbox-radius data (currently repeated at call sites) into the lib.
- Richer spellcard templates (:name/:type/hp bars) as lib macros over `states`, `phases`, `boss`, `finally`, and ordinary fields.
- Possible `col-or` rename in the touhou lib (noted in a prior round).

## Capabilities

Lib-only; no core changes.

## Impact

- `cards/lib/touhou.maku` and card call sites.
- Stance: `openspec/specs/language/spec.md` (stdlib section). Spellcard templates benefit from `states-return-routing`.
