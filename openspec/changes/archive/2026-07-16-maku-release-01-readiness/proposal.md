## Why

Maku's engine and render boundaries are now coherent, but the repository cannot yet sustain a public release: all-target builds are broken, generated/build artifacts are not governed, the Rust packages do not verify from crates.io tarballs, and broad prototype internals would become accidental public APIs. The first release should also remove repository-unused compatibility spellings rather than preserving them immediately after publication.

## What Changes

- Restore a clean, reproducible workspace/all-target/wasm/JavaScript check surface and remove tracked build outputs.
- Add CI, Rust toolchain/MSRV policy, generated-artifact freshness checks, and package-local host-boundary tests.
- Define a deliberate crates.io topology with separate engine, Touhou render-pack, native player, and wasm host packages; keep genre packs independently versioned rather than collecting all packs into one crate.
- Rename `maku-mesh-touhou` to the clearer pre-publication package name `maku-render-touhou` while preserving its backend-neutral render-pack role.
- Narrow and document the supported Rust API around host, session, render transport, and pack contracts without promising interpreter/simulation storage internals as stable APIs.
- Rename the obsolete Rust workspace root from `proto/` to `crates/`, and move the inlined standard library from repository-level `cards/lib/` to the canonical crate-local `crates/core/lib/` source.
- Make standard-library/card assets required by compilation available inside the packaged engine crate and add versioned local dependency declarations, license, repository, README, MSRV, and docs.rs metadata.
- Verify package contents and extracted-package builds in dependency publication order, and define the npm/wasm artifact relationship without requiring crates.io users to install frontend assets.
- **BREAKING** Remove repository-unused compatibility spellings only after auditing checked-in cards, translations, documentation snippets, and embedded test cards; retain current constructs whose legacy implementation backing is still semantically required.
- Record package versions and source revisions so downstream demos can pin a coherent Rust/wasm/JavaScript release unit.

## Capabilities

### New Capabilities
- `release-packaging`: Reproducible checks, supported package/API boundaries, crates.io/npm metadata, packaged assets, publication order, and release provenance.

### Modified Capabilities
- `language`: Remove specifically audited compatibility aliases while preserving canonical forms and rejecting unsupported legacy contexts clearly.

## Impact

- The Rust workspace moves from `proto/` to `crates/`; workspace/package manifests, public exports, standard-library embedding, generated wasm bindings, CI configuration, READMEs, licensing, and release metadata move with it.
- Checked-in `.maku` cards, translations, tutorials, documentation snippets, and Rust-embedded card fixtures used by the compatibility audit.
- Package consumers will use `maku-render-touhou` rather than the unpublished prototype name `maku-mesh-touhou`.
- Governing contracts remain `openspec/specs/language/spec.md`, `openspec/specs/render-rows/spec.md`, and `openspec/specs/mesh-renderer-api/spec.md`; physical storage and backend optimization remain outside the supported package API.
