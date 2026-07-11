# evolve re-expression — vel/slew/smooth in lib (evolve-design step 3)

Status: DESIGN (round 7). Scope: `vel`, `slew`, `smooth`. `stages` is
deferred to its own round — its three corpus sites use exit slots,
`forever`, and `(fn [exit] ...)` handoff, a state machine over dyns that
deserves separate treatment (likely over `states`, not raw evolve).

Semantics per docs/notes/evolve-design.md; live/deferred-init machinery
per docs/notes/live-evolve-design.md (milestone 3, bec0735).

## The obstacle: evolve identity inside dyn expressions

Corpus census (2026-07): `slew` is ALWAYS expression-embedded — the
dominant shape is the homing pattern `(vel p[r (slew rate init target)])`,
plus `(rot (slew ...))`; `smooth` has one site, let-bound inside a dyn
expr and consumed by a `slew`. These forms live inside per-tick
re-evaluated captured expressions (DynNode::Vel components, RotExpr).

`evolve` today is identified by construction: the node Rc ptr keys its
state cell. A `slew` macro expanding to `(evolve ...)` inside a captured
expression would construct a FRESH EvolveDyn every tick — fresh ptr,
fresh cell, no state. The builtins dodge this with the scan-site
machinery: `sf_stateful` keys state by `MotionStateKey::ScanSite
{ base, index }`, where base is the enclosing node's lowered id and
index is a per-evaluation counter, stable for a fixed expr tree.

## Kernel rule: sited evolves

**An `evolve` evaluated under an active scan context is a *sited*
evolve: its state is keyed by ScanSite, its init runs at the site's
first evaluation, its step advances when the context advances, and the
expression evaluates to the settled state VALUE (not a dyn).**

Rationale for returning the value: inside a dyn expression the ambient
clock is the enclosing slot's clock; a dyn value produced there would
immediately be sampled at the same t (the closed-evolve composition
rule), and "the settled value at the ambient tick" IS that sample.
This is exactly what `slew`/`smooth` return today.

Mechanics:
- State cell family: the m1 Val cells (`EvolveCell { state, tick }`),
  keyed `ScanSite` instead of `Node`. `ScanIo` gains val plumbing
  (reads via `readers.vals` / state val cells; a `val_writes` vec
  mirroring `n2_writes`).
- Step ctx map `{:t :dt :tick}`: tick from the cell (increments per
  advance), dt from ScanIo, t = tick * dt. Epoch-local by construction
  (cells clear on motion remat, same as n2 sites).
- The step closure is re-built each evaluation (the `(fn [s c] ...)`
  subform is re-evaluated), but over the same captured env — identical
  behavior, no extra state.
- Liveness: a sited evolve advances inside the enclosing expression's
  evaluation, which already runs against the real SigEnv per tick —
  channel-reading targets work with no extra classification. (The
  closed/live classifier only governs standalone evolves' off-clock
  sampling; a sited evolve has no independent clock to sample off.)
- Site collection: `collect_scan_sites` learns to intern a Val site for
  `(evolve ...)` heads (today it interns N2 sites for `slew`/`smooth`
  heads by name).

Follow-up (not this round, keeps the door open): `apply_evolve_step`
can itself provide a scan context (base = the evolve's node id), which
would let sited evolves nest inside standalone evolve steps. Until
then, an `evolve` evaluated in a step body without a scan context is a
plain construction (returns a dyn) — and the vel expansion shape is
recognized back to DynNode::Vel precisely so its component exprs keep
their scan-context hosting (see below).

## Capture-time macroexpansion

Captured dyn-expression forms are stored unexpanded, and card macros
expand per evaluation — per tick, inside scan exprs. Two problems: (a)
`collect_scan_sites` walks the captured form at spawn and would see the
macro NAME (`slew`), not the `evolve` head it expands to; (b) per-tick
expansion is per-tick allocation.

Fix: dyn capture sites (sf_vel components, rot/RotExpr, cart/polar
ClosedPt exprs, curve shapes) macroexpand the form before capture:
`expand_macros(form, env, ctx, world)` — recursively expand unbound-head
macro calls (same head-resolution rule as evaluate: skip heads bound in
env / defs / '$'-syms; track local binders introduced by let/fn/loop
params so shadowed names are left alone, the rewrite.rs discipline).
Expansion is evaluation of macro bodies, so it needs ctx/world — both
available at every capture site. Captured forms are then post-expansion
ASTs, which is also what the minimal-kernel contract wants the lowerer
to see (recognition of EXPANSION shapes).

## Lib definitions (the semantics)

Prelude (language-level, genre-neutral):

```clojure
(defmacro slew
  ([rate target] `(slew ~rate ~target ~target))   ; init defaults to target
  ([rate init target]
   `(evolve ~init
      (fn [s c]
        (let [d (- (mod (+ (- ~target s) 180) 360) 180)   ; shortest arc
              lim (* ~rate (:dt c))]
          (+ s (clamp d (- 0 lim) lim)))))))

(defmacro smooth [k target]
  `(evolve ~target (fn [s c] (+ s (* ~k (- ~target s))))))
