# Design: migrate ad hoc TODOs to OpenSpec

## Context

`docs/notes/TODO.md` (~400 lines) interleaves four kinds of content: open work items, settled decisions/constraints, process documentation (perf rig, gates), and historical narrative. The design notes under `docs/notes/` (compiled-dyn-design.md, evolve-design.md, channel-unification.md, model-split.md, builtins-audit.md, render-output-design.md, mesh-renderer-spec.md) are the real decision authority; `docs/language.md` is the language spec authority. OpenSpec is initialized (`openspec/`, spec-driven schema) but empty. Work proceeds in user-gated "rounds" — one coherent workstream per round.

## Goals / Non-Goals

**Goals:**
- Every open item gets a durable, individually addressable backlog entry.
- Decision content keeps exactly one home; no duplication between backlog and notes.
- TODO.md stops being an append-only dumping ground.
- Nothing is lost: migration is move-and-edit, never summarize-and-discard.

**Non-Goals:**
- No engine code changes; no changes to `docs/language.md`.
- No re-litigating decisions during migration — content moves with its stance intact.
- No full artifact generation (design/specs/tasks) for backlog stubs.
- No migration of already-completed narrative (that stays in git history and note status headers, per TODO.md's own rule).

## Decisions

1. **Backlog stub = proposal.md only.** Tasks written months before implementation rot; each round starts by generating design/tasks fresh against current code. Alternative (full artifacts per change) rejected as busywork that would immediately drift.

2. **Granularity: one change ≈ one plausible implementation round.** Matches the existing round protocol. Small related bullets fold into one change (e.g. AxisSel scatter folds into the compiled-dyn milestone-B change); sprawling sections split.

3. **Provisional workstream mapping** (final merge/split judgment happens during apply, with each proposal written from the actual TODO text):

   | Change name | Source in TODO.md |
   |---|---|
   | `states-return-routing` | states body-return as next label |
   | `scoped-channel-overrides` | `(with {$chan v} body)` |
   | `pattern-embedding-adapters` | callable-pattern arg passing / shared-cell adapters |
   | `entity-view-destructuring` | match_pattern map destructuring over entity views |
   | `channel-unification` | manifest/load-time schema pass (governing: channel-unification.md, needs ratification) |
   | `evolve-followups` | per-dyn-field epochs, soft-cull fades, F1 lint, masked-SoA path; vel/stages re-expression deferrals recorded |
   | `extraction-3d-embedding` | extraction + 3D embedding |
   | `blocking-lasers` | DMK §13.7 world-geometry extent |
   | `rng-spawn-order-independence` | sequential splitmix limitation |
   | `compiled-dyn-milestone-b` | ClosedPt group pose, AxisSel lane scatter, ReadScan+Channel ops, motion readers over SoA columns |
   | `entity-representation-flip` | spec id + capture vector + state cells replacing per-row node clones |
   | `f32-hot-columns` | mixed numeric width contract; oracle as precision meter |
   | `collision-streaming` | per-layer streaming passes; index capture cost |
   | `ir-unification` | JIT gap 1: ProjectorNum/ResolvedRow*/DynNum onto NumProgram |
   | `jit-native-codegen` | the JIT/native tier itself (blocked on ir-unification, settled semantics) |
   | `group-integrator-dedup` | one integrator per (program, captures, birth) group |
   | `spec-store-dedup` | cross-spawn lane widening / memory |
   | `render-schema-per-kind` | per-kind row schemas, rename/pick adapter, mesh/sprite-batch kind |
   | `rule-lowering-remainder` | interpreted rule scans (beam/cull/hp), partial prefiltering, rewrite.rs follow-ups |
   | `core-lib-stratification` | builtin kernel shrink (governing: builtins-audit.md) |
   | `model-split` | dyn kernel to model/ (governing: model-split.md; sequenced after vel/stages re-expression) |
   | `pose-figure-unification` | collapse DynLike::Dyn(Pose) asymmetry onto Dyn<Figure> |
   | `gameplay-out-of-core` | bare hostile `(cull)`, palette tables behind host config |
   | `stdlib-touhou` | hitbox-radius data move, spellcard templates over states |
   | `interp-mod-split` | split interp/mod.rs (engineering debt) |
   | `host-api-docs` | docs/host-api.md, signal tapping/plotting, tick-rate config |

4. **Decision-only sections get note homes.** "Data Model Targets", the intrinsics criterion, and the stdlib stance currently exist *only* in TODO.md — they are decisions without a design note. They move verbatim to new notes: `docs/notes/data-model.md` and `docs/notes/intrinsics.md` (stdlib stance folds into the latter or the touhou lib header comment). Alternative (leave in TODO.md) violates the index-only requirement; alternative (turn into OpenSpec specs/) rejected — `openspec/specs/` describes tracked capabilities with scenarios, and these are architecture targets, not testable requirements.

5. **Process docs → `docs/notes/perf-campaign.md`.** Rig commands, MAKU_WALL_ONLY / interleaved-A/B methodology, `sample` ground-truth procedure, oracle gates, current wall table, and the round history pointer. This is the standing reference the round protocol keeps re-deriving.

6. **`openspec/config.yaml` context gets filled** with the constraints every future artifact must respect: bit-exact determinism across tiers, oracle gating (MAKU_LOWER_ORACLE), no-sugar-in-lang (optimize expansion shapes, not names), DynNode size guard, commit-per-change-set discipline, round gating on explicit user go-ahead.

7. **Blocked-on relationships are recorded in prose**, in each proposal's Why/Impact (e.g. `jit-native-codegen` blocked on `ir-unification`; `model-split` sequenced after vel/stages re-expression). OpenSpec has no dependency graph; inventing one in frontmatter is not worth the tooling mismatch.

## Risks / Trade-offs

- [Nuance loss during triage] → Migration copies TODO text verbatim into the destination first, then edits for the new home; the pre-migration TODO.md stays in git history; final diff review checks every deleted TODO line has a destination.
- [Backlog sprawl — 25 stubs with no priority signal] → TODO.md's index keeps a short "likely next rounds" line (pointers only); round reports continue to propose candidates. Accepted trade: OpenSpec list is unordered.
- [Two sources of truth during a partial migration] → Do the whole migration in one round; TODO.md rewrite is the last step and the commit that lands it removes all migrated content at once.
- [Stub proposals going stale as design notes evolve] → Stubs cite notes by path instead of restating them, so staleness is bounded to the "why now" framing, which is cheap to refresh at pick-up time.

## Open Questions

- Whether `extraction-3d-embedding`, `blocking-lasers`, and other far-horizon items deserve stubs at all or just an index line — default: stub them anyway (uniform rule is simpler than a second tier).
