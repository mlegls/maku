# Design — compiled-dyn milestone B remainder

## Context

Milestone B (group evaluation of shared programs) is partly landed:

- Round 19: batched Vel steps (`lower::run_lanes`, `sim::VelBatchScratch`)
  — rows whose figure is constant wrappers over one compiled-integrand
  Vel node step as lanes of one program run — plus the pos_only pose
  fast path (`vel_chain_ptr`/`wrapper_chain_pos_pose`). −34% wall on the
  scaled fruit rig.
- Round 22: input slots + structural interning — rand draws and numeric
  env captures fill per-entity capture vectors over `(%capture i)`
  marker programs; programs intern by op-stream identity and fuse batch
  groups across spawn sites. −8% wall.

The remainder is JIT prep more than wall win on the current rig, but it
completes the "one interned, input-slotted IR runs all motion surfaces"
stopping point (openspec/specs/lowering/spec.md, "JIT readiness"):

1. **ClosedPt group pose evaluation** — the pos-only pose fill for
   closed shapes (collide phase 0, cull) still runs per row through
   `dyn_figure_pose_in`, one `eval_num_program_pair` per row per phase.
2. **AxisSel lane scatter** — array-valued spawn-meta signals
   (`NumDynRepr::AxisSel`) evaluate the full shared array per entity per
   tick in `refresh_dyn_cols`, then each row keeps its lane.
3. **Homing-slew coverage** — the milestone-A bail census's dominant
   uncompiled shape, `(slew 60 90 (angle-of (- (live $chan) pos)))`
   (225 per-entity nodes), needs scan-cell reads, channel/stream reads,
   and pose-pair math to classify.
4. **Cheap win: motion readers** — `row_motion_readers` is called per
   entity per phase; the `Row` snapshot backing allocates three Vecs per
   call for schemas beyond the ≤2-n2 inline case.
5. **Candidate lever: cull-time reuse of the collide-phase pose**
   (~11% of step; `fast_pos_pose` runs 2×/row/tick) — exact for Vel
   chains ONLY if nothing between the phases mutates n2 state or
   figures. Needs a rule-effect audit before it can land.

Standing constraints: bit-exact determinism across tiers (oracle-gated),
all-or-nothing classification, callback-free total ops behind the
`(program, input lanes, scratch)` executor boundary, DynNode ≤ 96 bytes,
wall-only interleaved A/B for perf claims.

## Goals / Non-Goals

**Goals:**

- Batched pos-only pose fill for wrapper-chain-over-ClosedPt rows at the
  collide and cull phases, bit-identical to the per-row path.
- AxisSel shared signals evaluate once per (program, env, tau) group per
  tick; rows scatter their lanes (SS5 array-of-signals interchange).
- `NumProgram` grows the auxiliary-input surface (scan-cell reads,
  channel/stream reads, pose-pair math) needed to classify the
  homing-slew census shape, without breaking totality or interning.
- Reader-construction overhead measured and reduced where the wall
  supports it.
- The cull-reuse lever audited; landed if sound, dropped with recorded
  rationale if not.

**Non-Goals:**

- IR unification across the four evaluator families (readiness item 1) —
  its own change.
- Fused scan ADVANCE ops (eval+step in one program) — milestone C. The
  evolve step stays interpreted here; we compile only the read side.
- The entity-representation flip (spec id + capture vector replacing
  per-row node clones).
- f32 hot-column widths, parallel scheduling, cranelift/wasm emission.
- Collider materialization and render-row batching (separate rounds).

## Decisions

### D1: Auxiliary inputs are driver-filled lanes, not callback ops

Scan-cell and channel/stream reads enter programs as **program-declared
input tables** the driver resolves before the run:

```rust
pub struct NumProgram {
    ops, n_regs, n_inputs,               // as today
    aux: Option<Rc<AuxTables>>,          // None for today's programs
}
pub struct AuxTables {
    scans: Vec<u32>,        // evolve site indices relative to the node base
    chans: Vec<ChanRef>,    // resolved per run through the eval's SigEnv
}
pub enum ChanRef { Stream(u64), Named(Rc<str>) }
```

