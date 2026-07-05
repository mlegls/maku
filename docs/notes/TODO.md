# Implementation notes / prototype-vs-spec gaps

The spec (docs/language.md) is authoritative; this tracks what the prototype
(`proto/`) has not yet realized, plus engineering debt. §-references are to
language.md.

## Language features spec'd but unimplemented
- `phases` machine + scoped `goto` + opts-as-card-data (§8) — `until` covers
  phase-end cancellation; the machine itself is unexercised by running code.
- `race` general form (§8) — only the `until` degenerate case exists.
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
- Extraction (§10), ancestor clocks (§13.1), 3D embedding (§12).

## Known approximations (documented in code)
- Pathers render as points; laser collision uses a constant beam half-width
  (`:width` should feed it, §13.7).
- Trigger predicates: single-column `≤` crossings only (§13.13).
- Interaction matrix rows engine-fixed; hit effect knows the `lives` and
  `iframe-until` columns by name (§13.10). Iframes and inputs are no longer
  global/fixed (per-entity column; named-channel Inputs) — the matrix rows
  are the last hardcoding.
- RNG is sequential splitmix, not counter-keyed by spawn path (§5) — replay
  determinism holds, order-independence does not.
- Scrub-back across a swap/add boundary restores the pre-change program
  (correct); seeks are exploration — branch commits only on resume.

## Engineering debt
- `core/src/interp.rs` is a 2.9k-line monolith — split into modules
  (reader glue / dyn+motion / eval / actions / spawn / card) before the
  builtin vocabulary grows further.
- ~~Host API extraction~~ done: `core::host::Instance` (card management,
  wire dispatch, render/event/channel/timeline reads); the macroquad player
  is now input+draw+net only. Write `docs/host-api.md` from it as the first
  non-macroquad frontend exercises it.
- Signal tapping/plotting (design.md §11) — select a subexpression, plot
  over t.
- Fixed 120 Hz tick assumption in several places (`TICK_RATE`).
- AOT/wasm: hot-layer compilation unstarted; core-vs-lib builtin
  stratification undecided.

## Doc roadmap
- Tutorial ports (DMK Basic Tutorials t01–t09, tbosses, tstages → our
  tutorials, each with a runnable cards/tutorials/*.dmk companion swept by
  tutorial_cards_run): 01–03 done (03 = Miracle Fruit + Danmaku Chimera);
  04 next (lasers). Tutorials are standalone; DMK mappings live in
  docs/from-dmk.md.
- `docs/host-api.md` — write alongside the first non-macroquad frontend.
- Tutorials — after the first frontend, against a stable surface.
