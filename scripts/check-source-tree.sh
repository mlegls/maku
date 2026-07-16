#!/bin/sh
# Reject compiler/build products that have accidentally entered the Git index.
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_root"

tracked_ignored=$(git ls-files --cached --ignored --exclude-standard)
if [ -n "$tracked_ignored" ]; then
  echo "tracked files match repository ignore rules:" >&2
  printf '%s\n' "$tracked_ignored" >&2
  exit 1
fi

if git ls-files 'crates/target/**' | grep -q .; then
  echo "tracked Cargo output found under crates/target/:" >&2
  git ls-files 'crates/target/**' >&2
  exit 1
fi
