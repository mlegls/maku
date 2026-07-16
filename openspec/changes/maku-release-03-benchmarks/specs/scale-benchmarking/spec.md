## ADDED Requirements

### Requirement: Versioned deterministic workload definitions
The benchmark suite SHALL define versioned deterministic workloads with fixed source/generator hash, seed, input tape, tick interval, warm-up, and sample interval. Workloads SHALL independently vary live entities, motion/render shape, collider geometry/layers/query pairs/contact density, and `deftick` rule class/count/match rate.

#### Scenario: Repeated fixture generation
- **WHEN** the same fixture version and parameters are generated on native and browser runners
- **THEN** expanded source hashes and declared expected semantic counters are identical

### Requirement: Semantic correctness accompanies timing
Every benchmark sample set SHALL verify deterministic state digest and expected ranges or exact values for live entities, render output, collision contacts/effects, and rule actions. A timing result with failed semantic verification MUST NOT be included in a performance claim.

#### Scenario: Faster but incorrect collider run
- **WHEN** a candidate backend omits contacts and completes under budget
- **THEN** the result is marked invalid rather than reported as improved collider throughput

### Requirement: Staged renderer measurements
The suite SHALL measure simulation, BYO render transport, bundled Touhou render-pack construction, and named host drawing as distinct incremental and cumulative stages over the same workload. BYO transport SHALL consume typed batches without semantic row expansion; cold schema/profile/resource setup SHALL be reported separately from warmed steady state.

#### Scenario: BYO renderer result
- **WHEN** reporting renderer headroom for a custom frontend
- **THEN** the measured cost includes actual tick advancement and `render_frame()` transport but excludes `TouhouMesh::build()` and host drawing

#### Scenario: Bundled render-pack result
- **WHEN** reporting the bundled Touhou path
- **THEN** transport and warmed pack construction are reported separately and together before any Macroquad or Canvas adapter cost

### Requirement: Measured frame-budget headroom
For a presentation target, the runner SHALL record actual simulation ticks advanced per displayed frame and stage durations for that frame. BYO and bundled-renderer headroom MUST be computed from measured complete-frame distributions, including catch-up frames, rather than multiplying an average or median tick duration.

#### Scenario: Nominal 120 Hz simulation and 60 Hz presentation
- **WHEN** a displayed frame advances two ticks
- **THEN** BYO headroom is the 16.667 ms period minus measured simulation, transport, and declared non-render host cost for that observation

#### Scenario: Catch-up frame
- **WHEN** scheduler delay causes more than two ticks or elapsed-time clamping
- **THEN** the observation records its actual tick count and dropped/clamped wall time instead of being folded into a nominal two-tick estimate

### Requirement: Workload shape accompanies bullet count
Results and public claims SHALL report logical live bullets together with applicable render lanes, sprite instances/layers, beam segments, vertices, indices/triangles, commands, collider projections, active query pairs, candidates/contacts, rules by class, and predicate/action counts. A maximum bullet count MUST name its fixture and renderer tier.

#### Scenario: Multi-layer sprite profile
- **WHEN** one logical bullet emits multiple sprite layers
- **THEN** the result reports both one bullet and the resulting instance/layout/command counts

### Requirement: Comparable native and browser result schema
Native and browser runners SHALL emit one versioned machine-readable envelope containing raw samples, summaries, fixture identity, correctness digest, stage/backend identity, build/tool versions, source revision, hardware/OS/browser/GPU, resolution/DPR, timing policy, allocation/memory metrics, and warm-up/sample configuration.

#### Scenario: Native-versus-wasm comparison
- **WHEN** results are compared across native and browser wasm
- **THEN** the report can verify identical fixture identity and semantic digest while retaining distinct executor and presentation-adapter labels

### Requirement: Sparse scale matrix covers governing axes
The maintained suite SHALL include one-axis sweeps and representative corners rather than relying on one aggregate card or an uncontrolled full factorial. It SHALL cover normal 10k scale and attempt 100k and 1M ceiling points, recording bounded failure or memory exhaustion as outcomes; collision fixtures SHALL include no/sparse/controlled/dense contacts, and rule fixtures SHALL include 0%, approximate 50%, and 100% matches across filter, render, update, and effect classes.

#### Scenario: Collider query sweep
- **WHEN** collider scaling is measured
- **THEN** entity count, geometry, active layers/query pairs, and contact density are controlled and reported independently

#### Scenario: Ceiling cannot complete
- **WHEN** a runner cannot reach 1M because of a declared memory or platform limit
- **THEN** it records the last successful point and failure class without inventing a throughput value

### Requirement: Presentation adapters are named precisely
End-to-end results SHALL identify the concrete presentation adapter, including `native-macroquad-compat`, `web-canvas2d`, or a future WebGPU adapter. CPU submission and GPU completion/presentation timing SHALL be distinguished where available, and Canvas2D MUST NOT be labeled as WebGPU or generic wasm renderer throughput.

#### Scenario: Canvas dominated frame
- **WHEN** Canvas drawing exceeds the frame budget while BYO transport remains below it
- **THEN** the report presents the remaining BYO headroom and the separate Canvas limitation rather than attributing the total to wasm simulation

### Requirement: CI validates harness correctness without noisy performance gates
Ordinary CI SHALL run smoke-sized native and browser fixtures validating generation, stage boundaries, counters, result schema, and semantic parity. Controlled release benchmarks SHALL be explicitly invoked under recorded conditions; machine wall-clock thresholds MUST NOT make normal CI flaky.

#### Scenario: Pull request benchmark smoke
- **WHEN** CI runs on an unpinned shared worker
- **THEN** it validates benchmark outputs structurally and semantically but does not fail solely because a timing percentile differs from a workstation baseline
