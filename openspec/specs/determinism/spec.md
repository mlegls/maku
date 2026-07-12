# determinism Specification

## Purpose
Why replays, the lowering oracle, and cross-tier equivalence are
trustworthy: one contract governing op order, math shims, RNG stream
order, and fallback behavior. Rationale:
`openspec/specs/lowering/spec.md`, `openspec/specs/perf/spec.md`.

## Requirements
### Requirement: Lowered execution is bit-exact against the interpreter
Every lowered/compiled evaluation tier (IR interpreter loops today; JIT/native and wasm kernels later) MUST produce bit-identical results to the reference interpreter: same operations, same operation order, shared math shims (no platform libm variance, no fast-math), and the same numeric width per storage class.
*Why:* replay/scrub, the lowering oracle, and cross-host reproducibility all assume one answer per program. Rationale and gap list: `openspec/specs/lowering/spec.md`.

#### Scenario: Oracle dual-run
- **WHEN** the simulation runs with `MAKU_LOWER_ORACLE=1`
- **THEN** every compiled program's output is checked against an interpreted re-run of the same forms and any mismatch panics with the divergence site

#### Scenario: New lowering surface
- **WHEN** a change adds a compiled path for a previously interpreted surface
- **THEN** the path is oracle-instrumented before landing, and the oracle re-run keys state and inputs through the same per-entity values (e.g. a batch lane's own capture vector and node)

### Requirement: Changes to lowering or hot paths pass the oracle gates
A change touching lowering, motion/render/collision hot paths, or numeric evaluation MUST pass the full core unit suite and the ignored oracle card suites (`MAKU_LOWER_ORACLE=1 cargo test --release -- --ignored`) before landing, verified first-hand.
*Why:* the card corpus is the semantic oracle; unit tests alone have missed order-of-evaluation regressions. Process detail: `openspec/specs/perf/spec.md`.

#### Scenario: Landing a perf round
- **WHEN** a perf/lowering change-set is ready to commit
- **THEN** both gates run green on the exact tree being committed

### Requirement: Replay is deterministic
Running the same card with the same seed and the same input trace MUST produce identical simulation states and render frames at every tick, across sessions and across lowering tiers.

#### Scenario: Same-seed re-run
- **WHEN** two simulations boot the same card with the same seed and step the same number of ticks with identical inputs
- **THEN** their render outputs are equal at every tick

### Requirement: RNG draws consume one sequential stream in defined order
Random draws MUST consume the single sequential splitmix stream in the order defined by the interpreted substitution walk. Optimizations that move draws (e.g. spawn-time capture vectors over marker programs) MUST preserve the exact draw order, including bail/fallback paths.
*Why:* the draw order IS the replay contract. Known limitation (current behavior, not a guarantee to preserve): spawn-order independence does not hold — reordering spawns shifts the stream; tracked as the `rng-spawn-order-independence` change.

#### Scenario: Capture-vector extraction
- **WHEN** a spawn site's rand expressions are extracted to per-entity capture vectors
- **THEN** trajectories are identical to the per-entity substitution semantics for the same seed

### Requirement: Compiled-path failures fall back to interpretation exactly
When a compiled pass cannot complete (unlowerable form, runtime kind surprise, schema violation), the driver MUST discard the compiled attempt without world effects and re-run the pass interpreted, reproducing the interpreted behavior, error, and error site exactly.
*Why:* all-or-nothing at the driver level keeps kernels total and error-free — the JIT totality contract. See `openspec/specs/lowering/spec.md`.

#### Scenario: Batch abort
- **WHEN** a batch render fill hits a field whose kind contradicts the schema mid-pass
- **THEN** the batch is discarded with no schema registrations or partial rows committed, and the row-at-a-time re-run raises the same error at the same row

### Requirement: Cross-lane combining uses a fixed merge order
Kernels MUST NOT read across lanes or touch world state during a run, and all cross-lane combining (frame item order, collision index build, channel accumulation) MUST occur in a fixed merge order independent of thread schedule, so any legal schedule — including single-threaded wasm — produces bit-identical output.
*Why:* parallelism is a backend/driver property, not an IR marking; invariants recorded in `openspec/specs/render-rows/spec.md` "Parallelism".

#### Scenario: Thread-count invariance
- **WHEN** the same tick's batch work executes on one thread or many
- **THEN** the resulting frame, collision events, and channel values are bit-identical

