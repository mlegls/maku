# Maku benchmark reports

This directory is the durable index for Maku performance baselines, analyses,
and publication-ready progress updates. Reports are dated and remain immutable
once published so future representation, numeric-width, runtime, and renderer
changes can be compared without rewriting historical results.

## Current baseline

- [`2026-07-f64-baseline.md`](2026-07-f64-baseline.md) — first staged native,
  wasm, Touhou-pack, Macroquad compatibility, and Canvas2D baseline.
- [`2026-07-progress-update.md`](2026-07-progress-update.md) — concise community
  update derived from that baseline.

Raw result envelopes, generated tables, schemas, fixtures, and reproduction
commands live under [`bench/`](../../bench/). The measurement and claim policy
is documented in [`bench/README.md`](../../bench/README.md).

## Adding a report

1. Keep raw result envelopes under a distinguishable series in
   `bench/results/`.
2. Add a dated report here rather than replacing an earlier baseline.
3. Link the exact result series, source revisions, environment, workload shape,
   cadence, and reported percentiles.
4. Separate simulation, transport, render-pack, host-adapter, completion, and
   presentation costs where the measured tier exposes them.
5. Record bounded failures and negative headroom; do not turn ceiling attempts
   into throughput claims.
6. Add the report to this index and update the root README when it becomes the
   current public baseline.
