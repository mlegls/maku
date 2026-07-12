# evolve-semantics Specification

## Purpose
The kernel's one stateful signal constructor and the dyn ≅ t->T
equivalence: fold semantics, epochs, closed-vs-live sampling, sited
evolves. Rationale: `openspec/specs/evolve-semantics/spec.md`.

## Requirements
### Requirement: Dyns are callable and functions are dyns
Dyn values MUST be callable in application position â `(d 3.5)` samples at epoch-local t = 3.5, and curve dyns take the curve parameter as a second argument `(d t u)`. A plain `(fn [t] ...)` MUST be accepted anywhere a dyn is expected.
*Why:* dyn<T> â t -> T; application-as-sampling replaces a `sample` builtin. Rationale: `openspec/specs/evolve-semantics/spec.md`.

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
*Why:* per-slot epochs are the remat contract; this is how handle-preserving continuity (`(evolve (:pos e) ...)`) is expressed. See `openspec/specs/evolve-semantics/spec.md` and `openspec/specs/language/spec.md`.

#### Scenario: Partial remat
- **WHEN** `(remat h spec-map)` replaces one slot of an entity with two evolve-bearing slots
- **THEN** the replaced slot's evolve restarts (init re-evaluated, epoch t = 0) and the other slot's state and epoch clock are unchanged

### Requirement: Sited evolves evaluate to their settled state value
An `evolve` evaluated under an active scan context (inside a per-tick re-evaluated captured expression such as a vel component or rot expr) MUST be treated as a sited evolve: state keyed by ScanSite (enclosing node's lowered id + per-evaluation counter, stable for a fixed expr tree), init run at the site's first evaluation, step advanced when the context advances, and the expression evaluating to the settled state VALUE, not a dyn. Dyn capture sites MUST macroexpand forms before capture, so macro-produced evolves are collected as sites at spawn.

#### Scenario: Homing-slew shape
- **WHEN** a slew (a prelude macro over a sited evolve) appears inside a vel component
- **THEN** each tick the vel expression reads the slew's settled per-entity state, advancing once per tick with the enclosing node


## Design

Moved verbatim from docs/notes/evolve-design.md.

---


The kernel's one stateful signal constructor. Everything else in the dyn
family is either a plain function of time or lib code over `evolve`.

Naming: this was drafted as `scan`, but `scan` is reserved for the array
adverb (prefix-fold over sequences). The two are the same combinator over
different index domains — i -> T vs t -> T — but they return different
kinds of values (array of intermediates vs a dyn), so they get different
names. `evolve` = state evolving over ticks. Internal engine naming
(`ScanSite`, `scan_builtin_spec`, the `ctx.scan` context) may keep the old
word until it's convenient to sweep.

## The equivalence: dyn<T> ≅ t -> T

- Dyns are first-class values, callable as functions: `(d 3.5)` samples d
  at epoch-local t = 3.5. Curve dyns take the curve parameter as a second
  argument: `(d t u)`. This subsumes the `sample` builtin — application
  dispatching on the callee's type replaces the name.
- The equivalence runs both ways: a plain `(fn [t] ...)` is accepted
  anywhere a dyn is expected. The whole stateless family (`linear`,
  orbits, easings, ...) therefore needs no constructors — they are
  ordinary lib functions returning lambdas. `def` names them for free.
- `evolve` is legal in any expression position; it simply returns a dyn
  value, and that value's type is what constrains where it fits.

## evolve

```
(evolve init (fn [s ctx] step-expr))
```

- Returns a dyn whose value at tick n of its epoch is the n-fold
  application of the step to `init`.
- `init` is an expression evaluated at epoch start (so it can capture the
  current pose — this is how handle-preserving continuity through remat
  is expressed when wanted).
- The carried state is any value: scalar (`smooth`, `slew`), pose
  (`vel`, `pather`), small map (`stages` = `{:stage i :t local-t}`).
  A state that is a figure makes the dyn a pose/figure dyn.
- `ctx` is syntactically a map — `{:t :dt :tick}` (epoch-local t, tick
  duration, epoch-local tick index); destructure for direct values.
  `dt` in ctx is what keeps semantics tick-rate-aware: rate-independence
  is the step body's responsibility (`(+ p (* v (:dt ctx)))`).

## Step timing and epochs

- The step advances exactly once, at the tick boundary; it reads
  pre-tick state (same rule as change-col). Within-tick sampling sees
  the settled value.
- Per-slot epochs (remat contract): rematting a slot resets its evolve
  state to a fresh `init` evaluation and restarts epoch-local t/tick;
  untouched slots keep both.

## Closed vs live evolves — the sampling rule

- A **closed** evolve's step reads only `s`, ctx, and other dyns it
  closes over (sampled at the same t). Its fold is a pure function of
  epoch-local t: `(d t)` = fold of step over ticks 0..t. The engine's
  per-tick advance is memoized monotone sampling; random-access calls
  replay the fold (deterministic; cost is a profiling concern).
- A **live** evolve's step reads entity cells / live channels. It is
  still a valid slot dyn — advanced by the engine on the entity's clock —
  but `(d t)` off-clock is an error (its value depends on the input
  trace, not on t). The t->T equivalence holds exactly where it can hold.
- Cross-entity reads and RNG inside steps: forbidden initially
  (closedness is a capability and the columnar-lowering unit; revisit on
  demand).
- Liveness is classified SYNTACTICALLY at construction, rooted at the
  step fn's params: param-rooted keyword access ((:x s), (:dt c)) is the
  fold's own state and stays closed; capture-rooted access ((:hp e)),
  channel reads, rand, and world-reading heads mark the evolve live.
  Conservative in both directions (a false-live only forbids off-clock
  sampling; a false-closed errors at advance).
- Live evolves accept a one-behind cell in the post-boundary sampling
  window (after the world tick increments, before the new boundary's
  pass) where replay is impossible; closed evolves keep
  exact-match-else-replay so memoization stays invisible.
- `init` is a deferred thunk evaluated at epoch start (closed env for
  closed evolves, real env for live) — this is what makes
  `(evolve (:pos e) ...)` remat continuity work: the re-run init sees
  the post-remat world.

Note: the old `sample` special evaluated dyns against a fresh
`MotionState`, so stateful dyns sampled that way silently read
init-state. The closed/live rule replaces that accident with a
definition.

## Sited evolves — evolve identity inside dyn expressions

An `evolve` evaluated under an active scan context (inside a per-tick
re-evaluated captured expression: a vel component, a rot expr) is a
*sited* evolve: its state is keyed by ScanSite (enclosing node's lowered
id + a per-evaluation counter, stable for a fixed expr tree), its init
runs at the site's first evaluation, its step advances when the context
advances, and the expression evaluates to the settled state VALUE (not a
dyn) — inside the enclosing slot's clock, "the dyn sampled at the
ambient tick" IS that value. A sited evolve advances inside the
enclosing expression's evaluation against the real SigEnv, so
channel-reading targets need no extra classification (it has no
independent clock to sample off). Dyn capture sites macroexpand forms
BEFORE capture, so a macro expanding to an evolve is collected as a site
at spawn and expansion stays out of the hot loop. Known limitation:
cart/polar/rot capture guards (`contains_unbound_axis`) run on the raw
form, so a macro whose expansion introduces t-dependence is not
recognized as a dyn expression. Follow-up kept open: standalone evolve
steps do not yet provide a scan context, so a sited evolve nested in an
UNRECOGNIZED vel-like evolve's step errors (the vel expansion-shape
recognizer re-hosts components under Vel's scan context precisely to
keep the homing-slew shape working).

