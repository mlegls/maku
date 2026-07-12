# Dissolve docs/notes into OpenSpec

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`docs/notes/` and OpenSpec mix purposes: the notes hand-maintain change-lifecycle metadata in prose ("settled 2026-07", "DONE round 19", "decided") that OpenSpec models natively, and every new decision faces a judgment call about which home it goes to. A project that had used OpenSpec from the beginning would have no docs/notes at all. Get to that state, so the workflow is One Right Way: decision → spec delta in the implementing change; rationale → that change's design.md + a distilled spec section; status → change lifecycle (2026-07).

## What Changes

- Redistribute every note by content kind:
  - **Current truth** (semantics, invariants, subsystem architecture) → capability specs. Internal subsystems are capabilities too — `lowering` (from compiled-dyn-design.md), plus extensions to `determinism`/`render-rows`/`evolve-semantics`; specs carry `## Design`/`## Rationale` sections (verified safe: the archive merge preserves custom sections outside `## Requirements`).
  - **Target/unratified designs** → `design.md` inside the corresponding backlog change: channel-unification.md → `channel-unification`; model-split.md → `model-split`; data-model.md + docs/types.md targets → `entity-representation-flip`/`pose-figure-unification` (split at pick-up); mesh-renderer-spec.md is a completed work order → synthetic archived change or delete (git history).
  - **Round narrative / status headers / cost-anatomy analyses** → archived-change material or git history; status headers dissolve into change lifecycle.
  - **docs/design.md** (founding DMK essay) → archival (synthetic archived change or git history).
  - **docs/language.md** → normative semantics become language capability specs; the readable surface is the `language-reference` deliverable. `docs/` becomes user-facing only.
  - **perf-campaign.md** → methodology/gates fold into `determinism`/`work-tracking` specs; the wall table becomes a non-normative section of a perf capability spec; the intrinsics/data-model notes created by the round-23 migration dissolve the same way as the rest.
- Delete `docs/notes/` at the end; flip or remove the "normative surface" back-pointers added by `seed-capability-specs`.
- Update `openspec/config.yaml` context and the `work-tracking` spec (MODIFIED deltas): decisions live in specs/changes, not notes.

## Capabilities

MODIFIED: `work-tracking` (decision home changes from design notes to specs/changes). New: `lowering` and probably 2-4 language capability specs; final list at pick-up.

## Impact

- `docs/`, `docs/notes/` (deleted), `openspec/specs/`, many backlog change designs, `openspec/config.yaml`.
- Supersedes the "notes stay the rationale home" decision (2026-07, migrate-todos-to-openspec design) — deliberate reversal, user-driven.
- Large (one or two dedicated rounds); mechanical-move-heavy → good codex candidate for the redistribution with a hand-written mapping. Completeness gate like the TODO migration: every deleted line has a destination or is completed-narrative.
- Sequencing: best after `channel-unification` ratification question is resolved either way, and pairs naturally with `language-reference` (both split language.md's audiences).
