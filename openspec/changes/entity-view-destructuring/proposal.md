# Entity-view map destructuring in predicates

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Entity-view predicates don't support destructuring params: `(fn [{:keys [hp]}] ...)` errors with "did not match pattern". Pre-existing (eager map views failed identically), independent of the lazy `Val::EntityView` row tokens.

## What Changes

- Teach `match_pattern` map destructuring over entity views, if the idiom is wanted.

## Capabilities

To be finalized at pick-up.

## Impact

- `match_pattern` in the interpreter; entity-view value representation.
- Low priority until a card actually wants the idiom; note the compiled row-predicate recognizer (`rule-lowering-remainder`) would also need to handle the destructured shape.
