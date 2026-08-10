# Tasks — entity-representation-flip

- [x] 1. Slice 1 — spec table: `SpecStore` (refcount, generational ids,
  Weak template memo), `spec_id` column replacing the six
  `EntitySpecStore` columns, per-spec `MotionStateSchema`,
  install/cull/reuse/clone plumbing, remat re-mint,
  `resolve_node_pose` re-plumb (investigate call-site semantics first).
- [x] 2. Slice 1 gates: core suite + `MAKU_LOWER_ORACLE=1` + ignored
  release oracle card suites green; commit.
- [x] 3. Slice 2 — captures to rows: spec capture layout in per-leaf
  walk order, per-row capture vector, shared trees for rand-bearing
  compiled-path groups, ambient frame as row data, axis as row input,
  `instantiate_rand` demoted to cap drawing; Bail path keeps
  per-element specs.
- [x] 4. Slice 2 gates: same suites, plus RNG bit-parity — same-seed
  cross-commit A/B on rand-heavy cards must be bit-identical (this
  round, unlike the RNG round, changes no draw keying).
- [ ] 5. Slice 3 — explicit-id re-keying: closed-pose class cache,
  slots.rs dyn-field memo, collision projector front-end memo,
  vel-chain slot resolution, `examples/dbg.rs` histograms.
- [ ] 6. Rewrite/extend representation tests: round-22 node-walk tests
  (`rand_capture_slots_share_programs`,
  `env_capture_slots_intern_across_sites`), new tests for spec sharing
  across a group, refcount free/reuse with generation bump, bounded
  memo growth, snapshot/scrub determinism unchanged.
- [ ] 7. Full gates on the final tree: core suite, oracle mode, all
  ignored release oracle card suites, first-hand.
- [ ] 8. Perf verdict: interleaved wall-only A/B (representative suite
  aggregate + scaled fruit 12000t) vs pre-round commit; update
  `openspec/specs/perf/spec.md` walls table same-session.
- [ ] 9. Sync the lowering delta into `openspec/specs/lowering/spec.md`;
  record measured results and deviations in design.md.
- [ ] 10. Remove the folded `spec-store-dedup` stub (pointing here),
  update `group-integrator-dedup` to reference SpecId identity; archive
  the change.
