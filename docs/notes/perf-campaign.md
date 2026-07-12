# Perf campaign: rig, methodology, and status

Standing reference for the ongoing performance campaign (rounds 7-22
landed as of 2026-07; narrative lives in git history, not here). Open
perf workstreams are tracked as OpenSpec changes under
`openspec/changes/`.

## Measurement rig

- Bare walls: `MAKU_WALL_ONLY=1 cargo run --release --example profile`
  (workspace at `proto/`; add `--manifest-path proto/core/Cargo.toml`
  when not cd'd there). The flat profiler's own bookkeeping is ~18% on
  dense cards, so wall-only mode is the number that counts.
- No args runs the card suite (paths are CARGO_MANIFEST_DIR-relative).
- Scaled case: `profile cards/tutorials/t03.maku ex3-fruit-colors 12000`
  (the "scaled fruit rig" — 12k ticks of the densest tutorial pattern).
- macOS `sample <pid> <secs> -file out.txt` on a release binary built
  with `CARGO_PROFILE_RELEASE_DEBUG=true` is ground truth for where time
  goes; aggregate children of a stack anchor when comparing.

## Methodology rules (learned the hard way)

- **Interleaved A/B runs are required for deltas.** Machine-state drift
  makes a single baseline number misleading (observed: 2508 vs 2610ms on
  the same commit across a session). Alternate baseline/candidate runs
  in one sitting and compare medians.
- **Profiled walls can mask deltas.** Probe overhead can dominate and
  equalize builds whose wall-only times differ by 60%. Wall-only is the
  verdict; profiled/sample runs are for attribution only.
- **Struct sizes on hot enums are load-bearing.** Growing `DynNode`
  88→120 bytes cost ~60% wall (every pose-chain walk chases these
  enums). New per-variant data goes behind `Option<Rc<..>>` (one word);
  the `dyn_node_stays_small` test pins `size_of::<DynNode>() <= 96`.

## Verification gates (every round)

Normative surface: `openspec/specs/determinism/spec.md`.

1. `cargo test --release --manifest-path proto/core/Cargo.toml` — full
   core unit suite green.
2. `MAKU_LOWER_ORACLE=1 cargo test --release --manifest-path
   proto/core/Cargo.toml -- --ignored` — the 4 ignored card suites,
   which run every lowered program against the interpreter bit-exactly.
3. Verify first-hand (not from a subagent's report) before committing.

## Current walls (round 22, 2026-07, bare)

| case | wall | note |
|---|---|---|
| fruit (t03 ex3) 900t | 121.2ms | 5050ms at round 7 — 42x |
| scaled fruit 12000t | 2.40s | 2.61s at round-22 start (−8%); 16.9s at round 15 |
| reimu_vs_mima | ~132ms | |
| spell-2 | 22.4ms | |
| cradle | 48.8ms | |

Round 21 = milestone-C SoA render output (render-output-design.md);
round 22 = input slots + structural interning (capture vectors over
marker programs) plus the mesh renderer pack (maku-mesh-touhou; player
extracted to `proto/player`).

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
- Remaining interpreted rule scans (`evaluate_list_inner` ~8% — beam/
  cull/hp rules) — see the `rule-lowering-remainder` change.
- Milestone-B remainder (ClosedPt group pose, AxisSel scatter) is now
  JIT prep more than wall win on this rig (input slots + interning
  landed round 22 at −8%) — see `compiled-dyn-milestone-b`.
