<!-- Moved verbatim from docs/notes/data-model.md (dissolve-design-notes).
The already-implemented semantics are normative in openspec/specs/language;
the storage/SoA targets below are this change's design input. -->

# Data model targets

Settled architecture targets for the core data model. Moved verbatim
from the old `docs/notes/TODO.md` "Data Model Targets" section
(2026-07); these are decisions/constraints, not open work items. The
spec in `openspec/specs/language/spec.md` is authoritative where they overlap.

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
- Cold spec/runtime row split:
  ```text
  EntitySpec {
    figure_motion_plan : KernelPlanId
    collider_plans     : [KernelPlanId]
    render/schema refs : ...
    state_schema       : ...
    cache_policy       : ...
  }

  EntityRow {
    spec_id
    capture_range
    typed field/state slots
    per-slot epochs
    generation
  }
  ```
  `KernelProgram`/`KernelPlan` execution belongs to `ir-unification`; this
  representation owns stable spec identity and dense row bindings. Batch
  grouping, state lookup, and projector memoization must key explicit
  spec/program/plan ids rather than `Rc<DynNode>` pointer identity.

---

# Round design (2026-08 pick-up)

Bound to the current tree by a seam survey (2026-08-10). Starting facts
that shaped the slices: the *program* half of the flip already landed in
round 22 — batch keys are explicit `MotionProgramIdentity`, all three
intern tables are structural — so this round is entirely about *node and
storage* identity. The load-bearing seams: `MotionNodeId` is minted by
`Rc::as_ptr` interning (`motion.rs collect_node_state`), a
content-identical `MotionStateSchema` is allocated per entity
(`world.rs install_entity`), `instantiate_rand` re-`Rc::new`s the whole
figure spine per element even on the compiled path (programs shared,
nodes not), `dyn_cols` gets an unconditional fresh Rc per element
(`spawn.rs build_entity_specs`) which silently makes the `slots.rs` plan
memo per-entity and unbounded, `ClosedPoseScratch::class` holds raw
`*const DynNode` across ticks (latent ABA), and `exec.rs
resolve_node_pose` scans every live row's schema to find a node's
carrier.

## D1 — Spec table: identity, minting, lifetime

`World.specs: SpecStore` — slotted table of `EntitySpec { dyn_figure,
motion_schema: Rc<MotionStateSchema>, dyn_cols, collider_projector,
cache_policy, overrides, capture_layout }`. Rows hold
`spec_id: SpecId(index: u32, gen: u32)`.

- **Minting** happens where `build_entity_specs` runs today (spawn) and
  at remat rebuild. Front-end memo: a Weak-guarded map keyed on the
  figure-template Rc pointer (+ the other component identities), same
  pattern as the collision projection-plan cache — repeated spawns from
  a stable template reuse their spec without structural work.
- **Lifetime**: rows hold a refcount on their spec entry. `cull`
  releases the row's spec reference immediately (today dead rows pin
  trees until slot reuse). At refcount zero the entry frees, the slot
  goes on a free list, and its generation bumps — so a spawn-loop card
  that constructs a fresh figure every call (fresh Rc, memo miss) mints
  and frees specs and the table stays bounded by the live population.
  Cross-tick caches key the full `SpecId` including generation, which
  retires the ABA class outright.
- **Structural cross-site fusing is a non-goal this round**: batch-lane
  fusing across sites already comes from structural program interning
  (round 22); spec-level structural interning would only dedup memory
  between simultaneously-live identical sites and can layer on later
  behind the same SpecId surface. This is the recorded fold of
  `spec-store-dedup` — the table is the dedup mechanism, no parallel
  sharing scheme.

## D2 — Per-spec schemas; node identity stays pointer-keyed *within* a spec

`MotionStateSchema` (with its `node_ids` map) is built once per spec
over the spec's shared tree and shared by every row. Pointer-keyed
`node_ids` is fine at spec scope: the spec owns its immutable tree, so
pointers are stable for the spec's lifetime; what was broken was
per-entity schemas and *cross*-entity pointer lookup. Rows reach their
schema via `spec_id` (O(1)); state cells remain the dense
`[slot][row]` columns; the eval-time `state_key_for_node` contract is
unchanged. `MotionNodeId` therefore does NOT need a fragile
walk-ordinal assignment at eval time (which would break on
conditionally-visited subtrees like stage segments).

`resolve_node_pose` (exec.rs) must lose its O(live-rows) scan, and
under shared trees "which row carries this node pointer" becomes
ambiguous — the call site must already know its row (or resolve via
spec_id + row context). Slice 1 investigates the actual call-site
semantics and re-plumbs; this is the one place the design is bound
during implementation rather than up front.

## D3 — Captures move to the row; the spine stops cloning

