# perf

## ADDED Requirements

### Requirement: Deltas are measured by interleaved wall-only A/B runs
A claimed performance delta SHALL be established by interleaved A/B runs of bare walls (`MAKU_WALL_ONLY=1`) in one sitting — never by comparing against a baseline number from an earlier session (machine-state drift has produced ±5% on identical commits).

#### Scenario: Landing a perf claim
- **WHEN** a round reports a wall delta
- **THEN** the numbers come from alternated baseline/candidate runs performed together

### Requirement: Profiled output is attribution only
Wall-only runs SHALL be the verdict on any delta; profiled walls and `sample` output are for attribution only — probe overhead has fully masked a +60% wall regression. Attribution ground truth is macOS `sample` on a release binary built with debug symbols.

#### Scenario: Profiled rows look equal
- **WHEN** profiled walls show no difference between two builds
- **THEN** the delta is still judged by wall-only runs before concluding no change

### Requirement: Rounds update the standing walls
A landed perf round SHALL update this spec's `## Current walls` section with same-session measurements of the standard cases.

#### Scenario: Round lands
- **WHEN** a perf change-set is committed
- **THEN** the walls table reflects the new measurements in the same round
