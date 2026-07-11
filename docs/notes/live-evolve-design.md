# live evolves + engine-clock advance — implementation design

Status: IMPLEMENTED (milestones 1-2 in cd1ab4d, milestone 3 in bec0735).
Semantics per docs/notes/evolve-design.md. Two implementation notes on
top of the design below: the liveness classifier roots keyword access —
access rooted at the step fn's own params ((:x s), (:dt c)) is the
fold's own state and stays CLOSED; only capture-rooted access ((:hp e))
marks live. And live evolves accept a one-behind cell in the
post-boundary sampling window (after the world tick increments, before
the new boundary's pass) where replay is impossible; closed evolves
keep exact-match-else-replay so memoization stays invisible.
This unblocks evolve-design step 3 (re-express vel/slew/smooth/stages
in lib over evolve).

## Current state

- `(evolve init step)` (`interp/mod.rs` ~1344) evaluates BOTH init and step
  eagerly and returns `Val::Evolve(Rc<EvolveDyn { init: Val, step: Val }>)`;
  in a pose slot it becomes the stateless `DynNode::Evolve`.
- Every evaluation replays the fold from epoch start (`evolve_value`,
  motion.rs ~278): O(ticks-since-epoch) per SAMPLE, so an entity alive n
  ticks pays O(n²) total. Correct, pure in tau, closed-only (steps run
  against a closed SigEnv; channel reads error).
- Evolve nodes are not "scanned": `is_scanned_figure` doesn't see them, so
  the per-tick integrate pass skips them, and they carry no motion state.
- Motion state cells hold `[f64; 2]` (n2) or `DynPose` (dyn); the motion
  slot's epoch is `motion_birth` (remat m2), and a motion remat clears the
  slot's cells via the schema swap.

## Target semantics (from the settled contracts)

1. The step advances exactly once per tick at the tick boundary, reading
   pre-tick state (change-col's rule). Within-tick sampling sees the
   settled value.
2. Closed evolves stay pure in tau: off-clock/random-access sampling
   `(d 3.5)` replays the fold. On-clock sampling must not change results —
   memoization is invisible.
3. Live evolves (step reads channels/cells) are legal as slot dyns,
   advanced on the entity's clock with the REAL SigEnv; off-clock sampling
   errors ("live evolve sampled off its clock").
4. Remat: a motion-slot remat clears the slot's evolve state and re-runs
   init at the new epoch start. Init must therefore be RE-EVALUABLE — the
   current eager `init: Val` breaks "init can capture the current pose"
   the moment epochs restart.

## Design

### State storage: a Val cell kind

Add a third motion-state column family holding `Val`:

- `MotionStateSchema` gains `val_slots/val_keys` (like n2/dyn);
  `EntityStore` gains `state_val: Vec<Vec<Option<Val>>>`.
- An evolve node's cell key is `MotionStateKey::Node(id)` — evolve node
  ptrs are seeded into `node_ids` like every stateful node (they already
  are, via the generic traversals — verify `seed_dyn_node_ids_with_ptr`
  and `collect_node_state` get an Evolve arm that interns the node and a
  val slot).
- The stored value is a small struct, not bare state:
  `EvolveCell { state: Val, tick: u64 }` (epoch-local tick the state is
  settled AT). Storing the tick makes monotone advance self-describing and
  catches missed/double advances cheaply.

Rc-saturated Vals in dense columns is fine at this stage — the compiled-dyn
pass will later narrow the common scalar/pose states into lanes; do not
pre-optimize the representation here.

### Deferred init

`EvolveDyn` becomes `{ init: EvolveInit, step: Val }` with

```rust
enum EvolveInit {
    Value(Val),            // evaluated eagerly (today's path, kept for
                           // direct Val::Evolve uses outside slots)
    Thunk { form: Form, env: Env },
}
```

The `evolve` special stops evaluating `items[1]` and captures the form +
env. Epoch start = first advance (or first sample) after the cell is
empty: evaluate the thunk THEN, against the evaluation env of the moment
(closed env for closed evolves, real env for live). This is what makes
`(evolve (:pos e) ...)`-style continuity capture work across remat: the
re-run init sees the post-remat world.

Compatibility note: today "construction is epoch start". For spawn-time
slots the first advance happens on the spawn tick, so thunk-at-first-
advance is observationally identical for the corpus; record any exception
as a stop condition rather than hacking.

### Liveness classification: syntactic, at construction

Walk the step's body form (and the init thunk's form) at `evolve`
construction: any `$channel` symbol read, `(live ...)`, entity-view/
handle-reading head (`entities-where`, `entity-col`, `nearest-entity`,
keyword-head application is ambiguous — see below), or `rand` marks the
evolve LIVE. Everything else is CLOSED. Conservative direction: false
positives (closed classified live) only forbid off-clock sampling and
force real-env advance — safe; false negatives (live classified closed)
would error at advance when the channel read hits the closed env — also
safe, same failure as today. So the classifier only needs to be
*reasonable*, not perfect. Reuse the traversal bones of `contains_t` /
the rewrite pass's purity walk; don't build a new framework.

Keyword-head application `(:hp e)` on a captured entity view: classify
LIVE (it reads world state). A bare keyword access on a local map is then
misclassified live — acceptable per the conservative direction.

### Advance: the scanned-motion pass

- `is_scanned_figure`/`is_scanned` return true for figures containing an
  Evolve node (all evolves now advance on-clock when slotted).
- `step_motion_in` gets an Evolve arm: read `EvolveCell` (empty → run
  init, tick 0); if `cell.tick` < current epoch-local tick, apply step
  once with `{:t :dt :tick}` ctx built from the epoch-local clock; write
  back through a `write_val` sibling of `write_n2`/`write_dyn`. Exactly
  one step per boundary — the pass runs once per tick per entity.
- Env: closed evolves keep the closed SigEnv (semantics unchanged,
  enforcement stays by construction); live evolves get `ctx.sig` (the
  real one, which the step pass already holds) — but note the step
  timing: the scanned pass runs BEFORE rules mutate the world for this
  tick, so live reads see pre-tick state, consistent with rule 1.
- EVALUATION (`dyn_node_pose_u_in` Evolve arm):
  - On-clock (tau within the current tick): read the cell's settled state.
  - Off-clock closed: replay from epoch start (today's `evolve_value`).
  - Off-clock live: error.
  "On-clock" test: `(tau * rate).floor() == cell-settled epoch-local tick`
  within the same epsilon convention `evolve_value` uses.

### Epochs and remat

Nothing new needed beyond m2's machinery: the evolve cell is motion-slot
state, so a motion remat's schema swap clears it, the next advance re-runs
init, and epoch-local t restarts with `motion_birth`. The state-clear API
already takes the slot implicitly (schema swap = the motion slot's clear).
Per-dyn-field epochs (evolves inside dyn COLS surviving motion remats)
remain future work, sequenced with the per-dyn-field epoch split.

### What this unblocks / does not do

- Unblocks evolve-design step 3 (re-express `vel`/`slew`/`smooth`/
  `stages`/`pather` in lib over evolve): without engine-clock advance,
  lib-vel-as-evolve would be O(n²). After this lands, those re-expressions
  become a rewrite-shape + profiling exercise.
- Memoized monotone advance for closed evolves falls out of the same cell
  (on-clock reads hit the cell; replay only off-clock) — no separate
  memo machinery.
- Does NOT touch the push-based side (rules/change-col) or the
  compiled-dyn lowering; the Evolve arm stays interpreted (its step is a
  user fn — compiled-dyn's Interp fallback territory until the lib
  re-expression settles the common shapes).

## Milestones

1. **State plumbing**: val cell family + EvolveCell + schema/store/
   snapshot/reader support; evolve nodes seeded into node_ids; scanned
   classification includes evolves. No behavior change yet (eval still
   replays) — tests assert schema shape and state writes.
2. **On-clock advance + memoized reads**: step_motion Evolve arm; eval arm
   reads the cell on-clock, replays off-clock (closed). Oracle test:
   trajectories identical to replay-only for closed corpus evolves; O(n)
   total cost (add a step-count assertion via a counting step fn).
3. **Deferred init + live evolves**: EvolveInit thunk, liveness
   classifier, real-env advance for live steps, off-clock error. Tests:
   a channel-reading evolve tracks the channel per tick; off-clock sample
   errors; remat restarts a live evolve's epoch and re-runs init at the
   snapped pose.

Order matters: 1 and 2 are semantics-preserving and profile-visible
(closed evolves in the corpus stop being quadratic); 3 changes surface
capability and rides on the settled cell.
