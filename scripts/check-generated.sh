#!/bin/sh
# Rebuild the complete checked-in browser snapshot and reject partial drift.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
crates/web/build.sh

tracked='crates/js/maku/dist crates/js/maku/wasm crates/web/static/maku-codemirror.js'
# shellcheck disable=SC2086
if ! git diff --exit-code -- $tracked; then
  echo "generated browser bindings are stale; rebuild and commit the complete snapshot" >&2
  exit 1
fi
for required in \
  crates/js/maku/wasm/maku.js \
  crates/js/maku/wasm/maku.d.ts \
  crates/js/maku/wasm/maku_bg.wasm \
  crates/js/maku/wasm/maku_bg.wasm.d.ts \
  crates/js/maku/wasm/release.json \
  crates/js/maku/dist/index.js \
  crates/js/maku/dist/index.d.ts; do
  test -f "$required" || { echo "missing generated artifact: $required" >&2; exit 1; }
done
