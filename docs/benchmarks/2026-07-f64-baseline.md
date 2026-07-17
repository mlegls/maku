# Maku f64 performance baseline — July 2026

This is Maku's first versioned staged baseline. It is evidence for the declared fixtures and reference machine, not a universal maximum-bullet claim.

## Reproduce and inspect

- Raw envelopes: [`bench/results/maku-v1-f64/m4-pro-macos15-chrome150/`](../../bench/results/maku-v1-f64/m4-pro-macos15-chrome150/)
- Full generated table: [`2026-07-17-summary.md`](../../bench/results/maku-v1-f64/m4-pro-macos15-chrome150/2026-07-17-summary.md)
- Machine-readable table: [`2026-07-17-summary.csv`](../../bench/results/maku-v1-f64/m4-pro-macos15-chrome150/2026-07-17-summary.csv)
- Workload/result policy: [`bench/README.md`](../../bench/README.md)

Controlled runs used an Apple M4 Pro (14 CPU cores, 24 GiB), macOS 15.6.1, release builds, AC power, 120 Hz fixed simulation, and a 60 Hz/16.667 ms displayed-frame target. Browser runs used Playwright Chromium 140.0.7339.16 at 1512×982 CSS pixels and DPR 2. Each successful result contains 30 batches of 10 complete displayed-frame observations after 120 warm-up ticks. Percentiles use nearest rank.

Most envelopes use revision `574c25e8a98ef3d04bbea7fad248f1c37068c4d5`. Three cross-host trigonometric fixtures were rerun at `c6f5559e766bcde3bf25ba67640c67ec3a72b1f1` after the semantic digest was canonicalized to f32 observable bits; all successful native/wasm fixture digests then matched. The fixture and f64 storage series remained `maku-v1-f64`.

## Bullet sweep

The stock profile emits two sprite layers and 2N ordered commands for N logical bullets.

| Fixture/tier | p95 simulation | p95 transport | p95 pack | p95 host adapter | p95 60 Hz margin | Peak memory |
|---|---:|---:|---:|---:|---:|---:|
| Native 1k BYO | 1.616 ms | 0.013 ms | — | — | +15.036 ms | 188 MB RSS |
| Native 1k Touhou pack | 1.629 ms | 0.013 ms | 0.054 ms | — | +14.973 ms | 188 MB RSS |
| Native 1k Macroquad compatibility | 1.692 ms | 0.014 ms | 0.058 ms | 5.027 ms expansion/submission | +9.986 ms | 522 MB RSS |
| Wasm 1k BYO | 2.400 ms | 0.100 ms | — | — | +14.267 ms | 79 MB linear memory |
| Wasm 1k Canvas2D | 2.900 ms | 0.100 ms | 0.200 ms | 2.200 ms draw | +11.567 ms | 79 MB linear memory |
| Native 10k BYO | 17.234 ms | 0.137 ms | — | — | −0.705 ms | 1.84 GB RSS |
| Wasm 10k BYO | 23.000 ms | 0.700 ms | — | — | −6.933 ms | 747 MB linear memory |
| Wasm 10k Canvas2D | 23.800 ms | 0.700 ms | 1.200 ms | 33.400 ms draw | −41.833 ms | 747 MB linear memory |
| Native 100k Touhou pack | 214.818 ms | 1.926 ms | 6.189 ms | — | −206.041 ms | 7.05 GB RSS |

Thus 1k is comfortably inside the displayed-frame budget on every measured tier. The 10k fixture is a useful throughput point but is not a 60 FPS claim: simulation alone misses the target on both native and wasm. Native completed 100k as an offline ceiling observation, not a real-time result. Browser 100k trapped during warm-up after the successful 10k plateau, and 1M was rejected by the recorded 24 GiB/wasm32 memory preflight.

Transport and warmed Touhou pack construction are comparatively small. The concrete compatibility adapters are not interchangeable: Canvas2D at 10k spent p95 33.4 ms drawing, while the native compatibility adapter's per-command expansion/submission exceeded the controlled 120-second run bound at 10k. Neither result describes future WebGPU throughput.

## Collision/rule corner

The normal representative corner has 10k entities, 10k render lanes, 20k sprite instances/commands, 2,500 contacts per tick, and 20k predicate matches/actions per tick.

- Native simulation p95: **82.5 ms**; BYO margin: **−66.0 ms**.
- Wasm simulation p95: approximately **103 ms**; Canvas2D adds approximately **34.4 ms** p95 draw work.
- The 100k corner exhausted pending-write fuel during warm-up and is retained as a bounded failure.
- Controlled 100k collision-only attempts exceeded the five-minute run bound; normal 1k circle/capsule and no/sparse/controlled/dense sweeps are retained in full.

All accepted samples passed exact entity/render/contact/rule counters and canonical native/wasm semantic digest parity. Invalid or timed-out ceilings are retained as bounded failures and are excluded from throughput prose.

## Interpretation and next optimization

The first priority remains **`entity-representation-flip`**. Simulation scales approximately linearly from 1k to 10k, native RSS reaches roughly 1.84 GB at 10k and 7.05 GB at 100k, and wasm reaches its practical ceiling before 100k. Reducing entity/state representation cost has greater evidence than narrowing hot columns first or moving directly to GPU execution.

After representation work, the data supports:

1. `f32-hot-columns` to reduce hot storage and browser linear-memory pressure;
2. collision streaming/pending-write work for dense/corner workloads;
3. ordered command/adapter work to avoid compatibility-host per-command expansion overhead;
4. JIT or GPU execution only after these physical costs are remeasured.

This baseline is immutable. Those changes must create a distinguishable result series or source-revision comparison rather than rewriting these walls.
