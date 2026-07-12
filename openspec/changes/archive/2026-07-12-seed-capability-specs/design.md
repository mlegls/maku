# Design: seed capability specs

## Context

`openspec/specs/` holds only `work-tracking`. The three most settled behavioral contracts live as prose inside design notes: the determinism/oracle contract (`docs/notes/compiled-dyn-design.md`, `docs/notes/perf-campaign.md`), evolve semantics (`docs/notes/evolve-design.md`, settled 2026-07), and the render frame/schema semantics (`docs/notes/render-output-design.md`, landed round 21). Future changes touching these areas have nothing to write MODIFIED deltas against.

## Goals / Non-Goals

**Goals:**
- Three capability specs installed: `determinism`, `evolve-semantics`, `render-rows`.
- Each requirement normative (SHALL/MUST), scenario-backed, and citing its note for rationale.

**Non-Goals:**
- No behavior changes; the specs describe what already holds (and is test/oracle-enforced).
- No bulk conversion of note rationale — notes remain the home for alternatives/sequencing/narrative.
- No spec for unratified designs (channel-unification waits for ratification).

## Decisions

1. **Mechanism: change deltas, archived into place.** The delta specs in this change ARE the implementation; archiving installs them via the CLI's merge. Alternative (writing `openspec/specs/` files directly) rejected — archive would then collide on ADDED requirements.
2. **Purpose sections are filled post-archive.** The archive skeleton writes `## Purpose: TBD`; delta files cannot carry Purpose. The purposes are recorded here (below) and pasted in immediately after archive, as an explicit task.
   - `determinism`: Why replays, the lowering oracle, and cross-tier equivalence are trustworthy: one contract governing op order, math shims, RNG stream order, and fallback behavior.
   - `evolve-semantics`: The kernel's one stateful signal constructor and the dyn ≅ t→T equivalence: fold semantics, epochs, closed-vs-live sampling, sited evolves.
   - `render-rows`: The tick's render output as an ordered frame of rows and column batches: ordering, row-expansion equivalence, schema accretion, absence.
3. **Rationale placement** (per the verified 1.6.0 parse/merge behavior): short `*Why:*` lines inside requirement blocks; anything longer stays a note citation. Notes gain back-pointers ("normative surface: openspec/specs/<name>") so the two homes reference each other.
4. **Scope of `determinism`**: includes the process gate (oracle suites must pass before landing) because it is the enforcement arm of the contract — process and invariant are one capability. The known limitation (spawn-order dependence of the sequential RNG stream) is stated as current behavior with a pointer to the `rng-spawn-order-independence` change, not specced away.
5. **Width neutrality**: the determinism spec states same-ops/same-order/same-width-per-storage-class, not "f64" — the planned f32 hot columns (`f32-hot-columns`) then MODIFY the width table, not the invariant.

## Risks / Trade-offs

- [Spec drifts from note prose] → Each spec cites its note and vice versa; the work-tracking rule (decisions have one home) applies: normative statements now live in the spec, notes keep rationale.
- [Archive-then-edit Purpose is a two-step install] → Recorded as explicit ordered tasks; the skeleton TBD is visible if forgotten.
