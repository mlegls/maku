## ADDED Requirements

### Requirement: vel is the stock integrator evolve over component columns
`vel` SHALL denote the single stock integrator evolve
`p(τ+dt) = p(τ) + v·dt` over per-entity component columns
(`:vel-x`/`:vel-y`): the surface form constructs the one recognized
integrator shape whose components are dyn programs and whose settled values
are readable entity columns. (The engine constructs the recognized shape
directly from the `vel` surface form; a pure prelude-macro face over raw
`evolve` is deferred to the kernel-shrink worklist and must not change this
contract.) The component columns are RESERVED: user fields on those names
are a spawn error, and CONCURRENT integrators in one motion tree (frame
parent+child) are a spawn error — `stages` segments are mutually exclusive
and SHALL share the columns (only the active segment steps and writes).
Polar components convert polar→cart inside the integrator's component
evaluation. Position remains a per-slot stateful dyn: epochs, closed/live
classification, and segment history are unchanged by the re-expression. The
closed/live classification of the motion slot SHALL derive from the
component programs (all-closed components → replayable exactly as before;
channel-reading components → live). Spatial `clamp` SHALL continue to
compose inside the integrator step (state clamped per tick, no wind-up).

#### Scenario: Surface and behavior are unchanged
- **WHEN** a card uses `(vel c[vx vy])`, `(vel p[r θ])`, or the trailing
  child/field-map sugar
- **THEN** spawn, motion, capture draws, and rendered trajectories are
  bit-identical to the pre-re-expression engine across the oracle card suites

#### Scenario: Components are readable columns
- **WHEN** a rule or view reads an entity's vel component fields
- **THEN** it sees the value the integrator evaluated this tick (the settled
  current-tick evaluation, not a stale or re-evaluated one)

#### Scenario: Clamped integrator recognition survives
- **WHEN** a `clamp` wraps a vel-shaped motion
- **THEN** the clamp applies to integrator state each tick (no wind-up),
  recognized from the stock integrator shape rather than a `Vel` node

#### Scenario: Concurrent integrators fail loudly
- **WHEN** a motion tree carries two integrators active at the same instant
  (e.g. a `vel` frame with a `vel` child)
- **THEN** spawn errors instead of silently cross-wiring their shared
  component columns

#### Scenario: Staged integrators share columns
- **WHEN** different `stages` segments each use `vel`
- **THEN** the card loads and runs — only the active segment's integrator
  steps and writes the component columns

### Requirement: The integrator owns one component evaluation per tick
Vel component programs SHALL be evaluated exactly once per tick, during the
motion step, against the same post-control environment as other motion
integrands; the step SHALL write the evaluated values to the component columns,
and the post-motion dyn-column refresh SHALL skip integrator-owned columns.
Sited evolves inside components (the homing-slew shape) advance with that
single evaluation.

#### Scenario: No double advance for sited evolves in components
- **WHEN** a slew appears inside a vel component
- **THEN** its state advances exactly once per tick, and the column read by
  later rule/render code equals the value the integrator consumed

### Requirement: Dyn field slots carry their own epochs
Every dyn-valued entity field slot SHALL carry its own epoch column.
Installing a dyn into a field (at spawn, via a remat field-map key, or via a
field write) SHALL restart that field's epoch; the field's dyn runs on
`τ_field = t − epoch_field`. Spawn-time installs anchor at birth, preserving
existing behavior. Motion remats MUST NOT touch dyn field epochs, and field
installs MUST NOT touch the motion slot. A static (number/keyword) field write
SHALL remove any dyn occupying that field slot — last writer wins, rather than
the dyn silently re-overwriting the static value on the next refresh.

#### Scenario: Mid-life fade starts at the event
- **WHEN** a rule remats `{:opacity (fade→0 over d)}` on a bullet at tick T
- **THEN** the fade's local time starts at 0 at tick T (not at the spawn tick)

#### Scenario: Fade survives a motion remat
- **WHEN** an entity with a half-finished `:opacity` fade has its motion slot
  rematted
- **THEN** the fade continues uninterrupted on its own epoch clock

#### Scenario: Soft-cull is expressible as lib code
- **WHEN** lib code implements `(soft-cull b d)` as an opacity-fade remat plus
  a deadline field and a stock cull rule
- **THEN** the bullet fades from the event tick and is culled at the deadline,
  with no engine verb involved

### Requirement: The F1 lint reports closed-form-integrable vel components
When a vel component program is closed-form-integrable (a constant or a
piecewise-affine `lerp` profile), card load SHALL emit a lint naming the
suggested closed rewrite. The compiler MUST NOT rewrite silently: a scan stays
a scan as written.

#### Scenario: Constant integrand lints
- **WHEN** a card spawns with `(vel c[100 0])`
- **THEN** load reports the closed `linear` equivalent as a suggestion and the
  motion remains Scanned as written
