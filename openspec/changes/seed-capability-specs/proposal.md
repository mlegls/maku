# Seed openspec/specs/ with settled contracts

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`openspec/specs/` is empty (only `work-tracking` arrives when the migration change archives), so changes touching settled behavior have nothing to write deltas against. The genuinely settled behavioral contracts are currently buried inside design notes as prose; extracting them as capability specs makes future change deltas mechanical.

## What Changes

- Extract the settled contracts into `openspec/specs/<capability>/spec.md` (requirement + scenario format), citing the design notes for rationale rather than duplicating it:
  - `determinism`: bit-exact replay across lowering tiers, oracle gating, RNG stream contract (from `docs/notes/compiled-dyn-design.md` and `docs/notes/perf-campaign.md`).
  - `evolve-semantics`: remat/change-col contract, per-slot epochs, closed-vs-live sampling (from `docs/notes/evolve-design.md`, `docs/language.md`).
  - `render-rows`: schema merge-by-key rules, batch/row equivalence, tick-cadence snapshot trade (from `docs/notes/render-output-design.md`).
- Justification MAY live in the spec files (verified against openspec 1.6.0 archive/parse behavior): `## Purpose` and custom `##` sections outside `## Requirements` are preserved verbatim by the archive merge and ignored by the parser; short `*Why:*` lines inside requirement blocks survive too, but MODIFIED deltas replace whole blocks and only scenario loss is machine-checked — so anything longer than a sentence or two goes in `## Rationale`/`## Purpose` or stays a design-note citation.
- Do NOT bulk-convert long-form design rationale: notes stay the home for alternatives/sequencing/narrative (decided 2026-07).
- Further capabilities accrete via ordinary change deltas, not another seeding pass.

## Capabilities

The three seed capabilities above (final list at pick-up).

## Impact

- `openspec/specs/` only; no code changes. Mechanism: either direct spec files or a change whose deltas are ADDED requirements that archive into place.
- Design notes gain "normative surface lives in openspec/specs/<name>" pointers where extracted.
