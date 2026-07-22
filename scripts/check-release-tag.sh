#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"
version=$(cargo metadata --manifest-path crates/Cargo.toml --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "maku") | .version')
npm_version=$(jq -r '.version' crates/js/maku/package.json)
tag=${GITHUB_REF_NAME:-${1:-}}

if [ "$npm_version" != "$version" ]; then
  echo "maku $version and npm $npm_version versions differ" >&2
  exit 1
fi
if [ "$tag" != "v$version" ]; then
  echo "release tag '$tag' must equal v$version" >&2
  exit 1
fi
if git show-ref --verify --quiet refs/remotes/origin/main \
    && ! git merge-base --is-ancestor HEAD refs/remotes/origin/main; then
  echo "release commit is not reachable from origin/main" >&2
  exit 1
fi
printf 'release identity OK: %s at %s\n' "$tag" "$(git rev-parse HEAD)"
