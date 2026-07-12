# User-facing language reference/guide

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`docs/language.md` is the language authority but reads internally-facing — it is a design spec, not something a card author learns from or looks things up in (decided 2026-07). The tutorials teach progressively but there is no reference in between: no place to look up a form's signature, arguments, and semantics without reading the design spec.

## What Changes

- Write a separate user-facing language reference/guide under `docs/` (e.g. `docs/reference.md` or a per-topic set alongside the tutorials).
- `docs/language.md` stays the internal authoritative design spec; the reference derives from it and cites it, never contradicts it. Consider a status header on language.md marking it internal.
- Audience and voice match the tutorials: teach Maku directly, standalone; DMK/BDSL mapping stays in `docs/from-dmk.md` only.

## Capabilities

Docs only; likely no spec deltas (unless a `docs` capability is wanted).

## Impact

- `docs/`; pairs with the tutorial-site/reader-view work and `docs/host-api.md` tracked in `host-api-docs`.
- Sequencing: cheaper after the surface stops moving in the areas being documented; per-topic increments can track settled areas without waiting for everything.
