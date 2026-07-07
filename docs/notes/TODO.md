# Implementation notes / prototype-vs-spec gaps

The spec (docs/language.md) is authoritative; this tracks what the prototype
(`proto/`) has not yet realized, plus engineering debt. §-references are to
language.md.

## Language features spec'd but unimplemented
- `states` leftovers (§8): state-body return value as the next label
  (routing is goto-or-state-order); richer spellcard templates than the
  `phases` macro (:name/:type/hp bars) as card macros once the boss
  tutorial demands them (`phases` itself is lib code now, with its
  `(finally …)` tail compiling to core `finally` — extend it there, not in
  the engine).
- `(with {$chan v} body)` scoped channel overrides (§3/§13.8).
- Pattern-embedding scope adapters (§10) — callable patterns embed bare:
  defaults only, no argument passing, shared cells.
- Channel manifest / load-time contract check (§3) — `$wind` on a host that
  doesn't provide it should fail at load, not at tick 400.
- `remat` / `manipulate`: queries (style axes + :where over the bullet
  view), single-slot remat (motion; epoch rebases whole-bullet), and
  set-style landed with tutorial 02. Still missing: per-slot epochs
  (a half-finished fade surviving a motion remat), soft-cull fades,
  the F1 lint, and the masked-SoA fast path (all callbacks bill fuel).
  SoA now has a concrete benchmark: t03 ex2 runs ~2.5ms/tick (release)
  at its ~1700-bullet steady state, dominated by per-bullet tree-walked
  signal eval; debug builds are 10-20x worse (tutorial run lines now
  say --release).
- Extraction (§10), 3D embedding (§12). (Ancestor clocks closed by the
  t09 audit: clocks are ordinary values — capture $tick, read
  (live $tick) against the epoch; (live …) now counts as
  time-dependence for signal deferral, which was the one engine fix.)

