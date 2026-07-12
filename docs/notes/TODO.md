# Prototype TODO — index

This file is an index only. Open work items live as OpenSpec changes;
settled decisions live in the design notes; completed work lives in git
history and the notes' status headers. Do not add work items or
decision bodies here.

## Open work

Tracked under `openspec/changes/` (one change per workstream, proposal
stubs until picked up): `openspec list` enumerates them; each proposal
states why the work matters and which design notes govern it.

Likely next rounds: `compiled-dyn-milestone-b`,
`entity-representation-flip`, `ir-unification`, `rule-lowering-remainder`,
`channel-unification`.

## Decisions and constraints

- `docs/language.md` — the authoritative language spec.
- `docs/notes/data-model.md` — core data-model targets (figures, dyn,
  SoA entity storage, projectors, masks/number semantics).
- `docs/notes/intrinsics.md` — intrinsic criterion, array-verb
  direction, stdlib stance, no-sugar-in-lang principle.
- `docs/notes/compiled-dyn-design.md` — lowering tiers, milestones,
  JIT-readiness gaps, determinism contract.
- `docs/notes/evolve-design.md` — evolve/remat semantics.
- `docs/notes/channel-unification.md` — channel/cell unification
  (converged, not yet ratified).
- `docs/notes/model-split.md` — dyn-kernel move sequencing.
- `docs/notes/builtins-audit.md` — kernel-shrink audit and worklist.
- `docs/notes/render-output-design.md` — SoA render output; mesh
  renderers are hosts.
- `docs/notes/mesh-renderer-spec.md` — the landed mesh pack build plan.

## Process

- `docs/notes/perf-campaign.md` — perf rig, measurement methodology,
  verification gates, current walls, remaining levers.
- `openspec/config.yaml` — standing constraints inherited by OpenSpec
  artifacts.
