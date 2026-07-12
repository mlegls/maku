# Tasks: migrate ad hoc TODOs to OpenSpec

## 1. Note homes for non-backlog content

- [x] 1.1 Create `docs/notes/perf-campaign.md`: rig commands (MAKU_WALL_ONLY profile example, scaled fruit case), interleaved-A/B methodology, macOS `sample` ground-truth procedure, verification gates (core tests + MAKU_LOWER_ORACLE suite), current wall table, pointer to git history for round narrative. Move the corresponding TODO.md content verbatim, then edit for the new home. Commit.
- [x] 1.2 Create `docs/notes/data-model.md` from TODO.md "Data Model Targets" (verbatim move, then edit). Commit.
- [x] 1.3 Create `docs/notes/intrinsics.md` from TODO.md "Intrinsics / Arrays" plus the Standard Library stance bullets that are decisions rather than work. Commit.
- [x] 1.4 Fill `openspec/config.yaml` `context:` with standing constraints: bit-exact determinism across lowering tiers, oracle gating, no-sugar-in-lang / optimize-expansion-shapes, DynNode ≤96-byte guard, commit-per-change-set, rounds gated on explicit user go-ahead. Commit.

## 2. Backlog stubs (one `openspec new change` + proposal.md each; commit in a few batched change-sets)

- [x] 2.1 Language-gap stubs: `states-return-routing`, `scoped-channel-overrides`, `pattern-embedding-adapters`, `entity-view-destructuring`, `channel-unification`, `evolve-followups`, `extraction-3d-embedding`, `blocking-lasers`, `rng-spawn-order-independence` — each proposal written from the actual TODO.md text, citing governing notes by path, recording blocked-on relationships in prose.
- [x] 2.2 Compiled-dyn / scale stubs: `compiled-dyn-milestone-b`, `entity-representation-flip`, `f32-hot-columns`, `collision-streaming`, `ir-unification`, `jit-native-codegen`, `group-integrator-dedup`, `spec-store-dedup`.
- [x] 2.3 Engine/refactor stubs: `render-schema-per-kind`, `rule-lowering-remainder`, `core-lib-stratification`, `model-split`, `pose-figure-unification`, `gameplay-out-of-core`, `interp-mod-split`.
- [x] 2.4 Stdlib/docs stubs: `stdlib-touhou`, `host-api-docs`.
- [x] 2.5 During 2.1–2.4, exercise the design's merge/split judgment: fold or split workstreams where the TODO text demands it, and note any deviation from the design table in this file. (Outcome: table followed as written — all 26 stubs created. Additions beyond the table: the two round-21 micro-levers with no stub of their own were folded in — `fast_pos_pose` cull-time reuse into `compiled-dyn-milestone-b`, per-batch symbol-id table into `rule-lowering-remainder`; web-host mesh adoption folded into `host-api-docs`; likely pick-up-time folds recorded in prose: `spec-store-dedup`/`group-integrator-dedup` into `entity-representation-flip`.)

## 3. TODO.md rewrite and verification

- [x] 3.1 Rewrite `docs/notes/TODO.md` as an index only: pointer to `openspec/changes/` (+ `openspec list`), pointers to design notes for decisions (including the three new notes), a one-line "likely next rounds" pointer list. Remove all migrated content in this same commit.
- [x] 3.2 Verify completeness: diff pre-migration TODO.md against destinations — every deleted line is either moved (notes/proposal) or was completed-narrative; no open item remains only in git history. (One gap found and fixed: the "channel-free bullets recompute, never table" decision, added to `jit-native-codegen`.)
- [x] 3.3 Run `openspec validate --all` (or per-change validate) and `openspec list`; fix any schema complaints. Commit the rewrite. (Outcome: `migrate-todos-to-openspec` validates; the 26 stubs fail strict validation because proposal-only stubs have no spec deltas — by design, recorded in design.md Risks; validate each at pick-up.)
