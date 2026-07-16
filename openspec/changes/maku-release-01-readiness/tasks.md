## 1. Stabilize the Repository Baseline

- [ ] 1.1 Replace stale `ProjectorNum` use in `proto/core/examples/dbg.rs` with the landed IR/projector API and make workspace all-target checks pass.
- [ ] 1.2 Remove tracked files under ignored build-output directories and add a regression check that rejects newly tracked compiler products.
- [ ] 1.3 Declare the Rust toolchain/MSRV and document platform linker/tool prerequisites without baking one developer machine's paths into published packages.
- [ ] 1.4 Add checked-in fast and release check entry points covering Rust workspace/all-target tests, wasm target checks, JavaScript checks, and clean-tree verification.
- [ ] 1.5 Add CI jobs using those entry points, with ignored oracle/card suites and artifact verification in an explicit release lane.

## 2. Audit and Remove Compatibility Surface

- [ ] 2.1 Create the compatibility inventory with canonical replacement, semantic versus implementation classification, repository consumers, tests, migration note, and disposition for each candidate.
- [ ] 2.2 Scan all repository `.maku` files, library sources, documentation snippets, Rust-embedded cards, and web-demo fixtures; add a repeatable corpus check for selected forms.
- [ ] 2.3 Migrate repository consumers and docs from `value-or` and selected old Touhou `spawn-*` aliases to canonical forms, then remove those unused aliases and update focused tests/diagnostics.
- [ ] 2.4 Audit direct `:facing`/`:opacity`/`:pts` render compatibility separately from valid genre-library metadata, remove only selected direct aliases, and preserve canonical library behavior in tests.
- [ ] 2.5 Add differential coverage for canonical `pather`, motion-state fallback, prelude import idempotence, and projector-context rejection; retain their required implementation paths and document why they are not alias cleanup.
- [ ] 2.6 Run the complete card/tutorial/translation corpus and release ignored oracle suites after compatibility removal.

## 3. Define Package Assets and Public API

- [ ] 3.1 Move or establish canonical standard-library sources within the `maku` package root and update engine/web/card loading so package builds never read above the extracted crate.
- [ ] 3.2 Add a freshness check for any generated or served copies of canonical library/card assets.
- [ ] 3.3 Inventory public Rust exports and classify supported facade/render/session contracts versus unstable interpreter, storage, lowering, and backend internals.
- [ ] 3.4 Narrow module visibility or mark unavoidable implementation exports explicitly unstable, and add external smoke crates/examples that use only the supported facade.
- [ ] 3.5 Add crate-level and public-boundary documentation sufficient for docs.rs while preserving the fixed render frame ABI and dependency direction.

## 4. Prepare Publishable Packages

- [ ] 4.1 Rename package/code/document references from `maku-mesh-touhou` to `maku-render-touhou` atomically, without extracting a generic renderer crate.
- [ ] 4.2 Add workspace-shared version, edition, MSRV, license, repository, homepage/documentation, keywords/categories, and package README metadata as appropriate.
- [ ] 4.3 Add compatible versions to every local path dependency and verify registry-normalized dependency graphs and publication order.
- [ ] 4.4 Define intentional package include/exclude contents for engine, render pack, player, and web host; keep editor/site-only files out of Rust archives unless explicitly required.
- [ ] 4.5 Check crates.io name availability/ownership and document whether `maku-web` is registry-facing or solely a wasm/npm artifact producer.
- [ ] 4.6 Run `cargo package --list` and isolated extracted-package build/test verification for each package in dependency order.
- [ ] 4.7 Run crates.io publication dry-runs with no workspace-owned metadata, external-file, or path-without-version warnings.

## 5. Version Browser Artifacts and Host Boundaries

- [ ] 5.1 Define and emit a browser release identity containing package versions, source revision, frame ABI, and wasm/JavaScript tool versions.
- [ ] 5.2 Make wasm-bindgen glue and JavaScript wrapper compatibility fail early on mixed release/frame ABI versions.
- [ ] 5.3 Add deterministic generated-binding freshness verification or move generated bindings to a release-only producer with an equivalent clean-checkout test.
- [ ] 5.4 Add focused native player and wasm host tests for input forwarding, errors, mixed frame construction, manifest resolution, typed-view bounds, and resource lifetime.

## 6. Final Release-Readiness Verification

- [ ] 6.1 Run the authoritative fast and release checks from a clean checkout and confirm no tracked/generated drift remains.
- [ ] 6.2 Build all package archives, wasm/npm artifacts, docs.rs surfaces, and supported external examples from declared versions only.
- [ ] 6.3 Record the coherent pre-release version/source revision and migration notes consumed by the docs/demo change.
- [ ] 6.4 Run strict OpenSpec validation and confirm package/API decisions remain compatible with `render-rows`, `mesh-renderer-api`, determinism, and lowering contracts.
