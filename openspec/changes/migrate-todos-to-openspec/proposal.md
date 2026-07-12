# Migrate ad hoc TODOs to OpenSpec

## Why

`docs/notes/TODO.md` has grown into a 400-line mixed bag: actionable open work, settled design decisions, process/rig documentation, and historical narrative all interleaved. Deciding "what's next" means re-reading the whole file, and each perf round appends more prose. OpenSpec is already initialized in this repo but empty; moving the *open work* into per-workstream changes gives each item a durable proposal with its own status, while the decisions that TODO.md also carries stay where they already have authority (docs/language.md and docs/notes/*-design.md).

## What Changes

- Triage every item in `docs/notes/TODO.md` into one of three buckets:
  1. **Open workstream** → becomes an OpenSpec change (proposal.md only — a backlog stub; design/specs/tasks are generated when the work is actually picked up).
  2. **Settled decision / constraint** → stays in (or moves to) the governing design note under `docs/notes/`; TODO.md stops duplicating it.
  3. **Process documentation** (perf rig, measurement methodology, gates) → moves to a dedicated `docs/notes/perf-campaign.md` note.
- Create one backlog change per coherent workstream (roughly one per plausible implementation round), each proposal citing the governing design notes rather than restating them.
- Shrink `docs/notes/TODO.md` to a short index: pointer to `openspec list` for open work, pointers to design notes for decisions. No open items live in it afterward.
- Fill in `openspec/config.yaml` `context:` with the project's standing constraints (determinism contract, oracle gates, no-sugar-in-lang principle, commit discipline) so future artifact generation inherits them.

## Capabilities

### New Capabilities
- `work-tracking`: how open work is tracked in this repo — every open work item is an OpenSpec change with a proposal; settled decisions live in design notes, not the backlog; TODO.md is an index only.

### Modified Capabilities

_None — `openspec/specs/` is empty; no existing requirements change._

## Impact

- `docs/notes/TODO.md` — rewritten to an index (content redistributed, not lost).
- `openspec/changes/` — ~15–25 new backlog changes (proposal stubs).
- `openspec/config.yaml` — project context filled in.
- `docs/notes/perf-campaign.md` — new; receives the rig/methodology/walls documentation.
- No engine code changes; `proto/` untouched.
