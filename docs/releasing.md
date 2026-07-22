# Release verification

Development lands on `dev`. Promote a coordinated, versioned release from
`dev` to protected `main`; direct development on `main` is unsupported. Normal
CI runs on both branches. Registry publication and player artifacts run only
for a protected `vX.Y.Z` tag whose version matches Cargo/npm and whose commit is
reachable from `main`.

Run `scripts/check.sh fast` during development and `scripts/check.sh release`
from a clean checkout before promotion. Tool versions are pinned by
`rust-toolchain.toml` and `mise.toml`.

## Public distribution topology

Maku has audience-aligned distributions:

1. crates.io: `maku`, the sole Rust SDK; `touhou` and `macroquad` are optional
   features.
2. npm: `@mlegls/maku`, containing wrapper, declarations, wasm, frame ABI, and
   the explicitly bundled Touhou pack.
3. GitHub Releases: Linux, macOS, and Windows `maku-player` binaries.

`maku-player`, `maku-web`, and `maku-bench` are private Cargo workspace
packages (`publish = false`). The old `maku-render-touhou` source now lives in
`maku::touhou`; no 0.2 versions are published for the three auxiliary 0.1
crates.

## OIDC trusted publishing

The `release` GitHub environment requires approval and stores no registry
secrets. Configure these trusted publishers with repository `mlegls/maku`,
workflow `publish.yml`, and environment `release`:

- crates.io: only the `maku` crate
- npm: only `@mlegls/maku`, allowing `npm publish`

The workflow exchanges GitHub OIDC through
`rust-lang/crates-io-auth-action`; npm 11.5.1+ performs its OIDC exchange and
provenance generation automatically. `NODE_AUTH_TOKEN` is cleared explicitly
so a local credential cannot disable trusted publishing.

## Tagged release

1. Verify `scripts/check.sh release` from the release commit.
2. Merge `dev` to `main` through its required checks.
3. Create and push an annotated `vX.Y.Z` tag on that exact commit.
4. Approve the GitHub `release` environment deployment.
5. The workflow validates version/tag/main ancestry, builds all player
   artifacts, publishes missing Cargo/npm versions idempotently, and creates
   the GitHub Release.
6. Verify crate/npm provenance, package hashes, player downloads, browser smoke,
   and downstream identity before removing rollback artifacts.

A retry uses the same immutable tag. `scripts/publish-release.sh` skips an exact
version already present and resumes missing registry products; never move a tag
or overwrite package bytes. If a published artifact is defective, yank it and
publish a patch.

After verified 0.2.0 replacements exist, yank the retired Cargo packages with:

```sh
cargo yank --version 0.1.0 maku-render-touhou
cargo yank --version 0.1.0 maku-player
cargo yank --version 0.1.0 maku-web
```

## Package contents

The `maku` manifest has an explicit include list. Its archive contains the
canonical `lib/*.maku` sources and all sources needed by `touhou` and
`macroquad`; `scripts/check-packages.sh` extracts and tests that archive with
all features. The npm archive contains the matching wrapper, declarations,
wasm unit, README, license, and release manifest. `scripts/check-source-tree.sh`
rejects license drift and tracked build products.
