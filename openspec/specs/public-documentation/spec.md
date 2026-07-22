# Public Documentation Specification

## Purpose

Define the canonical, audience-specific documentation and release-checked examples for Maku authors, embedders, renderer implementers, and frontend users.

## Requirements

### Requirement: Audience-specific canonical documentation
The project SHALL provide a lookup-oriented language reference for card authors, a host API guide for Rust embedders, renderer/feature guides for frontend authors, and installation guidance for frontend-native packages and player binaries. User documentation SHALL derive current behavior from settled capability specs, MUST distinguish normative current behavior from planned changes, and MUST distinguish private workspace packages from registry-supported products.

#### Scenario: Author looks up a language form
- **WHEN** a card author searches the language reference for a supported form
- **THEN** the reference describes syntax, evaluation context, value/result behavior, errors, and a runnable canonical example without requiring internal OpenSpec or Cargo-workspace knowledge

#### Scenario: Planned syntax is not presented as current
- **WHEN** a backlog change proposes state return routing or blocking lasers
- **THEN** public current-language documentation does not teach that syntax until its capability spec and implementation are landed

#### Scenario: User chooses an installation path
- **WHEN** a user identifies as a Rust embedder, browser developer, or card author
- **THEN** documentation directs them respectively to `maku` features, `@mlegls/maku`, or a player binary rather than presenting internal producer packages as equivalent choices

### Requirement: Supported host facade documentation
The host guide SHALL document construction/loading, supported render-kind negotiation, fixed-tick advancement, input and channel updates, events, render-frame lifetime, session replay/scrub behavior, errors, and shutdown around the supported `Instance` facade. It SHALL NOT require embedders to use interpreter nodes, simulation storage, or executor internals.

#### Scenario: Minimal host implementation
- **WHEN** a host author follows the guide from a package archive
- **THEN** they can load a card with libraries, provide inputs, advance deterministically, consume events and ordered render transport, and report errors using only documented APIs

### Requirement: Bring-your-own-renderer documentation
Renderer documentation SHALL explain stable schema identity, ordered row/batch transport, exact batch expansion semantics, schema binding, the optional `maku::touhou` feature/module, typed frame layouts, material/resource manifests, command ordering, and host-owned GPU/resource lifetime. It MUST distinguish transport cost, pack construction, and actual drawing, and MUST state that frontend releases declare a curated bundled-pack set rather than universally containing every pack.

#### Scenario: Custom renderer consumes batches
- **WHEN** a frontend implements its own renderer with default-feature `maku`
- **THEN** the guide shows how to consume typed batches directly without requiring row expansion, Touhou policy, Macroquad, or wasm-bindgen

#### Scenario: Touhou pack consumer
- **WHEN** a Rust frontend enables `maku/touhou` or a frontend-native distribution declares bundled Touhou support
- **THEN** the guide shows profile/schema binding, frame build lifetime, material/texture resolution, fixed source layouts, and ordered command submission through the corresponding supported surface

### Requirement: Documentation examples are release-checked
Runnable Rust, JavaScript, `.maku`, and player-installation examples in release documentation SHALL be compiled, executed, syntax/corpus checked, or artifact-smoke checked against the same package and artifact versions named by the documentation. Rust package examples MUST exercise the documented default and feature-gated single-crate surfaces.

#### Scenario: API rename
- **WHEN** a supported Rust or wasm symbol changes before release
- **THEN** the documentation check fails at every stale example instead of deploying mismatched prose

#### Scenario: Retired crate import
- **WHEN** current Rust documentation imports `maku_render_touhou`, `maku_player`, or `maku_web` as a supported dependency
- **THEN** documentation validation rejects the stale public topology

### Requirement: Documentation identifies versions and authority
Published documentation SHALL display or link the Maku package/artifact version and source revision it describes, and SHALL identify capability specs as semantic authority and release notes as migration authority.

#### Scenario: Deployed docs lag runtime
- **WHEN** a deployed demo reports a different release revision from its documentation bundle
- **THEN** deployment verification reports the mismatch
