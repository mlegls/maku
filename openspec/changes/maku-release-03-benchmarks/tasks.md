## 1. Define Benchmark Contracts

- [x] 1.1 Define and version the declarative workload schema for entity plateau, motion/render shape, collider geometry/layers/query pairs/contact density, and `deftick` rule class/count/match rate.
- [x] 1.2 Define and version the common native/browser result envelope, raw sample records, stage/backend identities, environment metadata, correctness digest, counters, and memory fields.
- [x] 1.3 Specify warm-up, sampling, percentile, frame-budget, catch-up/clamping, allocation/memory, and failure-recording policy consistent with `openspec/specs/perf/spec.md`.
- [x] 1.4 Select and record the first reference native/browser hardware, OS, browser, resolution/DPR, power, build, and presentation configurations.

## 2. Build Deterministic Workloads

- [x] 2.1 Implement deterministic fixture generation with fixed source hash, seed, input tape, tick intervals, expected counters, and stable expanded-card output.
- [x] 2.2 Add bullet-count one-axis fixtures at continuity, 1k, 10k, 100k, and attempted 1M plateaus with controlled motion and render shape.
- [x] 2.3 Add collider fixtures varying circle/capsule-chain geometry, active layer/query pairs, and no/sparse/controlled/dense contact rates at normal and ceiling scales.
- [x] 2.4 Add `deftick` fixtures varying filter-only, render-only, masked-update, and effect/action rules at 0%, approximately 50%, and 100% match rates.
- [x] 2.5 Add representative corner fixtures and retain historical representative cards/fruit profiling as continuity cases.
- [x] 2.6 Implement semantic verification for live entities, render counters, contacts/effects, rule actions, and deterministic state digests.

## 3. Implement Native Staged Runner

- [x] 3.1 Instrument actual fixed-step frame observations with tick count and separate simulation, transport, pack-build, host-overhead, adapter/submission, and available completion/presentation durations.
- [x] 3.2 Add simulation-only and BYO transport tiers that consume typed render batches without row expansion.
- [x] 3.3 Add warmed `maku-render-touhou` construction timing and counters for layouts, layers, segments, vertices, indices, and ordered commands.
- [x] 3.4 Add the named `native-macroquad-compat` end-to-end adapter tier while separating instance expansion/remapping, CPU submission, and any measurable GPU/present completion.
- [x] 3.5 Add allocation, peak resident memory, cold setup, and bounded-failure capture without contaminating minimally instrumented wall runs.
- [x] 3.6 Emit validated result envelopes and raw samples for every native tier.

## 4. Implement Browser-Wasm Staged Runner

- [x] 4.1 Add browser automation that loads the same generated cards, seed/input tape, cadence, warm-up, and sample intervals as native.
- [x] 4.2 Add wasm simulation-only, BYO transport, and warmed Touhou pack tiers with the common stage and semantic counters.
- [x] 4.3 Add the named `web-canvas2d` tier with Canvas command/draw timing, resolution/DPR, texture-cache warm state, RAF tick count, and elapsed-time clamping recorded.
- [x] 4.4 Capture wasm linear-memory growth/peak, browser process/environment data where available, cold load/setup, and bounded failure at ceiling ramps.
- [x] 4.5 Emit and validate the same machine-readable envelope/raw sample schema as native and verify fixture/digest parity across hosts.

## 5. Compute and Present Headroom

- [x] 5.1 Implement median/p95/p99/max summaries over complete observed tick and displayed-frame samples without deriving paired frames from average tick cost.
- [x] 5.2 Compute BYO renderer headroom, bundled-pack draw headroom, and end-to-end margin against declared 120 Hz simulation and 60 Hz presentation targets, preserving negative margins and catch-up observations.
- [x] 5.3 Generate tables/curves relating logical bullets to render/collision/rule shape, incremental stage cost, memory, and last successful ceiling point.
- [x] 5.4 Add claim-generation checks requiring fixture, percentile, backend/adapter, environment, cadence, workload counters, and source revision before producing report prose.

## 6. CI and Controlled Measurement

- [x] 6.1 Add smoke-sized native/browser CI fixtures validating generation, semantic digests, stage boundaries, counters, and result-schema parsing without wall-clock failure thresholds.
- [x] 6.2 Add explicit controlled-run commands for native and browser matrices using release artifacts and recorded environment/tool versions.
- [x] 6.3 Run minimally instrumented interleaved wall-only comparisons alongside instrumented attribution runs and flag disagreements per the governing perf spec.
- [x] 6.4 Store raw baseline artifacts and summaries under a versioned retention/provenance policy.

## 7. Establish and Publish the Baseline

- [x] 7.1 Run the native and browser one-axis sweeps and representative corners at normal and attempted ceiling scales on the declared reference systems.
- [x] 7.2 Review failed/invalid samples, semantic parity, memory ceilings, Canvas/native adapter attribution, and renderer-headroom calculations before accepting results.
- [x] 7.3 Publish reproducible raw results and a user-facing report separating simulation, BYO transport, bundled Touhou construction, and concrete host drawing.
- [x] 7.4 Prepare the Bullet Hell Engines update with historical comparison, workload/environment disclosure, p95/p99 headroom, current limitations, and links to artifacts/demo.
- [x] 7.5 Use the measured bottlenecks to recommend the next OpenSpec optimization among entity representation, f32 columns, collision streaming, JIT, or GPU work without changing benchmark history.
- [ ] 7.6 Run strict OpenSpec validation and confirm the benchmark suite preserves determinism, lowering oracle, render ordering, and existing perf methodology.
