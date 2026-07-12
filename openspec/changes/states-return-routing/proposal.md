# states: return-value routing

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`states` bodies cannot yet route control flow by returning the next state: authors must use explicit gotos or rely on state order alone, which keeps richer spellcard flow awkward to express.

## What Changes

- Support state-body return values as the next label, routed by goto-or-state-order.
- Keep richer spellcard templates in `cards/lib` macros, not engine primitives (see `stdlib-touhou`).

## Capabilities

To be finalized at pick-up; likely one capability covering `states` control-flow semantics.

## Impact

- `proto/core/src/interp/` states machinery; `docs/language.md` states section.
- Related: `stages` re-expression over `states` is tracked in `evolve-followups` — that round may want this landed first.
