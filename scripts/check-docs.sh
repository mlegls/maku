#!/bin/sh
# Release-check runnable documentation examples and local links.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"

bun scripts/check-doc-links.mjs
cargo test --locked --manifest-path tests/public-api-smoke/Cargo.toml
cargo test --locked --manifest-path crates/Cargo.toml -p maku --lib \
  sim::tests::tutorial_cards_run -- --ignored --exact --test-threads=1
(cd crates/js/maku && bun run check)

echo "documentation examples OK"
