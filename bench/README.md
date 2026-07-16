# Maku scale benchmarks

This directory contains the versioned workload and result contracts used by the native and browser benchmark runners. Benchmark results are evidence for one source revision and environment, not universal bullet-count promises.

## Versioned contracts

- `schema/v1/workload.schema.json`: declarative fixture axes and semantic expectations.
- `schema/v1/result.schema.json`: common native/browser envelope and raw displayed-frame observations.
- `environments/*.json`: sanitized reference-system records.
- `workloads/v1/*.json`: maintained one-axis and representative-corner matrix.
- `results/v1/<series>/<revision>/`: immutable raw runs, summaries, and provenance.

Changing fixture semantics, hot numeric representation, or either schema starts a distinguishable baseline series. Existing raw results are never rewritten.

## Measurement policy

- Build release artifacts from a clean recorded source revision. Cold load, schema/profile binding, texture creation, capacity growth, and cache warm-up are reported separately.
- Run warm-up before sampling. Controlled baselines use at least 30 fixed-size batches; CI uses smaller structural smoke runs and has no wall-clock threshold.
- A displayed-frame sample records its actual tick count and each observed stage. At 120 Hz simulation and 60 Hz presentation, the nominal period is 16.667 ms, but catch-up frames are not rewritten as two average ticks.
- Browser elapsed time is clamped only by the declared host policy. Both clamped/dropped time and actual ticks are retained.
- Summaries use nearest-rank median, p95, p99, and max over complete observations. Raw samples are authoritative.
- BYO headroom is `period - simulation - transport - host overhead`; bundled draw headroom additionally subtracts warmed pack construction; end-to-end margin additionally subtracts adapter/submission. Negative margins remain negative.
- Typed render batches are consumed directly in the BYO tier. Semantic row expansion is not a BYO transport measurement.
- Memory records native peak RSS and browser wasm linear-memory start/peak where available. Allocation counting is an opt-in attribution run and is not enabled in minimally instrumented wall runs.
- Unsupported ceilings, memory exhaustion, timeout, load/step error, and semantic mismatch are bounded outcomes with the last successful plateau; they are not discarded or converted into throughput values.
- Every accepted sample set verifies fixture identity, expanded-source hash, live entities, render/collision/rule expectations, and deterministic state digest.

## Performance claims

Generated report prose requires a fixture, percentile, executor, tier/adapter, cadence, workload counters, environment, and source revision. Instrumented stage attribution must agree with same-session minimally instrumented wall behavior. Optimization deltas continue to follow `openspec/specs/perf/spec.md`: interleaved `MAKU_WALL_ONLY=1` A/B runs are the verdict.
