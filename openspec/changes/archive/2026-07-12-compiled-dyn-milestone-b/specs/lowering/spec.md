# lowering delta — compiled-dyn milestone B remainder

## ADDED Requirements

### Requirement: Auxiliary inputs are driver-filled lanes
Scan-cell reads and channel/stream reads SHALL enter compiled programs as program-declared input tables whose values the driver resolves and passes in before the run — through the row's motion readers for scan cells and through the same SigEnv snapshot the interpreter would read for channels. Compiled ops SHALL remain total and callback-free; a missing or mistyped auxiliary value SHALL bail that evaluation at the driver level and rerun interpreted.

#### Scenario: Homing-slew integrand
- **WHEN** a motion signal contains a sited evolve read and a live channel read (the homing-slew shape)
- **THEN** it lowers to a program with scan/channel input tables, the driver fills the aux lanes at each run, and the result is bit-identical to the interpreted evaluation

#### Scenario: Missing scan cell
- **WHEN** a compiled program with a scan input evaluates before the site's first advance has stored a cell
- **THEN** the driver bails that evaluation and reruns it interpreted, with no error path inside the program

### Requirement: Group evaluation preserves scalar parity
Batched or shared evaluation of one program across a group — lane-batched pose fills, and once-per-group evaluation of shared array-valued signals with per-row lane scatter — SHALL produce results bit-identical to evaluating each row through the per-row path, and the lowering oracle SHALL check this per lane when enabled.

#### Scenario: ClosedPt pose fill as lanes
- **WHEN** rows whose figure is constant wrappers over one compiled closed-point node need pos-only poses for a phase
- **THEN** grouped rows run as lanes of one program-pair run followed by per-row wrapper composition, equal to each row's individual evaluation

#### Scenario: Array-valued spawn meta
- **WHEN** entities of one spawn group carry an axis-selected shared signal
- **THEN** the shared expression evaluates once per group per tick and each row selects its own lane, equal to per-row evaluation and selection
