# load-time-schema Specification

## Purpose
Everything a card needs from its host or declares about itself is
knowable before the first tick. This capability pins the single
load-time pass over the loaded card: stream scoping (a free `$name` is
a load error), the host-channel manifest (the `(from-host ...)` sites,
verified by hosts so a missing channel fails the load, never mid-run),
the declared render manifest, and load lints — with entity field tables
joining the same pass as their columns become statically declarable.
## Requirements
### Requirement: One load-time schema collection pass
There SHALL be exactly one load-time pass over the loaded card — the walk that resolves stream scoping — and every load-time schema table SHALL be collected by it: the host-channel manifest (the `(from-host ...)` sites), load lints, and declared render kinds (`defrender-kind` declarations and the kinds standing rules statically emit — the card's render manifest). Entity field tables join the same pass as their columns become statically declarable (their kinds are value-dependent today, so those tables remain runtime-accreted); undeclared render kinds likewise remain runtime-accreted, scoped per kind. The pass runs at card load, before tick 0, on every load path (fresh load, swap/add fragments).

#### Scenario: Card load runs the pass
- **WHEN** a card is loaded
- **THEN** the host-channel manifest, the render manifest, and stream-scoping errors are produced by the single pass, before the first tick

### Requirement: Missing host channels fail at load
The set of `(from-host :name)` sites in the loaded card SHALL be checked against the channels the host provides at load time. A required host channel the host does not provide SHALL fail the load with an error naming the channel — never mid-run.

#### Scenario: Host lacks a required channel
- **WHEN** a card containing `(from-host :wind)` is loaded on a host that provides no `wind` channel
- **THEN** the load fails naming `wind`, and no simulation tick runs
