# Tasks: seed capability specs

## 1. Author and verify the delta specs

- [x] 1.1 Write the three delta specs (`specs/determinism/spec.md`, `specs/evolve-semantics/spec.md`, `specs/render-rows/spec.md`) from the governing notes, with SHALL/MUST requirements, scenarios, and short `*Why:*` lines citing the notes.
- [x] 1.2 Verify every normative statement against its note (`docs/notes/compiled-dyn-design.md`, `docs/notes/perf-campaign.md`, `docs/notes/evolve-design.md`, `docs/notes/render-output-design.md`) — no invented requirements, no unratified designs.
- [x] 1.3 `openspec validate seed-capability-specs` passes. Commit the change artifacts.

## 2. Cross-references

- [x] 2.1 Add "Normative surface: `openspec/specs/<name>/spec.md`" pointers to the headers of `docs/notes/evolve-design.md`, `docs/notes/render-output-design.md`, and the determinism/oracle sections of `docs/notes/compiled-dyn-design.md` and `docs/notes/perf-campaign.md`. Commit.

## 3. Install (archive) and finish the main specs

- [ ] 3.1 Archive this change (`openspec archive seed-capability-specs -y`), installing the three specs into `openspec/specs/`.
- [ ] 3.2 Replace the skeleton `## Purpose` (TBD) sections of the three installed specs with the purposes recorded in design.md. Validate each installed spec (`openspec validate <name> --type spec`). Commit.
