# Draft: Bullet Hell Engines progress update — July 2026

Maku now has its first reproducible native/wasm performance baseline, alongside the public player and tutorials.

The headline is deliberately workload-specific: on an Apple M4 Pro, the deterministic 1k-bullet fixture (two stock Touhou sprite layers / 2k ordered commands) had:

- native BYO transport p95: **1.63 ms** simulation+transport, leaving **15.04 ms** of the 60 Hz frame budget;
- native Touhou-pack p95: about **1.70 ms** through mesh construction, leaving **14.97 ms**;
- native Macroquad compatibility path: **+9.99 ms** p95 end-to-end margin;
- browser wasm BYO: **+14.27 ms** p95 margin;
- Chromium Canvas2D: **+11.57 ms** p95 end-to-end margin.

At 10k, simulation itself no longer meets 60 FPS in this f64/interpreter baseline: native BYO margin was **−0.71 ms**, wasm BYO **−6.93 ms**, and Canvas2D **−41.83 ms**. Native completed 100k as an offline ceiling point at roughly 215 ms p95 simulation and 7.05 GB peak RSS. Browser 100k trapped during warm-up; 1M was a recorded memory-limit preflight failure rather than a made-up throughput number.

The representative 10k render+collision+rule corner (2,500 contacts and 20k predicate actions per tick) took about 82.5 ms p95 natively and 103 ms in wasm before host drawing. Transport and warmed Touhou construction stayed relatively small; entity/state memory and simulation dominate. The compatibility renderers also expose a separate ordered-command problem: the stock two-layer fixture emits 2N commands, Canvas2D adds 33.4 ms p95 at 10k, and the native compatibility adapter exceeded the controlled run bound there. Canvas2D is not being presented as WebGPU performance.

This is not directly comparable to Maku's historical `examples/profile.rs` number: that profiler measured representative simulation wall time only, without fixed synthetic shape, browser parity, render staging, memory, or host drawing. It remains a continuity tool rather than a public apples-to-apples baseline.

Every accepted run used 120 Hz fixed simulation, a 60 Hz presentation target, 120 warm-up ticks, 30×10 complete displayed-frame samples, nearest-rank p95/p99, exact semantic counters, and matching canonical native/wasm state digests. Raw successful and bounded-failure envelopes are retained with the environment and source revision.

The measurements reinforce the planned order: **entity representation first**, then f32 hot columns, collision/pending-write streaming, and only then JIT/GPU work based on a fresh distinguishable series.

Links:

- Performance report and raw-result links: `docs/performance-baseline-2026-07.md`
- Public player: https://neen.ink/projects/maku/play.html
- Tutorials: https://neen.ink/projects/maku/tutorials.html
- Repository: https://github.com/mlegls/maku
