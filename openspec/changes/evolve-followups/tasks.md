# Tasks

Order follows the design's migration plan: epochs → soft-cull → vel
re-expression → F1 lint → map-remat lowering. Every step lands green on the
core suite plus the 4 ignored oracle card suites
(`cargo test --release -- --ignored`), committed immediately.

## 1. Per-dyn-field epochs (D5)

- [x] 1.1 Add per-field epoch columns to dyn-valued field slots; spawn-time installs anchor at birth (assert no behavior change on the existing card corpus)
- [x] 1.2 Route field-dyn evaluation through `τ_field = t − epoch_field` in the dyn-column refresh
- [x] 1.3 Restart the field epoch on dyn installs via remat field-map keys and field writes; motion remats leave field epochs untouched (tests: mid-life fade starts at event; fade survives motion remat)
- [x] 1.4 Version the snapshot format for the new epoch columns; scrub/replay tests pass (snapshots are in-memory `Sim` clones — `Clone` coverage is the format; scrub test added)

## 2. Soft-cull fades (D6)

- [x] 2.1 Implement `soft-cull` in `crates/core/lib` (opacity-fade remat + deadline field + stock cull rule); test fade-from-event-tick and deadline cull

## 3. vel re-expression (D1–D4)

- [x] 3.1 Introduce the stock integrator evolve shape reading component columns; integrator step evaluates components once, writes the columns; refresh pass skips integrator-owned columns (columns stay out of `dyn_cols`; the step writes world columns directly)
- [x] 3.2 Re-express `(vel …)` — surface unchanged via `sf_vel` constructing the stock shape; reserved `:vel-x`/`:vel-y`, concurrent-integrator spawn error, stages segments share (pure prelude-macro face deferred to core-lib-stratification)
- [x] 3.3 Transfer classification: closed/live of the motion slot derives from component programs; capture guards and sited-evolve (homing-slew) behavior covered by tests
- [x] 3.4 Port the node-keyed machinery to the stock shape: batch lanes group by (component program ids, captures); state slot keying; `clamp_integrator` recognition; pos_only fast path; spawn capture walk order preserved
- [x] 3.5 Stage-exit `:vel` compat shim keyed off the stock integrator (stages round owns the real redesign)
- [x] 3.6 Delete `DynNode::Vel` and all its arms; DynNode size guard still ≤ 96 bytes
- [x] 3.7 Oracle gate + A/B: full suite and oracle card suites green; old-vs-new render dumps bit-identical on 6 corpus cards; cradle at parity, bullets-10k ~2.3%, exploding-stars ~6% (accepted, recorded in design "Measured" with f32/dense-lane mitigation path)

## 4. F1 lint (D7)

- [x] 4.1 Detect closed-form-integrable component programs (constants, piecewise-affine lerp profiles) at card load; emit the suggested closed rewrite; never rewrite silently (Advisory confidence in the checker, `motion/scanned-closed-form`; test covers constant, lerp-profile, and quiet cases)

## 5. Map-remat masked lowering (D8)

- [ ] 5.1 Recognize `(map (fn [b] (remat b …)) domain)` with statically-known slots; lower to masked epoch + slot updates under the existing masked-update plan domain; dynamic-slot/impure shapes fall back
- [ ] 5.2 Oracle parity tests for lowered vs interpreted batch remats

## 6. Spec sync

- [ ] 6.1 Update the evolve-semantics spec's design prose ("still open: vel — deferred to the model/ split") and the language spec's vel constructor note to the landed shape when syncing/archiving this change
