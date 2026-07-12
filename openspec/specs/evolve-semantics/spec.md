# evolve-semantics Specification

## Purpose
The kernel's one stateful signal constructor and the dyn ≅ t->T
equivalence: fold semantics, epochs, closed-vs-live sampling, sited
evolves. Rationale: `docs/notes/evolve-design.md`.

## Requirements
### Requirement: Dyns are callable and functions are dyns
Dyn values MUST be callable in application position â `(d 3.5)` samples at epoch-local t = 3.5, and curve dyns take the curve parameter as a second argument `(d t u)`. A plain `(fn [t] ...)` MUST be accepted anywhere a dyn is expected.
*Why:* dyn<T> â t -> T; application-as-sampling replaces a `sample` builtin. Rationale: `docs/notes/evolve-design.md`.

#### Scenario: Sampling by application
- **WHEN** a dyn value is applied to a number
- **THEN** it evaluates to the dyn's value at that epoch-local time

#### Scenario: Lambda in a dyn slot
- **WHEN** a `(fn [t] ...)` is supplied where a dyn is expected (e.g. a motion slot)
- **THEN** it is used as the dyn without any conversion constructor

### Requirement: evolve is a tick-boundary fold
`(evolve init step)` MUST return a dyn whose value at tick n of its epoch is the n-fold application of `step` to `init`. `init` MUST be evaluated at epoch start (as a deferred thunk, so it can capture the current world, e.g. `(evolve (:pos e) ...)`). The step MUST advance exactly once per tick boundary and read pre-tick state; within-tick sampling sees the settled value. The step's `ctx` map carries `{:t :dt :tick}` (epoch-local), and rate-independence is the step body's responsibility.

#### Scenario: Fold value
- **WHEN** an evolve's dyn is read on-clock at tick n
- **THEN** its value equals step applied n times to the epoch-start init value

#### Scenario: Within-tick stability
- **WHEN** the same evolve is sampled multiple times during one tick
- **THEN** every read sees the same settled value

### Requirement: Closed evolves sample anywhere, live evolves only on-clock
Liveness MUST be classified syntactically at construction, rooted at the step fn's params: param-rooted access stays closed; capture-rooted entity reads, channel reads, rand, and world-reading heads mark the evolve live. A closed evolve's `(d t)` MUST equal the fold of the step over ticks 0..t (random access replays the fold; monotone sampling may memoize invisibly). A live evolve MUST still advance on its entity's clock, but off-clock application MUST be an error; in the post-boundary sampling window (after the world tick increments, before the new boundary's pass) a live read MAY observe the one-behind cell, while closed evolves keep exact-match-else-replay so memoization stays invisible. Cross-entity reads and RNG inside steps are forbidden (closedness is the capability boundary; revisit on demand).
*Why:* the t->T equivalence holds exactly where it can hold; closedness is also the columnar-lowering unit.

#### Scenario: Closed random access
- **WHEN** a closed evolve is applied at an arbitrary past t
- **THEN** the result equals replaying the fold to that t, regardless of memoization

#### Scenario: Live off-clock error
- **WHEN** a live evolve (e.g. one reading a channel in its step) is applied off-clock
- **THEN** evaluation errors rather than returning a stale or replayed value

### Requirement: Remat resets per-slot epochs
Rematting a slot MUST reset that slot's evolve state to a fresh `init` evaluation (seeing the post-remat world) and restart its epoch-local t/tick; slots untouched by the remat MUST keep both their state and their epoch clock.
*Why:* per-slot epochs are the remat contract; this is how handle-preserving continuity (`(evolve (:pos e) ...)`) is expressed. See `docs/notes/evolve-design.md` and `docs/language.md`.

#### Scenario: Partial remat
- **WHEN** `(remat h spec-map)` replaces one slot of an entity with two evolve-bearing slots
- **THEN** the replaced slot's evolve restarts (init re-evaluated, epoch t = 0) and the other slot's state and epoch clock are unchanged

### Requirement: Sited evolves evaluate to their settled state value
An `evolve` evaluated under an active scan context (inside a per-tick re-evaluated captured expression such as a vel component or rot expr) MUST be treated as a sited evolve: state keyed by ScanSite (enclosing node's lowered id + per-evaluation counter, stable for a fixed expr tree), init run at the site's first evaluation, step advanced when the context advances, and the expression evaluating to the settled state VALUE, not a dyn. Dyn capture sites MUST macroexpand forms before capture, so macro-produced evolves are collected as sites at spawn.

#### Scenario: Homing-slew shape
- **WHEN** a slew (a prelude macro over a sited evolve) appears inside a vel component
- **THEN** each tick the vel expression reads the slew's settled per-entity state, advancing once per tick with the enclosing node

