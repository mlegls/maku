# release-packaging Specification

## Purpose
Define the reproducible checks, audience-aligned distribution topology, supported API boundary, registry contents, frontend artifacts, and provenance required for a coherent Maku release.

## Requirements

### Requirement: Authoritative release check
The repository SHALL provide one documented release-check entry point that verifies workspace tests and all targets, wasm target compilation, JavaScript checks, generated-artifact freshness, package extraction builds, and repository cleanliness. Slow oracle/corpus lanes MAY be separately invoked but MUST be part of the documented release gate.

#### Scenario: Stale example fails the release gate
- **WHEN** an example references an API removed by a completed change
- **THEN** the all-target portion fails before a package or release artifact is accepted

#### Scenario: Local and CI parity
- **WHEN** a contributor runs the documented release check from a clean checkout
- **THEN** it invokes the same checked-in commands used by CI rather than undocumented machine-local steps

### Requirement: Reproducible toolchain and source tree
The Cargo workspace SHALL live under the durable `crates/` repository root, declare its Rust toolchain or MSRV policy, and declare the tool versions needed to reproduce wasm/JavaScript artifacts. Build output directories MUST NOT contain tracked compiler products, and generated files MUST have either a deterministic no-diff freshness check or a release-only artifact policy.

#### Scenario: Obsolete prototype path remains
- **WHEN** live build scripts, CI, examples, editor integrations, or public documentation are scanned
- **THEN** they resolve the workspace under `crates/` and contain no operational dependency on `proto/`

#### Scenario: Generated wasm glue is stale
- **WHEN** the checked-in JavaScript glue does not match a rebuild with the declared toolchain and source revision
- **THEN** the release check fails with the stale artifact paths

#### Scenario: Ignored target artifacts are tracked
- **WHEN** a compiler output under an ignored target directory remains tracked
- **THEN** repository hygiene verification rejects the release candidate

### Requirement: Audience-aligned public package topology
The Rust workspace SHALL publish `maku` as its sole crates.io SDK. The default `maku` feature set SHALL contain no genre-pack, Macroquad, wasm-bindgen, or frontend dependency; supported first-party Rust integrations SHALL be opt-in features and modules of `maku`. Native player, wasm producer, benchmark, and future frontend bridge packages SHALL remain independently testable workspace packages marked non-publishable unless a demonstrated registry audience requires otherwise.

#### Scenario: Default Rust consumer
- **WHEN** a Rust game adds `maku` without features
- **THEN** Cargo resolves the language, simulation, host/session, and renderer-neutral transport SDK without Macroquad, wasm-bindgen, or Touhou implementation dependencies

#### Scenario: Touhou Rust consumer
- **WHEN** a Rust game enables the `touhou` feature
- **THEN** it can build frames through `maku::touhou` without depending on another Maku crate

#### Scenario: Macroquad Rust consumer
- **WHEN** a Rust game enables the `macroquad` feature
- **THEN** Cargo enables `touhou` and the optional Macroquad dependency and exposes the supported adapter through `maku::macroquad`

#### Scenario: Browser consumer
- **WHEN** a browser user installs Maku
- **THEN** they install the frontend-native `@mlegls/maku` package rather than a wasm host crate from crates.io

### Requirement: Extracted packages are self-contained
Every publishable registry package SHALL compile and test from the exact archive produced by its registry packaging command without reading files outside the extracted package root. The `maku` Cargo archive SHALL include its standard-library sources and all source required by enabled public features. Frontend-native packages SHALL include their runtime bytes, declarations, licenses, and release identity as one self-contained unit.

#### Scenario: Engine package verification
- **WHEN** the `maku` archive is extracted into an isolated directory and tested with default and all public features
- **THEN** its builds resolve every embedded standard-library, Touhou, and optional adapter source within that archive

#### Scenario: npm package verification
- **WHEN** the scoped browser package is packed and inspected
- **THEN** it contains the JavaScript wrapper, declarations, wasm binary/glue, license, and matching release identity without requiring a crates.io host package