```

(Exact expansion TBD against available builtins — `mod`/`clamp`
arities, pose arithmetic for smooth's pose targets. The point: the
shortest-arc-in-degrees semantics of slew is expressible as ordinary
lib math; no engine op is semantically required.)

`vel` (kernel-adjacent, stays wherever coordinate sugar lives):

```clojure
(defmacro vel [coords & child]
  `(evolve (cart 0 0)
     (fn [s c] (+ s (* ~coords (:dt c))))
     ...child-wrapping...))
```

vel constructs at spawn time (action position), so the macro expands
once — no site rule needed for vel itself. Its components land in the
step body and are re-evaluated per tick there.

Notes forced by the census:
- Free `t` in components (`(vel c[(lerp .. t ..) 0])`): inside the step
  body, ambient t must mean epoch-local t. The engine binds `t` (and
  the ctx map) in step evaluation; the macro does not rewrite forms.
- Polar (`p[r th]`): expansion converts through `(polar r th)` inside
  the step; the recognizer maps it to `DynNode::Vel { polar: true }`.
- Trailing child / map / array sugar: expansion routes through a lib
  defn doing the runtime dispatch (`wrap_elem_fields` equivalent), or
  the recognizer keeps handling it — decide at implementation.

## Shape recognition (the fast paths)

Per the minimal-kernel contract: the engine recognizes the macro
EXPANSION shapes (AST patterns, never names) and rebuilds the existing
optimized nodes:

- vel expansion shape -> `DynNode::Vel { a, b, polar }` with the
  component forms extracted from the step body. This is not only perf:
  it re-hosts the components under Vel's scan context, so nested sited
  evolves (the homing slew) keep working this round.
- slew/smooth expansion shapes -> optionally the existing N2 scan-site
  ops. NOT required initially: the generic sited-evolve path replaces
  sf_stateful's inlined arithmetic with one closure application per
  site per tick, and the corpus volume is small (slew 53k calls/6ms in
  the round-6 profile). Add recognition only if the profile says so.
- Recognition failure = generic sited/standalone evolve semantics, not
  an error. One known gap until the follow-up lands: an UNRECOGNIZED
  vel-like evolve whose step hosts a sited evolve (no scan ctx inside
  standalone steps yet) — must produce a clear error, not the current
  `ctx.scan.unwrap()` panic.

## Milestones

1. **Sited evolves** (kernel): Val ScanSite cells + ScanIo val
   plumbing; `evolve` special sited path (settled-value return, ctx
   map, advance discipline); `collect_scan_sites` interns Val sites for
   evolve heads; scan-context panic -> error. Tests: a counter evolve
   inside `(rot ...)` advances once per tick, persists across ticks,
   clears on remat; off-advance evaluation returns settled state.
2. **Capture-time macroexpansion**: `expand_macros` with lexical-scope
   discipline; wired at all dyn capture sites; tests for shadowing
   (locally-bound macro name left alone) and for a macro expanding to
   an evolve inside a rot expr (site collected, state persists).
3. **Lib macros + special removal + vel recognition**: prelude slew/
   smooth macros; vel macro; remove the `vel`/`slew`/`smooth` special
   arms and `scan_builtin_spec` name recognition; vel expansion-shape
   recognizer -> DynNode::Vel; corpus + oracle suites green; profile
   gate (slew/smooth generic path acceptable; vel unchanged via
   recognition).

`stages` re-expression: next round, over the `states` machinery.
