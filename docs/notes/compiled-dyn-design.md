# compiled dyn — implementation design

Status: stance REVISED 2026-07 (docs/notes/TODO.md "Compile dyn evaluation
to a flat program"): load-time lowering to the NumProgram IR now, with a
JIT/native-codegen tier as the planned destination — codegen compiles the
same IR per distinct program behind the same (program, input lanes,
scratch) executor boundary, and the IR interpreter loops become the cold
fallback. The control plane stays interpreted permanently; per-entity hot
loops (dyn columns, projector bodies, tick rules) are the replacement
target, dyn first. Interim rules for JIT-readiness: narrow executor
boundary, total callback-free ops (Interp fallback op = the one
interpreter re-entry), captures/rand as input slots (per-site program
sharing), structural interning as the compile cache key, and bit-exact
f64 semantics across tiers (oracle + replay determinism). This doc turns
the stance into a plan. Profile motivation (aggregate,
8 representative cards, post entities-where/value-or work): dyn:vel 879ms and
dyn:closed-pt 846ms self are the top two rows; dyn:frame 410ms is mostly
dispatch around them.

## Cost anatomy (what the interpreter pays per entity per tick)

A `DynNode::ClosedPt`/`Vel` evaluation (`motion.rs::dyn_node_pose_u_in`)
pays, per COMPONENT eval (`eval_sig_at_rate`):

1. A fresh `World::default()` — full World construction, immediately dropped
   ("signals never touch the world", but the evaluator signature demands one).
2. `sig.clone()` (SigEnv Rc clones) + a fresh `Ctx` with two fresh Rc-wrapped
   empty HashMaps.
3. `env.bind("t").bind("u")` (+"pos") — fresh Env nodes and Rc<str> key
   allocs per eval.
4. `read_scan_in` — clones the passed MotionState into a fresh
   Rc<RefCell<ScanIo>> (empty in the dense-reader era, so cheap — the real
   per-eval state cost is item 5's setup, not this clone), plus a node_ids
   borrow + key lookup.
5. Interpreting the Form tree: per node an enum dispatch, symbol lookups
   through Env (linked lookup), builtin dispatch by string match, Val
   allocations for every intermediate.

ClosedPt does this ×2 components ×2 samples (finite-difference heading) = 4
full passes; Vel ×2 plus the dense-state read. `dyn:frame` then composes
poses it re-derived from scratch. None of this work varies at steady state:
the forms, envs, scan-site set, and state slots are all fixed at spawn.

## Shape of the fix

Lower each signal Form to a flat register program once, share it across the
spawn group, and evaluate it against fixed scratch:

```rust
struct NumProgram {
    ops: Vec<NumOp>,          // topological, register-addressed
    n_regs: u16,
    inputs: Vec<InputSlot>,   // t, u, pos.x, pos.y, dt, captures…
    scan_sites: Vec<ScanSlot> // resolved n2 slot ids (schema keys)
}

enum NumOp {
    Const { dst, v: f64 },
    Input { dst, slot: u16 },
    Add { dst, a, b }, Sub…, Mul…, Div…, Neg…,
    Sin…, Cos…, Atan2…, Sqrt…, Pow…, Min…, Max…, Abs…, Floor…, Mod…,
    Select { dst, cond, then_r, else_r },       // strict if over nums
    ValueOr { dst, x, d },                      // %value-or over num-or-NaN
    ReadScan { dst, site: u16 },                // slew/smooth state read
    Channel { dst, chan: u16 },                 // per-tick refreshed input
    Interp { dst, form: u16 },                  // FALLBACK: boxed interpreter call
}
```

- Registers are `f64`; evaluation is a dumb `for op in ops` loop over a
  scratch `Vec<f64>` owned by the caller (Sim), reused across entities.
- `Interp` is the coverage escape hatch: any subform the lowering can't
  classify becomes one boxed call back into `eval_sig_at_rate` for THAT
  subtree only. A program that is 100% Interp is exactly today's cost; every
  classified node is pure win. This makes the pass incremental and never
  blocks a card from loading.
- Nothing/keyword-valued signal exprs stay on the interpreter (numeric
  programs only); the lowering rejects them at classification time.

### Inputs, not substitution (rand + env captures)

Two things currently SPECIALIZE the form tree per entity and would defeat
program sharing:

- `instantiate_rand` (`spawn.rs`) rewrites `(rand a b)` sites to per-entity
  `Form::Num` — fresh Form trees per spawn element.
- Spawn-local env bindings (repeater vars, spawn params) differ per element
  while the form is shared.

The compiled form inverts both: compile the PRE-substitution form once, with
each rand site and each free env variable classified as an `Input` slot. The
per-entity data is then a small capture vector (`Vec<f64>` or slots in the
entity SoA), filled at spawn — one program per spawn SITE, shared by the
whole group, which is also what group evaluation (TODO: AxisSel/SS5
interchange) needs. Env variables bound to non-numeric values make the
referencing subtree an `Interp` fallback.

RNG determinism is preserved: rand draws still happen at spawn in the same
order (they fill capture slots instead of rewriting forms).

### Scan sites and state

`slew`/`smooth` (ScanSite keys) become `ReadScan` ops in the eval program;
their per-tick ADVANCE stays where it is (`step_motion_in` writes n2 cells)
until milestone C. The program stores resolved `StateN2SlotId`s from the
existing motion schema — the schema is closed at load (stages lowering
guaranteed this), so slot resolution at compile time is total. `t`/`u`/`pos`
are ordinary inputs. Channels are inputs refreshed once per tick per program
run, not per entity.

### Where compilation hooks

`DynNode::ClosedPt`/`Vel`/`RotExpr`/`Path{progress}` gain an
`Option<Rc<NumProgram>>` (or a compiled/interpreted enum) built by a
`lower_sig(form, env, scan_schema) -> Option<NumProgram>` pass at DynNode
construction — which happens at card load for spec-store dyns and at spawn
for ad-hoc constructions; both are cold. `dyn_node_pose_u_in` checks for the
program and runs it with a scratch buffer from `MotionEvalCtx` (add
`scratch: &RefCell<Vec<f64>>` or pass through Sim); fallback to today's path
when absent.

## Milestones

### A — closed numeric expressions under ClosedPt/Vel — DONE (first slice)

Landed. Deviations/notes vs the plan below:
- Lowering is per-node at first eval (OnceCell on ClosedPt/Vel/RotExpr),
  over the POST-rand-substitution form — no capture-vector plumbing yet.
  Rand-free nodes are Rc-shared per spawn site so they lower once;
  rand-bearing nodes lower per entity at spawn (cold). The input-slot
  treatment moves to milestone B where sharing becomes load-bearing.
- All-or-nothing per form; no Interp fallback op yet. Unlowerable forms
  keep the interpreter per node.
- Heads shadowed by sig.defs bail (signal eval runs with empty
  macros/patterns, so defs are the only shadowing channel besides env).
- The Vel and RotExpr STEP arms reuse the programs: lowered integrands
  are scan-free, so the Vel step is a compiled sample and the RotExpr
  step is a no-op.
- Oracle: MAKU_LOWER_ORACLE=1 dual-runs and asserts 1e-9 agreement;
  card suites pass with it on.
- Coverage/result: 40% of closed-pt and 58% of vel evals compile
  (compiled evals ~30x cheaper: dyn:vel-c 305k evals / 19.5ms).
  Aggregate: dyn:closed-pt 277→179 + 11ms-c, dyn:vel 257→114 + 20ms-c,
  `*` 587→461ms (count 3.8M→2.8M). Remaining uncompiled evals are forms
  using user defns/scan sites/non-numeric captures — the Interp fallback
  op and richer classification are the coverage levers.
- Follow-up (landed): user-defn INLINING — bare numeric defs and literal
  `(fn [..] body)` def heads beta-reduce at lower time under the
  interpreter's def-scope rules (params → slot t/u → defs → builtins;
  never caller captures/pos/channels; depth-capped). Sound because
  signal slots now enforce live-only cell reads (Ctx.signal_scope,
  language.md control-cells) — without that pin, every pattern env's
  cell scope disabled inlining (measured: 100% of corpus lowerings).
- Measured after inlining: corpus coverage UNCHANGED (the corpus' defs
  don't sit on the hot bail paths). The actual remaining interpreted
  volume, by bail census (MAKU_LOWER_STATS-style dump, 2026-07): (a)
  225 per-entity homing-slew nodes
  `(slew 60 90 (angle-of (- (live $chan) pos)))` — needs ReadScan +
  Channel ops (milestones B/C) and angle-of/pose math over input pairs;
  (b) `lerpsmooth` with a static easing-kind symbol arg — an op away
  (389k builtin calls / 83ms aggregate); (c) keyword reads on captured
  values (`(:x delta)` on a captured pose could fold to Const at lower
  time; `(:vel exit)` map reads likewise when the capture is a literal
  map of nums/poses). These, not defs or the Interp fallback op, are
  the next coverage levers in expected-value order.
- Follow-up (landed, 300b6cb): (b) and (c) — a LerpSmooth op keyed on a
  statically resolved easing kind (env → defs → builtin resolution
  mirrored per scope; env-captured Val::Builtin under another name also
  resolves), and Form::Kw-head lowering. Discovery: the original
  `:x`/`:y` arm was DEAD — it matched a 3-item Sym-head shape the
  reader never emits (accessor sugar reads to 2-item Kw-head lists), so
  `(:x pos)` had never lowered and PosX/PosY ops never fired. Now:
  `(:x pos)`/`(:y pos)` emit pos ops when `pos` is unshadowed (a
  captured `pos` bails — it wins at non-pos eval sites), and keyword
  chains rooted at env-captured Maps/Poses fold to Const (sound because
  each DynNode's program cache sits beside its own captured env; def
  scope bails on all keyword access — F12 fresh-env rule). Remaining
  lever: (a) slew — milestone B/C machinery.

Original plan:

1. `interp/lower.rs`: `NumProgram` + `lower_sig` classifying: literals, `t`,
   `u`, `pos` component reads, captured numeric env vars, pure numeric
   builtins (the `is_builtin` pure table ∩ numeric ops), `if` with numeric
   arms (as strict Select — NOTE semantic check: interpreter `if` is lazy;
   Select is safe only when both arms are total numeric exprs, i.e. cannot
   error/diverge — division is total over f64, so numeric arms qualify;
   anything else → Interp fallback for the whole `if`), `%value-or` (numeric
   arms; Nothing encoded as NaN-boxing is NOT safe — instead classify only
   when `?x` is a scan read or channel with a known-num default, else
   fallback), rand-site inputs, channel reads.
2. Capture vector plumbing through spawn (replaces `instantiate_rand`'s form
   rewrite for compiled programs; keep the rewrite for fallback nodes).
3. Eval integration for ClosedPt (2 components × 2 taus over one program —
   run the program twice with different `t` inputs, no re-setup) and Vel.
4. Oracle: a debug/test mode that runs both paths and asserts agreement
   within 1e-9 per eval (behind a cfg or env flag, used by the corpus
   suites in CI once, not in release).

Exit criteria: corpus suites green; profile shows dyn:vel/closed-pt self
collapsing into a new `dyn:compiled` row that is a small fraction of the
current 1.7s.

### B — group evaluation + AxisSel lanes — first slice DONE (round 19)

Evaluate one program once per GROUP per tick where the only per-entity
inputs are `t`-offsets and capture slots: batch rows sharing a spawn site,
loop entities in the inner loop per op (SoA scratch: `Vec<f64>` per
register over the batch). This is where AxisSel (array-valued shared meta)
stops being evaluated per entity: the shared program runs once, lanes
scatter.

Landed (round 19), deviations vs the plan above:
- **Batched Vel steps** (`lower::run_lanes`, `sim::VelBatchScratch`):
  rows whose figure is constant wrappers (ConstFrame/Translate) over one
  compiled-integrand Vel node — the dominant bullet shape — collect as
  lanes during the scan-step walk, grouped by program-pair address, and
  run as one lane-batched program per component (one op decode per op
  per group; registers at `regs[r*n + lane]`, always-fresh dst makes
  `split_at_mut` legal). Integrator writes go straight to the n2 columns
  (`state_n2_at_slot`); single-cell schemas resolve the slot without
  hashing. Lanes are bit-identical to scalar runs by construction, and
  MAKU_LOWER_ORACLE=1 interpreter-checks every lane.
- **pos_only pose fast path** (`vel_chain_ptr`/`wrapper_chain_pos_pose`):
  the pos_only pose of that same shape is integrator state pushed through
  the constant wrappers (the Vel arm never evaluates its integrand when
  theta is discarded), so the collide fill and cull loops read it
  directly — no readers, no dispatch. Oracle asserts exact equality with
  the interpreted pose.
- **Spec-store dedup was NOT needed for this slice**: rand-free spawn
  groups already share node Rcs (and OnceCell programs), so grouping by
  program address works at ring granularity (~20 lanes in the fruit
  census), which already amortizes the per-row overhead. Structural
  program interning would fuse per-ring groups across spawns; it's a
  widening lever, not a prerequisite.
- Result (scaled fruit rig, 12k ticks, same-session): 5.86s → 3.84s
  (−34%); `step_motion_in`, `dyn_node_pose_u_in`, and per-row
  `motion_readers` collapsed out of the profile top; collider
  materialization is now the top row.

Still open under B: input slots (captures/rand as data — one program per
spawn SITE; unlocks cross-spawn sharing for rand-bearing groups and is
what the capture-vector plumbing below describes), ClosedPt group pose
evaluation (the pose fill for closed shapes still runs per row), AxisSel
lane scatter, and — per the milestone-A bail census — ReadScan + Channel
ops for the homing-slew integrands.

### C — beyond figure signals

- Scan ADVANCE (step_motion) ops join the program (fused eval+step).
- dyn cols (`refresh_dyn_cols`) run their `DynNum` programs on the same
  machinery.
- Projector bodies and tick-rule bodies follow onto flat programs
  (rules emit ordered effects — those stay sequential; only the pure
  per-row math compiles).

The RENDERER half of the rule/projector piece is DONE (round 8, out of the
planned order because `emit`/`%value-or` were the top two profile rows):
- `deftick` bodies are macroexpanded ONCE at registration (`sf_deftick` runs
  round-7 `expand_macros` and stores the expanded forms in `StandingRule`) —
  per-tick macro dispatch in rule bodies is gone, aligned with capture-time
  expansion semantics (card macros are fixed at load).
- `interp/rulelower.rs` recognizes the expansion shape
  `(map (fn [e] (let [p (:pos e)]? (emit :render {literal kw map}))) (entities-where (fn [x] ...)))`
  — pred must pass the existing `row_predicate`, `:shape` must be a literal
  `:point`/`:dot`, and each map value must classify as Num/Kw literal,
  `(:x|:y|:th p)` pose read, `(:field e)` field read, or
  `(%value-or (:field e) <literal-or-pose-read>)`. All heads
  shadow-checked against the rule env and sig.defs; any deviation bails the
  WHOLE form to the interpreter (all-or-nothing, same stance as milestone A).
- Execution (`Sim::run_compiled_tick_form`) resolves predicate symbols once
  per rule per tick, iterates alive rows in index order, reads the pose via
  the shared `entity_pose_at` (refactored out of `entity_field_at` so the
  paths cannot drift), and builds rows through the SAME `RenderRowFields`
  push/finish the interpreter uses (alias handling, schema checks, and error
  messages stay identical), pushing straight into `world.render_rows`.
- MAKU_LOWER_ORACLE=1 dual-runs each compiled form per tick and asserts the
  emitted `RenderRow`s are exactly equal (RenderRow/RenderData now derive
  PartialEq; both paths run identical arithmetic so exact f64 equality is
  correct).
- Not covered (interpreter fallback, by design): polyline/curve-samples
  rows (the beam rule — conditionals + deferred geometry), non-literal row
  maps, extra let bindings, and cull/field-write rules. Those are the
  remaining rule-lowering surface, alongside projector bodies.

Numeric row predicates are DONE (round 9): RowPredicate conjuncts extended
with `NumCmp` tests over a total numeric expression grammar (literals,
`inf`, `(:t e)`/`(:tick e)`, `(%value-or (:col e) default)`, 2-arg
arithmetic). Semantics parity is guaranteed by construction (only total
reads compile; bare col reads bail at recognition) plus a runtime bail: a
numeric read hitting a keyword-valued sym field aborts the compiled query
and reruns the interpreted fallback, reproducing the interpreted error
exactly — recognized predicates are pure, so the rerun is safe. Compiled
render rules complete their predicate scan before any row body (matching
entities-where-then-map phase order) and fall back whole on the same
signal. The existing resolve_predicate_query oracle covers the extension
with no new wiring.

Data-parallelism (rayon over batches) is deliberately NOT part of A/B; the
compiled form makes it nearly free later (pure lanes, fixed scratch,
deterministic merge), per the TODO stance.

## Cheap wins worth pulling forward (independent of the IR)

Ordered by expected value; all are interpreter-path fixes the compiled path
obsoletes but that de-risk the interim:

1. `Sim::motion_readers(row)` is called per entity per PHASE (scanned step,
   trace, cull, collision, render, entity-view pose reads) and each call
   builds a `state_n2_snapshot` HashMap, a `state_dyn_snapshot` HashMap, a
   full CLONE of the schema's `node_ids` map into a fresh RefCell, three Rc
   allocations, and two boxed closures. All of it is derivable from
   (schema, row) without copying: share the schema's node_ids as an
   Rc built once per schema (entity schemas are complete — lazy seeding
   only exists for ad-hoc direct evaluation), and make the readers close
   over the SoA columns + row index instead of snapshots. This is the
   dominant fixed overhead multiplier on every dyn row in the profile.
2. `eval_sig_at_rate` builds `World::default()` + fresh Ctx per eval —
   thread-local scratch World (reset tick-rate only) and a reusable Ctx
   template would cut fixed overhead from every remaining interpreted eval.
3. ClosedPt evaluates a and b at tau AND tau+eps solely for heading; when
   the consumer discards theta (e.g. `:pos` reads, collider centers) the
   second sample is waste — plumb a `need_theta: bool` through
   `dyn_figure_pose_in` call sites that provably drop theta.

## Testing strategy

- The dual-run oracle (milestone A.4) is the primary equivalence tool.
- Unit tests per op class: lowering rejects (impure builtin, non-numeric
  env capture, keyword result) fall back to Interp and still evaluate
  correctly end-to-end.
- Determinism test: spawn group with rand sites — same seed produces
  identical trajectories pre/post compilation (capture-vector draws happen
  in the old substitution order).
- The 4 ignored card suites remain the semantic oracle.

## Order of work

Milestone A is self-contained and directly attacks the top two profile rows.
B rides on A + spec-store dedup. C is post-B, sequenced with the rule/
projector lowering. The cheap wins can land any time (small, reviewable,
suite-verified) and are worth doing first if A takes more than a session.
