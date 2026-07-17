#!/bin/sh
# Explicit controlled baseline runner. Ordinary CI uses the smoke scripts.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"

host=${1:-}
environment=${2:-}
out=${3:-}
groups=${4:-bullet_sweep,collision_normal,collision_ceiling,rules,corners}
if [ "$host" != native ] && [ "$host" != browser ] && [ "$host" != all ]; then
  echo "usage: $0 native|browser|all ENVIRONMENT.json OUTPUT_DIR [comma-separated-groups]" >&2; exit 2
fi
[ -f "$environment" ] || { echo "missing environment record: $environment" >&2; exit 2; }
[ -n "$out" ] || { echo "output directory is required" >&2; exit 2; }
[ -z "$(git status --porcelain --untracked-files=all)" ] || { echo "controlled runs require a clean worktree" >&2; exit 1; }
revision=$(jq -r .source_revision crates/js/maku/wasm/release.json)
case "$revision" in ????????*) ;; *) echo "invalid browser release revision" >&2; exit 1;; esac
mkdir -p "$out"
cp "$environment" "$out/environment.json"
cp bench/matrix-v1.json "$out/matrix.json"
printf '%s\n' "$revision" > "$out/source-revision.txt"

ids=$(printf '%s' "$groups" | tr ',' '\n' | while IFS= read -r group; do jq -r --arg group "$group" '.workloads[$group][]' bench/matrix-v1.json; done)
if [ "$host" = native ] || [ "$host" = all ]; then
  MAKU_SOURCE_REVISION="$revision" cargo build --release --locked --manifest-path crates/Cargo.toml -p maku-bench --bin maku-bench-native --bin maku-bench-native-draw
  for id in $ids; do
    for tier in $(jq -r '.native_tiers[]' bench/matrix-v1.json); do
      result="$out/native-$id-$tier.json"
      binary=maku-bench-native; [ "$tier" = native-macroquad-compat ] && binary=maku-bench-native-draw
      if ! MAKU_SOURCE_REVISION="$revision" "crates/target/release/$binary" "bench/workloads/v1/$id.json" --tier "$tier" --environment "$environment" --output "$result" 2>"$result.stderr"; then
        bun scripts/write-benchmark-failure.mjs "bench/workloads/v1/$id.json" native "$tier" "$environment" "$result" runtime-error "$(cat "$result.stderr")"
      fi
      rm -f "$result.stderr"
    done
  done
fi
if [ "$host" = browser ] || [ "$host" = all ]; then
  for id in $ids; do
    for tier in $(jq -r '.browser_tiers[]' bench/matrix-v1.json); do
      result="$out/browser-$id-$tier.json"
      if ! bun scripts/run-browser-benchmark.mjs "bench/workloads/v1/$id.json" --tier "$tier" --environment "$environment" --output "$result" 2>"$result.stderr"; then
        bun scripts/write-benchmark-failure.mjs "bench/workloads/v1/$id.json" browser "$tier" "$environment" "$result" runtime-error "$(cat "$result.stderr")"
      fi
      rm -f "$result.stderr"
    done
  done
fi
bun scripts/check-benchmarks.mjs "$out"/*.json
bun scripts/summarize-benchmarks.mjs "$out"/*.json --output "$out/summary.md" --csv "$out/summary.csv"
