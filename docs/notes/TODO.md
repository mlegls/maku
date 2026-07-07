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
- Trigger predicates are still single-column `<=` crossings only.
- Blocking lasers / world-geometry extent from DMK §13.7 remains unported.
- RNG is sequential splitmix, so replay determinism holds but spawn-order
  independence does not.

## Engine Refactor

- Move remaining row-local `Entity` fields into final storage shapes:
  - `dyn_figure` -> shared spawn-site/program/archetype data where possible,
    leaving row storage mostly indices plus dense state.
- Remove pointer-keyed compatibility fallback from legacy scratch motion
  evaluation. Live entity stepping now requires stable lowered node ids, while
  old direct evaluation and schema-remap bridges still accept pointer keys.
- Lower lazy `stages` to a closed set of dyns at load time. Until then, it is
  isolated as an interpreted compatibility path: only lazy-stage dyn writes may
  extend dense schemas at runtime when a lazy segment is first constructed.
- Move remaining gameplay-domain behavior out of core:
  - bare hostile `(cull)`;
  - Touhou concepts such as bullet/shot/enemy/player/boss/laser;
  - style family/color/variant defaults and family->sprite assumptions.
- Move the Rust-side `laser` bridge into library code. Core should only expose
  figure construction plus collider/render projectors.
- Represent fill/warning/hot phases as dyn collider/render slots returning
  different data over time, not laser-specific lifecycle shortcuts.
- Move render tags and render-signal compatibility (`:hue`, `:scale`,
  `:facing`, `:opacity`) into ordinary renderer spec records or finite fields.
  Collider scale/radius should be explicit collider data, not borrowed from a
  render-specific `:scale`.
- Generalize `Style` from hardcoded family/color/variant Rust fields to a
  small interned opaque map/record. Touhou/host config should own the visual
  vocabulary.
- Materialize collider/render rows into per-tick scratch SoA buffers instead
  of per-entity vectors/enums in hot loops.
- Compile dyn evaluation to a flat program with fixed scratch storage. The
  interpreter path may remain as a compatibility implementation, but hot
  steady-state execution should not allocate or hash by node pointer.
- Decide and implement core-vs-lib builtin stratification before the compiler
  pass. Specials are the IR; builtins are intrinsics.

## Data Model Targets

- Core semantic shape:
  ```text
  Figure = Pose | Polyline | ParametricCurve | Composite...
  Dyn<F> = t -> F
  ColliderProjector = Figure, t -> [Collider]
  RenderProjector = Figure, t -> [Render]
  Entity = Dyn<Figure> * ColliderProjector * RenderProjector * Meta
  ```
- Pose is `(x, y, theta?)`; `theta = none` means facing is unspecified, while
  `theta = some 0` is an explicit zero angle.
- Sampling is not intrinsic to figures. It belongs to collider/render slots or
  authoring helpers. Parametric curves may later use analytic collision or
  mesh rendering without changing source semantics.
- Raw collider/render rows are boundary data, not normal authoring objects.
  Source code should usually construct specs/projectors.
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
  references. Query order should remain unspecified unless explicitly sorted.
- Source-level entity fields should be finite, flat, interned fields. Storage
  may distinguish builtin pose/state from user fields, but source should not
  expose separate arbitrary `cols` and `meta` concepts.
- Runtime metadata target:
  ```text
  nums    : NumFieldId    x entity_row -> f64
  syms    : SymFieldId    x entity_row -> Symbol
  handles : HandleFieldId x entity_row -> EntityRef
  present : bitsets or typed sentinel policy
  ```
  Unknown fields are load/reschema errors, not per-tick allocation.
- Retained entity storage should be cold data plus dense row state. Hot data
  should be per-tick derived SoA buffers for poses, colliders, render rows,
  and sampled curve points.

## Standard Library

- Keep Touhou/DMK/BDSL conventions in `cards/lib/touhou.maku` and related
  libraries. Core should remain a 2D graphing + collision engine.
- Richer spellcard templates (:name/:type/hp bars) should be lib macros over
  `states`, `phases`, `spawn-boss`, `finally`, and ordinary fields.
- Candidate stdlib moves:
  - `for` / `dotimes`, after deciding the lib-visible wait-loop primitive
    needed for scheduler performance;
  - family->hitbox-radius data currently repeated at call sites;
  - more card-facing Touhou short names (`bullet`, `shot`, `enemy`,
    `player`, `boss`) with compatibility aliases where useful.
- `defcontact` is the collision foundation: checks are data, contacts are
  code. Keep Touhou hit/graze/shot rules in lib over opaque layers and fields.

## Intrinsics / Arrays

- Intrinsic criterion: make an operation intrinsic only when it is hard to
  implement well in lib and is generically powerful. Everything else should
  start as lib code over `match` and seq views.
- Initial array/control candidates: `map`/each, `filter`, `fold`, `scan`,
  `each-prior`, `window`, `sort-by`, `best-by`, `count`, `nth`, `take`,
  `drop`, and `concat`.
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
