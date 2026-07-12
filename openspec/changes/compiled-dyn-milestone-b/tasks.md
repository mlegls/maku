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
- [x] 4. ClosedPt group pose fill: shared classification
      (VelChain / ClosedChain / interpreted) + pooled lane scratch;
      batched fill at collide phase 0 and the cull loop with per-row
      wrapper composition; per-lane oracle. Wall A/B recorded.
- [x] 5. AxisSel scatter: per-tick memo in `refresh_dyn_cols` keyed on
      (form Rc identity, env identity, tau bits); rows run
      `axis_select_val` only. Tests: shared-array group evaluates once
      (count via a probe or side-effect-free marker), per-row values
      unchanged; non-List form falls back per row.
- [x] 6. Readers audit (post-4): measured — the trace loop built
      readers for every alive row before checking trace_window (3.1M
      constructions on fruit-2000, the dominant sim:motion-readers
      volume). Moved construction inside the traced branch: 3.1M -> 8k
      calls; fruit wall 2370 -> 2290 ms (-3.4% vs pre-round baseline,
      interleaved A/B). RowStateSnapshot pooling unnecessary at the
      residual volume.
- [x] 7. Cull-reuse audit: SOUND — between the collide fill and cull,
      field writes and remat are PendingWrites drained at the NEXT
      step's start, and rule kills only clear the alive flag; nothing
      mutates n2 state or figures. Landed as class-cache-gated reuse of
      the collide sampled pose for Vel-chain rows (figure-root ptr
      validated; oracle re-derives and asserts per reused row). Fruit
      wall 2372 -> 2185 ms (-7.9% net for the round).
- [ ] 8. Final gates + bookkeeping: full suites green; aggregate wall
      delta recorded; design.md "Implementation notes (as built)"
      appended (deviations, audit outcomes); lowering spec Design
      milestone-state paragraph updated via the archive delta.
