#!/bin/sh
# Browser/Wasm structural and semantic smoke; no wall-clock thresholds.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/maku-browser-bench.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
workload=bench/workloads/v1/bullets-continuity.json
for tier in simulation-only byo-transport touhou-pack web-canvas2d; do
  bun scripts/run-browser-benchmark.mjs "$workload" --tier "$tier" --output "$tmp/browser-$tier.json" --smoke
done
cargo run --quiet --manifest-path crates/Cargo.toml -p maku-bench --bin maku-bench-native -- \
  "$workload" --tier touhou-pack --output "$tmp/native.json" --smoke
bun scripts/check-benchmarks.mjs "$tmp"/*.json
native_digest=$(jq -r '.correctness.state_digest' "$tmp/native.json")
wasm_digest=$(jq -r '.correctness.state_digest' "$tmp/browser-touhou-pack.json")
[ "$native_digest" = "$wasm_digest" ] || { echo "native/wasm digest mismatch: $native_digest != $wasm_digest" >&2; exit 1; }
