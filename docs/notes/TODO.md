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
  Entity = Dyn<Figure> * Dyn<[Collider]> * Dyn<[Render]> * Meta
  ```
  A pose carries position plus orientation, which is why current point-like
  figures can drive facing and subfire semantics. Interpreter `DynNode`s are
  prototype-level `Dyn<Pose>` expressions, not the semantic `Pose` value; a
  compiled core should store pose data compactly as `(x, y, theta)`. Unit
  vectors can be cached by optimized backends when useful, but the semantic
  pose representation is angular.
  The Rust prototype now represents the first slice as `Dyn<Figure>` through
  the `DynFigure` type alias. Its backing is still a compatibility enum
  (`DynRepr`) rather than the final typed expression IR, but figure-valued
  dyns no longer have a separate ad hoc top-level type.

  The important layer boundary is:
  ```text
  Figure:    abstract/math geometry evolving over time
  Collider:  per-tick collision data derived from the figure
  Render:    per-tick renderable records/meshes derived from the figure
  Meta:      opaque library/host data
  ```
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
  Semantically the collider and render sets are dynamic:
  ```text
  Entity = Dyn<Figure> * Dyn<[Collider]> * Dyn<[Render]> * Meta
  ```
  General dyn typing rule:
  ```text
  Dyn<T> accepts T, an expression with t in scope that returns T, or a
  function f(t) -> T. m"..." is only the numeric shorthand for this.
  ```
  Any typed dyn field follows this rule, so projector-specific dyn
  constructors should not be necessary. A collider slot can be `(if (< t 2)
  [:none] [:capsule-chain ...])`; a radius can be `m"0.1 + 0.02*t"`.
  For implementation and compilation, the common/static-arity case can lower
  to stable slots:
  ```text
  Entity = Dyn<Figure> * Rc<[DynCollider]> * Rc<[DynRender]> * Meta
  ```
  A slot that disappears evaluates to `Collider::None` or `Render::None`.
  This removes separate activity/mask concepts from the low-level model:
  warning, fill, changing radius, and disappearing colliders are just the
  collider/render dyn returning different per-tick data.

  Low-level collider/render specs are spawn arguments, not generally
  first-class functions. This keeps the primitive vocabulary small and lets
  each slot know the spawned `Dyn<Figure>` type at each tick:
  ```text
  (spawn dyn
    [[:collider :hostile-shot :capsule-chain
      {:domain hot-domain :radius r}]]
    [[:renderer :polyline
      {:domain render-domain :stroke w}]]
    meta)
  ```
  A slot's domain is applied to the current figure. For a parametric curve it
  supplies `u` values/ranges; for a polyline it can be omitted (`:full`) or
  used as an index/subrange selector. Higher-level constructors like Touhou
  `laser` produce these spawn arguments; core does not need one builtin per
  collider/render mode.

  The second spawn argument is an array of collider specs. Empty array means
  no colliders. Collision layer is universal collider metadata owned by core
  as an opaque routing key:
  ```text
  Collider slot {
    layer: Rc<str>,
    shape: Circle | CapsuleChain | ...
  }
  ```
  The third spawn argument is an array of render slot specs:
  ```text
  Render slot {
    shape: Point | Polyline | Mesh...
  }
  ```
  The current `colliders: Rc<[ColliderProjection]>` plus curve-specific
  compatibility slots should collapse into `colliders: Rc<[DynCollider]>`
  and `renderers: Rc<[DynRender]>`.
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
      entities now carry dyn collider/render slots plus `EntityCachePolicy`.
  2f. ~~Make layer universal collider metadata.~~ Done for existing point
      colliders: `ColliderProjection { layer, shape: Circle { radius } }`.
      Curve capsule-chain collision is still represented by the temporary
      curve slot and should be folded into dyn collider slots.
  2g. Target update: collider/render slots should become stable dyn
      slots (`Rc<[DynCollider]>`, `Rc<[DynRender]>`) that evaluate to
      `None` when inactive, rather than static slots plus activity masks.
  2h. ~~Introduce internal dyn collider/render slots.~~ Done for the current
      bridge: circle colliders are `DynCollider::CircleProjection`, curve
      collision shape data is `DynCollider::CapsuleChain`, and curve
      rendering is `DynRender::Polyline`.
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
      as a bridge, and curve capsule collision still borrows layer/radius
      from the spawning template's circle projection. The next step is to
      expose low-level slot construction so Touhou/library code can build
      those slots directly and the bridge can disappear.
  3. Represent fill as dyn collider/render slots returning different data
     over time rather than a laser-only lifecycle shortcut.
  4. Recast trails/pathers as derived curves over entity dyn history, with
     any retained samples treated as cache/policy rather than geometry.
  5. Move render tags into ordinary signal-valued meta/fields; collider
     scale/radius should be explicit collider data, not borrowed from a
     render-specific `:scale`.
  6. Remove remaining gameplay-domain metadata from core (`team`, `damage`,
     bare hostile `(cull)`) in favor of generic fields/tags and Touhou
     helpers.
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
  as well); the `{:hit n}` damage-map unwrap (DMK player() compat,
  still in sf_spawn); family→hitbox-radius data (currently `:hitbox` by
  hand at star/gem/lstar/gglcircle call sites); richer spellcard
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