The spec's tree keeps `RandCell::Compiled` (spec) nodes everywhere; the
per-entity data becomes one capture vector per row (`Rc<[f64]>` or
arena range), laid out by the spec's `capture_layout`: per rand-bearing
node, an offset + length, in exactly the per-leaf walk order the RNG
round established (`instantiate_rand`'s `node_n` ordering) — draws use
the same `rng_mix(elem_key, mix(NODE, ordinal))` keys and site
numbering, so every drawn value is bit-identical to today's. Eval
threads the row's captures + layout through the motion eval context;
`caps_of(&node.rand)` becomes a lookup of the node's slice in the row
vector. `RandCell::Caps` stays as the fallback for non-row-owned nodes
(ad-hoc direct eval) and the `Bail` path keeps per-element substituted
trees — those elements mint per-element specs, correct and bounded via
refcounting, just not shared.

Two per-element specializations that currently force clones become row
data: the ambient spawn frame (`framed(ctx.ambient)` wrapper) becomes a
per-row pose applied at the eval root instead of a per-spawn-call
wrapper node, and the `with_axis` group rebinding becomes a per-row
axis input (it already scatters as a lane input in the batch path) —
grouped spawns (rings, the common case) MUST share one spec.

## D4 — Explicit-id re-keying

- `ClosedPoseScratch::class`/`candidates`: raw `*const DynNode` →
  `SpecId` (generation included).
- `slots.rs` dyn-field plan memo and the collision projector front-end
  memo: keyed per spec (their payloads now genuinely shared per spec,
  so the maps stop growing with entity count).
- `vel_chain` state-slot resolution: per-spec, not per-row-pointer.
- `examples/dbg.rs` pointer histograms become spec-id histograms.
- Host-visible `Rc::ptr_eq` contracts (RenderSchema, EventLog) are NOT
  entity layout and stay untouched.

## D5 — Snapshot/clone

`EntityStore::clone` drops six Rc-bump columns for one `Vec<SpecId>` +
one captures column; `SpecStore` clones as slot-vector Rc bumps. All
pointer-keyed scratch is already dropped on `Sim::clone` and rebuilt,
so restore semantics are unchanged by construction.

## D6 — Remat

The remat drain rebuilds a figure mid-life; it mints-or-memoizes a spec
through the same path as spawn, swaps the row's `spec_id` (releasing
the old ref), resets state cells per the new schema, and redraws/copies
captures per the remat semantics already in place. Masked batch remats
go through the same spec mint once per plan, not per row.

## Slices

1. **Spec table**: `SpecStore` + `spec_id` column; `EntitySpecStore`'s
   six columns collapse; per-spec `MotionStateSchema`; install/cull/
   reuse/remat/clone plumbing; refcount + generational ids;
   `resolve_node_pose` re-plumb. Rand-bearing elements mint per-element
   specs in this slice (trees still cloned) — correct, bounded,
   rand-free groups already share.
2. **Captures to rows**: capture layout on the spec, row capture
   vector, shared trees for rand-bearing groups, ambient-frame row
   data, axis input; `instantiate_rand` demoted to drawing caps. RNG
   bit-parity gate (same-seed cross-commit A/B on rand-heavy cards).
3. **Re-keying + retirement**: SpecId keys in closed-pose class /
   slots memo / collision front-end / vel-chain slots; representation
   tests rewritten (the round-22 tests that walk node shapes); perf
   walls.

Each slice lands oracle-green (`MAKU_LOWER_ORACLE=1` core suite + the
ignored release card suites) before the next starts; the round's wall
verdict is interleaved A/B per `openspec/specs/perf/spec.md`.

## Measured (round close, 2026-08-10)

All gates green on every slice: 348 core tests plain and under
`MAKU_LOWER_ORACLE=1`, all 6 ignored release oracle card suites, and an
11-card corpus A/B (`abdump`, 600 ticks, rand-heavy included) that is
bit-identical against pre-round `e568850` — the round changed no
observable behavior, RNG draws included. Walls (5 interleaved
observations, medians): representative suite +1.19%, scaled fruit
12000t −2.28% — both inside ±5%; small cards pay the spec-store
bookkeeping, dense cards win. Landed as three commits: b4e59a8
(spec table), 0eb886f (captures/root frames/axis to rows), 5ffdcbd
(generational-id re-keying).

Deviations and findings vs the plan:

- Masked-remat spec sharing moved from slice 1 to slice 2 as predicted
  mid-flight: each rebuilt tree embedded its row's exit-pose anchor, so
  one-spec-per-plan was impossible until root anchors became row data
  (`RowFrame`). Slice 1 kept per-row remat specs (bit-identical),
  slice 2 resolved the deferral.
- Culled rows keep a tombstone spec ref until slot reuse — generation-
  valid handles may still read `:pos`/`:kind` from a dead row, so the
  release-at-cull design point became detach-from-carrier-index at cull,
  release at reuse. Carrier lookup tracks live rows separately.
- Spec identity needed more than pointer equality to fuse moving-spawner
  calls: structural fallbacks for dyn-col templates (form trees plus the
  visible env bindings — sound because same form + same visible bindings
  implies same eval), RowFrame templates by inner pointer, and Const
  templates by pose bits.
- Slice 2 dropped capture values from the vel batch key: lanes carry
  their own caps as inputs, so rand-bearing rows fuse into one batch
  group across draw values — the lane-widening half of the old
  `group-integrator-dedup` observation fell out for free (integrator
  state itself remains per-row; the fold-dedup half stays optional).
- Classification (closed-pose row class) became once-per-spec rather
  than once-per-row, and the vel-chain n2 slot got a per-spec memo with
  cached negatives.
- Residual accepted risk, documented after empirical probing: the
  no-live-carrier fallback in `resolve_node_pose` evaluates a shared
  rand-bearing spec tree with empty caps (the pre-flip per-entity clone
  kept its values). Task-frame nodes are task-constructed, the corpus
  and suites never reach the shape, and the failure is loud (panic in
  `caps[slot]`), not silent.
