# Round design (2026-08 pick-up)

Seam survey (first-hand, 2026-08-10) reframed the stub in three ways:

1. `ir-unification` already shipped width *typing* with zero width
   *variety*: `KernelType::F32`, typed registers, `F32ToF64`/`F64ToF32`
   conversion ops, width-bearing program identity, and F32 executor arms
   all exist (`lower.rs:12-40`, `sim/kernel.rs:596-660`), but no
   production lowering emits a single F32 op, and hot execution
   deliberately runs on the untyped f64 `NumProgram`
   (`motion.rs:526-535`).
2. The determinism contract is already width-parametric ("the same
   numeric width per storage class", determinism spec) — what it lacks
   is a storage-class table to point at.
3. The mesh pack (`touhou/frame.rs`) is already fully f32 `#[repr(C)]`
   with guarded f64→f32 casts at push, and `benchmark_digest` already
   canonicalizes every observable number to f32 bits. The observable
   surface tolerates f32 granularity today.

## D1 — Rounding lives at the storage boundary; arithmetic stays f64

The central decision. A storage class narrowing to f32 means: values
are **rounded once when they enter the class** and **widened on every
load**; between load and store, every tier computes in f64 with the
same ops in the same order. Consequences:

- Lowered-vs-interpreted bit-exactness is preserved *verbatim* at the
  new widths, because both tiers read the same rounded storage through
  the same accessors and do identical f64 arithmetic on it. The oracle
  stays a bit-exact assert, not a tolerance.
- The compiled-failure fallback contract ("interpreted re-run
  reproduces behavior exactly") survives unchanged.
- No parallel f32 evaluator appears inside the Val-based interpreter —
  which would be a private semantic evaluator, banned by the lowering
  spec.

Declared *compute* width per program stays F64 this round. Narrowed
compute (doubled SIMD lanes, WGSL kernels) is `jit-native-codegen` /
`gpu-kernel-backend` territory over ir-unification's typed programs;
the stub's trig argument-reduction care point (degrees→radians loses
~29 bits in f32 before `sinf` runs) transfers to those rounds — the
existing naive-f32 executor arms at `sim/kernel.rs:617-618,640` must
become reduce-in-f64 shims *when a production path first emits them*,
not now. What this round provides toward GPU is exactly what
`gpu-kernel-backend`'s proposal says it consumes: the dense f32
buffers, not a second width policy.

The entry-rounding rule is path-independent: whichever tier or path
produces a value destined for an f32 class rounds it identically
(compiled batch fill, interpreted row walk, spawn bail path). This is
what keeps every parity assert exact.

## D2 — The storage-class table

Narrow to f32 (physical storage; accessor APIs keep f64 signatures,
widening on load, rounding on store):

| class | today | becomes |
|---|---|---|
| integrator state `EntityStore::state_n2` | `Vec<Vec<[f64;2]>>` | `[f32;2]` |
| sampled poses `sampled_pose` | `[Vec<Option<Pose>>;2]` | `Option<Pose32>` |
| trace samples `TraceCache::samples` | `Vec<Pose>` | `Vec<Pose32>` |
| spawn root frames `root_frame` | `Vec<Option<Pose>>` | `Option<Pose32>` |
| capture vectors `captures` | `Rc<[f64]>` | `Rc<[f32]>` |
| user/meta num fields `WorldFields::num_values` | `Vec<Vec<Option<f64>>>` | `Option<f32>` |
| collider rows `ColliderData` | f64 center/radius/points | f32 |
| collision AABBs `CollisionIndex::aabbs` | `(f64,f64,f64,f64)` | f32 |
| render geometry (batch columns) | `NumColumn`/`Column::NumOpt` f64 | f32 |

Stays f64 (control plane and compute):

- `Val::Num`, `Form::Num`, `SigEnv` channels, `Inputs`, spawn-time pose
  math (`Pose` remains the f64 compute type everywhere).
- tau computation (integer-anchored `(tick − birth)/rate` — at f32 the
  quotient granularity degrades visibly past ~140k ticks for t-driven
  motion; tau enters programs as f64 and always will), `dt`,
  `evolve_tick`'s 1e-9 epsilon (tau stays f64 so it is untouched).
- `rand_unit_from_bits` (53-bit mantissa dependency — narrowing it
  changes the draw distribution; draws round only when stored into a
  capture vector).
- `EventLog.pos` (host-shared `Rc`, width-stable contract; filled by
  widening from f32 poses).
- `state_dyn` / `state_val` cells (cold, hold `DynPose`/`Val`).
- Per-tick compute scratch: vel-batch lanes (`tau`/`pos`/`caps`/
  `va`/`vb`), closed-pose regs and row-indexed `out`, `KernelLanes`
  f64 lanes, `NumProgram` register files. These are registers, not
  storage — narrowing them would change arithmetic, violating D1.

Wins at rest per row: `state_n2` slot 16→8B, `Option<Pose>` 32→16B
(×2 parity ring + root frame), num field cell 16→8B, capture element
8→4B, AABB 32→16B. At the 100k–1M ceiling this halves the resident
hot state and the per-tick traffic over it.

## D3 — `Pose32`

`Pose32 { x: f32, y: f32, theta: Option<f32> }` in `model/figure.rs`
with lossless-in-kind `From<&Pose>` (rounds) / `to_pose()` (widens).
Only storage columns use it; all math stays on `Pose`. `Option<f32>`
keeps the struct at 16B vs `Pose`'s 32B in `Option` — good enough; bit
packing a presence flag saves nothing further inside `Option<Pose32>`.

## D4 — Oracle asserts at the new widths

The oracle regime stays bit-exact. Asserts that compare a
freshly-computed f64 against a value that legitimately round-tripped
through an f32 class must round the *expected* side through the same
boundary (never loosen to a tolerance):

- dyn-field column bit-compare (`slots.rs:294`),
- masked-update bit-compare (`sim/mod.rs:1583`),
- cull-reused-pose exact compare (`sim/mod.rs:1338`),
- compiled-vs-interp render-row compare (`sim/mod.rs:1365`) — covered
  instead by the entry-rounding rule: both the batch fill and the
  interpreted row walk round render geometry identically, so the
  compare stays untouched (render rows keep f64 fields carrying
  f32-exact values; host row API stays type-stable).

`assert_num_close`'s 1e-9 (`motion.rs:1722`) is untouched: both sides
compute f64 from the same rounded inputs and stay bit-equal.

The spawn bail path substitutes the *rounded* draw (`draw as f32 as
f64`) so substituted constants equal what the capture vector carries —
the determinism spec's "Bail path matches compiled captures" scenario
at the new width.

## D5 — Drift is measured against the pre-round f64 build, not asserted

Bit-identity with the old build is impossible by construction; the
gate is a *meter*. The `abdump` example grows a compare mode that
reports per-card max |Δ| over positions instead of a bit-diff; run the
11-card corpus at 600 ticks against a worktree at the pre-round
commit and record the distribution in Measured and the perf spec. The
scripted release suites (`reimu_vs_mima_plays`, `duel_card_plays`,
`translations_run`, `tutorial_cards_run`) are the boundary-flip
detectors — they assert hits/grazes/kills at specific ticks, so a
collision or cull boundary flipped by ~1e-6 drift fails loudly. A
flipped outcome halts the round for investigation; it is a semantic
verdict, not noise.

Replay tapes recorded on f64 builds are not value-compatible with f32
builds (rounded draws, rounded state). Same policy as card edits:
tapes are tied to an engine version. Release-note item.

## D6 — Baseline series

`bench/README.md` already mandates it: changing hot numeric
representation starts a distinguishable series. The series id
(`maku-v1-f64` in the bench binaries and `bench/matrix-v1.json`)
becomes `maku-v1-f32` for new runs; existing raw results under
`bench/results/maku-v1-f64/` are never touched. The full controlled
browser matrix is an explicitly-invoked release activity
(scale-benchmarking spec) and is *not* re-run in this round; the
native smoke verification must pass so semantic digests verify under
f32.

`benchmark_digest` already canonicalizes to f32 bits, so its *kind* is
unchanged; any pinned digest constants in tests get re-derived (they
pin cross-tier equality at a revision, not eternal values).

## Slices

Three pi slices, each landing green on all gates (core suite plain +
`MAKU_LOWER_ORACLE=1`, release ignored oracle suites) before commit:

1. **Motion state + storage boundary**: `Pose32`; narrow `state_n2`,
   `sampled_pose`, `trace_cache`, `root_frame`, `captures`; accessor
   round/widen; bail-path rounding; cull-pose oracle expected-side
   rounding; round-trip and bail-parity tests.
2. **Num fields + collision**: narrow `WorldFields::num_values`,
   `ColliderData`, `CollisionIndex::aabbs`; dyn-field and
   masked-update oracle expected-side rounding; narrow-phase math
   widens to f64.
3. **Render geometry + bench series**: narrow batch `NumColumn`/
   `Column::NumOpt`; entry-rounding in both batch fill and row walk;
   collapse now-redundant f64→f32 casts in the touhou mesh push;
   digest pin re-derivation; bench series id bump + native smoke.

Close: drift meter over the corpus vs pre-round worktree, interleaved
wall-only A/B (suite + scaled fruit, ±5%), spec sync (determinism +
lowering + perf), Measured section, archive.

Out of scope, unchanged: `DynNode` (≤96B pin untouched), host-visible
`Rc::ptr_eq` contracts (RenderSchema, shared `EventLog`), the language,
draw keying, F32 *program emission* (no production lowering emits F32
ops this round).
