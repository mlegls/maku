# Release verification

Development lands on `dev`. Promote a coordinated, versioned release from
`dev` to the protected `main` branch; direct development on `main` is not
supported. Pushes to both branches run normal CI, while every push to `main`
runs the complete release gate and the idempotent registry publisher in
`.github/workflows/publish.yml`.

The `release` GitHub environment must define `CARGO_REGISTRY_TOKEN` and
`NPM_TOKEN`; configure required reviewers on that environment before the first
publication. The npm token must be allowed to publish public packages in the
`@mlegls` scope. The workflow also requests an OIDC identity and publishes npm
provenance. Missing credentials fail before any registry mutation.

Run `scripts/check.sh fast` during development and `scripts/check.sh release`
from a clean checkout before promoting to `main`. Tool versions are pinned by
`rust-toolchain.toml` and `mise.toml`.

## Package topology and order

`scripts/publish-release.sh` checks each exact version before publishing, so a
retry resumes a partially completed release without attempting to overwrite an
existing package. Rust packages publish independently in dependency order:

1. `maku`
2. `maku-render-touhou`
3. `maku-player` and `maku-web`

`maku-web` remains a crates.io package because its Rust/wasm host boundary is
independently testable and reusable. The browser-facing distribution is the
separate scoped npm package `@mlegls/maku`, built from `crates/js/maku`. Its
wasm binary, bindgen glue, wrapper, and render ABI are released as one unit;
the npm version does not permit mixing files from another Maku revision.

## Registry name check

On 2026-07-16 the crates.io API returned `404 crate does not exist` for
`maku`, `maku-render-touhou`, `maku-player`, and `maku-web`, using the required
identified API user agent. No existing crates.io owner therefore controls
those names. Registry availability is not a reservation; repeat this check
immediately before the first publish and publish the four packages in one
coordinated window.

The unscoped npm name `maku` is owned by an unrelated package (latest observed
version 0.1.12). Both `@mlegls/maku` and `@maku-engine/maku` returned 404; this
release selects `@mlegls/maku`, matching the repository owner. Confirm npm
account/scope authorization before publication.

## Package contents

Each Rust manifest has an explicit `include` list. Engine archives contain the
canonical inlined `lib/*.maku` sources; web archives intentionally omit the
Canvas/editor/static frontend; all archives contain the package README and an
identical MIT license. `scripts/check-source-tree.sh` rejects license drift and
tracked build products.
