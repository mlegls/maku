## ADDED Requirements

### Requirement: Scale baselines are versioned evidence
Standing scale baselines SHALL retain raw machine-readable samples, workload/schema versions, source revision, environment metadata, correctness status, and summarized distributions. A changed fixture, physical numeric contract, or result schema SHALL begin a distinguishable baseline series rather than silently rewriting historical walls.

#### Scenario: f32 storage lands after baseline
- **WHEN** hot-column width changes from the baseline's physical contract
- **THEN** new measurements identify a new baseline series while preserving the prior f64 evidence and fixture definition

### Requirement: Public performance claims state scope and percentile
A public throughput or frame-headroom claim SHALL name the workload, entity/render/collision/rule shape, executor, presentation adapter or BYO tier, hardware/browser/build context, target cadence, and statistic used. Maximum-count claims MUST use a declared percentile or complete sampled distribution and MUST NOT generalize one fixture to all cards.

#### Scenario: Community performance update
- **WHEN** a report says Maku handles a stated bullet count without dropping frames
- **THEN** it also states the measured p95 or p99 frame margin, renderer tier, workload shape, and environment needed to reproduce that statement

### Requirement: Staged walls do not replace interleaved delta methodology
Scale sweeps and staged renderer measurements SHALL complement, not replace, interleaved wall-only A/B runs for attributing implementation changes. Profiled or instrumented stage timing remains attribution evidence and MUST be checked against minimally instrumented end-to-end walls before declaring a regression or improvement.

#### Scenario: Instrumented stage improves but total wall regresses
- **WHEN** counters show one stage becoming faster while an interleaved minimally instrumented run becomes slower
- **THEN** the change is reported as an end-to-end regression pending further attribution
