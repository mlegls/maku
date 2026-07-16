## Context

The existing `crates/core/examples/profile.rs` measures simulation wall time for representative cards, and `openspec/specs/perf/spec.md` governs interleaved A/B optimization evidence. It does not provide deterministic scale sweeps, browser automation, renderer staging, memory results, or a machine-readable schema. Native and web hosts also have very different draw adapters: Macroquad currently expands instances for compatibility, while Canvas loops over sprites and ribbon triangles. Comparing only aggregate frame time would conflate engine, pack, adapter, rasterizer, and presentation behavior.

The current hosts run a 120 Hz fixed simulation and normally present at 60 Hz without interpolation. One displayed frame usually advances two ticks and builds one latest-state render frame, but catch-up frames can advance more. Benchmarking must measure that actual cadence rather than multiplying an average tick.

## Goals / Non-Goals

**Goals:**
- Produce repeatable normal-scale and ceiling-scale measurements across native and browser wasm.
- Attribute cost to simulation, render transport, bundled Touhou construction, host adapter/draw, and memory.
- Quantify renderer headroom for bring-your-own-renderer users.
- Exercise bullets, collider layer/query/contact shapes, and `deftick` rule density independently and in representative corners.
- Preserve semantic correctness while measuring and generate results suitable for automated comparison and public reporting.

**Non-Goals:**
- Promise one universal maximum bullet number independent of card, hardware, renderer, or collision/rule shape.
- Treat Canvas2D as representative of WebGPU or native GPU rendering.
- Add noisy wall-clock pass/fail thresholds to ordinary CI.
- Implement the optimizations whose need the suite evaluates.
- Benchmark row expansion as the canonical BYO renderer path.

## Decisions

### Use deterministic generated fixtures plus continuity cards

A fixture generator emits versioned cards/configurations from a declarative workload description and fixed seed/input tape. Families independently vary live entity plateau, motion complexity, render shape, collider geometry/layers/query pairs/contact density, and rule type/count/match rate. Existing representative cards and the historical fruit case remain continuity points, but synthetic fixtures make one-axis attribution possible.

Every sample verifies expected live entities, render lanes, contacts/effects, and deterministic state digest. Generation inputs and expanded card hashes are recorded with results.

### Measure cumulative tiers with explicit boundaries

For the same sampled displayed frame:

1. simulation-only times the actual `advance()` calls;
2. BYO transport additionally calls `render_frame()` and consumes typed batches without expansion or drawing;
3. bundled construction additionally calls warmed `TouhouMesh::build()`;
4. host draw additionally performs native adapter/submission or Canvas rendering.

Each tier reports both incremental and cumulative cost. Schema/profile binding, texture creation, and shader/variant cache warm-up are measured separately as cold-start data and excluded from warmed steady-state headroom.

### Define headroom from measured displayed frames

At target presentation period `P`, each frame records actual tick count `n`, simulation duration `S`, transport `T`, optional pack build `B`, non-render host overhead `H`, and draw adapter duration `D`.

- BYO renderer CPU headroom is `P - S - T - H`.
- Bundled-host draw headroom is `P - S - T - B - H`.
- End-to-end margin is `P - S - T - B - H - D`.

Claims use p95 or p99 distributions of complete paired/catch-up frame observations, never `2 × median_tick`. Negative margins and elapsed-time clamping are reported as missed budget/dropped wall time rather than hidden.

### Report workload shape, not only bullets

Every result includes logical live entities, emitted rows/batch lanes, sprite instances by fixed layout, beam segments/layers, vertices, indices/triangles, draw commands, projected colliders, active layer pairs, query count, candidate/contact count, rules by class, and predicate match/action counts. A public claim names the relevant counts and workload fixture.

### Use one schema across native and browser runners

A versioned JSON result envelope contains fixture/version/hash, source revision, backend/tier, build flags, durations, counters, allocation/memory data, correctness digest, hardware/OS/browser/GPU, resolution/DPR, timing policy, warm-up, sample count, and tool versions. Native and browser runners emit the same envelope. Raw samples remain available so summaries can be recomputed.

### Separate CI correctness from controlled performance runs

CI uses small fixtures to validate generation, counters, stage boundaries, result schema, native/browser execution, and semantic parity. Controlled release runs use release builds, pinned browser/toolchain, stable power state, recorded hardware, warm-up, at least 30 fixed-size sample batches, and interleaved baseline/candidate ordering where comparing implementations. Wall-clock regressions remain governed by `openspec/specs/perf/spec.md`; profile/sample output is attribution only.

### Build a sparse matrix, not a full factorial

Primary one-axis sweeps vary bullets, collision shape, and rules while holding others at a declared baseline. Representative corners combine normal 10k and ceiling 100k/1M scenarios. Contact density includes none/sparse/controlled/dense variants. Rule fixtures separate filter-only, render-only, masked update, and effect/action work at 0%, approximately 50%, and 100% matches. Unsupported or memory-exhausting ceilings are valid recorded outcomes.

### Treat renderer adapters as named backends

Results identify `native-macroquad-compat`, `web-canvas2d`, and future adapters such as `web-webgpu`; they do not collapse them into `native` and `wasm`. GPU completion/present timing is separate from CPU submission when available. The compute executor (interpreter/IR/native-JIT/GPU) and presentation adapter are orthogonal result fields.

## Risks / Trade-offs

- [Synthetic fixtures produce impressive but irrelevant peaks] → Pair one-axis fixtures with representative cards and publish full workload shape.
- [Browser scheduling noise overwhelms small differences] → Use batched observations, raw distributions, pinned environments, and avoid CI wall thresholds.
- [Instrumentation changes performance] → Keep counters preallocated/optional, compare instrumented and minimally instrumented walls, and use external profiling for attribution.
- [Maximum claims become hardware folklore] → Require machine/build/browser metadata and phrase results as fixture-specific headroom curves.
- [Canvas dominates and hides wasm engine gains] → Publish BYO transport and pack-build tiers independently from Canvas end-to-end.
- [Memory ceilings crash runners] → Ramp capacities, record last successful point and failure class, and isolate browser runs.
- [Future f32/entity/backend changes invalidate baselines] → Version fixtures/result schema and retain source revisions; changed physical contracts create new baseline series rather than rewriting old results.

## Migration Plan

1. Specify the workload and result schemas and preserve the historical profiler as a continuity runner.
2. Implement native simulation/BYO/pack tiers and validate stage counters.
3. Add collision and rule fixture families plus correctness digests.
4. Add browser-wasm simulation/BYO/pack automation and raw result export.
5. Instrument named host draw adapters and memory metrics.
6. Run normal/ceiling baselines, review claims for methodological accuracy, and publish raw plus summarized results.
7. Use profiles and headroom curves to select the next optimization change.

## Open Questions

- Exact reference hardware/browser matrix for the first public report.
- Whether native GPU completion timestamps are available through the compatibility player or require a later direct backend adapter.
- Practical upper memory limit for browser ceiling runs and whether 1M is reported as a completed point or bounded failure.
- Storage location and retention policy for raw benchmark result artifacts.
