#!/bin/sh
# Authoritative local/CI validation entry point.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
mode=${1:-fast}

case "$mode" in
  fast|release) ;;
  *) echo "usage: $0 [fast|release]" >&2; exit 2 ;;
esac

run_fast() {
  scripts/check-source-tree.sh
  scripts/check-stdlib-assets.sh
  bun scripts/check-compatibility.mjs
  cargo test --workspace --all-targets --manifest-path crates/Cargo.toml
  cargo test --locked --manifest-path tests/public-api-smoke/Cargo.toml
  cargo check --workspace --target wasm32-unknown-unknown --manifest-path crates/Cargo.toml
  (cd crates/js/maku && bun run check)
}

run_fast

if [ "$mode" = release ]; then
  MAKU_LOWER_ORACLE=1 cargo test --release -p maku --manifest-path crates/Cargo.toml
  MAKU_LOWER_ORACLE=1 cargo test --release -p maku --manifest-path crates/Cargo.toml -- --ignored
  scripts/check-generated.sh
  (cd crates/web/static && bun smoke.mjs)
  scripts/check-packages.sh

  # Release verification starts and ends on committed inputs. Generated
  # browser files must therefore reproduce without a diff.
  if [ -n "$(git status --porcelain --untracked-files=all)" ]; then
    echo "release check left the source tree dirty:" >&2
    git status --short >&2
    exit 1
  fi
fi
