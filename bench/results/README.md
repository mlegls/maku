# Retained benchmark results

Accepted controlled baselines live under:

```text
bench/results/<series>/<environment-id>/<source-revision>/<run-id>/
```

Each run directory retains the immutable workload matrix, environment record,
full source revision, every raw v1 result envelope, `summary.csv`, and
`summary.md`. Failed ceiling attempts are retained as bounded-failure envelopes
alongside the last successful plateau; successful files are never replaced in
place.

`maku-v1-f64` identifies the current f64 physical storage contract. A workload
schema, result schema, fixture semantics, or physical numeric contract change
starts a new series. Historical results remain available rather than being
regenerated under their old identity.

Ordinary CI creates temporary smoke results only. Use
`scripts/run-benchmark-matrix.sh` from a clean worktree for controlled runs,
then review semantic validity and attribution before copying the run directory
here. Published prose must be generated through
`scripts/summarize-benchmarks.mjs`, whose claim gate rejects dirty revisions,
invalid semantics, missing p95/p99 distributions, or incomplete workload and
environment identity.
