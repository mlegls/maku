## ADDED Requirements

### Requirement: One load-time schema collection pass
Channel manifests, per-kind render row schemas, and entity field tables SHALL be collected in one load-time schema pass — shared machinery, separate tables where the columns differ. The pass runs at card load, before tick 0.

#### Scenario: Card load builds all schema tables
- **WHEN** a card is loaded
- **THEN** the host-channel manifest, render row schemas, and entity field tables are all available before the first tick, from a single collection pass

### Requirement: Missing host channels fail at load
The set of `(from-host :name)` sites in the loaded card SHALL be checked against the channels the host provides at load time. A required host channel the host does not provide SHALL fail the load with an error naming the channel — never mid-run.

#### Scenario: Host lacks a required channel
- **WHEN** a card containing `(from-host :wind)` is loaded on a host that provides no `wind` channel
- **THEN** the load fails naming `wind`, and no simulation tick runs
