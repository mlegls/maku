# Tasks: dissolve docs/notes into OpenSpec

## 1. Delta specs

- [x] 1.1 Author delta specs: MODIFIED/RENAMED/REMOVED for `work-tracking`; ADDED for `language`, `lowering`, `perf`. Validate the change. Commit artifacts.

## 2. Verbatim moves into backlog change designs

- [ ] 2.1 channel-unification.md → `openspec/changes/channel-unification/design.md`; model-split.md → `model-split/design.md`; builtins-audit.md → `core-lib-stratification/design.md`; docs/types.md → `ir-unification/design.md` (header notes consumers); data-model.md's aspirational parts → `entity-representation-flip/design.md`. Delete the moved sources; update citations in the same commit.

## 3. Install specs and fill sections

- [ ] 3.1 Archive this change (installs `language`/`lowering`/`perf`, updates `work-tracking`).
- [ ] 3.2 Post-archive spec sections: Purposes for the three new specs; `language ## Reference` = docs/language.md verbatim (header noting it includes [decide] sections; requirements are the current-truth anchor) + `## Rationale` distilled from docs/design.md's durable conclusions + intrinsics criteria; `lowering ## Design` from compiled-dyn-design.md (tier plan, JIT gaps, milestone state, platform notes); `perf ## Rig`, `## Current walls`, `## Remaining levers`, draw-path A/B from perf-campaign.md; `evolve-semantics ## Design` from evolve-design.md; `render-rows ## Design` (+ host API + parallelism prose) from render-output-design.md. Validate all specs.
- [ ] 3.3 mesh-renderer-spec.md's behavior summary → `proto/mesh-touhou` crate doc comment; verify pack tests still pass.

## 4. Deletions and reference flips

- [ ] 4.1 Delete docs/notes/ entirely, docs/language.md, docs/types.md, docs/design.md. Update every reference repo-wide (openspec stubs, openspec/config.yaml, proto source comments, docs/, cards/, README) to the new spec/change-design paths — grep for `docs/notes`, `language.md`, `types.md`, `design.md`; no dangling citation may survive the commit.
- [ ] 4.2 Completeness gate: for each deleted file, check its section headers have destinations (spec section, change design, crate docs) or are completed-narrative/stale analysis; note exceptions here.
- [ ] 4.3 Drop "decided/settled/DONE round N" date-tag prose from moved content where lifecycle now carries it. Final `openspec validate --all --specs` green; update MEMORY.md memories that reference notes paths.
