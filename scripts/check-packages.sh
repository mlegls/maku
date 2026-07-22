#!/bin/sh
# Build and test the exact public registry archives.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
version=$(cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "maku") | .version')
tmp=$(mktemp -d "${TMPDIR:-/tmp}/maku-packages.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

metadata=$(cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1)
publishable=$(printf '%s' "$metadata" | jq -r '.packages[] | select(.publish == null) | .name')
if [ "$publishable" != "maku" ]; then
  echo "only maku may be Cargo-publishable; found: $publishable" >&2
  exit 1
fi

cargo package -p maku --manifest-path crates/Cargo.toml --allow-dirty --no-verify
archive="crates/target/package/maku-$version.crate"
test -f "$archive"
tar -xzf "$archive" -C "$tmp"
cargo test --all-targets --all-features --offline --manifest-path "$tmp/maku-$version/Cargo.toml"
cargo publish --dry-run -p maku --manifest-path crates/Cargo.toml --allow-dirty

# Validate the exact scoped browser package without requiring registry auth.
(cd crates/js/maku && bun publish --dry-run --access public)
