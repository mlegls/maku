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
if command -v xvfb-run >/dev/null 2>&1; then display=xvfb-run; else display=; fi
$display cargo run --quiet --manifest-path crates/Cargo.toml -p maku-bench \
  --bin maku-bench-native-draw -- \
  bench/workloads/v1/bullets-continuity.json \
  --tier native-macroquad-compat --output "$tmp/native-macroquad-compat.json" --smoke
bun scripts/check-benchmarks.mjs "$tmp"/*.json