New ops: `ScanIn { dst, idx }`, `ChanX { dst, idx }` / `ChanY { dst, idx }`
(pose-valued channels enter as two lanes), `Atan2 { dst, y, x }`. The
runner signatures gain one aux-values slice (`&[f64]`, laid out
scans ++ chans×2); `run_lanes` takes it at per-lane stride for scans and
shared per group for channels.

The driver (the eval call sites in `dyn_node_pose_u_in` /
`step_motion_in`) fetches scan cells through the row's `MotionReaders`
and channel values through the SAME `SigEnv` the interpreter would read
— same snapshot, same mid-tick republish visibility. A missing scan cell
(eval before the first advance — control-phase evals only, since
scan-step precedes collide), a non-`Val::Num` cell, or a non-Pose value
behind a pose-consumed channel **bails that eval at the driver level**
and reruns interpreted, per the totality contract (no error paths inside
kernels).

*Alternatives rejected:* a callback `ReadScan` op reaching into readers
(breaks the callback-free/total contract — the JIT seam); an `Interp`
fallback op (same); caching channel values per tick across phases
(exports republish mid-tick — would diverge from the interpreter).

Interning: `AuxTables` joins the structural key (site indices, stream
ids, channel-name bytes). Aux programs intern among themselves; they
never join the batched Vel STEP anyway (D2), so the fusion loss is nil.

### D2: Evolve reads compile; the advance stays interpreted

Post-expansion, `slew`/`smooth` are `(evolve init step)` at a ScanSite.
The read side (`sf_sited_evolve` with `advance=false`) is: stored cell
value, else evaluate init. Lowering maps a sited evolve to `ScanIn` when
the stored cell is expected (driver bails when absent — see D1) — init
and step forms are NOT inlined into the program; the site-counter
discipline is honored at lower time by walking the same
`collect_scan_sites` order so site indices match the interpreter's.

Consequences:

- `vel_step_plan` (the batched STEP) requires **aux-free** programs:
  a scan-bearing integrand's step must run the interpreted evolve
  advance, so those rows keep the per-row step path. What compiles is
  every read-side eval: Vel heading, ClosedPt/RotExpr samples — the
  paths collide/cull/trace/render pay per row per tick.
- Fusing the advance into the program (making these rows batchable) is
  milestone C, unchanged.

### D3: Pose-pair lowering (scalarization)

The Builder gains a value class: a lowered subexpression is `Num(reg)`
or `Pair(reg_x, reg_y)`. Pair sources: `pos` (PosX/PosY), `(live $s)` on
a pose-valued stream/channel (ChanX/ChanY), env-captured `Val::Pose`
(two Consts), `(cart a b)` over nums. Pair operators: `+`/`-`
componentwise over pairs. Pair consumers: `(angle-of p)` →
`Atan2(y, x)` then `.to_degrees()` as ops (matching
`builtins/geometry.rs` exactly), `(mag p)` → sqrt(x²+y²), `(:x p)`,
`(:y p)`. Anything else touching a Pair bails the whole form
(all-or-nothing). Theta never enters pairs — none of the covered
consumers read it, and pose arithmetic theta semantics stay the
interpreter's business.

DynNode is untouched (programs grow behind the existing
`OnceCell`/`Rc`; the ≤96-byte guard stays green).

### D4: ClosedPt pos-only batch fill

A shared pre-pass used by collide phase 0 and the cull loop:

- Classify each row once per phase: `VelChain` (pose = n2 state through
  wrappers — today's `fast_pos_pose`), `ClosedChain` (constant wrappers
  over ONE compiled **aux-free** ClosedPt — pos-only needs a single
  program-pair run at `(tau, u=0)`, no state), else the interpreted
  per-row path.