### Requirement: Deliberate supported Rust API
Published documentation SHALL identify host lifecycle, input/event/session contracts, render schema/transport, and feature-gated `maku::touhou` profile/frame and `maku::macroquad` adapter APIs as the supported public Rust surface. Interpreter representations, physical entity storage, lowering executors, private producer packages, and backend internals MUST NOT acquire compatibility promises merely because workspace packages need implementation access.

#### Scenario: External facade smoke crate
- **WHEN** a package-level smoke consumer is compiled against `maku` with the `touhou` feature
- **THEN** it can load and advance a card, negotiate render kinds, consume render transport, and build a Touhou frame without importing another Maku crate or simulation-storage internals

#### Scenario: Internal representation changes
- **WHEN** entity storage, kernel executor, private wasm producer, or private player implementation changes in a later release
- **THEN** documented `maku` host and feature examples remain source-compatible unless an explicit pre-1.0 API migration is announced

### Requirement: Complete publication metadata
Every publishable package SHALL declare version, edition/runtime policy, description, license, repository, README, and appropriate documentation metadata. Optional public dependencies SHALL be feature-gated and package names MUST be checked for registry ownership before release. Private workspace packages MUST be marked non-publishable and MUST NOT require registry metadata or trusted-publisher configuration.

#### Scenario: Publication dry run
- **WHEN** Cargo and npm packages are dry-run from a clean release checkout
- **THEN** only `maku` and `@mlegls/maku` are candidates for registry publication and both emit no workspace-owned metadata or archive warnings

### Requirement: Versioned browser release unit
The wasm binary, wasm-bindgen glue, JavaScript wrapper, render frontend ABI, bundled render-pack identities, and package manifest SHALL be produced as one versioned release unit containing the public Maku/npm version, Maku source revision, and frame ABI version. Private workspace package versions MUST NOT be presented as independently supported browser components. Consumers MUST be able to detect an incompatible or mixed artifact set before frame decoding.

#### Scenario: Mixed wrapper and wasm
- **WHEN** a JavaScript wrapper expects a different frame ABI or release identity from the loaded wasm artifact
- **THEN** initialization fails with a version diagnostic rather than decoding buffers with the wrong layout

#### Scenario: Bundled pack inspection
- **WHEN** a consumer inspects the browser release identity
- **THEN** it can determine that Touhou support is bundled without interpreting a private Cargo package version

### Requirement: Frontend-native distributions declare bundled packs
Official browser, native-player, and future engine-plugin distributions SHALL declare the render packs compiled into that release. A frontend MUST NOT imply that all present or future packs are universally bundled, and a missing required pack MUST produce an explicit capability error.

#### Scenario: Initial official frontends
- **WHEN** the browser package or native player release is inspected
- **THEN** its release metadata declares the bundled Touhou pack identity and contract version

#### Scenario: Future pack is introduced
- **WHEN** a second concrete render pack is implemented
- **THEN** each frontend can select and declare a curated pack set without adding that pack to the default Rust SDK or every frontend artifact

### Requirement: Native player binary distribution
The release process SHALL build the private native player package into documented platform artifacts for GitHub Releases. Player installation documentation SHALL prefer those artifacts for card authors while retaining workspace build instructions for contributors.

#### Scenario: Card author installs the player
- **WHEN** a non-Rust user follows player installation documentation
- **THEN** they can select a supported prebuilt platform artifact without installing the Rust toolchain or a crates.io host package

### Requirement: Host-boundary release coverage
Release verification SHALL exercise the native and wasm host boundaries in addition to library unit tests, including input forwarding, frame construction, material/resource resolution, typed-view bounds, and representative error paths.

#### Scenario: Wasm frame smoke
- **WHEN** the packaged wasm host loads a representative mixed sprite/beam card
- **THEN** it advances, builds a frame, resolves every command material/resource, and exposes in-bounds typed views
