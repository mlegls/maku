# Session — delta for rng-spawn-order-independence

## ADDED Requirements

### Requirement: The RNG seed is host-settable and rides the command tape
The world MUST expose a host-facing seed reset (`reset_rng(seed)`), and construction MUST
seed by implicitly calling it with the default constant. A mid-run reset MUST be recorded
as a command on the program tape and replayed at its tick during seek and re-run; replay
fidelity is promised only for tape-recorded resets. The active seed MUST be carried by
world snapshots so scrubbing across a reset is exact.

#### Scenario: Seek across a mid-run reset
- **WHEN** a session records a seed reset at tick N, steps past it, and then seeks back
  before N and forward again
- **THEN** the timeline after N is identical on both passes

#### Scenario: Construction is an implicit reset
- **WHEN** a world is constructed without any host seed call
- **THEN** its draws equal those of a world explicitly reset to the default seed
