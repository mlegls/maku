## ADDED Requirements

### Requirement: Authoritative release check
The repository SHALL provide one documented release-check entry point that verifies workspace tests and all targets, wasm target compilation, JavaScript checks, generated-artifact freshness, package extraction builds, and repository cleanliness. Slow oracle/corpus lanes MAY be separately invoked but MUST be part of the documented release gate.

#### Scenario: Stale example fails the release gate
- **WHEN** an example references an API removed by a completed change
- **THEN** the all-target portion fails before a package or release artifact is accepted

#### Scenario: Local and CI parity
- **WHEN** a contributor runs the documented release check from a clean checkout
- **THEN** it invokes the same checked-in commands used by CI rather than undocumented machine-local steps

### Requirement: Reproducible toolchain and source tree
The workspace SHALL declare its Rust toolchain or MSRV policy and the tool versions needed to reproduce wasm/JavaScript artifacts. Build output directories MUST NOT contain tracked compiler products, and generated files MUST have either a deterministic no-diff freshness check or a release-only artifact policy.

#### Scenario: Generated wasm glue is stale
- **WHEN** the checked-in JavaScript glue does not match a rebuild with the declared toolchain and source revision
- **THEN** the release check fails with the stale artifact paths

#### Scenario: Ignored target artifacts are tracked
- **WHEN** a compiler output under an ignored target directory remains tracked
- **THEN** repository hygiene verification rejects the release candidate

### Requirement: Independently publishable package topology
The Rust workspace SHALL publish the engine as `maku`, the Touhou render pack as `maku-render-touhou`, the native executable as `maku-player`, and the wasm host package as `maku-web` if that package remains registry-facing. Genre render packs SHALL be independently versioned packages; the engine MUST NOT depend on a genre pack or GPU/render host dependency.

#### Scenario: Engine dependency graph
- **WHEN** registry-normalized package metadata is inspected
- **THEN** `maku-render-touhou` depends on `maku`, hosts depend on the engine and selected pack, and no dependency returns from `maku` to either pack or host

#### Scenario: Future genre pack
- **WHEN** a second prepackaged genre renderer is introduced
- **THEN** it can publish independently without enabling or downloading the Touhou pack

### Requirement: Extracted packages are self-contained
Every registry package SHALL compile and test from the exact archive produced by `cargo package`, without reading files outside the extracted package root. Standard-library sources required by the engine SHALL have one declared canonical source included in the engine archive.

#### Scenario: Engine package verification
- **WHEN** the `maku` archive is extracted into an isolated directory
- **THEN** its build and tests resolve every embedded standard-library source within that directory

#### Scenario: Local dependency normalization
- **WHEN** a dependent workspace package is normalized for publication
- **THEN** each local dependency retains a compatible version requirement after its `path` is removed

### Requirement: Deliberate supported Rust API
Published documentation SHALL identify host lifecycle, input/event/session contracts, render schema/transport, and render-pack profile/frame ABI as the supported public surface. Interpreter representations, physical entity storage, lowering executors, and backend internals MUST NOT acquire compatibility promises merely because workspace packages need implementation access.

#### Scenario: External facade smoke crate
- **WHEN** a package-level smoke consumer is compiled against the documented API
- **THEN** it can load and advance a card, negotiate render kinds, consume render transport, and build a Touhou frame without importing interpreter or simulation-storage internals

#### Scenario: Internal representation changes
- **WHEN** entity storage or kernel executor implementation changes in a later release
- **THEN** the documented host and render-pack examples remain source-compatible unless an explicit pre-1.0 API migration is announced

### Requirement: Complete publication metadata
Every publishable package SHALL declare version, edition, MSRV policy, description, license, repository, README, and appropriate documentation metadata. Internal dependencies SHALL use tested compatible versions, and package names MUST be checked for registry availability or ownership before release.

#### Scenario: Publication dry run
- **WHEN** packages are dry-run in dependency order
- **THEN** Cargo emits no missing-metadata or path-without-version warnings owned by the workspace

### Requirement: Versioned browser release unit
The wasm binary, wasm-bindgen glue, JavaScript wrapper, render frontend ABI, and package manifest SHALL be produced as one versioned release unit containing the Maku source revision and frame ABI version. Consumers MUST be able to detect an incompatible or mixed artifact set before frame decoding.

#### Scenario: Mixed wrapper and wasm
- **WHEN** a JavaScript wrapper expects a different frame ABI or release identity from the loaded wasm artifact
- **THEN** initialization fails with a version diagnostic rather than decoding buffers with the wrong layout

### Requirement: Host-boundary release coverage
Release verification SHALL exercise the native and wasm host boundaries in addition to library unit tests, including input forwarding, frame construction, material/resource resolution, typed-view bounds, and representative error paths.

#### Scenario: Wasm frame smoke
- **WHEN** the packaged wasm host loads a representative mixed sprite/beam card
- **THEN** it advances, builds a frame, resolves every command material/resource, and exposes in-bounds typed views
