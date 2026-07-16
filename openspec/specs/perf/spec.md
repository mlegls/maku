# perf Specification

## Purpose
Performance measurement: the rig, the rules that make a wall delta
trustworthy, and the standing measurements rounds update.

## Requirements
### Requirement: Deltas are measured by interleaved wall-only A/B runs
A claimed performance delta SHALL be established by interleaved A/B runs of bare walls (`MAKU_WALL_ONLY=1`) in one sitting — never by comparing against a baseline number from an earlier session (machine-state drift has produced ±5% on identical commits).

#### Scenario: Landing a perf claim
- **WHEN** a round reports a wall delta
- **THEN** the numbers come from alternated baseline/candidate runs performed together

### Requirement: Profiled output is attribution only
Wall-only runs SHALL be the verdict on any delta; profiled walls and `sample` output are for attribution only — probe overhead has fully masked a +60% wall regression. Attribution ground truth is macOS `sample` on a release binary built with debug symbols.

#### Scenario: Profiled rows look equal
- **WHEN** profiled walls show no difference between two builds
- **THEN** the delta is still judged by wall-only runs before concluding no change

### Requirement: Rounds update the standing walls
A landed perf round SHALL update this spec's `## Current walls` section with same-session measurements of the standard cases.

#### Scenario: Round lands
- **WHEN** a perf change-set is committed
- **THEN** the walls table reflects the new measurements in the same round

## Measurement rig

- Bare walls: `MAKU_WALL_ONLY=1 cargo run --release --example profile`
  (workspace at `crates/`; add `--manifest-path crates/core/Cargo.toml`
  when not cd'd there). The flat profiler's own bookkeeping is ~18% on
  dense cards, so wall-only mode is the number that counts.
- No args runs the card suite (paths are CARGO_MANIFEST_DIR-relative).
- Scaled case: `profile cards/tutorials/t03.maku ex3-fruit-colors 12000`
  (the "scaled fruit rig" — 12k ticks of the densest tutorial pattern).
- macOS `sample <pid> <secs> -file out.txt` on a release binary built
  with `CARGO_PROFILE_RELEASE_DEBUG=true` is ground truth for where time
  goes; aggregate children of a stack anchor when comparing.

## Draw-path A/B: old immediate-mode player vs mesh pack (2026-07)

Measured once, post-round-22 (temporary probe timing the bullet draw
path per frame — `render()`/`render_frame()` through geometry
submission — over ticks 0..900 of t03 ex3-fruit-colors, peak 2112
rows, interleaved runs):

- old player (`draw_circle`/`draw_line` per row): mean ~1.01ms/frame
  (0.99/1.02/1.02) — matches the design note's ~1µs-per-call estimate
  (2112 rows × 2 calls).
- mesh player: mean ~0.69ms/frame (0.70/0.68/0.68) — ~32% faster.
- Split of the mesh path: `render_frame()` ~0.03ms, `TouhouMesh::build`
  ~0.03ms, `draw_frame` (u32→u16 HashMap remap + macroquad Vertex
  conversion + `draw_mesh`) ~0.65ms — the conversion seam is ~95% of
  the cost; the pack itself is nearly free. The remap HashMap is
  unnecessary (pack indices are sequential quads — range chunking
  would do) and is the obvious next lever if the draw path ever
  matters; at 10k rows the seam as-is extrapolates to ~3ms.

## Remaining levers (round-21 sample, payoff order)

- Compiled tick passes ~28% of step (predicate scan + batch field
  reads/sym columns), mostly irreducible per-row reads now; sym columns
  still clone `Rc<str>` per row (a per-batch symbol-id table is the next
  representation step if it ever shows).
- Collision index capture ~14% of step (AABB build, memory-bound) —
  see the `collision-streaming` change.
- `fast_pos_pose` ~11%: called 2x/row/tick (collide fill + cull); a
  cull-time reuse of the collide pose is exact for Vel chains ONLY if
  nothing between the phases mutates n2 state or figures — needs a
  rule-effect audit before it's sound.
- Remaining interpreted rules (round 23 landed the rule-lowering
  remainder: simple cull rules compile, and-chain predicates fold, mixed
  and-chains prefilter): what's left is the beam RENDER rule
  (polyline/curve-samples emission) and compound-body rules (e.g. the
  enemy-death `(seq (event …) (cull e))`), ~11% incl of profiled reimu
  (`phase:rules`). Cheap extension recorded in the round's design.md:
  recognize `(seq …)` bodies of recognized actions.
- Milestone-B remainder (ClosedPt group pose, AxisSel scatter) is now
  JIT prep more than wall win on this rig (input slots + interning
  landed round 22 at −8%) — see `compiled-dyn-milestone-b`.
## Current walls (typed-kernel unification, 2026-07, bare)

The final same-session verdict used five interleaved A/B observations per
candidate against the pre-change `b11ec85` baseline:

| case | baseline median | typed-kernel median | delta |
|---|---:|---:|---:|
| representative suite aggregate | 373.7ms | 364.8ms | −2.38% |
| scaled fruit 12000t | 2344.2ms | 2261.9ms | −3.51% |

Both governing cases pass the ±5% threshold. The five representative
candidate walls were 360.7/360.8/364.8/364.8/365.2ms; the scaled candidate
walls were 2250.6/2269.9/2261.5/2261.9/2265.4ms.

A current candidate suite run, for standing per-card attribution:

| case | wall |
|---|---:|
| fruit (t03 ex3) 900t | 110.7ms |
| reimu_vs_mima | 126.8ms |
| spell-2 | 20.6ms |
| cradle | 49.1ms |

This round lands typed `KernelProgram`/`KernelPlan` execution plus cached CPU
artifacts for motion, dyn fields, filters, fixed updates, render projection,
and collider projection. CPU artifacts are derived from validated common
plans; they do not reintroduce private semantic evaluators.

