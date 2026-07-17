#!/bin/sh
# Opt-in allocation attribution build; normal/minimal wall binaries do not
# include the counting allocator.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd); cd "$repo_root"
workload=${1:-}; tier=${2:-}; environment=${3:-}; output=${4:-}
[ -f "$workload" ] && [ -f "$environment" ] && [ -n "$output" ] || { echo "usage: $0 WORKLOAD TIER ENVIRONMENT OUTPUT" >&2; exit 2; }
revision=$(jq -r .source_revision crates/js/maku/wasm/release.json)
target=$(mktemp -d "${TMPDIR:-/tmp}/maku-alloc-target.XXXXXX"); trap 'rm -rf "$target"' EXIT HUP INT TERM
binary=maku-bench-native; [ "$tier" = native-macroquad-compat ] && binary=maku-bench-native-draw
MAKU_SOURCE_REVISION="$revision" CARGO_TARGET_DIR="$target" cargo build --release --locked --manifest-path crates/Cargo.toml -p maku-bench --features allocation-counting --bin "$binary"
"$target/release/$binary" "$workload" --tier "$tier" --environment "$environment" --output "$output"
bun scripts/check-benchmarks.mjs "$output"
jq -e '.memory.allocations != null and .memory.allocated_bytes != null' "$output" >/dev/null
