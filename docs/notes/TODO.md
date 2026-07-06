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
- ~~Pathers render as points; laser `:width` ignored by collision~~ done
  with tutorial 04: pathers record trails (rendered + capsule-chain
  hitbox, bounded by the window); `:width` scales laser collision.
  Remaining §13.7: blocking lasers (world geometry → extent).
- Trigger predicates: single-column `≤` crossings only (§13.13).
- Shipped: `defcontact` moved contact resolution out of the engine. Layers
  are opaque tags, teams are query metadata only, Touhou hit/graze/shot
  rules live in `cards/lib/touhou.dmk`, and `$graze`/`$hits` are stdlib
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
  gates; invuln; $enemies/$nearest-enemy as defchannels) and
  `player-rig`. Authored as files, inlined via include_str — every host
  resolves `(import "touhou")` identically; users import the lib, they
  don't edit it.
- Channel conventions still engine (sim/channels.rs): the per-pilot
  families ($player-k/$lives-k/$nearest-enemy-k), the host-contract
  default/mock list (move-x…boss-hp), $lives counters, and $boss =
  world.boss (the move anchor as engine state; wants generic named
  anchors, which would also de-genre `move`). Per-pilot DECISION
  (2026-07): no computed channel names — they'd make the channel
  namespace dynamic (bad for host bindings and eventual static
  analysis). Instead the engine block just DELETES: touhou stays
  single-player (scalar $player/$lives/$nearest-enemy as derived
  channels over the one piloted entity — no per-tick map build for the
  common case), and multiplayer becomes an opt-in lib/template that
  defines a map-valued $players channel ad hoc (needs strict non-cyclic
  `get` at read sites; pilot ids are sparse, so a map keyed by pilot
  number, not an array).
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
  map). The family/color/variant vocabulary belongs to touhou.dmk; the
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
  tutorials, each with a runnable cards/tutorials/*.dmk companion swept by
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
