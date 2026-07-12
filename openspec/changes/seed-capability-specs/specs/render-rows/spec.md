# render-rows

## ADDED Requirements

### Requirement: The tick's render output is an ordered frame
The render output of a tick MUST be an ordered frame whose draw order is emission order: standing-rule registration order, and within one rule's pass, entity row order. Batching MUST NOT change the observable order — a batch occupies one position in the stream and expanding it in place reproduces the row sequence exactly.
*Why:* settled before JIT work so optimization has fixed semantics. Rationale: `docs/notes/render-output-design.md`.

#### Scenario: Batch-vs-row order
- **WHEN** a rule's pass is emitted as a column batch instead of rows
- **THEN** expanding the frame yields exactly the row sequence the row-at-a-time path would have produced, in the same positions

### Requirement: Row expansion is the semantic reference
`RenderRow` MUST remain the canonical row form; a batch is a layout, not a new value universe. Expansion from batch to rows MUST be total and exact, and the compat `render()` MUST be defined as frame expansion.

#### Scenario: Oracle expansion equality
- **WHEN** the oracle is enabled and a compiled pass emits a batch
- **THEN** the expanded batch compares row-equal (`==`) against the interpreted re-run's rows

### Requirement: Render schemas accrete one kind per key
The per-world render field schema MUST enforce one kind per key, accreting as new keys appear. Batch fills MUST validate keys against the world schema plus a local pending set and commit registrations only when the whole pass succeeds; any error or kind surprise aborts the batch (world untouched) and re-runs the rule row-at-a-time, reproducing the interpreted error, error site, and partial-row state exactly.
*Why:* per-kind registered schemas and manifest negotiation are future work (`render-schema-per-kind` change); this is the current contract.

#### Scenario: Staged registration abort
- **WHEN** a batch pass encounters a field whose kind contradicts an earlier row's kind
- **THEN** no staged registration is committed and the interpreted re-run raises the identical schema error

### Requirement: Absent fields stay absent
A field whose value is `nothing` for a row MUST simply not be present on that row. Columns therefore carry optional presence, and a field that is `nothing` for every matched row contributes no column at all that pass.

#### Scenario: All-nothing field
- **WHEN** a rule emits a field that evaluates to `nothing` for every matched row in a pass
- **THEN** the resulting batch has no column for that field and expanded rows lack the key

### Requirement: Frames are tick-cadence snapshots
Rule-emitted render rows MUST be snapshots at tick cadence; the engine provides no frame-time re-evaluation or interpolation. Hosts that render between ticks own any interpolation policy.
*Why:* decided trade (round 21); keeps the frame API pure transport.

#### Scenario: Host rendering between ticks
- **WHEN** a host draws at a higher frame rate than the tick rate
- **THEN** consecutive draws between ticks observe the same frame; any smoothing is host-side
