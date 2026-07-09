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
- Channel manifest/load-time checking: missing host channels such as `$wind`
  should fail at load, not mid-run.
- `remat` / `manipulate`: still missing per-slot epochs, soft-cull fades,
  the F1 lint, and the masked-SoA fast path. Current callbacks all bill fuel.
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
- Remove pointer-keyed compatibility fallback from legacy scratch motion
  evaluation. Live entity stepping now requires stable lowered node ids, while
  old direct evaluation still accepts pointer keys.
- Lower lazy `stages` to a closed set of dyns at load time. Until then, it is
  isolated as an interpreted compatibility path: only lazy-stage dyn writes may
  extend dense schemas at runtime when a lazy segment is first constructed.
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
- Projector constructor target surface: `(collider :pose|:parametric
  [e ctx] body)` as the builtin that constructs a parameterized projector
  value (kind annotation intact, not a callable fn, capture discipline
  owned by the special); `defcollider`/`defrenderer` become `def` plus
  that form. Bare `(fn [e ctx] ...)` is deliberately NOT a projector:
  no kind annotation, and closure capture fights spec-store dedup.
- Continue renderer migration toward the §7 target surface. The Rust-side
  `Style`/`RenderItem` bridge is gone: hosts consume `RenderRow` values
  (structural point/polyline geometry plus open keyed num/sym fields checked
  against an accreted per-world schema), style axes are ordinary sym fields
  flattened from the `:style` spawn map, the stock sprite row is a lib
  deftick rule, and the default dot is a spawn-injected `{:shape :point}`
  spec (trace-backed elements carry it per-element). Remaining work:
  per-kind registered row schemas with manifest negotiation (the current
  schema is one global key->kind map), `defrenderer` bodies returning
  schema-checked row maps directly instead of point/polyline slot specs,
  the builtin field rename/pick adapter, and a mesh/sprite-batch kind.
  Host palette tables (`style_rgb`, `dot_radius`) remain stock host policy
  in `host.rs`; move them behind host/profile config when a second
  frontend needs different vocabulary.
- Compile dyn evaluation to a flat program with fixed scratch storage. The
  interpreter path may remain as a compatibility implementation, but hot
  steady-state execution should not allocate or hash by node pointer.
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
  - `for` / `dotimes`, after deciding the lib-visible wait-loop primitive
    needed for scheduler performance;
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
- Remove fixed 120 Hz assumptions where `TICK_RATE` leaks into APIs or data.
- AOT/wasm compiler work is unstarted.

## Docs

- Tutorials t01-t09, tbosses, and tstages are ported. Future doc work should
  focus on stabilizing the new tutorial site, reader view, and host API docs.
- `docs/from-dmk.md` remains the place for DMK/BDSL mapping notes; tutorials
  should stay standalone and idiomatic for Maku.
