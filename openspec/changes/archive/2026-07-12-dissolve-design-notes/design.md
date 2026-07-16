# Design: dissolve docs/notes into OpenSpec

## Context

`docs/notes/` (10 files, ~1,700 lines) plus `docs/design.md`, `docs/types.md`, `docs/language.md` (~1,100 lines) carry four content kinds with hand-maintained lifecycle tags. OpenSpec models all four natively: current truth → capability specs (custom `##` sections outside `## Requirements` survive the archive merge, so specs can carry design/rationale/reference prose); target designs → backlog changes' `design.md` (artifacts may exist early on stubs); status/narrative → change lifecycle + git history; open work → the backlog (already migrated).

## Goals / Non-Goals

**Goals:**
- `docs/` user-facing only (tutorials, from-dmk.md, player.md); `docs/notes/` deleted; no internal design/language/types under docs/.
- One home per content kind; no "normative surface" back-pointers, no "decided/DONE" prose tags.
- Lossless: every deleted line has a destination or is completed-narrative/stale analysis (git history).

**Non-Goals:**
- No semantics changes; no re-litigation of any decision while moving it.
- Not the user-facing language reference (that's `language-reference`) — this change moves the *internal* language spec.
- No conversion of archived material into scenarios; only living truth gets requirement format.

## Decisions

1. **Three new capability specs** via this change's deltas, custom sections added post-archive (established two-step):
   - `language` — umbrella requirements distilled from docs/language.md; the full spec text moves VERBATIM into a `## Reference` section (lossless; future language changes write deltas against requirements and edit Reference in the same change). Also absorbs: intrinsics.md criteria and the founding essay's durable conclusions (condensed into `## Rationale`), data-model.md's already-true semantics (numeric masks, one Number type).
   - `lowering` — compiled-dyn-design.md's current truth: requirements for the executor boundary (lanes+scratch, total callback-free ops), all-or-nothing program classification, structural interning as the compile-cache key, the DynNode ≤96-byte guard, permanent IR-interpreter fallback tier, interpreted control plane; `## Design` carries tier architecture, JIT gaps, milestone state, platform notes.
   - `perf` — measurement methodology as requirements (interleaved A/B for deltas, wall-only verdicts, sample for attribution); `## Current walls` + `## Remaining levers` as non-normative sections future rounds update.
2. **MODIFIED `work-tracking`**: decision home becomes capability specs + change designs; the TODO-index and perf-campaign.md requirements are REMOVED/MODIFIED accordingly. `docs/notes/TODO.md` deleted — `openspec list` is the index.
3. **Verbatim moves into backlog change designs** (target/unratified content): channel-unification.md → `channel-unification/design.md`; model-split.md → `model-split/design.md`; docs/types.md → `ir-unification/design.md` (header notes it also feeds entity-representation-flip and pose-figure-unification); builtins-audit.md → `core-lib-stratification/design.md`; data-model.md's aspirational storage/target parts → `entity-representation-flip/design.md`.
4. **Design sections for existing specs** (post-archive edits): evolve-design.md rationale → `evolve-semantics ## Design`; render-output-design.md rationale/host API/parallelism → `render-rows ## Design`; determinism keeps its requirements, gaining the shared-math-shims/width-table design prose.
5. **mesh-renderer-spec.md** → the pack's behavior summary becomes `crates/render-touhou` crate docs (module doc comment); the work-order narrative is git history.
6. **docs/design.md** (founding essay) → durable conclusions condensed into `language ## Rationale`; the essay itself is git history (no synthetic archive entry — the archive convention is date-prefixed real changes; don't abuse it).
7. **Citations flip**: all backlog stubs and `openspec/config.yaml` cite `docs/notes/...` — grep-driven update to the new spec/change-design paths in the same commit as each deletion, so no dangling reference ever lands.
8. **No more date tags**: moved content drops "decided/settled/DONE round N" prefixes; lifecycle is the metadata.

## Risks / Trade-offs

- [Fidelity loss in distillation] → language.md moves verbatim (Reference section); only requirements are new prose. Completeness gate: post-move grep of deleted files' section headers against destinations.
- [Requirements too coarse for language] → acceptable: umbrella requirements anchor deltas; Reference carries precision. Refine per-area when a change actually touches that area.
- [Big-bang migration] → ordered tasks, one commit per coherent move; the tree is valid (no dangling citations) after every commit.
- [Reversal of the seed-specs "cite the notes" pattern hours after landing it] → deliberate, user-driven; back-pointers die with the notes.