- `ClosedChain` rows collect lanes grouped by interned program-pair
  address + polar (the `VelBatchScratch` shape: contiguous-row fast key,
  pooled groups), with per-lane `(tau, caps)`; one `run_lanes` per
  component per group; per-row wrapper composition afterward (cheap pure
  arithmetic, exactly `wrapper_chain_pos_pose`'s arms with the sampled
  point in place of state).
- Under `MAKU_LOWER_ORACLE=1` every lane re-runs its own node through
  `dyn_figure_pose_in` pos_only and asserts exact equality (the
  `fast_pos_pose` precedent).

Lane results are bit-identical to scalar runs by `run_lanes`
construction, and wrapper composition is the same op order as the
per-row walk, so parity holds exactly.

### D5: AxisSel evaluates once per group per tick

No IR change. `refresh_dyn_cols` keeps a per-tick memo
`(form identity, env identity, tau bits) → Val`: the first row of a
group evaluates the shared array expression once; subsequent rows hit
the memo and run only `axis_select_val` on their captured path/flat
index. Identity is the `Rc` address inside `Form::List` (AxisSel clones
share the spawn signal's Rc); non-List forms (bare sym — rare) skip the
memo and evaluate per row, unchanged. Tau joins the key because rows
from different spawn ticks share the form but not the sample time.

*Alternative rejected:* lowering AxisSel to NumProgram lanes — the array
result is a `Val` (arbitrary element kinds), not a numeric lane set;
interchange at the Val level is the semantics-preserving move, and the
memo removes the O(rows × array-size) blowup, which is the actual cost.

### D6: Readers — measure, then pool (audit-gated)

Round 19 already landed the ≤2-n2 inline backing and shared node-id
maps, and D4 removes most reader construction from collide/cull
entirely. What remains: the `Row` snapshot backing allocates three Vecs
per call for dyn/val-bearing schemas, and trace/step still construct per
row. Task order: measure the post-D4 wall share of
`sim:motion-readers`; if it still registers, pool the snapshot vectors
in a Sim-owned scratch (same shape as `VelBatchScratch` recycling)
rather than restructuring to borrows — snapshot semantics ("reads see
values as of construction while the step writes through") are
load-bearing in the step phase, so borrowing is not a drop-in. If the
wall share is noise post-D4, record that and stop.

### D7: Cull-time pose reuse (audit-gated)

Between the collide fill (tick T, pose cached via `set_sampled_pose`)
and the cull loop (after `tick += 1`), the phases that run are: standing
rules, pending-write drain, and dyn-col refresh. For `VelChain` rows the
pose depends only on n2 state and the wrapper chain — both can be
mutated between the phases (remat, `set_dyn_figure`, field writes
driving state resets, `set_motion_schema`). The audit enumerates every
mutation path reachable from those phases; if each one flows through a
narrow set of entity-mutating entry points, add a per-tick row-dirty
mark set by those entry points, and let cull reuse the sampled pose for
un-dirtied `VelChain`/`ClosedChain` rows (ClosedChain rows additionally
require tau-invariance — they are NOT tau-invariant, so ClosedChain
reuse requires re-running lanes at the new tau; only the VelChain case
reuses directly). Oracle mode re-derives and asserts. If the audit finds
an ungateable path, drop the lever and record the finding here.

## Risks / Trade-offs

- [Aux tables widen the interning key] → aux programs never batch-step,
  so lost fusion is zero; eval-side sharing still occurs among identical
  sites.
- [Pair lowering grows the classification surface] → strict
  all-or-nothing per form; every new op class gets scalar-vs-interpreter
  unit tests plus the standing card-suite oracle.
- [Channel snapshot divergence] → channels resolve through the same
  SigEnv at each run, never cached across phases; oracle card suites
  exercise mid-tick republish.
- [Site-counter mismatch between lower-time walk and `form_site_count`]
  → the lowering walk reuses `collect_scan_sites`' traversal; a
  dedicated test pins a form with nested/skipped evolve regions.
- [ClosedPt batch diverging from per-row wrapper composition] → shared
  helper for the wrapper walk; per-lane oracle.
- [D7 dirty-tracking misses a mutation path] → oracle asserts
  re-derivation equality on every reused pose; land only with the audit
  written down.
- [Perf regression risk from added classification passes] → wall-only
  interleaved A/B on the scaled fruit rig per item, per
  openspec/specs/perf/spec.md; items that don't pay their way get
  reverted (the census rig may not exercise AxisSel/slew heavily — those
  two are coverage/JIT-prep items and are justified by the bail census,
  not the fruit wall).

## Migration Plan

Each numbered item lands as its own commit(s), gated on: full core unit
suite, the 4 ignored oracle card suites under `MAKU_LOWER_ORACLE=1`, and
mesh tests. No card/user-facing surface changes; no spec-store or replay
format changes. Rollback is per-commit revert.

## Open Questions

None blocking. D6 and D7 carry their own audit outcomes; either lands or
is dropped with rationale recorded in this file at archive time.

## Implementation notes (as built)

- **Aux inputs (D1–D3)**: `NumProgram` gained `aux: Option<Rc<AuxTables>>`
  (slots + typed chan refs) and an explicit `result` register — the pair
  work exposed a latent assumption that the result is the last op's dst,
  which pair component selection breaks. New ops are `AuxIn` (one op for
  scan cells and channel components; the driver lays out the aux slice
  per the slots table) and `Atan2`; `mag` composes existing ops. Channel
  kind (Num/Pose) is fixed by the first consumer at lower time; a
  conflict bails the form. `run_lanes` never takes aux (aux programs
  never batch), only the scalar runner does.
- **Evolve reads**: only Current-scope, num-state evolves lower (def
  bodies would number sites the static walk can't see; pose-state
  evolves — `smooth` — driver-bail). The census slew shape compiles on
  the eval side; the STEP keeps the interpreted advance, so the step
  arms and `vel_step_plan` require aux-free programs. ClosedPt compiles
  with aux OFF: its sites never advance, so a ScanIn would bail forever.
- **ClosedPt batch fill (D4)**: landed with a cross-tick classification
  cache (figure-root ptr → RowClass) and a 16-tick rediscovery gate for
  cards with no closed rows; cull re-lanes only the tick's candidates.
  Cost on the (closed-free) scaled fruit rig: ~+1.1% before the other
  items, of which ~0.3% is binary-layout noise — recorded as the price
  of the pose-fill kernel seam. Corpus closed-pt eval volume is ~8ms
  today, so this is JIT prep, not a wall win.
- **AxisSel (D5)**: as designed — a per-refresh memo keyed on
  (form Rc identity, env identity, tau bits); no IR change.
- **Readers (D6)**: the audit found the real cost was the trace loop
  constructing readers for every alive row before checking
  `trace_window` (3.1M constructions per 2000 fruit ticks). Moving
  construction inside the traced branch cut it to 8k; snapshot pooling
  was unnecessary at the residual volume.
- **Cull reuse (D7)**: audit PASSED — field writes and remat are
  PendingWrites drained at the next step's start, and rule kills only
  clear the alive flag, so nothing between collide and cull mutates n2
  state or figures. Landed as class-cache-gated reuse of the collide
  sampled pose for Vel-chain rows (figure-root validated, oracle
  re-derives and asserts per reused row). No dirty-mark machinery was
  needed.
- **Round wall total** (scaled fruit rig, wall-only interleaved A/B vs
  the pre-round baseline): 2372 → 2185 ms median, **−7.9%**.
- Known pre-existing (not this round): the full unit suite under
  MAKU_LOWER_ORACLE=1 fails in `compiled_render_rule_emits_column_batch`
  (batching disabled under oracle), and the parallel test harness
  aborts intermittently on macOS ("failed to initiate panic, error 5"),
  both reproduced at the pre-round commit.
