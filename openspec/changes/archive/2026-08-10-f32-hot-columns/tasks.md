# Tasks — f32-hot-columns

- [x] 1. Design bound to surveyed seams (D1–D6, slices) committed
- [x] 2. Slice 1: `Pose32` + narrow `state_n2`/`sampled_pose`/
       `trace_cache`/`root_frame`/`captures`; accessor round/widen;
       bail-path rounding; cull-pose oracle expected-side rounding;
       round-trip + bail-parity tests
- [x] 3. Slice 1 gates first-hand: core suite plain + oracle, release
       ignored oracle suites; commit
- [x] 4. Slice 2: narrow `WorldFields::num_values`, `ColliderData`,
       `CollisionIndex::aabbs`; dyn-field + masked-update oracle
       expected-side rounding; narrow-phase widens to f64
- [x] 5. Slice 2 gates first-hand; commit
- [x] 6. Slice 3: narrow render batch geometry columns with
       entry-rounding on both fill paths; collapse redundant casts in
       touhou mesh push; digest pin re-derivation; bench series id →
       `maku-v1-f32`; native bench smoke verifies
- [x] 7. Slice 3 gates first-hand; commit
- [x] 8. Drift meter: corpus max-|Δ| vs pre-round worktree recorded;
       scripted suites' behavioral outcomes unchanged (or investigated)
- [x] 9. Interleaved wall-only A/B (suite + scaled fruit) within ±5%;
       walls table updated
- [x] 10. Spec deltas synced (determinism, lowering, perf), Measured
       section written, change archived
