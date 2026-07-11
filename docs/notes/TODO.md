# Prototype TODO

The spec in `docs/language.md` is authoritative. This file tracks only work
that is still open in the prototype or decisions that should constrain that
work.

## Language Gaps

- `states`: support state-body return values as the next label, routed by
  goto-or-state-order. Keep richer spellcard templates in `cards/lib` macros,
  not engine primitives.
- Scoped channel overrides: `(with {$chan v} body)`.
- Pattern embedding scope adapters: callable patterns currently embed bare
  defaults only, without argument passing or shared-cell adapters.
- Entity-view predicates don't support destructuring params
  (`(fn [{:keys [hp]}] ...)` errors "did not match pattern"): pre-existing
  (eager map views failed identically), independent of the lazy
  `Val::EntityView` row tokens. Teach `match_pattern` map destructuring
  over entity views if the idiom is wanted.
- Channel manifest/load-time checking: missing host channels such as `$wind`
  should fail at load, not mid-run. Decided: channel manifests, per-kind
  render row schemas, and entity field tables are ONE load-time schema
  collection pass — shared machinery, separate tables where the columns
  differ. NOTE: the channel/cell unification design
  (`docs/notes/channel-unification.md`, converged 2026-07, not yet
  ratified) makes the manifest check fall out of scoping — a free `$name`
  neither bound nor def'd is a load error, and the manifest is the set
  of `(from-host :name)` sites — a standalone stream-valued expression
  (anonymous injected streams work; bind! just names one), so host
  injection is not special syntax. Cells dissolve into let-bound sigiled streams; the
  dynamic cell scope (CELLS_KEY/cell_scope/adapter caller-cells) becomes
  deletable kernel surface.
