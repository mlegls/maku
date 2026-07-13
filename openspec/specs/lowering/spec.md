# lowering Specification

## Purpose
The typed fixed-width lowering architecture: how per-entity hot loops install
validated `KernelProgram` computations behind domain `KernelPlan` bindings,
with driver-owned fallback, merge order, and specialized CPU artifacts. The
same callback-free boundary is the seam future native/wasm/GPU tiers consume.
## Requirements
### Requirement: The executor boundary is lanes plus scratch
Batch call sites SHALL hand executors a compiled program, input lanes, and scratch storage — never reaching into op internals — and compiled ops SHALL be total and callback-free. This boundary is the seam a JIT/native tier drops into behind the same signature.

#### Scenario: Vel batch
- **WHEN** rows sharing one compiled integrand program run as a batch
- **THEN** they execute as lanes of one program run through the `run_lanes`-shaped boundary, writing output columns directly

### Requirement: Programs classify all-or-nothing
A surface expression SHALL either lower to a complete compiled program or stay fully interpreted — no partial compilation with interpreter re-entry from inside a program. Runtime aborts (e.g. a numeric read hitting a keyword-valued field) stay at the driver level: abort the pass, rerun interpreted (per the determinism spec's fallback requirement).

One driver-level composition is permitted and is not partial compilation: an entities-where predicate whose post-expansion form is a short-circuiting conjunction chain MAY split into a fully-compiled prefix filter plus an interpreted residual, where the residual is evaluated by the interpreter at the driver level on prefix-surviving rows only. This split SHALL be exactly semantics-preserving: because the interpreter short-circuits the chain, rows the compiled prefix rejects would never have had their residual evaluated interpreted either. Non-short-circuiting conjunction shapes (the `*` product form) SHALL NOT split — they compile whole or fall back whole.

#### Scenario: Unlowerable subform
- **WHEN** a motion expression contains a form the lowerer does not cover
- **THEN** the whole expression evaluates on the interpreter path, bit-identically to pre-lowering behavior

#### Scenario: Mixed short-circuit predicate
- **WHEN** an entities-where predicate expands to a short-circuit conjunction whose leading conjuncts are recognized row tests and whose tail is an unrecognized form
- **THEN** the recognized prefix runs as a compiled row scan and the interpreted residual runs only on rows the prefix accepts, with match set, errors, and evaluation order identical to the fully interpreted scan

#### Scenario: Mixed product predicate
- **WHEN** an entities-where predicate is a `*` product conjunction with any unrecognized conjunct
- **THEN** the whole predicate evaluates interpreted per row (no prefix split), because the interpreter evaluates every product conjunct on every row

### Requirement: Programs are structurally interned
Compiled programs SHALL be interned by structural identity of their op stream (the compile-cache key). Rand draws and numeric environment captures SHALL lower to input slots filled by per-entity capture vectors, so sites differing only in captured or drawn values share one program.

#### Scenario: Two spawn sites, same shape
- **WHEN** two spawn sites differ only in a captured numeric value
- **THEN** they share one interned program and may fuse into one batch group

### Requirement: Hot node types stay small
`DynNode` SHALL stay ≤ 96 bytes (test-pinned by `dyn_node_stays_small`); new per-variant data goes behind a one-word `Option<Rc<..>>`. Pose-chain walks chase these enums on every hot path; the 88→120-byte draft cost ~60% wall.

#### Scenario: Adding variant data
- **WHEN** a change adds per-variant data to DynNode
- **THEN** the size-guard test fails unless the data is behind a one-word indirection

### Requirement: The IR interpreter is the permanent fallback tier
The IR-interpreter executor tier SHALL remain supported on every host as the universal fallback for cold/uncompiled programs, and the interpreted CONTROL PLANE (card loading, macros, scheduler/action tree, states/phases, live eval/swap) SHALL stay interpreted and user-facing permanently. Interpreted per-entity hot loops SHALL NOT be parallelized or pre-computed — that work does not transfer to the compiled form.

#### Scenario: Wasm host without codegen
- **WHEN** a host cannot run native codegen
- **THEN** every card still runs correctly on the IR interpreter tier with identical semantics

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

### Requirement: Cull rules compile
The tick-rule lowerer SHALL recognize the cull-rule expansion shape — a `map` whose function body is exactly a `cull` of the row parameter over an `entities-where` whose predicate compiles to row tests — and execute it as a compiled predicate scan followed by cull application per matched row in row-index order, through the same action-application path the interpreter uses for `cull` actions. Any deviation from the shape SHALL bail the whole form to the interpreter. Under the lowering oracle, the compiled scan's predicted match set SHALL be checked against the interpreted evaluation's produced cull actions (set and order) with the interpreted path as the single applier, so oracle runs never double-apply effects.

#### Scenario: Enemy hp cull rule
- **WHEN** a standing rule culls entities matching a compiled-recognizable predicate (e.g. team keyword equals plus an hp column comparison)
- **THEN** the rule runs as a compiled scan plus per-row cull with world effects identical to the interpreted rule, and the oracle dual-run confirms match set and action order

#### Scenario: Cull body deviation
- **WHEN** the map body is anything other than exactly a cull of the row parameter (extra forms, shadowed `cull`, different argument)
- **THEN** the whole form stays interpreted

### Requirement: Predicate recognition covers short-circuit conjunction expansion
The row-predicate recognizer SHALL recognize the post-expansion if-chain shape that short-circuit conjunction macros produce and fold it into the same conjunct list as the product form, keyed on the expansion structure (never on macro names). Disjunction if-chain shapes SHALL be recognized structurally only to fall back whole — no disjunction row tests. Because recognized conjuncts are pure and total by construction, evaluating all folded conjuncts is bit-identical to short-circuit evaluation.

#### Scenario: and-chain predicate compiles
- **WHEN** an entities-where predicate is the expansion of a short-circuit conjunction of recognizable row tests
- **THEN** it compiles to the same row-test conjunct scan the equivalent product form compiles to, with an identical match set to interpreted evaluation

#### Scenario: or-chain predicate bails
- **WHEN** an entities-where predicate expands to a disjunction if-chain
- **THEN** the predicate falls back whole to interpreted per-row evaluation

### Requirement: Registered rule bodies get the load-time rewrite
Macro expansion performed at `deftick` registration SHALL be followed by the same load-time AST rewrite applied to card forms (value-or intrinsic recognition and trivial-definition inlining), under the same shadowing discipline (names bound in the enclosing environment or card definitions suppress the rewrite), before tick-form lowering runs. Expansion shapes inside macro-generated rule bodies thereby become recognizable to the lowerer instead of retaining interpreted cost.

#### Scenario: Macro-generated rule body lowers
- **WHEN** a deftick body produced by a card macro contains a shape that rewrites to the value-or intrinsic (e.g. a default-column read)
- **THEN** after registration-time rewrite the tick form compiles under the existing recognizers, and evaluation is bit-identical to the unrewritten interpreted form

#### Scenario: Shadowed name suppresses rewrite
- **WHEN** a rule body binds a local name that shadows a rewrite-eligible head
- **THEN** the rewrite leaves that form untouched and evaluation uses the local binding

## Design

The material below line 138 is the non-normative historical compiled-dyn design archive. Its old `NumProgram`, `ProjectorNum`, resolved-row evaluator, and “still open” references describe the route to the cutover, not the current architecture.

---

# typed kernels — landed implementation status

Status: LANDED 2026-07. `KernelProgram` is the canonical fixed-width executable identity: type-local F32/F64/U32/U64/Symbol/Handle/Mask register files, validated typed operands, fixed flattened outputs, and structural interning over layouts plus operation order. `KernelPlan` binds a program to motion, dyn-field, entity-filter, render-row, collider-row, or masked-update domains with declared direct/indirect/capture/channel/tick/axis/state inputs, output/presence targets, stale-handle policy, whole-plan fallback, and deterministic merge ownership.

The permanent generic CPU executor is op-major SoA: it decodes each operation outside the lane loop and never constructs an interpreter `Val`. Measured hot paths may install a specialized CPU artifact derived from the same validated program/plan identity. Motion retains the proven `NumProgram::run_lanes` implementation only as its specialized F64 backend through `NumKernelBridge`; it is not a second public/domain IR. Filters, fixed updates, render projection, and collider projection likewise use cached typed-plan artifacts in production and the generic executor as their oracle/reference path.

Every supported compiled surface is callback-free. Drivers resolve all source forms, schemas, symbols, columns, captures, handles, and presence before execution; a missing/stale/mistyped gather abandons the whole plan before publication and reruns the semantic interpreter. Drivers own filtering, compaction, geometry allocation, collision contacts, queued writes, render publication, and canonical row order.

The migration removed the private compiled `ResolvedRowTest`/`ResolvedRowNum`, `ProjectorNum`, and render-row evaluator paths. Semantic `DynNum` and collider expression sources remain for interpreted fallback and cold plan installation, outside executable kernels. All four ignored release oracle card suites and the final interleaved representative/scaled performance gate pass.

## Historical JIT-readiness archive

## JIT readiness — what must land before a codegen backend starts

JIT surface (decided): motion/dyn signals, collider materialization, and
deftick row math (predicates, render row values, field writes/bind) —
exactly the hot-loop set; everything else is control plane and stays
interpreted. Gaps, in dependency order:

1. **One typed kernel contract — LANDED.** Motion/dyn, collider projection,
   filters/rule predicates, fixed render projection, and masked updates now
   share structurally interned `KernelProgram` identity and domain
   `KernelPlan` bindings. Private compiled projector/resolved-row/render
   evaluators are gone; only semantic fallback/source representations remain.
2. **Input slots + capture vectors — LANDED.** Rand draws and numeric
   captures use declared inputs, and program identity excludes per-site
   values. Sites with the same typed layouts, widths, outputs, and operation
   order share a program and may batch under compatible plans.
3. **Totality contract (decided): no Interp fallback op in JIT v1.**
   All-or-nothing classification means every compiled program is total
   and infallible — no interpreter re-entry from native code, no error
   paths or lane masks in kernels. Runtime None-aborts (numeric read
   hitting a keyword-valued sym field) stay at the DRIVER level: abort
   the batch, rerun interpreted.
4. **Batch call convention at all fixed-width surfaces — LANDED.** The
   generic ABI is typed input-major lanes plus reusable scratch to typed
   output-major lanes. Motion, dyn fields, filters, render projection,
   collider projection, and masked updates install compatible plans; CPU
   specializations preserve that plan identity and keep the generic executor
   as the oracle/reference implementation.
5. **Determinism across tiers** (normative surface:
   `openspec/specs/determinism/spec.md`): kernels call shared extern math shims
   (sin/cos/pow/rem_euclid — no platform libm, no fast-math, GPU tiers
   included); oracle extends to a three-way interpreter ↔ IR-loop ↔ JIT
   check. REVISED 2026-07 (scale target): the contract is same
   ops/order/WIDTH per storage class, not blanket f64 — control plane
   stays f64, hot columns (positions, integrator state, radii, render)
   go f32. The IR gets a width per program; the oracle doubles as the
   f32-drift meter over the card corpus.
6. **Tech/platform**: cranelift is the presumptive native backend (pure
   Rust, fast compile, fits lazy compile-at-first-eval with the IR loop
   as warmup); macOS hardened runtime needs MAP_JIT handling. REVISED
   2026-07: a wasm host cannot do native codegen, but it CAN instantiate
   a generated wasm module at card load importing the same linear
   memory — kernels read/write the SoA columns in place. The same typed
   `KernelProgram`/`KernelPlan` contract is the second emission target; cards
   known in advance may compile it offline at publish time.
   (precompiled kernel module shipped with the card). The IR interpreter
   tier stays permanently supported as the universal fallback.

Parallelism (decided, round 21): data-parallel execution of the
per-entity loops is a backend/driver property, NOT an IR marking — every
kernel under the batch convention is parallel-safe by construction
(total, callback-free, disjoint per-lane writes), so a per-program flag
would be constant true. The semantic invariants that make any schedule
legal and bit-deterministic (no cross-lane reads, no `&mut World` during
kernel runs, fixed merge order for all cross-lane combining) are recorded
in render-output-design.md "Parallelism"; scheduling itself (rayon/SIMD/
single-threaded wasm) lives in the driver loops per host.

Non-blockers: evolve/live/stages coverage (classification excludes
stages; sited-evolve READS and live-channel reads lower as aux inputs
since the milestone-B remainder, per-row interpretation covers the
rest indefinitely), and the model/ split (orthogonal; if it lands
first, `Dyn<E>` with E = kernel handle is its natural instantiation).

Sequencing status: collider and render batching, typed IR unification,
input slots/interning, and CPU-specialized plan artifacts have landed. A
Cranelift or wasm backend now drops behind `KernelProgram`/`KernelPlan` and
the typed lane/scratch ABI, with the existing oracle as its acceptance gate.

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
whole group, which is also what group evaluation (AxisSel/SS5
interchange — milestone B below) needs. Env variables bound to non-numeric values make the
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
  openspec/specs/language/spec.md control-cells) — without that pin, every pattern env's
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

