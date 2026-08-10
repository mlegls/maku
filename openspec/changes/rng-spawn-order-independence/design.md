# Design — RNG spawn-order independence

## Context

All randomness funnels through `World::next_rand` (world.rs:1425), a splitmix64 step over
a single `u64` counter initialized to a hardcoded constant (world.rs:1210). Five call
sites exist: the `(rand a b)` head in `evaluate_list_inner` (interp/mod.rs:1605), and the
spawn-capture pair `draw_caps`/`subst_rand` (spawn.rs:465/502). A draw's value therefore
depends on how many draws happened before it, globally: reordering two unrelated spawns
shifts every subsequent draw in the run.

Two facts discovered while mapping the seams sharpen the picture:

- **Uncaptured rand is already degenerate, not "per-eval".** Every scratch-world
  evaluation context (`World::for_eval` — direct signal eval at motion.rs:1178, evolve
  step/init at motion.rs:779/797, `FnPose` at motion.rs:1914, collider-projector bodies
  at slots.rs:465/497, pending-field fns at sim/mod.rs:1174/1248, masked-update values at
  sim/mod.rs:1380) gets a fresh world whose counter restarts at the same constant. `(rand)`
  there returns the *same value on every evaluation, for every entity, at every tick*. A
  random-walk evolve `(+ p (rand -1 1))` walks in a straight line today. The comment at
  motion.rs:409-411 claiming "per-eval rand semantics" is wrong about the current code.
- **There is no seed surface at all.** No constructor parameter, no host command, no tape
  field. Scrubbing works only because `impl Clone for World` copies the counter
  (world.rs:1094) inside snapshots.

The language spec already prescribes the destination (language/spec.md:480): counter-based
`rand(seed, path, k)`, element k's randomness independent of evaluation order. This change
implements it. Decision-level choices were settled in proposal.md (2026-08-10); this
design binds them to the seams.

## Goals / Non-Goals

**Goals:**

