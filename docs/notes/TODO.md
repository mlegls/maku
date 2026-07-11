# Prototype TODO

The spec in `docs/language.md` is authoritative. This file tracks only work
that is still open in the prototype or decisions that should constrain that
work. Completed work lives in git history and the design notes' status
headers, not here.

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
  neither bound nor def'd is a load error, and the manifest is the set of
  `(from-host :name)` sites. Cells dissolve into let-bound sigiled
  streams; the dynamic cell scope (CELLS_KEY/cell_scope/adapter
  caller-cells) becomes deletable kernel surface.
- `remat` / `change-col`: contract settled and landed (write queue,
  functional `change-col` composition, partial `(remat h spec-map)`,
  per-slot epochs, sited/live evolves, slew/smooth as prelude macros;
  semantics in `docs/language.md` and `docs/notes/evolve-design.md`).
  Known edge: the player-hit iframe guard reads pre-tick state, so two
  damage contacts in ONE tick both pass the guard — the atomic
  multi-field remat spec covers it if a card ever needs it. Still open
  on this track:
  - per-dyn-field epochs (fades surviving motion remats), soft-cull
    fades, the F1 lint, and the masked-SoA fast path (the lowering
    target for batch `map`-remat shapes);
  - `vel` re-expression: pure surface change until the model/ split
    (b.vel introspection, clamp_integrator, and the compiled integrand
    programs all key on `DynNode::Vel` — recognition is semantically
    mandatory), so deferred to that split;
  - `stages` re-expression: own round, likely over `states`, not raw
    evolve (its corpus sites use exit slots, `forever`, and
    `(fn [exit] ...)` handoff);
  - known limitation: cart/polar/rot capture guards
    (`contains_unbound_axis`) run on the RAW form, before expansion — a
    macro whose expansion introduces t-dependence is not recognized as a
    dyn expression. Revisit if a real card hits it.
- Extraction and 3D embedding remain unimplemented.
- Tick/rule ergonomics are still settling. Core now has primitive `deftick`
  plus domain expressions such as `(entities-where ...)` and `(collisions
  :a :b)`; row-wise helpers/macros should live in lib/prelude rather than
  reintroducing core `defrule` magic.
- Blocking lasers / world-geometry extent from DMK §13.7 remains unported.
- RNG is sequential splitmix, so replay determinism holds but spawn-order
  independence does not.
- Array-valued dyn meta binds per spawn element (`NumDynRepr::AxisSel`),
  but each entity still evaluates the full shared array per tick and keeps
  its lane — the compiled-dyn pass should recognize the shared program and
  evaluate once per group, scattering lanes (SS5 array-of-signals/
  signal-of-array interchange; compiled-dyn milestone B).

## Engine Refactor

- Deduplicate `EntitySpecStore` cold dyn/projector data into shared
  spawn-site/program/archetype storage where possible. (Milestone B's
  first slice turned out not to need it: rand-free spawn groups already
  share node/program Rcs, and ring-sized lanes amortize op decode. Still
  the lever for cross-spawn lane widening — structural program interning
  would fuse per-ring groups — and for memory.)
- Move remaining gameplay-domain behavior out of core: bare hostile
  `(cull)`. Host palette tables (`style_rgb`, `dot_radius`) remain stock
  host policy in `host.rs`; move them behind host/profile config when a
  second frontend needs different vocabulary.
- Remaining render-surface work: per-kind registered row schemas with
  manifest negotiation (the current schema is one global key->kind map),
  the builtin field rename/pick adapter, and a mesh/sprite-batch kind.
  Known trade (decided): rule-emitted rows are tick-cadence snapshots, so
  frame-time re-evaluation/interpolation is a host concern.
