## ADDED Requirements

### Requirement: One load-time schema collection pass
There SHALL be exactly one load-time pass over the loaded card — the walk that resolves stream scoping — and every load-time schema table SHALL be collected by it: today the host-channel manifest (the `(from-host ...)` sites) and load lints; per-kind render row schemas and entity field tables join the same pass as their columns become statically declarable (their kinds are value-dependent today, so those tables remain runtime-accreted). The pass runs at card load, before tick 0, on every load path (fresh load, swap/add fragments).

#### Scenario: Card load runs the pass
- **WHEN** a card is loaded
- **THEN** the host-channel manifest and stream-scoping errors are produced by the single pass, before the first tick

### Requirement: Missing host channels fail at load
The set of `(from-host :name)` sites in the loaded card SHALL be checked against the channels the host provides at load time. A required host channel the host does not provide SHALL fail the load with an error naming the channel — never mid-run.

#### Scenario: Host lacks a required channel
- **WHEN** a card containing `(from-host :wind)` is loaded on a host that provides no `wind` channel
- **THEN** the load fails naming `wind`, and no simulation tick runs
