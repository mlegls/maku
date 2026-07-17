#!/bin/sh
# Build and test the exact registry archives before their first publication.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
version=$(cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "maku") | .version')
tmp=$(mktemp -d "${TMPDIR:-/tmp}/maku-packages.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

core_patch='patch.crates-io.maku.path="crates/core"'
pack_patch='patch.crates-io.maku-render-touhou.path="crates/render-touhou"'

cargo package -p maku --manifest-path crates/Cargo.toml --allow-dirty --no-verify
cargo package -p maku-render-touhou --manifest-path crates/Cargo.toml --allow-dirty --no-verify \
  --config "$core_patch"
cargo package -p maku-player --manifest-path crates/Cargo.toml --allow-dirty --no-verify \
  --config "$core_patch" --config "$pack_patch"
cargo package -p maku-web --manifest-path crates/Cargo.toml --allow-dirty --no-verify \
  --config "$core_patch" --config "$pack_patch"

for package in maku maku-render-touhou maku-player maku-web; do
  archive="crates/target/package/$package-$version.crate"
  test -f "$archive"
  tar -xzf "$archive" -C "$tmp"
done

cat > "$tmp/core-patch.toml" <<EOF
[patch.crates-io]
maku = { path = "$tmp/maku-$version" }
EOF
cat > "$tmp/host-patch.toml" <<EOF
[patch.crates-io]
maku = { path = "$tmp/maku-$version" }
maku-render-touhou = { path = "$tmp/maku-render-touhou-$version" }
EOF

cargo test --all-targets --offline --manifest-path "$tmp/maku-$version/Cargo.toml"
cargo test --all-targets --offline --manifest-path "$tmp/maku-render-touhou-$version/Cargo.toml" \
  --config "$tmp/core-patch.toml"
for package in maku-player maku-web; do
  cargo test --all-targets --offline --manifest-path "$tmp/$package-$version/Cargo.toml" \
    --config "$tmp/host-patch.toml"
done

# Cargo's dry run exercises normalized manifests and upload checks. Patches
# stand in only for same-version packages that do not exist until this first
# dependency-ordered publication.
cargo publish --dry-run -p maku --manifest-path crates/Cargo.toml --allow-dirty
cargo publish --dry-run -p maku-render-touhou --manifest-path crates/Cargo.toml --allow-dirty \
  --config "$core_patch"
cargo publish --dry-run -p maku-player --manifest-path crates/Cargo.toml --allow-dirty \
  --config "$core_patch" --config "$pack_patch"
cargo publish --dry-run -p maku-web --manifest-path crates/Cargo.toml --allow-dirty \
  --config "$core_patch" --config "$pack_patch"

# Validate the exact scoped browser package without requiring registry auth.
(cd crates/js/maku && bun publish --dry-run --access public)
