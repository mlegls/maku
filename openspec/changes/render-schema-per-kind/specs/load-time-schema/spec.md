## MODIFIED Requirements

### Requirement: One load-time schema collection pass
There SHALL be exactly one load-time pass over the loaded card — the walk that resolves stream scoping — and every load-time schema table SHALL be collected by it: the host-channel manifest (the `(from-host ...)` sites), load lints, and declared render kinds (`defrender-kind` declarations and the kinds standing rules statically emit — the card's render manifest). Entity field tables join the same pass as their columns become statically declarable (their kinds are value-dependent today, so those tables remain runtime-accreted); undeclared render kinds likewise remain runtime-accreted, scoped per kind. The pass runs at card load, before tick 0, on every load path (fresh load, swap/add fragments).

#### Scenario: Card load runs the pass
- **WHEN** a card is loaded
- **THEN** the host-channel manifest, the render manifest, and stream-scoping errors are produced by the single pass, before the first tick
