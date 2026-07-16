## Context

The workspace contains four packages with sound architectural dependency direction, but packaging was not previously a constraint. `maku` embeds standard-library sources from outside its package root, dependent packages use path-only requirements, package metadata is incomplete, and all-target compilation exposes stale IR cleanup. The repository also tracks historical build products and generated wasm outputs without a reproducible freshness gate. The current package name `maku-mesh-touhou` predates the settled description of that crate as a backend-neutral Touhou render pack.

The language and render contracts are already stable enough to form the supported boundary (`openspec/specs/language/spec.md`, `openspec/specs/render-rows/spec.md`, `openspec/specs/mesh-renderer-api/spec.md`). Entity storage, interpreter representation, lowering implementation, and backend selection remain intentionally changeable.

## Goals / Non-Goals

**Goals:**
- Make a clean checkout pass one documented release check covering Rust, wasm, JavaScript, generated artifacts, and package extraction.
- Publish independently useful crates with explicit API, asset, metadata, dependency, and version contracts.
- Start the public release without unused compatibility spellings or stale examples.
- Preserve core-to-pack-to-host dependency direction and backend neutrality.

**Non-Goals:**
- Publish actual registry releases or choose permanent 1.0 stability guarantees.
- Combine all hosts or genre packs into one Cargo package.
- Extract generic mesh primitives before a second genre pack demonstrates reuse.
- Implement f32 storage, JIT, GPU simulation, WebGPU drawing, or performance optimizations.
- Remove `pather`, motion-state fallback, prelude idempotence, or other behavior merely because its implementation is labeled legacy.

## Decisions

### Keep four independently published packages

The release topology remains `maku`, `maku-render-touhou`, `maku-player`, and `maku-web`, with the JavaScript package as the browser-facing distribution of the web artifact. `maku-render-touhou` depends on `maku`; the two hosts depend on both. Internal dependencies carry both `path` and exact compatible version requirements.

An outer feature-gated host crate was considered. It either creates a dependency cycle with the Touhou pack or requires splitting the current engine into an additional public core package. Separate packages better match Cargo installation, wasm-pack, npm, platform dependencies, and release cadence.

### Name genre policy explicitly

Rename the unpublished `maku-mesh-touhou` package and directory references to `maku-render-touhou`. The crate is a Touhou render pack that emits instances, meshes, resources, materials, and ordered commands; it is neither a universal renderer nor a GPU backend. Future genre packs publish separately. Shared primitives are extracted only after a second pack proves the abstraction; a future umbrella can re-export optional packs without owning their implementations.

### Support a narrow facade while permitting explicit unstable internals

The documented supported surface is the host facade, input/event/session contracts required by hosts, render transport/schema types, and the Touhou profile/frame ABI. Interpreter and simulation implementation modules are made private where practical; any symbol that must remain public for current crate boundaries is explicitly documented as unstable and excluded from compatibility promises. Package tests compile representative external consumers against only the supported facade.

### Give the Cargo workspace a durable repository home

Rename the obsolete `proto/` workspace root to `crates/`. It is the root of several publishable Rust crates, not one prototype package. All checked-in commands, CI, examples, editor integrations, documentation, and active planning references use `crates/`; `crates/target/` remains ignored and no compiler products are tracked.

### Make packaged assets crate-local and singular

Canonical standard-library sources needed by `include_str!` live at `crates/core/lib/` inside the `maku` package root and are listed by `cargo package`. Repository cards and browser manifests consume that same source or a generated copy guarded by a no-diff check. Package builds MUST NOT read above the extracted package root.

### One release check is the local/CI authority

A checked-in command runs formatting checks without mass-reformatting intentional compiler code, workspace tests/all targets, release ignored oracle suites as an explicit long lane, wasm target checks, JavaScript checks, generated binding freshness, crate package listing/build verification, and repository cleanliness. Fast pull-request lanes and slower release/oracle lanes remain distinguishable, but both use the same scripted commands locally and in CI.

### Audit compatibility by syntax and semantics separately

A machine-readable inventory names each compatibility form, canonical replacement, repository uses, external migration note, and disposition. Corpus scanning covers `.maku` files, documentation snippets, and Rust embedded sources. Alias removal follows migration of repository consumers and targeted tests. Internal legacy-labelled algorithms remain until equivalence and caller migration establish that they are redundant.

### Version and provenance the whole browser unit

Rust package versions, wasm-bindgen glue, wasm bytes, JavaScript wrapper, render frontend, and manifest schema carry a coherent release/source revision. Generated files are rebuilt in CI and compared with tracked distribution inputs or produced solely as release artifacts; stale mixtures are rejected.

## Risks / Trade-offs

- [Narrowing public exports breaks prototype callers] → Perform repository-wide call-site migration, provide explicit unstable escape hatches only where required, and use pre-1.0 release notes.
- [Moving standard-library sources creates duplicates] → Establish one canonical crate-local source and enforce generated-copy freshness.
- [Renaming the pack causes churn] → Do it before first publication and update every manifest, import, document, generated artifact, and downstream sync manifest atomically.
- [Removing aliases surprises unseen external cards] → Publish an inventory and migration table, keep canonical replacements simple, and limit removal to aliases absent from the checked-in corpus and explicitly selected in the delta spec.
- [CI becomes prohibitively slow] → Split fast and release/oracle lanes while retaining one documented release aggregate.
- [Tracked wasm output varies by toolchain] → Pin Rust/wasm-bindgen/wasm-opt/Bun versions or make the release job the sole artifact producer.

## Migration Plan

1. Rename the Rust workspace root from `proto/` to `crates/`, move the inlined standard library to `crates/core/lib/`, and repair all-target compilation and repository hygiene without changing package names.
2. Add toolchain policy and authoritative check scripts; establish a green baseline.
3. Complete compatibility inventory, migrate repository sources, and remove selected aliases with corpus tests.
4. Verify packaged assets and narrow/document the public facade.
5. Rename the Touhou package and update dependency/version metadata.
6. Verify each extracted crate in publication order, then rebuild wasm/npm artifacts from those versions.
7. Record the coherent release revision consumed by documentation and downstream demo work.

Rollback remains possible before registry publication by reverting the package rename and facade restriction commits. Once a version is published, corrections use new versions rather than replacing registry artifacts.

## Open Questions

- Final license identifier and repository URL metadata.
- Whether `maku-web` is published to crates.io for Rust reuse or only built as an internal producer for the npm package; it MUST still pass package verification if retained as a workspace package.
- Whether generated wasm glue remains tracked or moves entirely to release artifacts after downstream synchronization is automated.
