#!/usr/bin/env bash
# Idempotently publish the coordinated Rust and npm packages for this checkout.
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"

version=$(cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "maku") | .version')
npm_name=$(jq -r '.name' crates/js/maku/package.json)
npm_version=$(jq -r '.version' crates/js/maku/package.json)

if [[ "$npm_version" != "$version" ]]; then
  echo "Rust version $version does not match $npm_name version $npm_version" >&2
  exit 1
fi

mismatched=$(
  cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1 \
    | jq -r --arg version "$version" '.packages[] | select(.name | startswith("maku")) | select(.version != $version) | "\(.name) \(.version)"'
)
if [[ -n "$mismatched" ]]; then
  printf 'coordinated package version mismatch:\n%s\n' "$mismatched" >&2
  exit 1
fi

crate_published() {
  curl --fail --silent --show-error \
    --user-agent 'maku-release-ci (https://github.com/mlegls/maku)' \
    "https://crates.io/api/v1/crates/$1/$version" >/dev/null 2>&1
}

wait_for_crate() {
  local package=$1
  for _ in {1..60}; do
    if crate_published "$package"; then return 0; fi
    sleep 10
  done
  echo "$package $version did not become available from crates.io" >&2
  return 1
}

publish_crate() {
  local package=$1
  if crate_published "$package"; then
    echo "crates.io already has $package $version; skipping"
    return
  fi
  echo "publishing $package $version"
  cargo publish --locked --manifest-path crates/Cargo.toml -p "$package"
  wait_for_crate "$package"
}

# Registry resolution requires dependency order. The final two packages are
# kept sequential so retries and logs remain deterministic.
publish_crate maku
publish_crate maku-render-touhou
publish_crate maku-player
publish_crate maku-web

if npm view "$npm_name@$npm_version" version --json 2>/dev/null \
  | jq -e --arg version "$npm_version" '. == $version' >/dev/null; then
  echo "npm already has $npm_name $npm_version; skipping"
else
  echo "publishing $npm_name $npm_version"
  (cd crates/js/maku && npm publish --access public --provenance)
fi