## What re-expresses over evolve (lib, then AST-rewrite fast paths)

- `vel`: `(evolve p0 (fn [p {:keys [dt]}] (+ p (* v dt))))`
- `slew` / `smooth`: scalar state moving toward / exponentially
  approaching a target.
- `stages`: state = stage index + stage-local t; step advances the
  machine.
- `pather` / `path`: pose state along a curve (or closed-form where the
  path is static).
- spatial `clamp`: stateless projection, composes *inside* a step
  (clamping integrator state per tick avoids wind-up).
- `linear` exits the family entirely: static velocity means the closed
  form `p0 + v*t`, a pure `(fn [t] ...)` in lib. `vel` is the
  integrator; `linear` never was.

These become the first real test of the minimal-kernel contract: the lib
definitions are the semantics; the engine recognizes their expansion
shapes for the optimized paths, never the names.

## Kernel consequences

- Kernel signal surface ≈ function application + `evolve`.
- `sample` builtin: delete once dyn application lands.
- Push/pull split stands: `evolve` is the pull-based per-slot stateful
  dyn (compilable to masked-SoA scan sites, one per slot); deftick rules
  + `change-col` are the push-based whole-entity `(e, ctx) -> e'` step.
  Same step semantics, two faces; the entity-level form is deliberately
  NOT the dyn kernel (a whole-entity function cannot be lowered
  per-column).

## Implementation order

1. DONE — dyn values callable in application position (`(d t)`, `(d t u)`);
   `(fn [t] ...)` accepted in dyn slots; `sample` deleted.
2. DONE — `evolve` special replaces the reserved `scan` stub, including
   engine-clock advance (EvolveCell val state, step once per boundary,
   on-clock reads hit the settled cell), deferred init, the liveness
   classifier, live evolves, and sited evolves (above).
3. Re-express the stateful family in lib. DONE for `slew`/`smooth`
   (prelude macros over sited evolves; `sf_stateful`,
   `scan_builtin_spec`, and the deferred-instance `Val::Thunk`
   mechanism retired; `smooth` is pose-only in lib — the old numeric
   arm returned a shape nothing used). Still open: `vel` (deferred to
   the model/ split — b.vel introspection, clamp_integrator, and the
   compiled integrand programs key on `DynNode::Vel`, so re-expression
   is pure surface until then) and `stages` (own round, likely over
   `states`: its corpus sites are a state machine over dyns, not a raw
   fold).
