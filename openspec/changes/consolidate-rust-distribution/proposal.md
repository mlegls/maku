## Why

The four-crate registry topology exposes internal host and render-pack boundaries to users who generally need either one Rust SDK, a ready-to-run player, or a frontend-native package. Consolidating the public Rust surface now, immediately after the bootstrap release, avoids making coordinated internal crate publication and versioning a permanent compatibility burden.

## What Changes

- **BREAKING**: Make `maku` the only public crates.io SDK and expose the Touhou render pack and Macroquad adapter as opt-in Cargo features/modules.
- Keep the wasm producer and native player as private workspace packages; distribute browser bindings through `@mlegls/maku` and the player through prebuilt GitHub Release binaries.
- Preserve internal crate/module boundaries where useful without presenting them as independently versioned registry products.
- Change Rust imports from `maku_render_touhou::*` and `maku_player::macroquad_compat::*` to feature-gated `maku::touhou::*` and `maku::macroquad::*` surfaces.
- Stop publishing new versions of `maku-render-touhou`, `maku-player`, and `maku-web`; retain their published `0.1.0` records as historical bootstrap artifacts rather than deleting or silently replacing them.
- Simplify package validation, OIDC publication, browser release identity, documentation, and public API smoke tests around one Rust package plus frontend-native distributions.
- Make render-pack inclusion explicit per frontend release: official frontends initially bundle Touhou, while Rust users opt in with features and future frontends declare their included pack identities.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `release-packaging`: Replace the independently publishable four-crate topology with one public Rust SDK, private build/host packages, and frontend-native distributions.
- `public-documentation`: Document one Rust dependency with feature-gated pack/adapter surfaces and distinguish those surfaces from browser/player installation.
- `web-demo-delivery`: Identify the browser artifact by the public Maku/npm release and bundled render-pack capabilities rather than independently published Rust host/pack versions.

## Impact

This changes `openspec/specs/release-packaging/spec.md`, `openspec/specs/public-documentation/spec.md`, and `openspec/specs/web-demo-delivery/spec.md`. Implementation affects Cargo manifests and source layout under `crates/`, public imports, package/release scripts, generated wasm identity, CI/OIDC publishing, public smoke tests, documentation, and downstream browser snapshots. Language semantics, ordered typed render transport, fixed frame ABI v1, and the render-pack/backend separation governed by `openspec/specs/mesh-renderer-api/spec.md` remain unchanged.
