# f32 hot columns (mixed numeric width)

## Why

Mixed numeric width is the contract, not blanket f64 (decided 2026-07).
f64 was Val/EDN inheritance, never load-bearing: determinism means same
ops/order/width per tier, and replay compatibility — both restate over
f32. f32 halves resident hot state and the bandwidth over it (the
profile's top fixed cost) and provides the dense f32 buffers
`gpu-kernel-backend` consumes (WGSL has no f64). Needed for the 100k+
scale tier.

## What Changes

- Hot storage classes narrow to f32: integrator state, sampled/trace/
  root-frame poses, capture vectors, user/meta num fields, collider
  rows and AABBs, render batch geometry. Control plane (interpreter,
  Val, channels, tau, RNG bit-to-float, spawn-time pose math) stays
  f64.
- Rounding happens once at the storage boundary (entry into the
  class); every load widens; all arithmetic between load and store
  stays f64 with unchanged ops/order — so lowered-vs-interpreted
  bit-exactness is preserved verbatim at the new widths and the oracle
  stays a bit-exact gate. Oracle asserts whose expected side is
  freshly-computed f64 round it through the same boundary.
- Compute width per program remains F64; F32 program emission, SIMD
  lane doubling, and GPU math shims stay with `jit-native-codegen` /
  `gpu-kernel-backend` over `ir-unification`'s typed programs.
- Drift vs the pre-round f64 build is measured (corpus meter +
  scripted behavioral suites as boundary-flip detectors), not
  asserted; the bench baseline series forks (`maku-v1-f64` →
  `maku-v1-f32`) preserving prior evidence.

## Capabilities

Numeric-width contract change; determinism spec gains the
storage-class table; lowering spec gains the physical-width
requirement; perf/bench series forks.

## Impact

- `crates/core/src/interp/world.rs` (columns + accessors),
  `model/figure.rs` (`Pose32`), `model/colliders.rs`,
  `model/renderers.rs`, `sim/{mod,slots,collision,render}.rs` oracle
  expected-side rounding, `interp/spawn.rs` bail path,
  `crates/bench` series id.
- Governing: `openspec/specs/determinism/spec.md`,
  `openspec/specs/lowering/spec.md`, `openspec/specs/perf/spec.md`,
  `bench/README.md` series rule.
