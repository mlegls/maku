## ADDED Requirements

### Requirement: Audience-specific canonical documentation
The project SHALL provide a lookup-oriented language reference for card authors, a host API guide for embedders, and renderer/package guides for frontend authors. User documentation SHALL derive current behavior from settled capability specs and MUST distinguish normative current behavior from planned changes.

#### Scenario: Author looks up a language form
- **WHEN** a card author searches the language reference for a supported form
- **THEN** the reference describes syntax, evaluation context, value/result behavior, errors, and a runnable canonical example without requiring internal OpenSpec knowledge

#### Scenario: Planned syntax is not presented as current
- **WHEN** a backlog change proposes state return routing or blocking lasers
- **THEN** public current-language documentation does not teach that syntax until its capability spec and implementation are landed

### Requirement: Supported host facade documentation
The host guide SHALL document construction/loading, supported render-kind negotiation, fixed-tick advancement, input and channel updates, events, render-frame lifetime, session replay/scrub behavior, errors, and shutdown around the supported `Instance` facade. It SHALL NOT require embedders to use interpreter nodes, simulation storage, or executor internals.

#### Scenario: Minimal host implementation
- **WHEN** a host author follows the guide from a package archive
- **THEN** they can load a card with libraries, provide inputs, advance deterministically, consume events and ordered render transport, and report errors using only documented APIs

### Requirement: Bring-your-own-renderer documentation
Renderer documentation SHALL explain stable schema identity, ordered row/batch transport, exact batch expansion semantics, schema binding, the optional Touhou render pack, typed frame layouts, material/resource manifests, command ordering, and host-owned GPU/resource lifetime. It MUST distinguish transport cost, pack construction, and actual drawing.

#### Scenario: Custom renderer consumes batches
- **WHEN** a frontend implements its own renderer
- **THEN** the guide shows how to consume typed batches directly without requiring row expansion or Touhou policy

#### Scenario: Touhou pack consumer
- **WHEN** a frontend selects the bundled Touhou pack
- **THEN** the guide shows profile/schema binding, frame build lifetime, material/texture resolution, fixed source layouts, and ordered command submission

### Requirement: Documentation examples are release-checked
Runnable Rust, JavaScript, and `.maku` examples in release documentation SHALL be compiled, executed, or syntax/corpus checked against the same package and artifact versions named by the documentation.

#### Scenario: API rename
- **WHEN** a supported Rust or wasm symbol changes before release
- **THEN** the documentation check fails at every stale example instead of deploying mismatched prose

### Requirement: Documentation identifies versions and authority
Published documentation SHALL display or link the Maku package/artifact version and source revision it describes, and SHALL identify capability specs as semantic authority and release notes as migration authority.

#### Scenario: Deployed docs lag runtime
- **WHEN** a deployed demo reports a different release revision from its documentation bundle
- **THEN** deployment verification reports the mismatch