- A draw's value depends only on the drawing context's local causal history (its task's
  key, its entity's key, its tick, its local ordinal) — never on global draw interleaving.
- Host-facing `reset_rng(seed)`; construction is an implicit call of it; mid-run resets
  land on the command tape.
- Per-entity keys stored in the entity store, usable later by parallel hot loops.
- The capture contract (`draw_caps` mirrors `subst_rand`) weakens from same-walk-order to
  same-site-numbering.
- Keyed scratch contexts fix the degenerate-constant bug: rand in evolve steps,
  pending-field fns, masked-update values, and collider-projector bodies becomes properly
  per-entity-per-tick random, replay- and scrub-safe.

**Non-Goals:**

- Draw stability across card edits (edits may renumber sites; tapes are tied to a card
  version).
- Preserving the old stream — every rand-using card's output changes once.
- Noise functions (rider requirement only: gradients derive from the same primitive).
- Statistical-quality guarantees beyond "good enough for gameplay" (no Philox).

## Decisions

### D1 — Re-base the existing stream primitive instead of replacing its call sites

`World.rng: u64` becomes `World.seed: u64` (persistent, host-settable) plus a transient
draw scope `{ base: u64, n: u64 }`. `next_rand()` keeps its name and signature —
`mix(base, n++)` through the splitmix64 finalizer, mapped to f64 exactly as today
(`(z >> 11) as f64 / (1 << 53)`) — so all five call sites survive untouched. What changes
is that executors *re-base* the scope before each evaluation unit, so the sequence a unit
sees is a pure function of its context key, not of what ran before it. Draws *within* one
unit stay locally sequential — allowed by the invariant, and it keeps the change small.
Alternative considered: threading an explicit `rand_at(seed, key, k)` through every call
site — more honest types, but five signature chains (`sf_spawn → plan_spawn →
instantiate_rand_geometry → instantiate_rand → draw_caps`, plus `subst_rand`) for no
semantic difference.

The mixer: `rng_mix(h, v) = splitmix64_finalize(h ^ v.wrapping_mul(GOLDEN))`. Stateless,
a few ns, and 64-bit birthday collisions are irrelevant at danmaku scale. The algorithm
is behind the contract ("stateless mix of seed, derived key, counter"); a later swap is
the same one-time-break class as this change. Domain-separation constants (small u64
tags: TASK, FORK, RULE, LOAD, SPAWN, CAPS, EVOLVE, FIELD, COLLIDER) keep sibling streams
from aliasing.

### D2 — Context keys follow the task tree, not an entity tree

Entities don't execute code; tasks and rules do, so the hierarchy that matters is the
task tree. `Task` (sim/exec.rs:41) gains `rng_key: u64` and `fork_n: u64`:

- Root tasks (card load, hot-add, hot-swap): `rng_key = mix(mix(seed, TASK), root_ordinal)`
  where the ordinal is a per-Sim task-creation counter — load order is the program itself,
  and hot-adds are already tape commands, so this is deterministic under replay.
- Forked tasks: `rng_key = mix(parent.rng_key, mix(FORK, parent.fork_n++))` — discriminated
  by the parent's own local fork count, never a global counter.

Before each task step, the executor sets `world scope = { mix(task.rng_key, tick), 0 }`.
Interpreted tick rules get `mix(mix(mix(seed, RULE), rule_index), tick)` per rule per
tick. Load-time top-level evaluation runs under the owning root task's key. `manipulate`
callbacks, `states` goto labels, and wait predicates all run inside a task step and are
covered by its scope.

Accepted consequence: draws inside a rule body that iterates rows depend on row order,
which is world state — deterministic under replay, but not independent of spawn history.
Recorded as a limitation; rand in row-iterating rule bodies is rare.

### D3 — Entity keys derive from the spawning scope; capture numbering replaces capture order

`plan_spawn` consumes one slot from the current scope to form
`spawn_key = mix(scope.base, mix(SPAWN, scope.n++))`, then each flattened element gets
`elem_key = mix(spawn_key, elem_ordinal)` (flatten order — local to the spawn). The key
rides `EntitySpec` into `install_entity` and lands in a new `EntityStore` column
`rng_key: Vec<u64>` (pushed in `push_row`, reset in `reuse_free_row`, cloned/truncated
with the rest).

Captures: `draw_caps` draws site k from `mix(mix(elem_key, CAPS), k)` honoring the
`RandSite` bounds; `subst_rand` numbers sites in its walk order and draws the same keys.
`extract_rand` assigns slots in the identical structural order, so numbering agreement is
inherited from the existing shared walk — the round-22 contract shrinks from "consume the
global stream in exactly the same order at the same time" to "agree on site numbering",
which the oracle already exercises. The textual `rand`/`rand-int`/`randpm1` head matching
(spawn.rs:382 etc. vs the prelude defns) must keep site *numbering* identical rather than
draw *counts* identical — a strictly weaker obligation.

Rand in spawn *meta maps* and collider args (evaluated via site #1 before the capture
pass) draws from the task scope like any other task-body rand. Two mechanisms in one
`(spawn ...)` remains true, but both are now order-independent.

### D4 — Keyed scratch worlds where a row is in hand; explicit constant where not

`World::for_eval_keyed(tick_rate, base)` sets the scratch scope. Call sites that know the
entity row pass `mix(mix(entity_rng_key, DOMAIN), tick)`: evolve step/init (EVOLVE),
pending-field fns and masked-update values (FIELD), collider-projector bodies (COLLIDER).
This turns the degenerate constant into real per-entity-per-tick randomness that is
scrub-safe by construction (the key is a pure function of snapshot state). Pure signal
evaluation (motion.rs:1178) and `FnPose` have no row; they keep plain `for_eval` and its
fixed base — same behavior as today, now documented as deliberate: uncaptured rand in a
rowless signal context is a deterministic constant.

### D5 — Seed API: `World::reset_rng`, implicit at construction, taped as a command

`World::reset_rng(&mut self, seed: u64)` sets `self.seed`; `with_entity_capacity` calls
`reset_rng(DEFAULT_SEED)` with the current constant (`0x9e37_79b9_7f4a_7c15`), so default
behavior is unchanged in shape. `Sim::reset_rng` passes through. `Session` records mid-run
resets as `ProgCmd::Seed(u64)` — replayed at their tick exactly like Add/Swap, which is
what makes a mid-run reset scrub-safe; `host.rs` gains a `seed <n>` command. Replay
fidelity is promised only for tape-recorded calls. Snapshots carry the seed via the
existing `World` clone; task keys ride the `Sim` clone; scope state is transient and
re-derived, never snapshot-relevant.

### D6 — Verification: the direct test is "unrelated draws don't shift mine"

Old-vs-new A/B parity (the abdump worktree method) is only meaningful for rand-free cards
— for the rest, the oracle (lowered vs interpreted within the new engine, under
`MAKU_LOWER_ORACLE=1`, core suite + the ignored card suites in release) carries the
verification weight, exactly as it did for the vel re-expression. The new property gets a
direct test: two cards identical except that one task performs extra `(rand)` draws (or
two spawns are reordered across tasks); the *other* task's entities must render
identically. Under the old scheme this is precisely what failed. Existing pins survive by
inspection: same-seed re-run tests compare a sim against itself; `states_markov_routing`
is statistical; the impure-remat test asserts a range; the bail-path test asserts
`caps_of` emptiness, which D3 preserves (bail still substitutes constants into forms).

## Risks / Trade-offs

- [Missed re-base: an executor path evaluates user code without setting the scope, and
  silently inherits the previous unit's scope] → the scope carries a debug-only "stale"
  flag set after each unit; `next_rand` debug-asserts freshness. Cheap and catches every
  such path in the test suites.
- [The textual rand-head matching in extract/subst diverges from a future prelude edit
  (e.g. someone renames `rand-int`)] → unchanged risk from today, now lower-stakes: a
  divergence breaks site numbering only for the edited card, and the oracle catches it.
- [Keyed scratch contexts change behavior cards may have accidentally relied on (constant
  rand in evolve steps)] → accepted; it was a bug, the corpus is small, and the oracle
  card suites will surface any visual drift for review.
- [Session `ProgCmd::Seed` interacts with `truncate_future`/branching] → it is an
  ordinary command; the existing rewind machinery already handles command replay, and the
  branch test extends to cover a seed command before the branch point.
- [Perf: `next_rand` gains one extra mix over today's single step, and executors do a few
  mixes per task-step] → nanoseconds against interpreter dispatch; hot compiled tiers
  never draw (all randomness frozen in caps at spawn — confirmed: `NumOp` has no rand,
  `CompiledTickAction` values are constants). Verify with the standard interleaved wall
  A/B on the perf cards anyway.

## Migration Plan

Single breaking switch, no compatibility window: every rand-using card's draws change
once. Land before `entity-representation-flip` so the flip doesn't bake more capture
vectors into the sequential contract. Rollback is `git revert` of the round's commits —
no persistent-format changes except the additive `ProgCmd::Seed` variant.

## Open Questions

None — decisions were settled at proposal level; this design binds them to seams. The one
deliberate deferral: element keys use flatten ordinals, not the structural
`SpawnElem.path`; if array spawning later needs sub-spawn stability under reshaping,
switching the mix input to the path components is a local change in `plan_spawn`.
