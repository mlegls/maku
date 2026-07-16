## Why

Maku has gained orders of magnitude in throughput, but the current profiler cannot support reproducible claims about maximum bullets, collision workloads, rule density, native-versus-wasm cost, or renderer headroom. A public performance update needs deterministic workloads and staged measurements that distinguish engine transport, the bundled Touhou render pack, and host drawing.

## What Changes

- Add versioned deterministic workload generators for live-bullet scale, collider geometry/layers/query pairs/contact density, and `deftick` rule count/type/match rate.
- Measure simulation-only, bring-your-own-renderer transport, bundled Touhou mesh construction, and actual native/Canvas host drawing as separate cumulative tiers.
- Provide native and browser-wasm runners over identical cards, seeds, input tapes, warm-up, sampled intervals, and correctness invariants.
- Report logical entities together with render lanes, sprite instances/layers, beam segments, vertices, triangles, draw commands, collider projections/queries/contacts, and rule match counts.
- Report median, p95, p99, maxima, allocations, resident/linear memory, build mode, hardware, OS, browser, resolution/DPR, presentation policy, and source revision in a machine-readable result format.
- Define 120 Hz simulation and 60 Hz presentation budgets and compute measured paired-tick renderer headroom rather than inferring it from average tick cost.
- Preserve the existing interleaved wall-only A/B and oracle methodology for optimization comparisons while adding stable scale and cross-host baselines.
- Add smoke-sized CI validation for benchmark fixtures and separate explicitly invoked measurement runs that do not turn noisy wall-clock thresholds into ordinary CI failures.
- Produce a reproducible report suitable for the project site and Bullet Hell Engines community update, clearly labeling Canvas2D versus native GPU hosts and avoiding claims that a BYO renderer was measured when rows were expanded or drawn.

## Capabilities

### New Capabilities
- `scale-benchmarking`: Deterministic workload definitions, staged native/wasm benchmark runners, result schema, correctness gates, frame-budget/headroom calculations, and publication requirements.

### Modified Capabilities
- `perf`: Extend performance evidence from implementation-delta A/B measurements to versioned scale baselines and renderer-headroom reporting without weakening existing attribution rules.

## Impact

- Benchmark fixtures/cards, core profiling tools, native player instrumentation, wasm/browser automation, Touhou render-pack instrumentation, result artifacts, and performance documentation.
- Provides evidence for prioritizing `entity-representation-flip`, `f32-hot-columns`, `collision-streaming`, `jit-native-codegen`, and `gpu-kernel-backend`; it does not presuppose which backend wins.
- Governing correctness remains `openspec/specs/determinism/spec.md`, `openspec/specs/lowering/spec.md`, `openspec/specs/render-rows/spec.md`, `openspec/specs/mesh-renderer-api/spec.md`, and the existing methodology in `openspec/specs/perf/spec.md`.
