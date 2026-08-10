# Tasks — rng-spawn-order-independence

## 1. Core primitive and seed surface

- [x] 1.1 Replace `World.rng: u64` with `seed: u64` + transient scope `{base, n}`; `rng_mix` splitmix-finalizer helper with domain constants; `next_rand` = mix(base, n++) with today's f64 mapping; `reset_rng(seed)`; constructor calls it with the existing default constant; `for_eval_keyed(tick_rate, base)`; debug-only stale-scope assert in `next_rand`.
- [x] 1.2 Executor re-basing: `Task.rng_key`/`fork_n` (root = mix(seed, TASK, creation ordinal); fork = mix(parent key, FORK, parent fork_n++)); scope set to mix(task key, tick) before each task step; interpreted tick rules mix(seed, RULE, rule index, tick); load-time top-level forms run under the owning root task's key.

## 2. Entity keys and captures

- [x] 2.1 `spawn_key` drawn from the current scope in `plan_spawn`; `elem_key = mix(spawn_key, elem ordinal)`; carried on `EntitySpec`; `EntityStore.rng_key` column (push_row, reuse_free_row, clone, truncate); assigned in `install_entity`.
- [x] 2.2 `draw_caps` draws site k from mix(elem_key, CAPS, k) honoring RandSite bounds; `subst_rand` numbers its walk and draws the same keys; update the round-22 contract comment (order → numbering).

## 3. Keyed scratch contexts

- [x] 3.1 Rowful `for_eval` sites switch to `for_eval_keyed`: evolve step/init (EVOLVE), pending-field fns and masked-update values (FIELD), collider-projector bodies (COLLIDER) — base = mix(entity rng key, domain, tick). Rowless sites (signal eval, FnPose) stay on plain `for_eval`; fix the stale motion.rs:409 comment.

## 4. Session and host

- [x] 4.1 `Sim::reset_rng` passthrough; `ProgCmd::Seed(u64)` recorded/replayed like Add/Swap; `host.rs` `seed <n>` command.

## 5. Verification

- [ ] 5.1 New tests: unrelated-draws-don't-shift (extra draws in task B leave task A's render rows identical); random-walk evolve varies per tick/entity and is scrub-exact; seek across a mid-run seed reset; two-load same-seed determinism still holds.
- [ ] 5.2 Full gates: core suite + MAKU_LOWER_ORACLE=1 + the ignored oracle card suites in release; fix any pinned tests that encoded the sequential stream.
- [ ] 5.3 Rand-free A/B parity spot-check against pre-change worktree (abdump); interleaved wall A/B on the perf cards for the executor-mixing overhead.

## 6. Sync and archive

- [ ] 6.1 Sync deltas into openspec/specs (determinism, session); update language/spec.md:480 rand bullet and motion comments to landed state; record measured results in design.md; archive the change.
