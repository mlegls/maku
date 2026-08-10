# Evolve/remat follow-ups — design

## Context

The `remat`/`change-col` contract is landed (see proposal). Five slices remain;
the `vel` target shape was settled 2026-08-10 (proposal): one stock integrator
evolve over dyn vel meta columns. `stages` re-expression is explicitly NOT this
change (own round, over `states`).

Current mechanics this design builds on (as read from code, 2026-08-10):

- `sf_vel` (`interp/mod.rs`) builds `DynNode::Vel { a, b, polar, env, programs,
  rand }`; components are compiled by `motion::compile_sig` into integrand
  programs at construction.
- Tick phases (`sim/mod.rs step_with`): drain writes → channels → control →
  scan-step (per-row motion walk + vel batch lanes doing `pos += v·dt` into
  `state_n2`) → `refresh_dyn_cols` (dyn field columns) → collision/render/rules.
  Vel integrands evaluate DURING the step against the current tick's env.
- `b.vel` introspection and remat exit-state `:vel` already read finite
  differences from `sampled_pose` (`velocity_from_samples`) — they are NOT
  node-keyed. The actually node-keyed consumers are: `clamp_integrator`'s
  wrapper walk, `vel_step_plan`/batch grouping (Vel address keys the n2 state
  slot and the lane group), the pos_only fast path, spawn capture
  (`subst_rand`'s walk into `a`/`b`), and stage-exit vel keys.
- Dyn meta columns already exist and refresh per tick
  (`slots::refresh_dyn_field_columns`), currently after motion.

## Goals / Non-Goals

**Goals:**

- Dissolve `DynNode::Vel` into (a) dyn vel component columns + (b) one stock
  integrator evolve shape recognized for batch/kernel execution.
- Per-dyn-field epochs: a dyn installed into a field mid-life runs on its own
  restarted clock.
- Soft-cull fades as lib code over those epochs.
- The F1 lint (no silent strengthening) over the new component programs.
- Extend masked-update lowering to batch `map`-remat shapes.

**Non-Goals:**

- `stages` re-expression (own round; stage-exit machinery gets a compat shim,
  not a redesign).
- Changing the motion-slot signal model: pos stays a per-slot stateful dyn with
  epochs/τ/closed-live/segment history. The fully-push variant (tick rule
  accumulating pos) is rejected — it exits the signal model.
- f32 widths, GPU execution, RNG seeding changes (separate changes).

## Decisions

### D1. `vel` = stock integrator evolve over dyn vel columns

Surface unchanged: `(vel c[vx vy])`, `(vel p[r θ])`, trailing-child/field-map
sugar all stay. `vel` becomes a prelude macro; its expansion carries the
components as dyn entity fields (the elem-fields mechanism `wrap_elem_fields`
already carries trailing field maps from dyn constructors to spawn) and sets the
motion slot to the ONE stock integrator evolve: `p(τ+dt) = p(τ) + v·dt` reading
those component columns. Polar: the polar→cart conversion moves into the
component field programs (`vel-x = r·cos θ`, …) so the integrator shape stays
unique — no polar flag on the recognized shape.

Alternatives rejected: keeping a `Vel` node with re-keyed identity (keeps a
redundant variant every kernel tier must special-case); the push variant (see
Non-Goals).

### D2. One evaluation per tick: the integrator owns it and writes the columns

Today integrands evaluate during scan-step against the post-control env. That
clock is preserved: component programs evaluate as part of the integrator step
(fused, in registers, exactly as the vel batch does now), and the step WRITES
the evaluated values to the `:vel-x`/`:vel-y` columns as its scratch outputs.
The post-motion `refresh_dyn_cols` pass SKIPS integrator-owned columns. So:
one evaluation per component per tick, at the same point in the tick as today;
views/rules read the materialized columns; `b.vel` keeps
`velocity_from_samples`. Fusion is semantics (single evaluation, defined
order), not merely optimization.

### D3. Classification transfers to component programs

Closed/live classification of the motion slot derives from the component field
programs: all-closed components → Scanned/replayable exactly as today;
channel-reading components → live. `contains_unbound_axis` capture guards apply
to components as raw forms, unchanged (known pre-expansion limitation stands,
recorded in the proposal).

### D4. Identity re-keying

- Per-row integrator state keys off the stock evolve node's slot (ordinary
  per-slot n2 state), replacing Vel-address keying.
- Batch lanes group by (component program ids, captures) — explicit program
  identity, aligned with `entity-representation-flip`'s direction; the
  ConstFrame wrapper walk (ring offsets outside the integrator) is unchanged.
- `clamp_integrator` recognizes the stock integrator evolve instead of
  `DynNode::Vel`; clamp still composes inside the step (integrator-state
  semantics, no wind-up).
- Spawn capture (`subst_rand`/`draw_caps`) walks component programs in the
  same documented order as today's `a`,`b` walk — the draw-order contract
  stays stable pending `rng-spawn-order-independence`.
- Stage-exit `:vel` keys: keep a shim keyed off the stock integrator until the
  `stages` round; exit velocity itself is already finite-differenced.

