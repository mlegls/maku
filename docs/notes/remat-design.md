# remat / change-col — implementation design

Status: contract SETTLED (docs/notes/TODO.md "`remat` / `manip`"); this doc
turns it into an implementation plan. Nothing here re-opens the semantics.

## Contract (recap)

- `(remat handle spec)` is the handle-preserving primitive, strictly 1:1
  (single handle, single-element spec; multi-element figures error). The
  spec is PARTIAL: supplied slots replace, absent slots retain.
- Epochs are per-slot: a rematted slot's local `t` restarts and its runtime
  state (scan/evolve cells) clears; untouched slots keep clock and state —
  field-only remats never disturb motion.
- Writes apply at/right before the NEXT tick. All reads within a tick see
  pre-tick state; ephemeral row indices stay stable within the tick.
- `(change-col e col f)` is the field-write primitive: a FUNCTIONAL update.
  Writes queue their update function; at the boundary a slot's queued
  functions compose in deterministic action-execution order over the
  pre-tick value. `set-col` = lib sugar `(fn [_] v)`; last-writer-wins is a
  consequence, not a rule. Update functions must be pure.
- A new figure evaluates anchored where the entity currently IS (current
  world pose); callers wanting the old parent frame pass it explicitly.

## Current implementation vs contract

`ActionV::Remat` (interp/mod.rs ~2105) and `ActionV::SetCol` (~2154) exist
but both apply IMMEDIATELY mid-tick:

- `remat` takes only a dyn-or-`(fn [exit] ...)` (the motion slot; no partial
  spec), snapshots exit `{:pos :vel :t}`, anchors the new dyn at the snapped
  world pose — that part matches the contract — but then resets the WHOLE
  entity: `reset_birth` (one global clock, not per-slot), full motion-schema
  swap.
- `set-col` writes the field in place, visible to later reads in the same
  tick. Lib code leans on read-modify-write in the argument
  (`(set-col b :hits (+ (col-or (:hits b) 0) 1))`), which is exactly the
  lost-update shape `change-col` exists to fix.
- No write queue, no `change-col`, no per-slot epochs. Callbacks bill fuel
  (keep that).

## Plan

### Milestone 1 — write queue + `change-col` (fields only) — DONE

Landed as described below (drain at the start of `Sim::step_with` before
channel refresh; closed-SigEnv application; set-col as prelude sugar; lib
read-modify-write sites migrated). Deviations/known edges:
- Fuel: a local 100k-writes drain cap instead of engine fuel billing per
  application — revisit when fuel policy is unified.
- The player-hit iframe guard reads pre-tick state, so two damage contacts
  in ONE tick both pass the guard and both decrement `:lives`/`:hits` (old
  immediate set-col let the first write block the second). The guard spans
  columns, so it cannot fold into a single change-col fn — this is the
  atomic multi-field case milestone 2's partial remat spec covers. Same
  shape applies to the `:grazed` latch. No corpus card exhibits same-tick
  double contacts; the suites stayed green.

