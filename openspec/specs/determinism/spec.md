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

### Requirement: RNG draws are keyed by local causal context
A random draw MUST be a stateless mix of the run seed, the drawing context's key, and a local counter — never a read of shared sequential state. Context keys MUST derive hierarchically: root tasks from the seed and their creation ordinal, forked tasks from the parent task's key and the parent's local fork count, tick rules from the rule index, and every per-tick scope from its context key mixed with the tick. A draw's value MUST be independent of how many draws other contexts performed.
*Why:* the sequential stream made spawn order part of the replay contract; keyed draws are what let array spawning, scrubbing, and future parallel entity loops coexist. Landed 2026-08 (`rng-spawn-order-independence`): scope-based `next_rand` over `rng_mix`, task-tree keys, per-rule and per-load re-basing.

#### Scenario: Unrelated draws do not shift a task's stream
- **WHEN** two runs differ only in extra `(rand)` draws performed by a different task (or in the relative order of two spawns issued by different tasks)
- **THEN** the unmodified task's entities render identically in both runs

#### Scenario: Same seed still replays exactly
- **WHEN** two simulations boot the same card with the same seed and identical inputs
- **THEN** all draws, and therefore all render frames, are identical at every tick

### Requirement: Entities carry rng keys and capture vectors agree on site numbering
Each spawned element MUST receive a persistent rng key derived from the spawning scope and its element ordinal, stored in the entity store and cloned with snapshots. Capture vectors MUST draw site k from the element key, the signal node's ordinal in the figure's instantiation walk, the capture domain, and k — per-node keying, so sibling nodes' equal site numbers do not alias to the same draw. The compiled extraction (`draw_caps`) and the interpreted substitution walk (`subst_rand`) MUST assign identical site numbers to identical sites within a node. Agreement on temporal draw order is no longer required. Explicit non-goal: draw stability across card edits (edits may renumber sites; tapes are tied to a card version).

#### Scenario: Bail path matches compiled captures
- **WHEN** extraction bails and spawn falls back to form substitution
- **THEN** the substituted constants equal the values the capture vector would have carried for the same sites, because both derive from the same element key and site numbers

### Requirement: Rowful scratch evaluation contexts draw keyed randomness
Evaluation contexts that run against a scratch world while holding an entity row (evolve step and init application, pending-field functions, masked-update values, collider-projector bodies) MUST base their draws on the entity's rng key, a domain tag, and the tick — making uncaptured rand in these contexts per-entity, per-tick, and scrub-safe. Evolve cells further mix the stable node id and an init/step salt so co-located cells never share a stream. Rowless scratch contexts (direct signal evaluation, `FnPose`) keep a fixed base: uncaptured rand there is a deterministic constant by design.

#### Scenario: Random-walk evolve actually walks
- **WHEN** an evolve step body draws `(rand -1 1)` each tick
- **THEN** the drawn value differs across ticks and across entities, and rewinding then re-stepping to the same tick reproduces the identical value

### Requirement: Hot storage classes are f32 with rounding at the storage boundary

(Landed 2026-08, `f32-hot-columns`.) Hot per-entity storage classes —
integrator state, sampled/trace/root-frame poses, capture vectors,
user/meta numeric field cells, collider rows and AABBs, render batch
geometry — SHALL store f32. Control-plane values — interpreter
`Val`/`Form` numbers, signal channels, inputs, tick time tau and `dt`,
the RNG bit-to-float conversion, event log positions, spawn-time pose
math — SHALL stay f64. A value entering an f32 class SHALL be rounded
exactly once at that boundary and widened on every load; all
arithmetic between load and store SHALL stay f64 with unchanged
operations and order, on every tier and every path (compiled batch
fill, interpreted row walk, spawn bail substitution, env-capture
overlays). Bit-exactness between lowered tiers and the reference
interpreter therefore holds verbatim at the new widths, and oracle
asserts whose expected side is freshly computed SHALL round it through
the same boundary rather than loosen to a tolerance.

#### Scenario: Oracle stays bit-exact over f32 storage

- **WHEN** the simulation runs with `MAKU_LOWER_ORACLE=1` on f32
  columns
- **THEN** every compiled program's output still bit-matches the
  interpreted re-run, because both tiers read the same rounded storage
  and compute f64 between load and store

#### Scenario: Bail path substitutes the rounded draw

- **WHEN** extraction bails and spawn falls back to form substitution
- **THEN** the substituted constant equals the f32-rounded draw the
  capture vector carries, widened to f64

#### Scenario: Batch and row render fills agree

- **WHEN** the same render surface is produced by the compiled batch
  fill and the interpreted row walk
- **THEN** both round geometry through f32 at entry and the expanded
  rows compare bit-equal

### Requirement: Width migration drift is measured, not silently accepted

A change that narrows a storage class SHALL measure value drift
against the prior-width build over the card corpus (a meter reporting
per-card maximum delta over observable render output), and SHALL treat
any flipped behavioral outcome in the scripted card suites (hits,
grazes, kills, entity counts at specific ticks) as a
halt-and-investigate semantic verdict. Replay tapes are tied to an
engine width version; prior-width tapes are not value-compatible.
Measured 2026-08 (f64→f32, 11-card corpus, 600 ticks): structure
identical on every card, max |Δ| 0 to 1.2e-4 playfield units.

#### Scenario: Collision boundary flip

- **WHEN** rounding moves a position across a collision or cull
  boundary in a scripted suite card
- **THEN** the suite fails loudly and the round stops for
  investigation instead of re-pinning the outcome

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