- Compile dyn evaluation to a flat program with fixed scratch storage.
  Design and status: `docs/notes/compiled-dyn-design.md` — milestone A
  (closed numeric ClosedPt/Vel/RotExpr programs, defn inlining, LerpSmooth
  and Kw-head/Const-fold coverage) is DONE and oracle-gated
  (MAKU_LOWER_ORACLE=1); the renderer half of milestone C (compiled
  deftick render rules + numeric row predicates, `interp/rulelower.rs`)
  is DONE. Milestone B first slice (round 19) is DONE: batched Vel steps
  (rows whose figure is constant wrappers over one compiled-integrand
  Vel node run as lanes of one program run — `run_lanes`,
  `VelBatchScratch` — writing n2 columns directly) plus the pos_only
  pose fast path (collide fill + cull read integrator state through the
  wrappers; single-cell schemas resolve their slot without hashing),
  both oracle-checked per lane/row. Still open under B: input slots
  (captures/rand as data — one program per spawn SITE; with structural
  interning this widens lanes across spawns), ClosedPt group pose
  evaluation, AxisSel lane scatter, and the bail census's homing-slew
  nodes needing ReadScan + Channel ops. Remaining cheap win #1: motion
  readers closing over SoA columns + row index instead of building
  per-row snapshots (readers are constructed per entity per phase).
  Also open: partial prefiltering for mixed entities-where predicates
  (recognized-plus-residual conjunctions still fall back whole).
  Stance (revised 2026-07): the lowering tiers. The current tier is
  AOT-to-IR at card load (NumProgram, executed by the `run`/`run_lanes`
  interpreter loops); the planned destination is a JIT/native-codegen
  tier that compiles the SAME NumProgram per distinct program and slots
  in behind the same (program, input lanes, scratch) boundary — the
  match-loop executors demote to the fallback tier for cold/uncompiled
  programs. Interim work must prepare for that, not fight it: keep the
  executor boundary narrow (batch call sites hand over lanes + scratch,
  never reach into op internals), keep ops total and callback-free (the
  planned Interp fallback op is the one interpreter re-entry point, and
  it defines the JIT→interpreter ABI), make captures/rand INPUT SLOTS so
  programs are per-site-shared and compile once, and treat structural
  program interning as the compile-cache key. Hard requirement carried
  into codegen: bit-exact f64 semantics vs the IR interpreter (same op
  order, same libm, no fast-math) — the lowering oracle and replay/scrub
  determinism both depend on it. The full JIT-readiness gap list and
  sequencing (IR unification, input slots/interning, no-Interp-op
  totality contract, batch seams, cranelift/platform notes) is in
  compiled-dyn-design.md "JIT readiness".
  The interpreter splits by role: the CONTROL PLANE (card loading,
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
- Perf campaign (ongoing; rounds 7-21 landed — narrative in git history).
  Rig: `MAKU_WALL_ONLY=1 cargo run --release --example profile` for bare
  walls (the flat profiler's own bookkeeping is ~18% on dense cards);
  macOS `sample` on the release binary is ground truth; scaled case
  `profile cards/tutorials/t03.maku ex3-fruit-colors 12000`. Bare walls
  as of round 21: fruit 119.3ms/900t (5050ms at round 7 — 42x), scaled
  12k rig 2.49s (3.11s at round-21 start, −20% same-session; 16.9s at
  round 15), reimu ~137ms, spell-2 20.7ms, cradle 48.7ms. Round 21 =
  milestone-C SoA render output (render-output-design.md): compiled
  point rules emit column batches with direct numeric gather; the
  `eval_compiled_row_val` + recycle rows collapsed. Remaining levers on
  the round-21 sample, in payoff order:
  - compiled tick passes ~28% of step (predicate scan + batch field
    reads/sym columns) but mostly irreducible per-row reads now; sym
    columns still clone `Rc<str>` per row (a per-batch symbol-id table
    is the next representation step if it ever shows);
  - collision index capture ~14% of step (AABB build, memory-bound);
  - `fast_pos_pose` ~11%: called 2x/row/tick (collide fill + cull); a
    cull-time reuse of the collide pose is exact for Vel chains ONLY if
    nothing between the phases mutates n2 state or figures — needs a
    rule-effect audit before it's sound;
  - remaining interpreted rule scans (`evaluate_list_inner` ~8% — beam/
    cull/hp rules) — the rule-lowering surface;
  - milestone-B widening (input slots, interning, ClosedPt group pose)
    is now JIT prep more than wall win on this rig.
- Follow-ups on the load-time AST rewrite pass (`interp/rewrite.rs`):
  (b) macro-expansion output is not rewritten (expansion is lazy per-eval;
  shapes inside macro-generated forms keep interpreted cost); (c) purity
  edge: a pure higher-order builtin applying an impure user fn passed BY
  NAME is classified pure — conservative table fix if it ever bites.
- Keep dyn coercions as explicit language-semantic branches while the
  interpreter is untyped. `interp::coerce` owns the value-level `DynLike`
  bridge; a future trait-style coercion surface should be over typed IR
  targets, not scattered Rust conversions over raw values.
- Collapse the remaining pose/figure asymmetry. `DynLike::Dyn(Pose)` is a
  typed dynamic value, not a data atom; the target is still plain `Figure`
  values lifted through `Dyn<Figure>`, with `linear` and friends represented
  as optimized `Dyn<Pose>` constructors that lift to figure dynamics.
- Move the dyn kernel (and entity spec / state-schema semantic halves) to
  model/ as a backend-parametric `Dyn<E>`, AFTER the remaining evolve
  re-expression (vel/stages) shrinks the kernel (moving now would enshrine
  Vel/Stages, which become lib shapes). Direction + sequencing:
  `docs/notes/model-split.md`.
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
  kernel-shrink worklist live in `docs/notes/builtins-audit.md` (wave 1 is
  done; easings and derived array verbs wait on profiling, `map`/`filter`
  intrinsic-ification and the `channel` merge wait on their flags).
  Evolve semantics (the one stateful constructor, closed-vs-live sampling,
  dyn<T> ≅ t -> T with application-as-sampling) are settled in
  `docs/notes/evolve-design.md`.
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
- Candidate stdlib move: family->hitbox-radius data currently repeated at
  call sites.
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
- Host-facing tick-rate configurability remains a later policy decision
  (the rate is World-owned `TickTiming`; runtime paths read it).
- AOT/wasm compiler work is unstarted.

## Docs

- Tutorials t01-t09, tbosses, and tstages are ported. Future doc work should
  focus on stabilizing the new tutorial site, reader view, and host API docs.
- `docs/from-dmk.md` remains the place for DMK/BDSL mapping notes; tutorials
  should stay standalone and idiomatic for Maku.