1. `World` gains a pending-write queue drained at the tick boundary, before
   any reads of the new tick:
   ```rust
   enum PendingWrite {
       Field { target: EntityRef, col: SymId, f: Val },   // change-col
       Remat { target: EntityRef, spec: RematSpec },      // milestone 2
   }
   pending: Vec<PendingWrite>   // push order IS action-execution order
   ```
   Drain: group by (target, col) preserving order; fold the functions over
   the pre-tick value; dead handles drop their writes silently (same policy
   as today's `world.find` miss).
2. `change-col` action: evaluates handle/col/f now, queues. `f` is applied
   at the boundary against a CLOSED environment — same rule as evolve steps
   (SigEnv with defs only, channels/cells empty) — enforcing purity by
   construction rather than lint. Each application bills fuel.
3. `set-col` becomes lib sugar over `change-col` with the constant function.
   Sym-valued fields ride the same queue (a constant function is the only
   sensible update for syms; composing arbitrary fns over syms is allowed
   but untyped, as with everything else).
4. Migrate `cards/lib/touhou.maku` read-modify-write sites
   (`:hits`, `:graze`, `:lives`, `:hp`, ...) to `change-col` folds so
   concurrent hits accumulate.
5. MIGRATION RISK: deferral changes same-tick read-after-write. Known
   suspects: `:iframe-until` written on hit and read by the graze/hit rules
   (possibly same tick), `:game-over-fired` latch. The card suites are the
   oracle; where a card genuinely needs same-tick visibility, restructure
   the rule (usually: fold the guard into the same change-col fn, which
   sees the composed pre-value chain).

Tests: two decrements in one tick accumulate; within-tick reads see
pre-tick values; order determinism (two writers, non-commutative fns);
queued write to a dead entity is dropped; a step fn touching a channel
errors.

### Milestone 2 — partial remat spec + per-slot epochs — DONE

Landed as planned. Notes on the landed shape:
- `motion_birth` is the motion-slot epoch; `birth` remains the entity clock.
  Motion-pose sampling (step/trace/cull/collision/render/view reads,
  curve sampling, nearest-entity, drain-time exit snapshot) reads motion
  tau; `(:t e)` and dyn meta fields (`refresh_dyn_cols`) keep the spawn
  clock — so a motion remat no longer resets entity age. Corpus audit
  found no card depending on the old whole-entity reset.
- Spec map: reserved `:motion` key; every other keyword key is a field
  entry (Num/Kw constant or update fn) applied through the same code path
  as `PendingWrite::Field`, motion first, then fields in spec order.
- Queue-time validation: a direct (non-fn) motion value must be a single
  pose dyn (`as_dyn_pose`); fn-valued motion is checked at drain when its
  result is coerced. Field values must be Num/Kw/Fn/Builtin.
- Motion fns apply against the same closed defs-only env as field fns.
- Per-dyn-field epochs (fades surviving motion remats) remain the "later"
  slice, as planned.

Original plan:

1. `RematSpec`: motion slot (dyn or `(fn [exit] ...)`) and/or field
   entries (value or update fn). Surface: `(remat h spec-map)` with the
   existing `(remat h dyn-or-fn)` kept as the motion-only sugar (all corpus
   uses today are this form).
2. Per-slot epochs: split `birth` into a motion-slot epoch plus (later)
   per-dyn-field epochs. Field-only remats/change-cols never touch the
   motion epoch; a motion remat resets it and clears exactly the motion
   slot's cells (schema swap stays, but scoped). The 1:1 guard errors on
   multi-element figures at queue time, not drain time (fail loud at the
   call site).
3. `remat` moves onto the same queue: exit snapshot is taken at DRAIN time
   (boundary), not queue time, so it reflects the pre-tick-boundary pose —
   consistent with "reads see pre-tick state".

Tests: field-only remat leaves motion phase untouched (position continuous);
motion remat restarts slot t but retains fields; queued remat + change-col
on the same entity in one tick both apply, in order.

### Milestone 3 — live evolve integration

Per-slot epochs are what live evolves key on: an evolve cell lives in its
slot's state and clears on that slot's remat. This is the prerequisite for
engine-clock advance of live evolves (evolve-design.md milestone), not part
of this work — just don't paint over it: the epoch/state-clear API should
take a slot, not an entity.

### Later (profile-gated)

The aggregate-over-domain write shape
`(map (fn [e] (change-col e :x f)) (entities-where ...))` is the recognizer
fusion target for the masked-SoA fast path; the queue design above keeps
per-(target,col) folds independent, which is exactly what the fused form
needs. Soft-cull fades and the F1 lint also remain open.

## Order of work

Milestone 1 is self-contained and immediately fixes real lost-update bugs
in lib; it can start now. Milestone 2 wants milestone 1's queue. Milestone 3
is sequenced under evolve work, not here.
