## ADDED Requirements

### Requirement: Batch map-remat shapes lower to masked updates
Tick-rule shapes of the form `(map (fn [b] (remat b …)) domain)` whose remat
spec touches statically-known slots SHALL lower to masked SoA updates: masked
epoch writes plus masked slot updates under the existing masked-update plan
domain, with evaluation bit-identical to the interpreted per-entity path.
Shapes with dynamic slot sets or impure update functions SHALL fall back to
the interpreted path unchanged.

#### Scenario: Uniform batch remat lowers
- **WHEN** a deftick rule maps a remat with fixed slots over an entity domain
- **THEN** the lowered masked update applies the same boundary-queued writes
  in the same order as the interpreter, verified under MAKU_LOWER_ORACLE

#### Scenario: Unsupported shape falls back
- **WHEN** the remat spec's slot set depends on per-entity data
- **THEN** the shape is not lowered and the interpreted path runs unchanged
