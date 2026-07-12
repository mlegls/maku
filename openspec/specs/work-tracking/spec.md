# work-tracking Specification

## Purpose
TBD - created by archiving change migrate-todos-to-openspec. Update Purpose after archive.
## Requirements
### Requirement: Open work lives as OpenSpec changes
Every open work item in the prototype SHALL be tracked as an OpenSpec change under `openspec/changes/`, containing at minimum a `proposal.md` (a backlog stub). Design, specs, and tasks artifacts SHALL be generated only when the work is picked up for implementation, not at backlog creation.

#### Scenario: Backlog item exists as a change
- **WHEN** a work item from `docs/notes/TODO.md` is migrated
- **THEN** a change directory exists under `openspec/changes/<workstream-name>/` with a `proposal.md` stating why the work matters, what it covers, and which design notes govern it

#### Scenario: New work is proposed
- **WHEN** a new open work item is identified after migration
- **THEN** it is captured as a new OpenSpec change (proposal stub), not appended to `docs/notes/TODO.md`

### Requirement: Decisions live in design notes, not the backlog
Settled design decisions and constraints SHALL live in `docs/language.md` or `docs/notes/*.md` design notes. Backlog proposals SHALL reference the governing note by path and SHALL NOT restate decision content.

#### Scenario: Proposal references a governing note
- **WHEN** a backlog proposal depends on a settled decision (e.g. the determinism contract, mixed numeric width)
- **THEN** the proposal cites the note path (e.g. `docs/notes/compiled-dyn-design.md`) instead of duplicating its content

### Requirement: TODO.md is an index only
`docs/notes/TODO.md` SHALL contain only pointers: to `openspec/changes/` (or `openspec list`) for open work, and to the design notes for decisions. It SHALL NOT contain open work items or decision bodies.

#### Scenario: Reading TODO.md after migration
- **WHEN** a reader opens `docs/notes/TODO.md`
- **THEN** they find a short index directing them to OpenSpec for open work and to named design notes for decisions, with no inline work items

### Requirement: Process documentation has a durable home
Measurement methodology, the perf rig commands, verification gates, and campaign wall history SHALL live in `docs/notes/perf-campaign.md`.

#### Scenario: Locating the perf rig
- **WHEN** a contributor needs the profiling commands or the interleaved A/B measurement rules
- **THEN** they find them in `docs/notes/perf-campaign.md`, not in TODO.md or scattered in change proposals

