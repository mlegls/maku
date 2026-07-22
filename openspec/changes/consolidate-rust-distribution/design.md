## Context

Maku 0.1.0 bootstrapped four crates.io packages that mirror workspace implementation boundaries. The intended audiences instead divide by distribution channel: Rust game developers consume an SDK, browser users consume npm, card authors consume a player binary, and future Godot/Unity users will consume native frontend packages. The public topology in `openspec/specs/release-packaging/spec.md` therefore makes internal wasm host, native host, and genre-pack versioning user-visible without improving those audiences' installation paths.

The render architecture in `openspec/specs/mesh-renderer-api/spec.md` remains authoritative: language/card semantics stay above backend IR, render transport stays ordered and typed, packs compile semantic style into fixed primitive recipes, and hosts own drawing/resources. This change moves code and distribution boundaries without weakening those contracts.

## Goals / Non-Goals

**Goals:**

- Make `maku` the sole crates.io SDK, with no frontend or renderer dependency in its default feature set.
- Expose the existing Touhou pack as `maku::touhou` behind `touhou`, and the Macroquad adapter as `maku::macroquad` behind `macroquad`.
- Keep player and wasm producers independently testable as private workspace packages.
- Preserve the npm package, generated wasm unit, frame ABI v1, and frontend behavior.
- Distribute ready-to-run player binaries independently of crates.io SDK organization.
- Make bundled pack identities explicit in frontend release metadata.

**Non-Goals:**

- Creating a generic runtime pack-plugin ABI before a second pack exists.
- Making every frontend bundle every future pack.
- Changing card semantics, simulation determinism, render transport, frame layouts, or host-owned submission.
- Rewriting archived OpenSpec changes or replacing published 0.1.0 artifacts.
- Publishing Godot or Unity integrations in this change.

## Decisions

### One public SDK, private producer packages

`crates/core` remains package `maku` and becomes the only publishable Cargo workspace member. `crates/player`, `crates/web`, and `crates/bench` remain packages for build isolation but set `publish = false`. `crates/render-touhou` is removed after its authoritative sources move into `crates/core/src/touhou`.

This separates repository modularity from registry topology. Keeping forwarding crates was rejected because it preserves the unwanted public names and cannot let `maku` re-export a pack that depends on `maku` without a dependency cycle.

### Feature-gated SDK composition

The SDK features are:

```toml
[features]
default = []
touhou = []
macroquad = ["touhou", "dep:macroquad"]
```

The default package contains language, deterministic simulation, host/session APIs, and renderer-neutral transport only. The Touhou source moves under `maku::touhou`; the existing Macroquad compatibility adapter moves from the player library under `maku::macroquad`. `macroquad` remains an optional dependency and wasm dependencies remain in the private web producer.

A public `web` feature was rejected because browser users install npm and Rust embedders do not need wasm-bindgen exports as an SDK surface. The private producer activates `maku/touhou` and owns wasm-bindgen.

### Physical source moves, not cross-package path inclusion

Cargo archives cannot include sibling-package source safely. Touhou and Macroquad public implementations therefore move physically beneath `crates/core/src/`, with imports converted to same-crate paths. Feature-gated unit tests move with them. This preserves exact isolated `cargo package` verification.

### Frontends publish curated pack sets

Official 0.2 frontends continue bundling Touhou, but no frontend promises to include all future packs. Browser release identity replaces independently published render/web package versions with an ordered `render_packs` capability list containing stable pack ID and contract version. Cards/frontends validate required pack capabilities explicitly. Assets may later become lazy data, but executable pack code remains compiled into each selected frontend until another concrete pack justifies a plugin mechanism.

### Player distribution is a binary product

The native player remains a private Cargo package and is tested in the workspace. Release automation builds platform artifacts for GitHub Releases rather than requiring card authors to install Rust or understand SDK features. Rust developers can still build it from the workspace; crates.io is not its primary distribution channel.

### Version and registry migration

The consolidated public surface starts at `maku 0.2.0`, reflecting pre-1.0 breaking import/package changes. The npm/browser release advances to 0.2.0 as the coordinated runtime artifact even though its JavaScript API remains compatible. No 0.2 versions are published for the three retired Cargo packages.

After `maku 0.2.0`, npm 0.2.0, and player binaries are verified, the three auxiliary Cargo 0.1.0 versions are yanked with migration notes pointing to `maku` features or frontend-native distributions. Their immutable registry records and the 0.1.0 release evidence remain historical; nothing is overwritten.

### Release automation follows version tags

OIDC publication moves from arbitrary `main` pushes to protected `v*` tags. The workflow verifies that the tag version equals `maku` and npm versions and that the commit is on protected `main`, runs the full release gate, publishes missing `maku`/npm versions idempotently, and attaches player binaries. Only `maku` needs a crates.io trusted-publisher relationship.

## Risks / Trade-offs

- **Feature combinations can hide dependency leaks** → CI tests default, `touhou`, and all-feature SDK builds independently, plus native and wasm workspace targets.
- **Moving public paths breaks 0.1 Rust consumers** → use a 0.2 version, publish a migration table, and keep the 0.1 source/tag available.
- **One SDK feature couples pack compatibility to Maku versions** → acceptable while there is one concrete pack; revisit only when a second pack demonstrates independent demand.
- **Frontend bundles can grow as packs accumulate** → bundle only an explicit curated set and publish pack identities in release metadata.
- **Yanked auxiliary packages remain visible** → registry history is immutable; docs and yank messages make the replacement unambiguous.
- **Cross-platform player packaging increases CI surface** → keep binary builds separate from SDK archive validation and smoke each supported artifact before release publication.

## Migration Plan

1. Land delta specs and source/Cargo changes on `dev`; keep the published 0.1.0 artifacts untouched.
2. Move Touhou and Macroquad code, update internal consumers/imports, and pass default/feature/workspace/wasm tests.
3. Update release identity, docs, package checks, OIDC publication, and player artifact builds; regenerate the complete npm/wasm snapshot.
4. Promote the 0.2.0 release commit to `main`, create protected `v0.2.0`, and approve the `release` environment.
5. Verify crates.io, npm provenance, player downloads, docs, and neen.ink synchronization.
6. Yank only the three auxiliary Cargo 0.1.0 versions and retain rollback/source tags.

Rollback before publication is a revert on `dev`. After 0.2.0 publication, registries remain immutable; rollback means yanking a defective 0.2.0 and publishing a corrected patch, never retagging or replacing bytes.

## Open Questions

None. A second concrete render pack will trigger a separate decision about independent pack packages or runtime pack data.