### D5. Per-dyn-field epochs

Every dyn-valued field slot carries its own epoch column. Installing a dyn into
a field (spawn init, remat field-map key, `set-col` with a dyn value) writes
`epoch_field := now`; the field's dyn runs on `τ_field = t − epoch_field`.
Spawn-time installs get `epoch_field = birth`, so existing cards see no change
(dyn meta fields today run on the spawn clock). A motion remat still touches
only the motion slot — a half-finished `:opacity` fade keeps its own epoch.
This is what "fades surviving motion remats" needs, and it makes a mid-life
fade start at the event instead of the spawn tick.

### D6. Soft-cull fades are lib code

`(soft-cull b dur)` = remat `{:opacity (fade → 0 over dur)}` plus a deadline
field and the stock cull rule (same pattern as `invuln`/`:iframe-until`). No
engine verb; depends only on D5. Lives in `crates/core/lib` per the stdlib
stance.

### D7. F1 lint

At card load, after component programs compile: if a component is
closed-form-integrable (constant, or piecewise-affine `lerp` profile), emit the
lint with the suggested closed rewrite (`linear`/closed form). Never rewrite
silently — a scan stays a scan (`openspec/specs/language/spec.md` §F1). The
recognizer inspects the compiled component program shape, so it survives the
re-expression unchanged in spirit.

### D8. Masked-SoA fast path for batch `map`-remat

Masked-update plans for pure slot-local callbacks are landed (lowering spec,
2026-07). Extend recognition to `(map (fn [b] (remat b …)) domain)` shapes
whose remat spec has statically-known slots: lower to masked epoch writes +
slot updates. Depends on D5 only in that epoch columns are just more masked
columns.

## Risks / Trade-offs

- [Risk] Double evaluation or order drift of components between integrator and
  view reads → D2: integrator owns the single evaluation; refresh pass skips
  integrator-owned columns; oracle suite gates the whole re-expression.
- [Risk] `DynNode` ≤ 96-byte guard when adding the stock integrator evolve
  shape → per-variant data behind `Option<Rc<..>>` as usual; the variant may
  reuse the existing evolve node with a recognized step shape rather than a new
  variant at all.
- [Risk] Batch-lane perf regression from column materialization → execution
  stays fused in registers (today's vel-batch structure); column writes happen
  from registers at step end. Wall-only interleaved A/B per perf methodology;
  the cradle card is the canary.
- [Risk] Snapshot/replay format: motion state slot layout changes and new
  epoch columns → version the snapshot; acceptable in 0.x, note in release
  record.
- [Risk] `group-integrator-dedup` tension (per-row columns vs shared folds) →
  fold state dedup keys on (programs, captures, birth) regardless; materialized
  component columns are per-row but cheap relative to fold state. Revisit at
  that change with measurements.

## Migration Plan

Implementation order (each step lands green on the core suite + the 4 ignored
oracle card suites, committed immediately):

1. D5 per-dyn-field epochs (small, independent, unblocks D6/D8).
2. D6 soft-cull fades in lib (exercises D5).
3. D1–D4 vel re-expression: introduce the stock integrator shape + component
   columns behind the existing surface, port batch lanes/clamp/capture/pos_only
   walkers, then DELETE `DynNode::Vel` and its arms.
4. D7 F1 lint over component programs.
5. D8 map-remat masked lowering.

## Open Questions

All three resolved at implementation (2026-08-10, slice 3):

- Column naming: reserved `:vel-x`/`:vel-y` (grep showed no corpus use).
  User fields on those names are a spawn error. CONCURRENT integrators in
  one tree (frame parent+child) are a spawn error — they would cross-wire
  the shared columns and headings; `stages` segments are mutually exclusive
  and share them safely (t08's fairy wave is the corpus witness).
- Representation: a dedicated compact variant (`DynNode::StockIntegrator`
  with data behind one `Rc`), old `Vel` deleted. 96-byte pin holds.
- Surface: `sf_vel` remains the construction site producing the recognized
  shape; the pure prelude-macro face over raw evolve is deferred to the
  kernel-shrink worklist (`core-lib-stratification`) and must not change
  the contract.

## Measured (slice 3 A/B, 2026-08-10)

Old-vs-new render-row dumps bit-identical over bowap, polar-demo, cradle,
spell-2, exploding-stars, and t08 ex2-fairy-wave (300–600 ticks each).
Wall-only interleaved A/B: cradle at parity; bullets-10k scale bench ~2.3%
slower (old ~16.9 → new ~17.3 ms/frame median); exploding-stars ~6% slower
(~32.9 → ~34.6 ms/2000 ticks) — worst case: many tiny short-lived vel
bullets, where the per-tick component-column writes dominate. Phase probes
attribute the delta to scan-step (column writes, semantically required) and
collide-mat (~5ns/row extra from the `Rc` indirection + eager component
reads in readers). Accepted: the materialization cost is the D2 contract;
`f32-hot-columns`/dense lanes are the recorded mitigation path.
