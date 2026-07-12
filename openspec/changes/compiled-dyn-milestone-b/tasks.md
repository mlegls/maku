# Tasks — compiled-dyn milestone B remainder

Gates for every task that touches lowering/eval: full core unit suite,
the 4 ignored oracle card suites under `MAKU_LOWER_ORACLE=1`, mesh
tests. Perf claims via wall-only interleaved A/B on the scaled fruit rig
(openspec/specs/perf/spec.md). Commit per coherent change-set.

- [x] 1. Aux-input IR extension: `AuxTables` (scan site indices, chan
      refs) behind `Option<Rc<..>>` on `NumProgram`; `ScanIn`/`ChanX`/
      `ChanY`/`Atan2` ops; aux slice through `run`/`run_num_program_caps`/
      `run_lanes`; aux joins the interning key. Unit tests: op semantics
      vs interpreter, interning with/without aux.
- [x] 2. Pose-pair lowering: Num/Pair value classes in the Builder;
      pair sources (`pos`, pose-valued `(live $s)`, captured `Val::Pose`,
      `cart`), pair ops (`+`/`-`), pair consumers (`angle-of`, `mag`,
      `:x`/`:y`); all-or-nothing bail on any other pair use. Unit tests
      per source/op/consumer against `eval_sig_at_rate`.
- [x] 3. Evolve-read lowering + drivers: sited-evolve reads lower to
      `ScanIn` with lower-time site numbering matching
      `collect_scan_sites`; drivers in `dyn_node_pose_u_in` (ClosedPt/
      Vel/RotExpr arms) fetch scan cells via readers and channels via
      SigEnv, bail-to-interpreter on missing/mistyped aux;
      `vel_step_plan` requires aux-free programs (scan-bearing rows keep
      the interpreted step). Test: the census homing-slew shape compiles,
      oracle-equal over a multi-tick run; site-counter pin test with
      nested evolve regions.
- [ ] 4. ClosedPt group pose fill: shared classification
      (VelChain / ClosedChain / interpreted) + pooled lane scratch;
      batched fill at collide phase 0 and the cull loop with per-row
      wrapper composition; per-lane oracle. Wall A/B recorded.
- [ ] 5. AxisSel scatter: per-tick memo in `refresh_dyn_cols` keyed on
      (form Rc identity, env identity, tau bits); rows run
      `axis_select_val` only. Tests: shared-array group evaluates once
      (count via a probe or side-effect-free marker), per-row values
      unchanged; non-List form falls back per row.
- [ ] 6. Readers audit (post-4): measure `sim:motion-readers` wall
      share; if it registers, pool `RowStateSnapshot` vectors in
      Sim-owned scratch; else record the measurement in design.md and
      stop.
- [ ] 7. Cull-reuse audit: enumerate mutation paths (rules, pending
      writes, remat, schema/figure sets) between collide fill and cull;
      if gateable, per-tick row-dirty marks + VelChain sampled-pose reuse
      in cull with oracle re-derivation; else record the blocking path in
      design.md and drop the lever.
- [ ] 8. Final gates + bookkeeping: full suites green; aggregate wall
      delta recorded; design.md "Implementation notes (as built)"
      appended (deviations, audit outcomes); lowering spec Design
      milestone-state paragraph updated via the archive delta.