Input slots + interning LANDED (round 22, second slice of B): rand sites
extract to `(%capture i)` markers once at node construction; spawn draws
a per-entity capture vector (same RNG order as the old substitution
walk) and clones share the marker programs, so rand-bearing rows join
the vel batch lanes; numeric env captures lower to input slots in the
same vector; programs intern structurally, fusing batch groups across
spawn sites and repeated constructions (fruit −8% wall). Implementation
notes: DynNode carries the slot data as one word (`Option<Rc<RandCell>>`
— the inline draft grew the enum 88→120 bytes and cost ~60% wall on
pose-chain walks; a size-guard test pins ≤96); lowering bails keep the
per-entity substitution path bit-exactly; the batch oracle re-runs each
LANE's own node (clones share programs but not state keys or caps).
The B remainder LANDED (2026-07-12-compiled-dyn-milestone-b, design.md
there has the as-built notes): ClosedPt group pose evaluation (batched
pos-only fill at collide/cull, class-cache gated), AxisSel lane scatter
(once-per-group memo in refresh_dyn_cols), and the homing-slew census
shape via auxiliary inputs — scan-cell and channel/stream reads as
DRIVER-FILLED input tables (AuxIn/Atan2 ops, pose-pair scalarization;
the "Auxiliary inputs are driver-filled lanes" requirement above), with
the evolve ADVANCE still interpreted (milestone C) so aux programs never
join batched steps. Same round: trace readers built only for traced
rows, and cull reuses the collide-phase pose for Vel-chain rows
(audit-gated, oracle-asserted). Round wall on the scaled fruit rig:
−7.9%. Still open under B: the entity-representation flip (spec id +
capture vector replacing per-row node clones — the 1M-row layout).
Related group-level lever, recorded as the `group-integrator-dedup`
backlog change: integrator-state dedup
per (program, captures, birth) — ring lanes carry bit-identical folds,
the per-bullet angle lives in the wrapper frame.

