# determinism — delta for f32-hot-columns

## ADDED Requirements

### Requirement: Hot storage classes are f32 with rounding at the storage boundary

Hot per-entity storage classes — integrator state, sampled/trace/
root-frame poses, capture vectors, user/meta numeric field cells,
collider rows and AABBs, render batch geometry — SHALL store f32.
Control-plane values — interpreter `Val`/`Form` numbers, signal
channels, inputs, tick time tau and `dt`, the RNG bit-to-float
conversion, event log positions, spawn-time pose math — SHALL stay
f64. A value entering an f32 class SHALL be rounded exactly once at
that boundary and widened on every load; all arithmetic between load
and store SHALL stay f64 with unchanged operations and order, on every
tier and every path (compiled batch fill, interpreted row walk, spawn
bail substitution). Bit-exactness between lowered tiers and the
reference interpreter therefore holds verbatim at the new widths, and
oracle asserts whose expected side is freshly computed SHALL round it
through the same boundary rather than loosen to a tolerance.

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
per-card maximum position delta), and SHALL treat any flipped
behavioral outcome in the scripted card suites (hits, grazes, kills,
entity counts at specific ticks) as a halt-and-investigate semantic
verdict. Replay tapes are tied to an engine width version; prior-width
tapes are not value-compatible.

#### Scenario: Collision boundary flip

- **WHEN** rounding moves a position across a collision or cull
  boundary in a scripted suite card
- **THEN** the suite fails loudly and the round stops for
  investigation instead of re-pinning the outcome
