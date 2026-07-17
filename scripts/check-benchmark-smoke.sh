#!/bin/sh
# Structural/semantic benchmark smoke only; never applies wall-clock thresholds.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/maku-bench-smoke.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

for tier in simulation-only byo-transport touhou-pack; do
  cargo run --quiet --manifest-path crates/Cargo.toml -p maku-bench \
    --bin maku-bench-native -- \
    bench/workloads/v1/bullets-continuity.json \
    --tier "$tier" --output "$tmp/$tier.json" --smoke
done
bun scripts/check-benchmarks.mjs "$tmp"/*.json
