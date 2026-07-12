# work-tracking delta

## RENAMED Requirements
- FROM: `### Requirement: Decisions live in design notes, not the backlog`
- TO: `### Requirement: Decisions live in capability specs and change designs`

## MODIFIED Requirements

### Requirement: Decisions live in capability specs and change designs
Settled decisions SHALL live in `openspec/specs/` capability specs: requirements for the normative surface, `## Design`/`## Rationale`/`## Reference` sections for the load-bearing why and detailed prose. The rationale trail of a decision SHALL live in the design.md of the change that made it (archived with the change). Target designs for unimplemented work SHALL live as design.md inside the corresponding backlog change. There SHALL be no standalone design-notes directory.

#### Scenario: Making a decision
- **WHEN** a change settles new behavior or reverses old behavior
- **THEN** its delta specs update the affected capability spec in that same change, and the why stays in that change's design.md

#### Scenario: Converged but unratified design
- **WHEN** a design has converged but is not yet ratified or implemented
- **THEN** it lives as its backlog change's design.md, and picking up the change is the ratification decision

### Requirement: Open work lives as OpenSpec changes
Every open work item SHALL be tracked as an OpenSpec change under `openspec/changes/`, containing at minimum a `proposal.md` (a backlog stub). Design, specs, and tasks artifacts SHALL be generated when the work is picked up; a stub MAY carry a design.md early when a converged target design exists for it.

#### Scenario: Backlog item exists as a change
- **WHEN** an open work item is identified
- **THEN** a change directory exists under `openspec/changes/<workstream-name>/` with a `proposal.md` stating why the work matters and which capability specs govern it

#### Scenario: New work is proposed
- **WHEN** a new open work item is identified
- **THEN** it is captured as a new OpenSpec change stub, not as a notes file or TODO entry

### Requirement: Process documentation has a durable home
Measurement methodology, the perf rig commands, verification gates, and standing wall measurements SHALL live in the `perf` capability spec (`openspec/specs/perf/spec.md`).

#### Scenario: Locating the perf rig
- **WHEN** a contributor needs the profiling commands or the measurement rules
- **THEN** they find them in the `perf` capability spec, not in a notes file

## REMOVED Requirements

### Requirement: TODO.md is an index only
**Reason**: `docs/notes/` (including TODO.md) is deleted; `openspec list` and `openspec/changes/` are the index, and capability specs are the decision record.
**Migration**: consult `openspec list` for open work and `openspec/specs/` for decisions.