### C — beyond figure signals

The host-boundary half — SoA render output (typed columns per compiled
rule, schema objects hosts negotiate against, `render_frame()` API) — is
DONE (round 21, render-output-design.md): compiled point rules fill
column batches (direct numeric gather, staged schema checks,
abort-and-rerun error parity), hosts read the frame in place. This
settles the render semantics/API the JIT's render kernels compile
against — the batch fill is the render surface's kernel seam (gap 4).

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
  maps, extra let bindings, compound rule bodies (e.g. the enemy-death
  `(seq (event …) (cull e))`), and field-write rules. Those are the
  remaining rule-lowering surface, alongside projector bodies.

The rule-lowering remainder is DONE (round 23, `rule-lowering-remainder`):
- Cull rules — a map body that is exactly `(cull e)` over a compiled
  predicate — execute as a compiled scan plus per-row cull in row order
  through the interpreter's action-application path. Oracle mode predicts
  the match set without applying and asserts it against the interpreted
  cull actions, which stay the sole applier (effects never double-apply).
- The recognizer folds the short-circuit conjunction (`and`) expansion
  chain — the left-nested `(let [s acc] (if s next s))` shape, keyed on
  structure never the binder name — into the same conjunct list as the
  `(* …)` product form; disjunction chains bail whole.
- `deftick` macro-expansion output gets the load-time rewrite (value-or
  intrinsic + trivial-def inlining) at registration, with enclosing env
  bindings as shadows, so macro-generated rule bodies lower too.
- Mixed and-chain predicates split into a compiled prefix filter plus the
  interpreted residual evaluated only on prefix survivors — exact because
  the interpreter short-circuits the chain, so prefix-rejected rows never
  evaluate the tail on either path. `*` products keep whole-form fallback
  (the interpreter evaluates every product conjunct on every row).

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
deterministic merge), per the recorded stance (see the
`jit-native-codegen` backlog change).

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