## Known approximations (documented in code)
- Core semantic model target (2026-07):
  ```text
  Figure = Pose | Polyline | ParametricCurve | Composite...
  Dyn<F> = t -> F
  ColliderProjector = Figure, t -> [Collider]
  RenderProjector = Figure, t -> [Render]
  Entity = Dyn<Figure> * ColliderProjector * RenderProjector * Meta
  ColliderSpec / RenderSpec = source-level data that lowers to a projector
  ```
  A pose carries position plus orientation, which is why current point-like
  figures can drive facing and subfire semantics. Interpreter `DynNode`s are
  prototype-level `Dyn<Pose>` expressions, not the semantic `Pose` value; a
  compiled core should store pose data compactly as `(x, y, theta?)`, where
  `theta = none` means "derive facing from context" and `theta = some 0`
  remains distinguishable from an unspecified angle. Unit vectors can be
  cached by optimized backends when useful, but the semantic pose
  representation is angular.
  The Rust prototype now represents the first slice as `Dyn<Figure>` through
  the `DynFigure` type alias. Its backing is still a compatibility enum
  (`DynRepr`) rather than the final typed expression IR, but figure-valued
  dyns no longer have a separate ad hoc top-level type.

  The important layer boundary is:
  ```text
  Figure:    abstract/math geometry evolving over time
  Collider:  per-tick collision rows derived from the current figure
  Render:    per-tick renderable records/meshes derived from the current figure
  Meta:      opaque library/host data
  ```
  `Collider` and `Render` are realized rows at the engine/host boundary, not
  the normal authoring representation. Card/library code should usually
  construct specs/projectors, not raw rows with baked world positions. A later
  internal escape hatch may expose raw row constructors for custom projectors
  or backend tests, but they should not be the ordinary `spawn` surface.

  The abstract figure should not be locked to one sampled/renderable form.
  A parametric curve may render as a sampled polyline today, a mesh tomorrow,
  and collide through a sampled capsule chain today or an analytic distance
  test later. Sampling is therefore not intrinsic to the figure; it belongs
  to the collider/render dyn slot or to an authoring helper that generates
  a sampled figure.

  Candidate figure/collider/render shapes:
  ```text
  Figure =
    Pose(Pose)
    Polyline(Rc<[Pose]>)
    ParametricCurve { eval: u -> Pose, domain: CurveDomain }

  Collider =
    None
    Circle { layer, radius }
    CapsuleChain { layer, points, radius }
    AnalyticCurveDistance { source, tolerance, radius } // later

  Render =
    None
    Point { pose/style }
    Polyline { points, stroke/style }
    Mesh { source/cache/material } // later

  SampleSet = Values(Vec<u>) | RangeStep(min, max, step)
  ```
  Semantically collider and render are projectors over the figure, not
  independent static entity data. Syntax can still represent a projector as a
  dyn-like list/record schema, and implementation can lower common projectors
  to stable slots, but the meaning is:
  ```text
  collider(entity.figure(t), t) -> [Collider]
  renderer(entity.figure(t), t) -> [Render]
  ```
  A single projector can return zero, one, or many rows, so the semantic
  entity does not need "multiple renderers" or "multiple colliders" as
  primitive fields. Multiple source specs are just syntax/convenience for a
  projector that returns a list.
  General dyn typing rule:
  ```text
  Dyn<T> is the typed interpretation of any expression in a time-indexed
  context. The coercion/lowering walks the target type: every leaf may be
  static or dyn, and a fully static structure is just the degenerate
  constant dyn. m"..." is only the numeric shorthand for one leaf; it does
  not make maps/arrays special by itself.
  ```
  Ordinary structure types remain ordinary structure types: `List<T>`,
  `List<Dyn<T>>`, and `Dyn<List<T>>` are distinct. A list literal is a
  normal `List<T>` unless an expected `Dyn<List<T>>` context asks the
  coercer to lift the whole structure. The same source value can therefore
  flow through ordinary code as a structure and only be schema-checked/lifted
  when a typed boundary such as `spawn` expects `Dyn<[ColliderSpec]>`.
  Any typed dyn field follows this same handler, so projector-specific dyn
  constructors should not be necessary. If `Dyn<ColliderSpec>` is expected, both
  `{:layer :hostile :shape [:circle {:radius 0.08}]}` and
  `{:layer :hostile :shape [:circle {:radius m"0.08 + 0.02*t"}]}` go
  through the same structural coercion; the first const-folds, the second
  produces a dyn radius inside a preserved collider shape. A collider slot
  can also be `(if (< t 2) :none [:capsule-chain ...])` when the whole
  shape/list is dynamic.
  For implementation and compilation, the common/static-arity projector case
  can lower to stable slots:
  ```text
  Entity = Dyn<Figure> * Rc<[DynCollider]> * Rc<[DynRender]> * Meta
  ```
  A slot that disappears evaluates to `Collider::None` or `Render::None`.
  This removes separate activity/mask concepts from the low-level model:
  warning, fill, changing radius, and disappearing colliders are just the
  collider/render dyn returning different per-tick data.

  Target predicate model, especially if we adopt more k-like verbs/adverbs:
  there should be no distinct long-term language-level `Bool` type. Predicate
  values are numeric masks: `0` is false and nonzero integer values are true
  (schema/lint can prefer `0`/`1` for flag-like fields). There is no general
  truthiness for keywords, strings, lists, maps, poses, or geometry. Reader
  `true`/`false` may remain temporary compatibility sugar for `1`/`0`.
  Predicate schemas should be numeric subcontracts such as `Mask`, `Flag`,
  `Count`, and `Index`; comparisons and predicates should return numeric
  `0`/`1`, and conditionals/guards should consume masks. There are no
  runtime `and`/`or` logical folds; use arithmetic folds such as `*`, `+`,
  `min`, or `max` depending on the intended mask/count semantics. `not`
  is numeric: zero becomes `1`, and any nonzero number becomes `0`. This
  lets square waves and array-mask expressions flow naturally into
  `Dyn<Predicate>` contexts without explicit conversion while preserving
  schema vocabulary.

  Target entity/query model:
  ```text
  Entity indices are ephemeral row indices into the current world arrays.
  Handles are stable cross-time references.
  Query = predicate/filter over entity rows -> EntitySet(Vec<usize>)
  Manipulate = map/action over an EntitySet
  Aggregations = ordinary array/library code over EntitySet accessors
  ```
  Core should expose `entities-where` over predicate functions, e.g.
  `(entities-where (matches :team :enemy))` where `matches` expands to a row
  predicate like `(fn [x] (= x.team :enemy))`. Flat row access is keyword
  application/dot syntax, such as `(:pos entities)` and `b.pos`; helpers like
  `count-entities`, `sum-entities`, and `nearest-entity` are
  compatibility/library functions over those primitives. Language-level
  entity fields are unified and flat. Storage may distinguish builtin
  position/state from user fields, but there should not be separate
  arbitrary `cols` and `meta` concepts in source. Fields are finite but
  large: load/init determines an interned field layout with bounded dense
  slots suitable for hot queries and manipulation. Long-term,
  style/team/damage/kind should drift into that field layout so filters are
  masked comparisons over SoA arrays. Contact resolution should likewise
  materialize collider rows, bucket them by layer, and iterate layer-index
  vectors rather than nested entity scans. `entity_view` maps remain a
  debug/callback bridge, not the hot data model.

  Array/adverb model:
  K's adverb family is the north star for control-flow-like array code, but
  the builtin set is intentionally profiling-driven. Initial core candidates
  are `map`/each, `filter`, `fold`, `scan`, `each-prior`, `window`,
  `sort-by`, `best-by`, `count`, and the existing sequence basics
  (`nth`/`take`/`drop`/`concat`). `match` remains the OCaml-like basis for
  unstructured list/form manipulation and macro code. More specialized
  adverbs such as binsearch, case, join/split, encode/decode, converge, and
  while-style adverbs can start in the prelude/library unless profiling
  shows they need builtin lowering.

  Target number model (2026-07): the mask decision extends to
  int-vs-float. There is ONE language-level `Number` type, APL-style;
  representation is a storage property the runtime discovers, never a
  declared type — dense vectors are kept in the narrowest rep that holds
  the values (bitmask for 0/1 masks, i32, f32) and widen transparently.
  `Mask`/`Count`/`Index` schemas are integrality CONTRACTS enforced at
  typed boundaries, not types: an index/count position demands an
  exactly integral value (2.5 is a checked error, never a truncation),
  and NaN reaching a mask/conditional position is a checked error rather
  than silently truthy under `x != 0`. Comparison/logic verbs emit
  exactly 0/1. Scalar language values should be f64 (exact to 2^53;
  f32's 2^24 is ~77 hours of $tick at 60fps — real for an attract mode),
  while hot derived columns stay f32; mixed precision is replay-safe as
  long as WHERE narrowing happens is deterministic.

  The same uniformity-is-discovered principle settles vector/matrix
  syntax: no `m[]`-style uniform-literal marker. Following k, a
  homogeneous list silently IS a typed dense vector (auto-packed on
  construction, boxed-list fallback when mixed); printing may reflect
  the rep, the writer never annotates it. Typed boundaries (spawn slots,
  schemas) are where non-uniformity is rejected, naming the offending
  element. Forcing dense storage is a constructor's job, not grammar;
  true matrix semantics (matmul vs list-of-rows), if ever wanted, is
  rank/shape information in the VALUE (a shaped array), which bracket
  flavor could not express anyway.

  Low-level collider/render syntax should be separated from the semantic
  representation. Source-level specs are spawn arguments, not generally
  first-class functions; semantically those specs denote one collider
  projector and one render projector over the spawned figure:
  ```text
  (spawn dyn
    [[:collider :hostile-shot :capsule-chain
      {:domain hot-domain :radius r}]]
    [[:renderer :polyline
      {:domain render-domain :stroke w}]]
    meta)
  ```
  The lists above are syntax/coercion input:
  ```text
  Dyn<[ColliderSpec]> -> ColliderProjector -> [Collider]
  Dyn<[RenderSpec]>   -> RenderProjector   -> [Render]
  ```
  After lowering, the meaning is `collider(figure(t), t) -> [Collider]` and
  `renderer(figure(t), t) -> [Render]`. A spec's domain is applied to the
  current figure. For a parametric curve it supplies `u` values/ranges; for a
  polyline it can be omitted (`:full`) or used as an index/subrange selector.
  Higher-level constructors like Touhou `laser` produce these spawn arguments;
  core does not need one builtin per collider/render mode.

  The second spawn argument is an array of collider specs that composes into
  one collider projector. Empty array means a projector that returns no
  colliders. Collision layer is universal collider metadata owned by core as
  an opaque routing key:
  ```text
  Collider slot {
    layer: Symbol,
    shape: Circle | CapsuleChain | ...
  }
  ```
  That syntax does not directly construct a raw `Collider`; it describes how
  to derive raw collider rows from the current figure.
  The third spawn argument is an array of render specs that composes into one
  render projector:
  ```text
  Render slot {
    shape: Point | Polyline | Mesh...
  }
  ```
  Likewise, render specs describe projection. Raw render rows are the
  per-frame output consumed by the host or an optional renderer backend.
  The prototype now stores user collider/render arguments as generic
  dyn-like metadata. Each tick realizes those structures to lists, then
  decodes the concrete list entries through the collider/render schemas.
  Compatibility curve projections still use internal Rust slots until the
  Rust-side `laser` bridge can move into library code.
  Normalized curves are just `Range(0, 1)`. Higher-level helpers can turn
  min/max/step descriptions into `Values`; callers/slots must provide
  sample sets compatible with the source domain when they use sampled
  slots. Traces are derived figures over an entity's dyn history, not a
  separate primitive kind; retained samples are a cache/policy for integrated
  dyns. Facing is part of each pose sample; finite-difference facing is only
  a possible helper/default, not the core representation. Interpolation over
  trace sample indices is an explicit higher-level helper, not implicit core
  behavior.

  A filling laser should become a Touhou/library constructor that creates a
  time-varying figure plus collider/render slots. Warning render and hot
  collision are separate dyn slots over the same figure, not special curve
  semantics in core. Core exposes figures, collider results/events, render
  data, and opaque indexed meta/fields; hosts decide how meta renders.
  Touhou names like bullet/shot/enemy/boss/player/laser are library
  constructors over this core.
- Zero-allocation data model target (2026-07; steady-state zero alloc —
  card load is the sanctioned allocation window):
  * The organizing split is COLD SOURCES vs HOT DERIVED. Retained entity
    state is cold: compiled dyn slots (mostly Rc into card data), curve
    params, meta. Almost everything the hot loops touch is per-tick
    derived output rebuilt from it.
  * Not one big dense matrix: mixed types, variable-width data (capsule
    chains, polylines), and passes touching disjoint column subsets all
    fight it — and dyn slots are programs, not numbers, so they cannot
    live in a matrix at all.
  * Retained storage: a dense entity slab (swap-remove keeps it packed;
    despawn is O(1) and alloc-free) addressed by GENERATIONAL HANDLES
    with a sparse id→slot map. Handles are needed regardless of the
    slab: guides, frame-tree parents, and (live boss)-style references
    must survive slot reuse. Handles are opaque values, not language
    numbers.
  * Hot derived state: SoA f32 columns rebuilt each tick, grouped by
    pass. Tick pipeline: (1) figure pass evaluates Dyn<Figure> down the
    frame tree and writes dense x/y/th columns; (2) collision pass reads
    those plus collider rows; (3) render extraction reads those plus
    render rows. The pose columns are the one matrix-like thing, and
    they are derived, not authoritative.
  * Colliders stored per shape subtype as PER-TICK SCRATCH, not retained
    state: since every DynCollider re-evaluates each tick anyway
    (growing radius, :none while dormant), evaluation appends results
    into per-shape scratch buffers (circles: cx/cy/r/layer/owner SoA;
    capsule chains: radius/layer/owner plus offset+len into a shared
    point buffer). Cleared to len=0 each tick with capacity retained —
    zero alloc after warmup, and narrowphase iterates homogeneous arrays
    with no per-element enum branch. The join is the OWNER SLOT INDEX
    carried in each row — no key column, no hash. Render rows follow the
    identical pattern, bucketed per render tag.
  * Curves never enter hot storage; only their samples do. A parametric
    curve is compiled eval + domain (cold) and materializes only when a
    slot domain samples it, into a per-tick bump arena (preallocated,
    reset each tick). Sampled curves from cards are Rc<[Pose]> built at
    load time.
  * Supporting rules: intern layer strings to small ints at spawn (the
    collision pass never touches Rc<str>); broadphase buckets/grid
    preallocated and reused; dyn evaluation uses a fixed scratch stack
    with dyns compiled to a flat program at spawn (the DynLike work
    already points there).
  * Motion state uses the same "finite schema at load, dense row storage at
    runtime" rule as entity fields. The current interpreter stores scanned
    state in `MotionState = HashMap<usize, Cell>`, keyed by `DynNode` pointer
    identities plus `site_key(base, counter)` for expression-local scan sites
    such as `slew`/`smooth`; this can allocate during tick execution. Target
    lowering discovers stateful sites in each retained dyn program and assigns
    dense slots:
    ```text
    state_n2 : StateN2SlotId x entity_row -> [f64; 2]
    state_dyn: StateDynSlotId x entity_row -> DynPose   // only while lazy stages remain interpreted
    ```
    `Vel`, `RotExpr`, `Path`, `Stages` bookkeeping, `slew`, and `smooth`
    become slot reads/writes. Lazy `Stages` segment construction is the
    outlier: either lower it to a closed set of segment dyns at load time or
    isolate it as an interpreted/allocating compatibility path. A compiled
    form should not hash by node pointer or grow per-entity maps in steady
    state.
- Render/meta boundary target (2026-07): keep the render projector output and
  entity meta separate even if both use symbol-keyed records as their value
  format. They differ on every axis that matters: render rows are typed
  per-frame projection products from a Figure and are hot (touched every
  frame); meta is engine-opaque, arbitrarily keyed, queried by lib logic
  (team/damage are lib concepts precisely because they live in meta), and
  mostly cold. Folding render into meta would make extraction rummage an
  untyped map per entity per frame. The host render boundary has two tiers:
  * Tier 0 (host renders): the per-tick derived snapshot IS the
    contract — per render tag, a packed array of fully RESOLVED rows
    (owner handle, world pose, evaluated params, sample slices into the
    frame arena). Dyns are already evaluated; the host consumes plain
    data and never touches the language. Meta is reachable by owner
    handle for lib conventions (team→sprite), but anything
    per-frame-hot belongs in a render param, not meta.
  * Tier 1 (engine meshes): a feature-gated tessellation module — or a
    sibling crate built purely on the Tier 0 API, which doubles as
    proof the boundary suffices: render row → vertices/indices written
    into preallocated buffers, batched per tag so the host maps tags to
    pipelines/materials and just draws. Keeps "core is a 2D graphing +
    collision engine" honest.
  * Convergence: the Tier 0 snapshot is the SAME per-tag scratch-buffer
    structure the zero-alloc model wants for render rows; building the
    data model correctly yields Tier 0 for free.
- Meta as shape-interned tables target (2026-07): keywords are GLOBALLY
  INTERNED to ints at card-load time (the sanctioned alloc window) — a
  keyword compare or map lookup is an int compare, and "string-keyed
  map vs int-indexed" is a false dichotomy once keys are keywords. Do
  NOT erase names at compile time: cross-card agreement on :hp and host
  inspection/serialization need the shared load-time table; keep the
  reverse int→name table for debugging and the host boundary (never on
  hot paths). Runtime string→keyword construction is confined to load
  time. Interned enums (layers, tags, teams, event names) fall out of the
  same mechanism.
  * Shape interning (hidden classes, spawn-shaped): a spawn site's meta
    literal has a static key set; the sorted key set interns to a shape
    id and entity meta is (shape_id, dense value slice). Reads compile
    to a fixed offset where the shape is statically known, a small
    shape-table lookup when polymorphic. RULE: meta keys are fixed at
    spawn — values mutable, adding keys post-spawn is an error (declare
    with an initial 0). That one rule keeps the implementation
    table-shaped with no V8-style shape-transition machinery.
  * Shape = archetype: same-shape metas share one columnar side table
    per shape (value columns + an owner-slot column; swap-remove within
    the table; the slab row stores (shape_id, row)). Deliberately NOT
    full archetype ECS — the primary slab stays unified so frame-tree
    eval order, generational handles, and swap-remove bookkeeping stay
    simple. Same join-by-owner pattern as collider scratch, retained
    instead of rebuilt.
  * Queries match by KEY SUBSET, not exact shape: "everything with :hp"
    hits every shape whose key set contains :hp, so accidental shape
    collisions between unrelated cards are harmless (they were meant to
    match) and shape fragmentation (:score added to one spawn site)
    only adds a table to walk. Shapes are static after load, so
    shape↔query membership is precomputed once; a runtime query is
    "iterate my shape tables, scan dense columns" — no hashing, no
    per-entity lookup. This is what lets lib-level systems (damage:
    every entity with :hp and :team) compile to dense column scans with
    core knowing nothing about hp.
  * Typed WorldFields: finite entity metadata is stored in separate
    preallocated matrices by value type/size, not as per-entity maps:
    ```text
    nums    : NumFieldId    x entity_row -> f64
    syms    : SymFieldId    x entity_row -> Symbol
    handles : HandleFieldId x entity_row -> EntityRef
    present : per-typed-field bitsets or typed sentinel policy
    ```
    Field names intern at load/lowering time into typed field ids. All
    fields that can ever be used by the loaded card are known before runtime
    spawn; unknown fields are load/reschema errors, not per-tick allocation.
    The current `cols` / `sym_fields` bridge keeps the semantics moving in
    that direction while dense SoA storage is still pending.
  * Determinism notes: interned ids are assigned by load order — either
    make load order canonical or keep ids out of anything observable
    (ordering, replay hashing). Query iteration order is shape-table
    order then row order, which swap-remove perturbs: document as
    unspecified or sort by owner slot where order matters, else it is a
    replay-determinism leak of exactly the kind the tick-identical
    smoke exists to catch.
- Core vocabulary migration toward the "2D graphing + collision engine"
  boundary:
  1. ~~Rename runtime `Bullet`/`bullets` to `Entity`/`entities`, keeping
     source syntax stable for compatibility.~~ Done in Rust core; docs/cards
     still use "bullet" for Touhou-authored entities where appropriate.
  2. ~~Rename geometry kinds internally (`Laser`/`Pather` → `Curve`/`Trail`)
     while keeping surface aliases until docs/cards move.~~ Done in core.
  3. Remove gameplay-domain fields from core (`team`, `damage`, bare
     hostile `(cull)`); replace them with generic indexed metadata/tags
     and explicit query culls.
  4. Move Touhou-facing API to short library names (`bullet`, `shot`,
     `enemy`, `player`, `boss`) with `spawn-*` compatibility aliases.
- Figure refactor staging:
  1. ~~Introduce internal curve/geometry vocabulary while preserving surface
     `laser`/`pather` aliases.~~ Done for entity kinds and pre-spawn values.
  2. ~~Collapse `Val::CurveV` and `Val::TrailV` toward one geometry value
     representation that `spawn` materializes.~~ Done as `CurveV` with
     parametric/traced backings.
  2a. ~~Carry `CurveDomain` into runtime curve sampling instead of flattening
      it immediately to `u-max`.~~ Done for curve entities; traced curves
      use dynamic integer-indexed sample domains, with interpolation reserved
      for an explicit higher-level helper.
  2b. ~~Collapse runtime `Entity` from separate pose motion plus static
      geometry into one `DynFigure` value.~~ Done.
  2c. ~~Move concrete curve sampling back out of `ParametricCurve`.~~ Done;
      the current compatibility slot owns `SampleSet` and dynamic
      `:u-max` while the abstract curve figure remains `eval + domain`.
  2d. ~~Split curve compatibility data into collision/render slot
      structs.~~ Done; sampling, activity, and width now belong to temporary
      slot-shaped components. Trace policy is separate cache policy.
  2e. ~~Move projection/cache fields out of the generic legacy bucket.~~ Done;
      entities now carry collider/render metadata, compatibility curve
      projections, and `EntityCachePolicy`.
  2f. ~~Make layer universal collider metadata.~~ Done; entity collider
      slots are now `ColliderSlot { layer, shape }`, with layer outside the
      circle/capsule-chain shape.
  2g. Target update: collider/render spawn args are generic dyn-like
      structures expected to realize to lists; typed collider/render data is
      decoded only at the collision/render boundary.
  2h. ~~Introduce internal dyn collider/render slots.~~ Done for the current
      bridge: collider slots are `DynCollider::Slot(ColliderSlot)`, with
      shape variants for circles and capsule chains, and curve rendering is
      `DynRender::Polyline`.
  2i. ~~Make figure dyns use the shared `Dyn<T>` shell.~~ Done;
      `DynFigure` is now `Dyn<Figure>`, backed by `DynRepr`. The remaining
      work is to generalize `DynRepr`/evaluation beyond pose and figure.
  2j. ~~Add scalar dyn slots to the shared dyn evaluator.~~ Done;
      compatibility curve `:u-max` and hot fill fractions now store
      `Dyn<f64>` rather than raw `(Form, Env)` pairs, and evaluate through
      the generic `eval_dyn` path.
  2k. ~~Wrap pose-valued dyns in `Dyn<Pose>`.~~ Done for the interpreter
      value/stage/curve-anchor boundary: `Val::Dyn`, stage continuations,
      and extended curve anchors now carry `DynPose`. `DynNode` remains the
      recursive prototype IR underneath, and raw node references still appear
      inside that IR and frame specs.
  2l. ~~Split typed dyn backing reps.~~ Done; `Dyn<T>` now stores
      `T::Repr`, with separate `NumDynRepr`, `PoseDynRepr`, and
      `FigureDynRepr`. This removes the shared repr's runtime type guards
      and lets figure curves keep `DynPose` frames and curve expressions.
  2m. ~~Introduce materialized collider/render data.~~ Done; the hot
      collision and render paths now consume `ColliderData`/`RenderData`
      values, with `None` as an explicit data variant rather than maybe
      semantics. Compatibility slots still produce these data values.
  2n. ~~Make collider/render slots typed dyn containers.~~ Done;
      `DynCollider` and `DynRender` are now aliases for
      `Dyn<ColliderData>` and `Dyn<RenderData>`, with typed projection
      reprs. The next step is to move slot materialization/evaluation out of
      sim hot paths and into slot evaluators that take entity context.
  2o. ~~Move slot materialization behind evaluator functions.~~ Done;
      collision and render now consume `eval_collider_slot` /
      `eval_render_slot`, while compatibility curve sampling lives in the
      neutral sim slot module rather than in either hot path.
  2p. ~~Rename compatibility slot reps to generic projection-shaped slots.~~
      Done; the runtime now talks about circle projections, capsule-chain
      collider slots, and polyline render slots instead of laser/compat
      slot variants. Current surface `laser` lowering still happens in Rust
      as a bridge.
  2q. ~~Make curve collider slots self-contained after spawn lowering.~~
      Done; curve values still supply only capsule-chain sampling/fill shape
      parameters, but action materialization now combines those parameters
      with template collider layer/radius into self-contained
      `ColliderSlot { layer, shape: CapsuleChain { ... } }` values on the
      entity. The next step is to expose low-level slot construction so
      Touhou/library code can build those slots directly and the Rust
      `laser` bridge can disappear.
  2r. ~~Expose static low-level collider/render slot specs through spawn
      arguments.~~ Done; `(colliders ...)` accepts explicit `:shape` maps
      for `:circle` and `:capsule-chain`, while `(renderers ...)` accepts
      explicit `:polyline` specs. The typed boundary is now the operator
      argument; the old meta-key compatibility path was removed in 2v.5.
  2s. ~~Prototype structural dyn coercion for ordinary maps/vectors at the
      spawn boundary.~~ Done; dynamic leaves inside precomputed collider
      structures now survive until `spawn` validates the expected
      collider/render schema. Current implementation covers static structure
      shape, dyn expression leaves, and whole-list dynamic expressions via
      `(colliders dyn-list-expr)` / `(renderers dyn-list-expr)`. The
      prototype carrier is named `DynLike` rather than `DynStruct` or
      `SourceExpr`: it is value-level coercion input, distinct from the
      EDN/form AST. `as_*` schema checks at typed boundaries turn dyn-like
      structures into `Dyn<T>`-backed slots, while static structures remain
      ordinary values unless an expected dyn type asks for lifting.
  2t. ~~Stage spawn slot checks by coercion family.~~ Done; first
      static paths still let `as_dynlike_list` reject non-list outer shapes
      early, while dynamic paths realize the generic `DynLike` first and then
      apply element schemas (`as_collider` / `as_render`). The recursive
      list/map dyn-like lift is shared; collider and render schemas only run
      after the expected list boundary is accepted.
  2u. ~~Generalize dynamic source leaves away from `DynNum`.~~ Done;
      `DynLike` now carries untyped `DynVal::Expr { form, env }`.
      Numeric interpretation happens only in numeric typed contexts such as
      `as_dyn_num`, leaving room for numeric masks, dynamic enum/shape
      choices, and other `Dyn<T>` targets without treating numbers as the
      only dynamic leaf kind.
  2v. ~~Make collider/render args generic dyn-list boundaries.~~ Done;
      entities and spawn actions now store generic `DynLike` collider/render
      metadata collected from explicit `(colliders ...)` / `(renderers ...)`
      arguments. At collision/render time it is
      realized to a concrete list and decoded through the typed schema; there
      is no special `DynColliderList` / `DynRenderList` semantic type.
  2v.1. ~~Collapse spawn actions to per-entity specs.~~ Done; `ActionV::Spawn`
      now carries `Vec<EntitySpec>` rather than a split payload of flattened
      figures plus call-level metadata. Execution applies the ambient frame,
      installs rows, interns columns, and pushes entities; spawn evaluation
      owns the per-element resolution.
  2v.2. ~~Introduce entity index-vector queries.~~ Done as a bridge:
      `entities-where` returns an ephemeral `EntitySet(Vec<usize>)`, and
      keyword application/dot syntax broadcasts flat field access over it.
      Entity sets are per-tick row-index views, not stable identity-keyed
      vectors. Tombstone/free-list/generation storage now keeps live rows
      from shifting when earlier rows die and prevents same-tick slot reuse.
      Entity capacity is an explicit host/session setting with
      `(resize-entities n)` recorded on the command tape; spawn errors
      instead of implicitly growing past the current capacity. Handles are
      now row+generation refs, so stale handles fail after slot reuse instead
      of accidentally targeting the replacement entity; the old separate
      entity id field has been removed.
      Stable per-entity control should keep handles or sort/key the view
      explicitly. Predicate queries such as `(entities-where (matches :team
      :enemy))` are now supported; query maps remain compatibility syntax.
      Existing `entity-pos` / `entity-col` compatibility aliases,
      `count-entities`, `sum-entities`, `nearest-entity`, and `manipulate`
      now use index-vector query resolution internally while preserving
      compatibility behavior.
  2v.3. ~~Remove curve-specific projection fields from entities.~~ Done;
      `CurveV` flattening now emits ordinary render spec data, and curve
      collision derives capsule-chain conversion from the materialized render
      projection instead of carrying `curve_collider` / `curve_renderer`
      side-channel fields on every entity.
  2v.4. ~~Remove runtime primary-hitbox entity field.~~ Done; compatibility
      `:hitbox` now rewrites the first static circle collider spec during
      spawn lowering, so collision consumes only the entity's collider specs.
  2v.5. ~~Remove `:colliders` / `:renderers` spawn meta compatibility.~~
      Done; collider and render specs are explicit `(colliders ...)` /
      `(renderers ...)` spawn arguments, and ordinary meta maps no longer
      carry projection data.
  2v.6. ~~Remove runtime damage entity field.~~ Done; numeric `:damage` and
      DMK `{:hit n}` damage maps now lower to the ordinary numeric `:damage`
      column consumed by Touhou contact code. Function-valued `:damage` was
      retired with the special field; richer dynamic contact metadata belongs
      with the finite meta/field layout work.
  2v.7. ~~Introduce world-local keyword/symbol interning for hot collision
      layers.~~ Done; `World` now owns a `SymbolTable`, collider slots/data and
      contact layer pairs store `Symbol`, and string/keyword names are interned
      at collider spec decode / `defcontact` registration boundaries.
  2v.8. ~~Intern event names in the retained log.~~ Done; explicit `(event
      :name)` actions and trigger events store `Symbol` internally, while
      `Sim::events_vec`, `Sim::with_events`, and host APIs resolve names back
      to strings at the boundary.
  2v.9. ~~Rename entity collider/render storage as projectors.~~ Done as a
      bridge; entities now carry `collider_projector` and `render_projector`
      fields (bridge structs around spec lists), while per-element spawn
      fragments are named `collider_specs` / `render_specs`. Realized
      `ColliderData` / `RenderData` remain boundary rows.
  2v.10. ~~Move compatibility style/signals under the render projector.~~
      Done as a bridge; `Entity` no longer owns `style` or `RenderSigs`.
      The current host renderer's legacy `:style`, `:hue`, `:scale`,
      `:facing`, and `:opacity` data lives on `RenderProjector` until those
      tags are lowered into ordinary renderer spec records.
  2v.11. ~~Namescape renderer compatibility fields in entity views.~~ Done;
      entity views now expose nested `:render {:style ...}` data, keyword
      access supports dotted map paths, and query maps accept
      `:render.style.family` / `:render.style.color` / `:render.style.variant`
      while flat style keys remain compatibility aliases.
  2v.12. ~~Intern entity column names internally.~~ Done; `TriggerRule`,
      contact `once` / `skip-if`, `set-col`, spawn column specs, and expose
      rules now store `ColName` symbols. `World` column layout maps symbols
      to dense slots, with string helpers retained at source/host/test
      boundaries.
  2v.13. ~~Move `:team` out of the entity struct.~~ Done as a bridge;
      entities now carry interned symbol fields (`sym_fields`), and source
      `:team :enemy` lowers to a `FieldName` / keyword-symbol pair. Query,
      view, cull-hostile, player-body checks, and host iframe checks read
      through keyword-field helpers.
  2v.14. ~~Generalize symbol-valued entity metadata.~~ Done as a bridge;
      symbol-valued spawn metadata now lowers into `sym_fields` unless the
      key is reserved for spawn/render/control compatibility. Entity views,
      field access, `matches`, and map queries can use arbitrary keyword
      metadata such as `:role :boss`.
  2v.15. ~~Introduce the `WorldFields` layout bridge.~~ Done; `World` now owns
      a `WorldFields` schema object with typed field-id names and numeric
      layout slots. Numeric values and symbol fields still live on entities,
      but layout/schema has a target home for future SoA matrices.
  2v.16. ~~Slot symbol-valued entity fields through `WorldFields`.~~ Done;
      keyword metadata such as `:team` and `:role` now lowers to interned
      symbol-field slots, with entity-local dense optional values as the
      temporary bridge before world-owned symbol matrices.
  2v.17. ~~Move numeric entity fields into `WorldFields`.~~ Done; numeric
      fields now live in world-owned slot-major matrices, with rows cleared
      on reuse. Culled rows remain invalid through `alive`, but numeric
      values survive until reuse so same-tick contact/trigger bookkeeping can
      observe writes made before culling.
  2v.18. ~~Introduce `EntityStore` as the intrinsic entity storage boundary.~~
      Done as a bridge; `World` now owns an `EntityStore` that contains rows,
      capacity, and the free-list/reuse policy, while `WorldFields` remains
      the finite user-field schema/matrix table.
  2v.19. ~~Put lifecycle/reuse operations behind `EntityStore` APIs.~~ Done;
      `EntityStore` now owns handle lookup, liveness, generation, cull, push,
      and reusable-row selection helpers. `EntityStore` rows and
      `WorldFields` matrices intentionally share one entity-row namespace.
  2v.20. ~~Move lifecycle columns into `EntityStore`.~~ Done; generation,
      alive flags, and freed-at ticks now live in `EntityStore` side vectors
      instead of on `Entity`, with tests and runtime code reading through the
      store APIs.
  2v.21. ~~Move birth ticks into `EntityStore`.~~ Done; entity birth ticks
      now live in a side vector with `birth`, `tau`, and `reset_birth`
      helpers, and remat/spawn update timing through the store.
  2v.22. ~~Replace `prev_pos` with sampled pose buffers.~~ Done; current and
      previous collision-pass poses now live in double-buffered
      `EntityStore` columns, and velocity/host/entity views read through
      sampled-pose helpers.
  2v.23. ~~Specify dense motion-state storage before moving `MotionState`.~~
      Done; the zero-allocation notes now identify current `HashMap` state
      allocation sites and target dense `state_n2` / compatibility
      `state_dyn` slot tables for scanned dyn programs.
  2v.24. ~~Introduce dense motion-state schema scaffolding.~~ Done;
      `MotionStateSchema` now tracks numeric and compatibility dyn slots, and
      a dyn-tree collector discovers node-level `Vel` / `Stages` state while
      leaving expression-local scan sites for expression lowering.
  2v.25. ~~Collect expression-local scan state sites.~~ Done; the schema
      collector walks scanned forms for `slew` / `smooth` in evaluation order
      and assigns `ScanSite` numeric slots under the containing node base.
  2v.26. ~~Attach motion-state schemas to entity rows.~~ Done; `EntityStore`
      now stores an `Rc<MotionStateSchema>` per row and refreshes it on
      spawn/reuse/remat, while the current `MotionState` map remains the
      execution bridge until dense state slots take over.
  2v.27. ~~Introduce dense motion-state columns.~~ Done as a bridge;
      `EntityStore` now owns slot-major `state_n2` and compatibility
      `state_dyn` columns sized/reset from each row's `MotionStateSchema`,
      and `Vel` integration mirrors its numeric state into dense row slots
      while legacy `MotionState` remains the read path.
  2v.28. ~~Represent scan-state builtin layout as data.~~ Done as a bridge;
      `scan_builtin_spec` now declares each stateful scan function's storage
      shape (`N2` for current `slew`/`smooth`), and both scan-site schema
      collection and scan-context dispatch consult that spec instead of
      keeping independent hardcoded name lists.
  2v.29. ~~Mirror scan-site state writes into dense columns.~~ Done as a
      bridge; advancing scan contexts now collect `ScanSite` `N2` writes from
      stateful scan functions and forward them through the dense motion-state
      write path, while legacy `MotionState` remains the read source.
  2v.30. ~~Read numeric motion state from dense columns.~~ Done as a bridge;
      dense-aware motion evaluation now reads `NodePtr` and `ScanSite` `N2`
      state before falling back to legacy `MotionState`, and sim stepping,
      collision sampling, rendering, trace recording, and culling pass
      row-local dense snapshots through that path.
  2w. ~~Give `DynLike` the target data shape.~~ Done as a bridge:
      `DynLike` is now `Atom(DataAtom) | Dyn(DynVal) | List | Map`, with
      map keys and leaves going through concrete atoms for `Num`, `Kw`,
      `Figure`, handles, and nothing. `DataAtom::Legacy(Val)` remains as a
      temporary escape hatch while interpreter/control objects (`Action`,
      `Fn`, `Form`, `Cells`) and old pose conveniences are migrated out of
      runtime data.
  2aa. ~~Remove runtime Vec2 values.~~ Done; coordinate constructors and
      point-valued channels now produce `Pose::point` (`theta = none`), while
      explicit rotations and tangent-producing samplers use oriented poses
      (`theta = some angle`).
  2x. ~~Switch predicates to numeric masks.~~ Done; comparisons and
      predicate builtins return `0`/`1`, source booleans evaluate to `0`/`1`,
      control guards consume numeric masks, `not` is numeric, and runtime
      `and`/`or` builtins were removed in favor of explicit arithmetic folds.
  2y. ~~Remove runtime bool values.~~ Done; `Val` no longer has a bool
      variant. `Form::Bool` remains syntax/quoted-form data, but evaluated
      booleans are numeric masks.
  2z. ~~Remove runtime string values.~~ Done; `Val` no longer has a string
      variant. `Form::Str` remains syntax/quoted-form data for imports,
      `m"..."`, and macro/form inspection; evaluated strings become
      keywords, and `form-name` returns keywords.
  2aa. ~~Clarify events as keyword symbols.~~ Done; `(event :name)` requires
      a keyword symbol and the action field is named accordingly. Event
      storage still uses `Rc<str>` as a bridge, but the target is the same
      load-time keyword/symbol interner used for tags, layers, and style
      names, with host conversion at the boundary.
  3. Represent fill as dyn collider/render slots returning different data
     over time rather than a laser-only lifecycle shortcut.
  4. Recast trails/pathers as derived curves over entity dyn history, with
     any retained samples treated as cache/policy rather than geometry.
  5. Move render tags into ordinary signal-valued meta/fields; collider
     scale/radius should be explicit collider data, not borrowed from a
     render-specific `:scale`.
  6. Remove remaining gameplay-domain metadata from core (`team`, bare
     hostile `(cull)`) in favor of generic fields/tags and Touhou helpers.
- ~~Pathers render as points; laser `:width` ignored by collision~~ done
  with tutorial 04: pathers record trails (rendered + capsule-chain
  hitbox, bounded by the window); `:width` scales laser collision.
  Remaining §13.7: blocking lasers (world geometry → extent).
- Trigger predicates: single-column `≤` crossings only (§13.13).
- Shipped: `defcontact` moved contact resolution out of the engine. Layers
  are opaque tags, teams are query metadata only, Touhou hit/graze/shot
  rules live in `cards/lib/touhou.maku`, and `$graze`/`$hits` are stdlib
  derived channels — `(sum-entities {:team :player-body} :col)` over
  per-entity counter columns, NOT per-entity :expose registrations: a host
  may layer its stock rig over a card that ships its own (the smoke does),
  and two exposes would fight over one channel name while the sum over
  every player body is what the HUD means. The engine keeps hot shape
  detection plus the two data prefilters (`:once`, `:skip-if`): CHECKS ARE
  DATA, CONTACTS ARE CODE.
- RNG is sequential splitmix, not counter-keyed by spawn path (§5) — replay
  determinism holds, order-independence does not.
- Scrub-back across a swap/add boundary restores the pre-change program
  (correct); seeks are exploration — branch commits only on resume.

## Standard library (cards/lib/, compile-time embedded)
- Shipped: `prelude` (AUTOIMPORTED, sentinel-deduped: `when`/`unless`),
  `touhou` (spawn templates, variadic metas; spawn-boss = enemy + phase
  machine owning the boss conventions — structured boss channel binding, registration
  wait, bound `boss`; `phases` as a macro over `states` with {:hp n}
  gates; invuln; spawn-player/player-rig; $player/$lives/$enemies/
  $nearest-enemy as defchannels) and `player-rig` as a compatibility shim.
  Authored as files, inlined via include_str — every host
  resolves `(import "touhou")` identically; users import the lib, they
  don't edit it.
- Channel conventions: core only refreshes host inputs, $tick, top-level
  defchannels, runtime bind-channel! producers, :expose, and export. Touhou
  keeps the single-player DMK/BDSL defaults. Multiplayer is an opt-in
  lib/template convention that binds its own per-pilot channels rather than
  asking the engine for computed channel-name families.
- `match` special SHIPPED: destructuring over forms AND values with `_`,
  binders, literals, quote-form patterns, `(as n p)`, vector rest/mid-rest
  patterns, and map key-presence discrimination. It now covers the phase
  clause/finally split directly; the older inspection vocabulary remains
  for generic macro walking.
- Macro-time power that carries the stdlib: `& rest` params, form-aware
  seq vocabulary (count/first/rest/nth/drop/take/concat), total `get`
  over map forms, form-type/form-name, map/filter specials.
- Seq values now use the tail-sharing rep: shared immutable backing plus
  O(1) rest/drop/take views, so match-recursive stdlib walkers are viable.
- Candidates to move next, in expressibility order: `for`/`dotimes`
  (blocked: `:every`/inf/array-iteration are scheduler semantics, not a
  desugar — would need a lib-visible wait-loop primitive that performs
  as well); family→hitbox-radius data (currently `:hitbox` by hand at
  star/gem/lstar/gglcircle call sites); richer spellcard
  templates (:name/:type/hp bars) over `states`.
- Core `finally` now runs on fork task death through inherited guards; keep
  docs/examples using that instead of states-owned finalizers.
- Intrinsics-pass leanings (2026-07 discussion, for the post-doc-port pass):
  * Math/matrix intrinsics are OUR spec; external linalg libraries are at
    most implementations behind it, never the definition — replay/scrub
    demands bit-identical results native↔wasm (libm-style transcendentals,
    no SIMD/FMA-variant results), and a dependency upgrade must never be a
    silent language change. The interesting parts (cyclic broadcast,
    signal lifting over t/u) exist in no library anyway; libs only ever
    cover inner kernels, relevant again when the JIT's typed strided
    descriptors arrive.
  * THE INTRINSIC CRITERION (settled in discussion): an operation is
    intrinsic iff it is hard to implement well / asymptotically or
    constant-factor better than the naive version AND generically
    powerful. Everything else is lib match-recursion over seq views.
  * Generative-art vocabulary: bezier/curves are pure dyns over u (laser
    :shape already samples dyn_pose_u) — lib code first, intrinsify if
    hot; the easing family is the same species and already builtin.
    Smooth noise (perlin/simplex) is a PURE fn of coords+seed — hot-layer
    intrinsic, integer-hash based for bit determinism, replay-clean
    (unlike the sequential-splitmix RNG).
  * Bullet-field image processing — the MOTIVATING use case for the
    matrix family (matrices enter as images of the world, not physics):
    rasterize the bullet field to a density grid, transform in frequency
    space, resample back to bullets. Intrinsics: fft/ifft (1D seqs, 2D
    matrices; own/vendored impl, fixed summation order — native↔wasm
    replay identity), rasterize (query → density grid; engine access),
    resample (density → positions; deterministic low-discrepancy
    sequence, not the RNG — order-independent). Lib: the artistic verbs
    (lpf = elementwise multiply in freq space via broadcasting,
    band-pass ring extraction, blur/convolution, morphology,
    edge-detect-then-spawn). Pipeline shape is manip-like (query → grid
    → transform → write back), control-layer, event-rate. Engines don't
    do this because their bullet sets aren't VALUES; our queryable
    immutable-snapshot world is what makes the field a legal operand.
    Audio-reactive FFT stays host-side as channels on the input tape.
  * Seq/dict verb set: steal k's non-string verbs + adverbs. Intrinsic
    (the hash/sort family per the criterion): grade-up/grade-down (sort
    as a PERMUTATION VALUE — composes with cyclic indexing), group
    (indices-by-value → dict), distinct, find. Lib over match + views:
    where, reverse, odometer (a formation generator), reshape,
    replicate/weed (filter), fill, cut, window (sliding — generative),
    encode/decode (mixed radix, pairs with odometer), amend @[x;y;f]
    (functional array/dict update — THE missing verb for immutable
    pipelines; intrinsify if hot), and the adverbs (over/scan/each/
    each-prior/each-left/each-right). Dicts need one entries/keys/vals
    intrinsic to be seq-able; dict verbs are then lib. Semantics to pin:
    pervasive broadcast (k pervades nested structure; adopt pervasion
    with OUR cyclic conformance at each depth). Distinct names per
    overload in function space (where/group/grade-up/fill/amend);
    glyph overloading returns only inside m"" where arity is
    syntactically visible — postfix adverb spelling expanding to the
    same lib fns at read time (zero IR growth), deferred until named
    forms feel heavy in real cards.
- A lib change is an engine rebuild (deliberate — not user-patchable);
  version the lib with the wire protocol when hosts start pinning.
- Styles, under "the engine has no bullet-hell domain understanding":
  a style is an interned OPAQUE record — identity for batching, data
  for queries, vehicle for render-signal tags. The engine keeps
  interning, query-by-record, the :hue/:scale/:facing/:opacity tags,
  and the flat draw-list contract (kind + pose + style + tag values);
  it should NOT privilege family/color/variant (currently hardcoded
  fields on the Rust Style struct — generalize to a small interned
  map). The family/color/variant vocabulary belongs to touhou.maku; the
  family→sprite and color→palette tables (now core::host) become host
  config shipped alongside the lib. DMK-style pools = interning as an
  optimization, never semantics.

## Engineering debt
- ~~`core/src/interp.rs` / `sim.rs` monoliths~~ both module-split
  (interp/{motion,spawn,world,builtins,card}, sim/{channels,collision,
  render,exec,tests}). Remaining: `interp/mod.rs` still holds eval + the
  specials table (~2.2k lines) — split before the vocabulary grows.
- ~~Host API extraction~~ done: `core::host::Instance` (card management,
  wire dispatch, render/event/channel/timeline reads); the macroquad player
  is now input+draw+net only. Write `docs/host-api.md` from it as the first
  non-macroquad frontend exercises it.
- Signal tapping/plotting (design.md §11) — select a subexpression, plot
  over t.
- Fixed 120 Hz tick assumption in several places (`TICK_RATE`).
- AOT/wasm: hot-layer compilation unstarted; core-vs-lib builtin
  stratification undecided. Specials are the IR, builtins are intrinsics;
  `match` replaced no builtins but makes `map`/`filter` demotable to lib
  code later. The tail-sharing seq rep now exists; demotion is deliberately
  deferred because map/filter are used at runtime over entity arrays, so move
  them when interpreter cost is measured or the JIT lands.

## Doc roadmap
- The plan of record (2026-07): with defcontact shipped as the collision
  foundation, port the REST of the DMK docs first, placing each piece
  core-vs-lib by the settled principles (checks/data vs contacts/code,
  genre in cards/lib). After the full port, one dedicated pass: define
  the intrinsic set (lang, math, array/matrix, engine), move everything
  non-intrinsic out of Rust into lib, then start on compilation
  (specials are the IR, intrinsics are the builtins).
- Tutorial ports (DMK Basic Tutorials t01–t09, tbosses, tstages → our
  tutorials, each with a runnable cards/tutorials/*.maku companion swept by
  tutorial_cards_run): 01–06 done (06 = bosses/phases/script structure,
  mapping DMK t07: bare `states`, the `phases` sugar table, spawn-boss,
  phase-edge policy as finally code; DMK's own t06 is a philosophy
  essay — concept mapping in docs/from-dmk.md instead of a port).
  07 done (= DMK t08: firing index → ordinary binders, formations as
  functions, empty-guided fires → frame nesting with the pivot shim,
  let-bound shared guides). DMK t09 done as a from-dmk.md appendix
  (one row per repeater modifier; no tutorial — 25 modifiers, six
  ideas, all already taught); writing it doubled as the §13.1
  ancestor-clock audit, which closed the decision and yielded the
  contains_t live-read fix. tbosses done as a host-boundary appendix.
  tstages done as Tutorial 8 plus a campaign host-boundary mapping.
  Tutorials are standalone; DMK mappings live in docs/from-dmk.md.
- `docs/host-api.md` — write alongside the first non-macroquad frontend.
- Tutorials — after the first frontend, against a stable surface.
