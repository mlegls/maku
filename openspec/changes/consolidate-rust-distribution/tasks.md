## 1. Public SDK consolidation

- [x] 1.1 Add default-empty `touhou` and `macroquad` feature definitions and optional Macroquad dependency to `crates/core/Cargo.toml`
- [x] 1.2 Move the Touhou render-pack implementation and tests under feature-gated `maku::touhou`
- [x] 1.3 Move the Macroquad compatibility adapter under feature-gated `maku::macroquad`
- [x] 1.4 Remove the retired `crates/render-touhou` workspace package and update workspace membership
- [x] 1.5 Verify default, Touhou, Macroquad, and all-feature `maku` builds independently

## 2. Private workspace consumers

- [x] 2.1 Mark `maku-player` non-publishable and migrate it to the `maku/macroquad` feature and public module paths
- [x] 2.2 Mark `maku-web` non-publishable and migrate it to the `maku/touhou` feature and public module paths
- [x] 2.3 Migrate benchmark code and dependencies to `maku::touhou` and `maku::macroquad`
- [x] 2.4 Update the external public API smoke crate to consume only `maku` with the Touhou feature
- [x] 2.5 Update examples, tests, and local tooling imports and pass all workspace/wasm targets

## 3. Release identity and package verification

- [x] 3.1 Advance the consolidated Cargo and npm/browser release version to 0.2.0 without changing frame ABI v1
- [x] 3.2 Replace browser render/web Cargo-version identity fields with an explicit bundled render-pack manifest
- [x] 3.3 Regenerate wasm bindings, declarations, wrapper, integrity metadata, and downstream sync inputs as one snapshot
- [x] 3.4 Simplify Cargo archive extraction and publish dry-runs to the sole public `maku` package with all public features
- [x] 3.5 Simplify the idempotent OIDC publisher to publish `maku` and `@mlegls/maku` only
- [x] 3.6 Add checks that private workspace packages cannot become registry publication candidates

## 4. Release automation and player artifacts

- [x] 4.1 Change publication triggering from `main` pushes to protected `v*` tags with tag/version/main-ancestry validation
- [x] 4.2 Add Linux, macOS, and Windows native-player artifact builds for GitHub Releases
- [x] 4.3 Keep the GitHub `release` environment and OIDC permissions while reducing crates.io trusted-publisher requirements to `maku`
- [x] 4.4 Document and test idempotent partial-release recovery, player artifact checks, and post-publication auxiliary-crate yanking

## 5. Public documentation and migration

- [x] 5.1 Update root, SDK, host, renderer, player, web, and release documentation for one Rust SDK and frontend-native installation paths
- [x] 5.2 Add a 0.1-to-0.2 Rust migration table for retired crate imports and Cargo dependencies
- [x] 5.3 Document explicit curated render-pack inclusion and missing-pack capability behavior for each frontend
- [x] 5.4 Remove stale current documentation claims that all four Cargo packages are independently supported
- [x] 5.5 Update release notes, package ownership state, and OIDC web-UI instructions for the consolidated topology

## 6. Validation and rollout

- [ ] 6.1 Run default/feature/all-target Rust checks, full workspace tests, wasm/browser smoke, documentation checks, package archive tests, and npm dry-run
- [ ] 6.2 Run the complete clean-checkout release gate and strict OpenSpec validation
- [ ] 6.3 Synchronize the delta specs into canonical specs and verify downstream manifest coherence
- [ ] 6.4 Publish and verify 0.2.0 through the protected tag workflow before yanking auxiliary Cargo 0.1.0 packages
