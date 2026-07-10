# evolve — design (settled 2026-07)

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

Note: the old `sample` special evaluated dyns against a fresh
`MotionState`, so stateful dyns sampled that way silently read
init-state. The closed/live rule replaces that accident with a
definition.

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
2. DONE (closed form) — `evolve` special replaces the reserved `scan`
   stub. Closed evolves only: evaluation replays the fold from epoch
   start (pure in tau; `Val::Evolve` + stateless `DynNode::Evolve`, no
   per-entity motion state). Steps run against a closed SigEnv (defs
   yes, channels/cells no), enforcing the closed rule. Live evolves
   (engine-clock advance through ScanSite state + per-slot epochs) and
   memoized monotone sampling are the follow-up — driven by remat and
   profiling respectively.
3. Re-express `vel`/`slew`/`smooth`/`stages`/`pather` in lib; keep the
   builtins as recognized expansion shapes (or delete + rewrite-match,
   per profiling).
