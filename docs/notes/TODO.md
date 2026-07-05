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
- Extraction (§10), ancestor clocks (§13.1), 3D embedding (§12).

## Known approximations (documented in code)
- ~~Pathers render as points; laser `:width` ignored by collision~~ done
  with tutorial 04: pathers record trails (rendered + capsule-chain
  hitbox, bounded by the window); `:width` scales laser collision.
  Remaining §13.7: blocking lasers (world geometry → extent).
- Trigger predicates: single-column `≤` crossings only (§13.13).
- Interaction matrix rows engine-fixed; hit effect knows the `lives` and
  `iframe-until` columns by name (§13.10). Iframes and inputs are no longer
  global/fixed (per-entity column; named-channel Inputs) — and after the
  stdlib extraction (spawn defaults, death trigger, invuln, rig are all
  library code now) the matrix rows + the three contact-resolution bodies
  are THE remaining genre hardcoding. Design settled (next big chunk),
  the principled line being CHECKS ARE DATA, CONTACTS ARE CODE:
  * `(defcontact [:a-layer :b-layer] opts? (fn [a b] …))` — card-level
    rule table replacing the hardcoded matrix. Layers become opaque tags
    (Rust Layer enum → interned str); TEAMS DROP OUT of collision
    entirely (layer pairs already partition the sets — :team stays as a
    plain queryable meta tag). The touhou rules (:damage/:graze ×
    :player-hurt, :shot × :hurt) move into lib/touhou.dmk next to the
    spawn templates that create those layers.
  * Detection: per-tick per-layer entity index; per rule iterate the
    smaller side outer (player-hurt is 1-2, hurt is few — same
    asymmetry the hardcoded pass exploits).
  * Hot prefilters as rule DATA, not callback code: {:once-latch :col}
    (graze's once-ever), {:skip-if [:b :col :gt $tick]} (iframe skip) —
    engine-evaluated per pair, so overlapping pairs don't re-enter the
    interpreter every tick. Cold resolution = manip-style callback
    (views + instant actions, canonical order, contact-rate only).
  * world.graze / world.player_hits die: counters become columns on the
    player entity written by the callbacks, published via :expose —
    hosts read $graze/$hits channels (host/UI contract change, native +
    web). Laser-persists-through-hit becomes callback policy (view
    exposes :kind; cull points only).
  * Risks: behavioral drift in latch/iframe edge cases (smoke must stay
    tick-identical), callback cost under sustained overlap (prefilters
    are the mitigation), event ordering (keep bullet-index order).
- RNG is sequential splitmix, not counter-keyed by spawn path (§5) — replay
  determinism holds, order-independence does not.
- Scrub-back across a swap/add boundary restores the pre-change program
  (correct); seeks are exploration — branch commits only on resume.

## Standard library (cards/lib/, compile-time embedded)
- Shipped: `prelude` (AUTOIMPORTED, sentinel-deduped: `when`/`unless`),
  `touhou` (spawn templates, variadic metas; spawn-boss = enemy + phase
  machine owning the boss conventions — $boss-hp exposure, registration
  wait, bound `boss`; `phases` as a macro over `states` with {:hp n}
  gates; invuln; $enemies/$nearest-enemy as defchannels) and
  `player-rig`. Authored as files, inlined via include_str — every host
  resolves `(import "touhou")` identically; users import the lib, they
  don't edit it.
- Channel conventions still engine (sim/channels.rs): the per-pilot
  families ($player-k/$lives-k/$nearest-enemy-k — need computed channel
  names to move), the host-contract default/mock list (move-x…boss-hp),
  $graze/$lives counters, and $boss = world.boss (the move anchor as
  engine state; wants generic named anchors, which would also de-genre
  `move`).
- `match` special SHIPPED: destructuring over forms AND values with `_`,
  binders, literals, quote-form patterns, `(as n p)`, vector rest/mid-rest
  patterns, and map key-presence discrimination. It now covers the phase
  clause/finally split directly; the older inspection vocabulary remains
  for generic macro walking.
- Macro-time power that carries the stdlib: `& rest` params, form-aware
  seq vocabulary (count/first/rest/nth/drop/take/concat), total `get`
  over map forms, form-type/form-name, map/filter specials.
- Candidates to move next, in expressibility order: `for`/`dotimes`
  (blocked: `:every`/inf/array-iteration are scheduler semantics, not a
  desugar — would need a lib-visible wait-loop primitive that performs
  as well); the `{:hit n}` damage-map unwrap (DMK player() compat,
  still in sf_spawn); family→hitbox-radius data (currently `:hitbox` by
  hand at star/gem/lstar/gglcircle call sites); richer spellcard
  templates (:name/:type/hp bars) over `states`.
- Core `finally` now runs on fork task death through inherited guards; keep
  docs/examples using that instead of states-owned finalizers.
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
  code later, blocked on a tail-sharing seq rep because naive match
  recursion over `Rc<Vec>` rest-copies is O(n²).

## Doc roadmap
- Tutorial ports (DMK Basic Tutorials t01–t09, tbosses, tstages → our
  tutorials, each with a runnable cards/tutorials/*.dmk companion swept by
  tutorial_cards_run): 01–05 done (05 = channels/host boundary/rig;
  native player binds T/Y/U/I → $rank). 06 next. Tutorials are
  standalone; DMK mappings live in docs/from-dmk.md.
- `docs/host-api.md` — write alongside the first non-macroquad frontend.
- Tutorials — after the first frontend, against a stable surface.
