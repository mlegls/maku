#!/bin/sh
# Keep every consumer pointed at the singular crate-local standard library.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
tmp=${TMPDIR:-/tmp}/maku-stdlib-check.$$
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp"

find crates/core/lib -maxdepth 1 -type f -name '*.maku' -exec basename {} \; | sort > "$tmp/actual"
sed -n 's/.*include_str!("\.\.\/lib\/\([^"]*\.maku\)").*/\1/p' crates/core/src/edn.rs | sort > "$tmp/embedded"
sed -n "s/.*'crates\/core\/lib\/\([^']*\.maku\)'.*/\1/p" crates/web/static/manifest.js | sort > "$tmp/web"

for consumer in embedded web; do
  if ! cmp -s "$tmp/actual" "$tmp/$consumer"; then
    echo "standard-library $consumer list is stale:" >&2
    diff -u "$tmp/actual" "$tmp/$consumer" >&2 || true
    exit 1
  fi
done