- `remat` / `manip`: semantics decided — `(remat handle spec)` is the
  handle-preserving primitive, strictly 1:1 (single handle, single-element
  spec; multi-element figures error). The spec is PARTIAL: supplied slots
  replace, absent slots retain. Epochs are per-slot: a rematted slot's
  local `t` restarts and its runtime state (scan cells) is cleared — the
  new figure has no relation to the old — while untouched slots keep clock
  and state, so field-only remats (the `set-col` story) never disturb
  motion. Writes apply at/right before the NEXT tick: all reads within a
  tick see pre-tick state, keeping ephemeral row indices
  (`entities-where`) stable within the tick. The field-write primitive is
  the FUNCTIONAL update `(change-col e col f)`: writes queue their update
  function, and at the tick boundary a slot's queued functions compose in
  deterministic action-execution order over the pre-tick value — so
  concurrent decrements accumulate instead of losing updates, a plain set
  is the constant function (`set-col` = lib sugar `(fn [_] v)`, making
  last-writer-wins a consequence, not a rule), and the aggregate-over-domain
  form stays the preferred idiom that the recognizer fuses. Remat's partial
  spec admits values or update functions per field; `change-col` is the
  single-field case. Update functions must be pure. A new
  figure evaluates anchored where the entity currently IS (current world
  pose); callers wanting the old parent frame store/pass it explicitly.
  Milestones 1 AND 2 are DONE (write queue + `change-col` + `set-col`-as-
  prelude-sugar + lib migration; partial `(remat h spec-map)` with the
  reserved `:motion` key, remat on the queue with drain-time exit snapshot,
  and the motion-slot epoch split from the entity clock — see
  `docs/notes/remat-design.md` for landed shapes and known edges; the
  atomic multi-field spec now covers the same-tick multi-contact iframe
  guard if a card needs it). Still missing: per-dyn-field epochs (fades
  surviving motion remats), soft-cull fades, the F1 lint, and the
  masked-SoA fast path (the lowering target for batch `map`-remat shapes).
  Live-evolve integration keys on the per-slot epochs (implementation
  design in `docs/notes/live-evolve-design.md`). Milestones 1–2 are DONE:
  EvolveCell Val motion cells (schema/store/snapshot/reader/write_val
  plumbing), evolves are scanned, step_motion_in advances closed evolves
  once per tick boundary against the closed SigEnv, on-clock pose reads
  hit the settled cell and off-clock sampling replays unchanged — closed
  evolves stopped being O(n²); a motion remat's schema swap restarts the
  epoch. Milestone 3 is DONE (bec0735): init is a deferred thunk
  evaluated at epoch start (closed env for closed, real env for live —
  `(evolve (:pos e) ...)` remat continuity works), a syntactic liveness
  classifier roots keyword access at the step fn's params (param-rooted
  access is the fold's own state → closed; capture-rooted, channels,
  rand, world-reading heads → live), live evolves advance in the
  scanned pass against the real SigEnv/world, and off-clock live
  sampling errors. Live evolves accept a one-behind cell in the
  post-boundary sampling window; closed keep exact-match-else-replay.
  This unblocks evolve-design step 3 (vel/slew/smooth/stages/pather in
  lib over evolve) — IN PROGRESS (round 7, design in
  docs/notes/evolve-reexpression-design.md): sited evolves (e988cd0 —
  an evolve under a scan context keys state by ScanSite and evaluates
  to the settled value) and capture-time macroexpansion (c975512) are
  on main; slew/smooth become prelude macros over sited evolves
  (retiring sf_stateful, scan_builtin_spec, and the Val::Thunk
  deferred-instance mechanism, with player_homing's let-smooth inlined).
  Remaining on this track: vel (blocked on nothing, but recognition is
  semantically mandatory so it's pure surface until the model/ split)
  and stages (own round, likely over states).
- Extraction and 3D embedding remain unimplemented.
- Tick/rule ergonomics are still settling. Core now has primitive `deftick`
  plus domain expressions such as `(entities-where ...)` and `(collisions
  :a :b)`; row-wise helpers/macros should live in lib/prelude rather than
  reintroducing core `defrule` magic.
- Blocking lasers / world-geometry extent from DMK §13.7 remains unported.
- RNG is sequential splitmix, so replay determinism holds but spawn-order
  independence does not.
- Array-valued dyn meta now binds per spawn element (`NumDynRepr::AxisSel`
  captures the element's axis path at spawn; evaluation selects one lane
  with the style-axis rules). Interim shape: each entity evaluates the
  full shared array per tick and keeps its lane — the compiled-dyn pass
  should recognize the shared program and evaluate once per group,
  scattering lanes (SS5 array-of-signals/signal-of-array interchange).

## Engine Refactor

- Deduplicate `EntitySpecStore` cold dyn/projector data into shared
  spawn-site/program/archetype storage where possible.
- (done) Motion state is keyed by stable lowered ids only; the pointer-keyed
  compatibility variants are gone and direct evaluation seeds ids via
  `MotionReaders::for_node/for_pose/for_figure`.
- DONE: lazy `stages` lower at construction to load-time-closed dyns. The
  `(fn [exit] ...)` closure runs once against a symbolic `StageExit` token;
  exit reads become deferred `StageExitPose` values resolved from fixed
  pos/vel cells (stable lowered ids, seeded like node ids) written at the
  stage boundary. `StageMake::Lazy`, `MotionStateKey::LazyStage`, and the
  runtime dense-schema extension path are gone — motion schemas are closed
  at load. Known limit: a closure that forces a numeric exit read EAGERLY
  at construction (outside a deferred dyn component form) errors at load
  ("stage exit can only be read inside a staged signal"); corpus bodies are
  all pure constructors and unaffected.
- Move remaining gameplay-domain behavior out of core: bare hostile
  `(cull)`. Core no longer knows style axes or lasers; the remaining
  family->color/radius tables are stock host policy in `host.rs` (see the
  renderer item). The laser bridge is gone: `(curve shape? {geometry})` is
  the core figure constructor, `laser`/`laser-shot` are lib spawn macros,
  lifecycle (`:warn`/`:active`/`:fill`) is ordinary entity fields that
  `laser-collider`/`beam-renderer` bodies translate per tick into STATIC
  collider/render descriptions (primitives take concrete numbers; no
  stored dyn slots), and beam end-of-life is a lib cull rule. The
  circle->capsule adaptation that borrowed collider truth from render
  specs is deleted; a :pose collider on a curve element yields no collider.
- Projector/render surface (done): `(collider :pose|:parametric [e ctx]
  body)` is the constructor special (`defcollider` = `def` + that form;
  bodies and spawn slots yield `primitive | [primitive] | nothing`,
  flattened one level; `colliders`/`ColliderSum` are gone), and render
  output is rule code calling `render` — `defrenderer`, render slots,
  and the spawn renderer arg are deleted, the stock dot is host policy
  over the absent-or-`:dot` `render` field, and deferred curve geometry
  is the opaque `(curve-samples e {:u-max h :resolution r})` value
  (static numbers, validated at construction) expanded when the action
  executes. Rationale for the asymmetry: collision is engine-pulled
  closed algebra (opaque values attached at spawn; user code never emits
  collider rows), rendering is host-pushed open data. Known trade:
  rule-emitted rows are tick-cadence snapshots, so frame-time
  re-evaluation/interpolation becomes a host concern. Remaining render
  work: per-kind registered row schemas with manifest negotiation (the
  current schema is one global key->kind map), the builtin field
  rename/pick adapter, and a mesh/sprite-batch kind. Host palette tables
  (`style_rgb`, `dot_radius`) remain stock host policy in `host.rs`;
  move them behind host/profile config when a second frontend needs
  different vocabulary.
- Compile dyn evaluation to a flat program with fixed scratch storage.
  Design: `docs/notes/compiled-dyn-design.md`. Milestone A (first slice)
  is DONE: fully-numeric ClosedPt/Vel/RotExpr component forms lower to
  flat f64 register programs (all-or-nothing per form, interpreter as
  per-node fallback, defs-shadowing guarded, oracle-verified via
  MAKU_LOWER_ORACLE=1); Vel/RotExpr step arms reuse the programs. 40%/58%
  of closed-pt/vel evals compile at ~30x per-eval; `*` interpreted count
  down 1M. User-defn inlining is also DONE (hygienic beta-reduction at
  lower time; required pinning live-only cell reads in signal slots —
  Ctx.signal_scope — since a captured cell scope otherwise disabled it
  for 100% of corpus lowerings) but bought no corpus coverage: the bail
  census showed the remaining interpreted volume was (a) homing-slew
  nodes (scan + channel inputs → milestones B/C), (b) `lerpsmooth` with
  a static easing-kind sym, (c) keyword reads on captured poses/maps.
  (b) and (c) are DONE (300b6cb): a LerpSmooth op with statically
  resolved easing kind, real Form::Kw-head lowering — the old `:x`/`:y`
  arm was dead code matching a form shape the reader never emits, so
  `(:x pos)`/`(:y pos)` PosX/PosY ops fire for the first time — and
  Const folding of keyword-access chains rooted at env-captured
  Maps/Poses (sound: the program cache lives on the node beside its
  captured env). Remaining lever: (a) slew's scan + channel inputs,
  which is milestone B (input slots + group evaluation) territory. The
  interpreter path may remain as a compatibility implementation, but hot
  steady-state execution should not allocate or hash by node pointer.
- Move the dyn kernel (and entity spec / state-schema semantic halves) to
  model/ as a backend-parametric `Dyn<E>`, AFTER the evolve re-expression
  shrinks the kernel (moving now would enshrine Vel/Stages, which become
  lib shapes). Direction + sequencing: `docs/notes/model-split.md`.
- DONE (cfd2218): entities-where predicates shaped as conjunctions of
  `(= (:field e) :kw)` tests compile to a RowPredicate over interned sym
  columns (with `:kind` computed as entity_view does); recognition is
  structural per query call (no cache — the sf_matches Fn is rebuilt per
  call, and recognition is trivially cheap next to per-row apply_fn),
  bails whole on anything else (mixed numeric tests fall back entirely
  to preserve error/effect order), and dual-runs under
  MAKU_LOWER_ORACLE. Also aligned entity_field_at's `:kind` for traced
  pathers (`:pather`, not `:point`) with entity_view. Aggregate: `=`
  452→193ms, `*` 455→312ms, entities-where inclusive 1569→1125ms.
  Remaining on this track: partial prefiltering (mixed predicates beyond
  the recognized set still fall back whole). Numeric comparisons are DONE
  (round 9): conjuncts shaped `(op lhs rhs)` over `<`/`<=`/`>`/`>=`/`=`
  compile when both sides classify as total numeric reads — literals,
  `inf`, `(:t e)`/`(:tick e)`, `(%value-or (:col e) default)`, and 2-arg
  `+`/`-`/`*` over those; a BARE `(:col e)` numeric read bails (a missing
  col is Nothing and the interpreted comparison errors). Exact-parity
  guard: a numeric read that hits a KEYWORD-valued field aborts the whole
  compiled query and reruns the interpreted fallback (predicates are pure,
  so the rerun reproduces the interpreted error byte-for-byte); compiled
  render rules truncate partial rows and re-interpret the form on that
  same signal, with the predicate scan completing before any row body so
  phase order matches entities-where-then-map. Numeric `=` mirrors
  val_eq's 1e-9 epsilon. This covered the three hot lib rules (hp-cull,
  game-over, beam end-of-life). Map queries (`sum-entities` channels)
  also resolve selector symbols once per query now instead of four
  string hashes per row.
  Stance (decided): this is a load-time lowering (AOT at card load), not a
  JIT. The interpreter splits by role: the CONTROL PLANE (card loading,
  macros, the scheduler/action tree, states/phases, live eval/swap) stays
  interpreted and user-facing permanently — it is cold and tooling wants
  it; the PER-ENTITY HOT LOOPS (dyn columns, projector bodies, tick rules
  over entity sets) are a prototype stand-in that the lowering replaces,
  with rules/projectors following dyn onto the same flat-program
  machinery. Do not parallelize the interpreted hot loops: the value
  representation is Rc-saturated (non-Send; threading means Arc-ifying or
  arena copies of exactly what gets replaced), rules emit ordered effects
  and draw from the sequential splitmix stream (parallel entity order
  changes RNG unless entities are independently seeded), and none of that
  work transfers to the compiled form — which gets data-parallelism nearly
  free (pure lanes over fixed scratch, deterministic merge points).
  Precomputing future ticks is likewise out: per-tick input/channel reads
  and the scrub/snapshot session model invalidate it. Interim interpreter
  investments that do survive: SoA layout, spec-store dedup, group
  evaluation of shared programs, fixed scratch, and hoisting
  per-spawn-site invariants to load time.
- DONE: profiling harness — `interp::profile` (flat self/inclusive-time
  by evaluated head symbol + dyn-node variant, thread-local, off by
  default) with `examples/profile.rs` running the representative set.
  First numbers (aggregate self time over 8 cases): `dyn:vel` 1.96s,
  `entities-where` 1.83s self / 7.8s incl (per-row predicate eval),
  `dyn:closed-pt` 1.59s, `value-or` 1.25s at 4.6M calls (a two-line
  prelude defn paying full interpreted-call overhead — the canonical
  AST-rewrite-intrinsic candidate), `dyn:frame` 0.89s self / 13.7s incl,
  `emit` 0.81s, then interpreter dispatch on `*`/`=`/`+`. `map` is 47ms
  self but 10.7s inclusive — its bodies (mostly entity queries) dominate.
  Intrinsic promotion and lib-ification calls should cite these numbers.
  Round-7 findings on the two rows left standing: `dyn:frame`'s ~324ms
  was mostly instrumentation artifact — the profiler String-allocated
  the entry key on every close INSIDE the parent's still-open window,
  charging recursive rows for their children's bookkeeping (fixed, with
  a Frame(Const, child) fast arm + construction-time Const-frame
  folding on top; frame_node in motion.rs). `%value-or`'s ~320ms/4.65M
  is real: renderer deftick bodies re-interpret per entity per tick,
  and every numeric field read (`e.scale` etc.) pays TWO string-hash
  symbol lookups (sym_field_resolved_at then col_get_at). Structural
  options, in payoff order: (a) lower projector/renderer bodies to
  field-read programs at deftick registration (milestone C's
  rule/projector half — value-or over a col read becomes slot+default),
  (b) memoize resolved column slots beside the keyword form (OnceCell,
  must handle not-yet-interned cols), (c) column-major row batching
  (per-tick only; gate on body purity).
  Round-8 resolution: (a) is DONE for the renderer half — deftick bodies
  macroexpand once at registration and the sprite-rule shape compiles to a
  native rule (`interp/rulelower.rs`, oracle-gated; see
  compiled-dyn-design.md milestone C for the landed shape and bail set) —
  plus the interpreter-path halves of (b): entity field reads resolve ONE
  symbol for both stores, and RowPredicate tests intern their symbols once
  per query instead of per row.
  Round-10 finding: head-level rows only covered a fraction of wall time.
  New phase frames in the profiler (`phase:*`, `sim:motion-readers`,
  `sig:setup`) attributed the rest: the collision pass was ~65% of
  aggregate wall — the O(n²) pair loop 992ms of ~2.0s, collider
  materialization 302ms (an `entity_view` built per entity per tick that
  static projectors never read). Fixed (sim/collision.rs): x-axis
  sweep-and-prune over per-entity union AABBs, one narrow-phase visit per
  unordered pair (overlap is exactly symmetric), stable (i, j) sort
  reproducing the old fact order that `(collisions ...)` observes, a
  brute-force dual-run under MAKU_LOWER_ORACLE=1, and a `needs_views`
  classification skipping view construction for Stable/unscoped
  projectors. Pairs 992→510ms (rest is genuine near-neighbor density in
  homing streams / dense rings); walls: reimu 939→629ms, spell-2
  385→215ms.
  Round-11: collision facts are now LAZY per queried layer pair — the
  pass only captures a snapshot (collider rows, union AABBs, per-layer
  entity lists, eligibility) into `World::CollisionIndex`; the
  `(collisions :a :b)` query computes layer-a × layer-b with AABB
  rejection, memoized per tick as the Rc CollisionSet payload. Query
  results derive from the captured snapshot, never live state (control
  tasks keep seeing the previous tick's facts), and same-layer bullet
  clouds nobody queries are never narrow-phased; the round-10 sweep is
  superseded and removed. Under MAKU_LOWER_ORACLE=1 every fresh query
  asserts against a raw all-pairs scan of the snapshot. Eager pair work
  (510ms, and 1603ms on the newly added miracle-fruit profile case) →
  63ms capture + 2.7ms queries; walls: spell-2 215→66ms, reimu
  629→365ms, fruit 5050→3555ms.
  Round-12: a macOS `sample` profile (the flat profiler's own frame
  bookkeeping is ~18% of wall on dense cards — MAKU_WALL_ONLY=1 on the
  example now measures bare) showed ~40% allocator traffic + ~10%
  SipHash, dominated by the touhou bullet collider: `(circle-collider
  {:layer :damage :r e.hitbox})` under a :pose defcollider scope paid a
  full entity_view + evaluator World/Ctx per entity per tick for a
  plain field read. `ProjectorNum::EntityCol` now recognizes `(:field
  e)` scope reads at spec build; materialization serves them via
  entity_field_at (exact view-value parity incl. error text), and
  needs_views tightened to "some ProjectorNum is a general Expr". Plus:
  stateless-schema MotionReaders fast path (shared no-op closures),
  all-Stable projectors evaluate by reference, interned scale read.
  Bare walls: fruit 2848→1783ms, polar −41%, stars −33%, cradle −26%,
  bowap −24%, spell-2 −21%, reimu −15%. Remaining levers from the
  sample, in payoff order: reader snapshot churn for SCANNED rows (vel
  bullets have n2 state, so the stateless path misses — cheap win #1
  proper: readers over the SoA columns, no per-entity maps/closures),
  FxHash for the hot HashMaps (MotionStateKey maps, symbol lookups,
  render schema — SipHash is ~10% of fruit), render-row building
  (RenderRowFields push/finish + per-row Rc), `Val` drop traffic.
  Round-13: both remaining alloc/hash levers. (a) Per-row reader
  construction (3 HashMaps + 3 boxed closures per entity per phase)
  replaced by a slot-indexed `RowStateSnapshot` behind method-based
  `MotionReaders` — values in schema slot order, key lookup a linear
  scan of the schema's tiny key vectors; snapshot-at-construction
  semantics unchanged (reads see pre-step values while the step pass
  writes through). (b) In-repo FxHash (`src/fxhash.rs`, the rustc-hash
  fold, 64-bit internally so wasm32 matches) on MotionState, schema
  slot/node-id maps, symbol table, world field slots, collision index,
  render schema. Bare walls: fruit 1783→1195ms (readers alone →1386),
  bowap −35%, reimu −20%, stars −20%, cradle −19%. Next levers from
  the round-12 sample, still open: render-row building
  (RenderRowFields push/finish + per-row Rc, phase:rules self ~554ms
  on fruit), `Val` drop traffic (drop_in_place<Val> ~124 samples);
  re-sample before choosing — the alloc profile has shifted.
- DONE: first expansion-shape intrinsic — `interp/rewrite.rs` is a load-time
  pass over card forms (hooked in `load_card`): structural, alpha-invariant,
  shadow-aware matching of `(if (nothing? ?x) ?d ?x)` → native `%value-or`
  (purity-guarded: `?x` must be re-eval-safe), plus trivial-defn inlining
  (single pure-builtin-call bodies inline at call sites with pure args).
  Hand-written shapes optimize identically to the lib defn, per the
  no-name-magic principle. Result: `value-or` 1252ms + `nothing?` 380ms +
  most of `if`/`fn` dispatch → `%value-or` 253ms; interpreted calls down
  4.6M. Trivial-defn collection runs to a fixpoint, so wrappers of wrappers
  inline transitively (`col-or`'s 419ms/1.5M interpreted calls folded into
  `%value-or`). Follow-ups: (b) macro-expansion output is not rewritten
  (expansion is
  lazy per-eval; shapes inside macro-generated forms keep interpreted
  cost); (c) purity edge: a pure higher-order builtin applying an impure
  user fn passed BY NAME is classified pure — conservative table fix if it
  ever bites.
- DONE: entities-where lazy row tokens — predicate queries pass a
  generation-checked `Val::EntityView` instead of eagerly materializing a
  map per row; keyword access/`get` read entity storage directly, any other
  builtin use materializes the old map view. Eager views were also
  force-sampling every entity's pose per row, so dyn eval halved too.
  Aggregate: entities-where 1825→184ms self; dyn:vel 2030→972, closed-pt
  1706→878, dyn:frame 895→412ms self.
- DONE: emit :render fast path — literal keyword-map row forms build the
  RenderRow directly (no intermediate Val::Map, no linear rescans; both
  paths share `RenderRowFields` so they can't drift), and render actions/
  `world.render_rows` hold `Rc<RenderRow>` (host boundary clones). emit
  333ms self (was ~810ms).
- DONE: scratch worlds for closed evaluation — `World::default()`
  preallocates DEFAULT_ENTITY_CAPACITY (8192) rows across ~17 vectors and
  was constructed PER COMPONENT EVAL in `eval_sig_at_rate` (plus FnPose,
  evolve steps, projector bodies, boundary-write fns). `World::for_eval`
  skips the preallocation: dyn:vel 1029→258ms, dyn:closed-pt 915→271ms
  self; cradle wall halved. Moral for the compiled-dyn pass: fixed-cost
  eval SETUP dominated the interpreted path more than form interpretation
  itself; the lowering must keep per-eval setup at zero (scratch reuse),
  and the remaining interpreted rows (`*` 589ms, `=` 463ms, dyn:frame
  429ms self) are now genuine per-node dispatch — the compiled program's
  actual target.
- Keep dyn coercions as explicit language-semantic branches while the
  interpreter is untyped. `interp::coerce` owns the value-level `DynLike`
  bridge; a future trait-style coercion surface should be over typed IR
  targets, not scattered Rust conversions over raw values.
- Collapse the remaining pose/figure asymmetry. `DynLike::Dyn(Pose)` is a
  typed dynamic value, not a data atom; the target is still plain `Figure`
  values lifted through `Dyn<Figure>`, with `linear` and friends represented
  as optimized `Dyn<Pose>` constructors that lift to figure dynamics.
- Continue core-vs-lib builtin stratification before the compiler pass.
  Current interpreter categories:
  - `interp/builtins/math.rs`: deterministic numeric intrinsics;
  - `interp/builtins/array.rs`: sequence/control-like value operations;
  - `interp/builtins/language.rs`: form/value inspection for macros;
  - `interp/builtins/geometry.rs`: primitive pose/dyn figure constructors;
  - `interp/engine.rs`: engine-facing special forms that need `World`,
    handles, rows, channels, or action construction.
  Specials are the IR; pure builtins are intrinsics. Anything expressible in
  `.maku` without hot-path or boundary semantics should move toward lib code.
  Governing principle (decided): NO sugar in lang. Minimize the surface to a
  semantic kernel; the surface vocabulary is lib macros over it, and
  optimization recognizes the macro EXPANSION SHAPE (AST patterns after
  expansion), never the name — hand-writing the same shape optimizes
  identically. Builtins return as AST-rewrite intrinsics driven by profiled
  bottlenecks (array/entity-domain paths expected first). The audit and the
  kernel-shrink worklist live in `docs/notes/builtins-audit.md`. Note
  `linear` is pos = v*t with STATIC velocity (it does not scan a dyn;
  lifting a dyn argument would mean v(t)*t, not integration) — velocity
  semantics come from the `evolve` integrator (currently the reserved
  `scan` stub, to be renamed; design SETTLED — see
  `docs/notes/evolve-design.md`: dyn<T> ≅ t -> T with
  application-as-sampling, `(evolve init (fn [s ctx] ...))` as the one
  stateful constructor, closed-vs-live sampling rule; `scan` stays free
  for the array adverb), with `linear` as a plain lib `(fn [t] ...)` — no
  lowering node needed.
- Finish shared model extraction. `model::figure` is top-level and generic
  over curve evaluators, while `interp` aliases it with `DynPose`. Symbol ids,
  entity handles, primitive data atoms, and runtime collider/render boundary
  rows live under `model`. Built-in collider/render projector cases still live
  under `interp` until their specs no longer depend directly on interpreter
  `Dyn`/`DynLike`/`Env` types.

## Data Model Targets

- Core semantic shape:
  ```text
  Figure = Pose | Polyline | ParametricCurve | Composite...
  Dyn<F> = t -> F
  Meta = finite typed fields, possibly dyn and figure-dependent in spawn slots
  EntityView<F> = ordinary entity handle/view plus entity-scoped meta and
                  figure-specific fields/getters
  MetaEnv = projector view of Meta, defaulting to shared entity namespace
  ProjectorContext = age/t, world tick, extraction-pass context
  ColliderProjector<F> = opaque source value lowered by extraction with
                         (EntityView<F>, ProjectorContext) -> [Collider]
  RenderRule = tick/render-domain code that emits open host render rows
  Collider = literal collision row, not a figure-to-collider spec
  SpawnedObject = Dyn<Figure> * Dyn<Meta> * [ColliderProjector<F>]
  ```
- Spawned objects are retained as row ids into SoA stores, not as an `Entity`
  row struct.
- Pose is `(x, y, theta?)`; `theta = none` means facing is unspecified, while
  `theta = some 0` is an explicit zero angle.
- Projectors are specialized by core figure type. Target surface can use
  `(defcollider :pose ...)`, `(defcollider :parametric ...)`,
  `(defrenderer :pose ...)`, etc.; the annotation selects the static shape of
  `e` and the extraction loop. Curve-specific render/collider fields stay in
  curve-specific loops/buffers and do not bloat pointlike entities.
- Sampling is not intrinsic to figures. It belongs to collider/render slots or
  authoring helpers. Parametric curves may later use analytic collision or
  mesh rendering without changing source semantics.
- Raw collider rows are boundary data emitted by extraction, not normal entity
  slots. Source code should construct opaque collider projector values through
  builtin primitive constructors and combinators. Render rows are now open
  schema-checked host-facing data constructed by render/tick code and slot
  extraction; entity count and render-row count are separate capacities. One
  entity may emit zero, one, or many rows, and non-entity systems may emit
  rows too. Render schemas merge by key with exact type compatibility
  (implemented as one accreted key->kind map; per-kind schemas are future
  work), and imported conflicting schemas should be adapted by a builtin
  field rename/pick operator (unimplemented).
- `defcollider` should become `defn` plus an expected return type
  `ColliderProjector<F> | [ColliderProjector<F>]`. Constructor argument records
  have known shape; their values are concrete typed expressions over the typed
  entity view/context. User code can compose/wrap/branch projectors for the
  same figure type, but cannot define a new primitive projector kind without a
  builtin registration.
  Do not grow the current dynamic spec-list bridge into the final API.
- Collider layer is universal core routing metadata:
  ```text
  Collider = None | Circle { layer, center, radius }
           | CapsuleChain { layer, points, radius } | ...
  Render   = None | Point | Polyline | Mesh | ...
  ```
- Predicate values are numeric masks. There should be no long-term runtime
  `Bool` type and no truthiness for keywords, strings, lists, maps, poses, or
  figures. `not` maps zero to `1` and any nonzero number to `0`.
- There is one language-level `Number` type. Integrality for masks/counts/
  indices is a schema contract at typed boundaries, not a separate source
  type.
- Homogeneous lists may be packed into dense vectors as a representation
  choice. Source syntax should not need a special uniform-literal marker.
- Entity indices are ephemeral row indices; handles are stable cross-time
  references. Query/domain values may remain index-backed and typed by what
  they index (`EntitySet`, `CollisionSet`, future figure-specific sets) so
  array operations can stay SoA-native. User code should not treat row indices
  as durable numbers; materialize handles/views only at action boundaries.
  Query order should remain unspecified unless explicitly sorted.
- Source-level entity fields are finite, flat, interned fields. Storage may
  distinguish builtin pose/state from user fields, but source no longer exposes
  separate arbitrary `cols` and `meta` concepts. Top-level numeric fields
  initialize SoA fields; dyn numeric values are evaluated into those fields
  each tick before collision/render/rule code reads entity views.
- Retained entity meta is flat primitive fields only. Do not add map/list
  storage or cold per-entity structure interning without a specific measured
  need; use source-level maps for macros/options and flat field adapters for
  namespace conflicts.
- Runtime metadata target:
  ```text
  nums    : NumFieldId    x entity_row -> f64
  syms    : SymFieldId    x entity_row -> Symbol
  handles : HandleFieldId x entity_row -> EntityRef
  present : bitsets or typed sentinel policy
  ```
  Unknown fields should become load/reschema errors, not per-tick allocation.
  The interpreter still interns fields opportunistically at spawn/write time;
  tightening this requires a schema collection pass.
- Retained entity storage should be cold data plus dense row state. Hot data
  should be per-tick derived SoA buffers for poses, colliders, render rows,
  and sampled curve points.

## Standard Library

- Keep Touhou/DMK/BDSL conventions in `cards/lib/touhou.maku` and related
  libraries. Core should remain a 2D graphing + collision/rule/render-row
  engine.
- Richer spellcard templates (:name/:type/hp bars) should be lib macros over
  `states`, `phases`, `boss`, `finally`, and ordinary fields.
- Candidate stdlib moves:
  - `for` / `dotimes`: decided — `loop`/`recur` (+ `wait`) is the primitive
    and is sufficient; per-iteration action-tree rebuild cost is a job for
    compiled scheduling, not a fused wait-loop special. Move them to lib;
  - family->hitbox-radius data currently repeated at call sites.
  (The short spawner names — `bullet`, `shot`, `enemy`, `player`, `boss`,
  `laser`, `laser-shot` — are done; `spawn-*` remain as aliases.)
- Collision effects now use `deftick` plus `(collisions ... )` domain
  expressions and ordinary `map`/destructuring. Keep Touhou hit/graze/shot
  rules in lib over opaque layers and fields; any ergonomic row-wise API should
  be lib/prelude sugar rather than a core special form.

## Intrinsics / Arrays

- Intrinsic criterion: make an operation intrinsic only when it is hard to
  implement well in lib and is generically powerful. Everything else should
  start as lib code over `match` and seq views.
- Initial array/control candidates: `map`/each, `filter`, `fold`, `scan`,
  `each-prior`, `window`, `sort-by`, `best-by`, `count`, `nth`, `take`,
  `drop`, `concat`, and transpose/zip-style operations for tuple domains.
  Function argument destructuring now reuses `match` pattern machinery, so
  collision pairs can be consumed as `(fn [[a b]] ...)` without a primitive
  `for-pairs`.
- K-inspired verbs/adverbs remain the direction, but the builtin set should
  be profiling-driven. Specialized operations such as binsearch, case,
  join/split, encode/decode, converge, and while-style adverbs can start in
  the prelude unless profiling proves they need lowering.
- Deterministic math/matrix intrinsics are part of this language, not delegated
  semantics. Native and wasm must replay identically; dependency upgrades
  must not silently change language behavior.
- Smooth noise should be a pure deterministic function of coords+seed, not
  sequential RNG state.
- Bullet-field image-processing ideas (rasterize query -> grid, FFT/filter,
  resample -> bullets) belong to a later intrinsic pass.

## Engineering Debt

- Split `interp/mod.rs` further. It still contains eval plus the specials
  table and will grow with vocabulary work.
- Write `docs/host-api.md` from `core::host::Instance` as the first
  non-macroquad frontend exercises it.
- Add signal tapping/plotting: select a subexpression and plot over `t`.
- (done) The tick rate is a World-owned `TickTiming` (single
  `DEFAULT_TICK_RATE = 120.0`); runtime paths read it, standalone eval
  helpers default. Host-facing rate configurability remains a later policy
  decision.
- AOT/wasm compiler work is unstarted.

## Docs

- Tutorials t01-t09, tbosses, and tstages are ported. Future doc work should
  focus on stabilizing the new tutorial site, reader view, and host API docs.
- `docs/from-dmk.md` remains the place for DMK/BDSL mapping notes; tutorials
  should stay standalone and idiomatic for Maku.
