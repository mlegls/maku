## 1. Consolidate Documentation Scope

- [x] 1.1 Reconcile this change with the active `host-api-docs` and `language-reference` stubs, record that their scope is superseded here, and archive or otherwise retire the duplicate active entries without syncing incomplete deltas.
- [x] 1.2 Inventory current docs, README/API examples, upstream web prose, neen.ink copies, and stale core-owned renderer descriptions against the release package/version decisions.
- [x] 1.3 Define the canonical document map, audience, governing capability links, example validation method, and release/source-version presentation.

## 2. Write Canonical Public Documentation

- [x] 2.1 Write the lookup-oriented language reference for settled syntax, values, dynamics/actions, entities, rules, state, channels/inputs, rendering, errors, and determinism, with runnable canonical examples.
- [x] 2.2 Write `docs/host-api.md` around `Instance` construction/loading, negotiation, advancement, inputs/channels/events, render-frame lifetime, session replay/scrub, errors, and shutdown using only supported package APIs.
- [x] 2.3 Update player, web, package, and render-pack guides for profile-owned Touhou policy, ordered typed transport, BYO renderers, material/resources, and host-owned GPU lifetime.
- [x] 2.4 Update tutorials and migration prose that still imply core-owned palettes, radii, sprites, dots, or beams; keep planned language features clearly separate from current behavior.
- [x] 2.5 Add compile/run/syntax checks for Rust, JavaScript, and `.maku` documentation examples against the declared release artifact.

## 3. Complete the Upstream Web Artifact

- [x] 3.1 Export complete material sampler metadata, including separate minimum and maximum filters, and add wasm ABI tests for every manifest field needed by Canvas and WebGPU adapters.
- [x] 3.2 Add release/frame ABI identity checks across wasm, bindgen glue, JavaScript wrapper, and renderer initialization.
- [x] 3.3 Create the deterministic render-pack showcase card/profile covering style axes, orientation, layered active/warning ribbons, and explicit fallback diagnostics.
- [x] 3.4 Update the upstream Canvas frontend to consume the current ordered render-pack ABI for the showcase and label it as Canvas2D rather than generic wasm/WebGPU throughput.
- [x] 3.5 Add browser smoke coverage for loading libraries/cards, advancing, building a mixed frame, resolving every material/resource, drawing ordered sprite/ribbon commands, and opening docs routes.
- [x] 3.6 Document the fixed WebGPU-compatible frame layouts, upload/copy lifetime, shader/material contract, ordered submission, and distinction from GPU simulation without implementing the adapter.

## 4. Produce a Downstream Sync Unit

- [x] 4.1 Emit the versioned web release manifest with package versions, source revision, frame ABI, tool versions, artifact paths, and integrity hashes.
- [x] 4.2 Separate reusable upstream runtime/render integration from page-specific UI through a narrow frontend adapter.
- [x] 4.3 Define the selected card/tutorial/library manifest and synchronization command/checklist for downstream consumers.
- [x] 4.4 Build the complete wasm/JavaScript/static artifact from the release-readiness package versions and verify its hashes from a clean checkout.

## 5. Refresh neen.ink

- [ ] 5.1 In the neen.ink repository, record the Maku upstream release/source revision and replace legacy `dots()`/`beams()` wasm protocol code with the synchronized render-pack runtime while preserving site-owned drawer/modal/navigation UI.
- [ ] 5.2 Synchronize the declared libraries, cards, showcase, tutorials, and manifest; remove stale compatibility forms and outdated rendering prose in the downstream copy.
- [ ] 5.3 Add neen.ink project-route/browser smoke tests covering JS imports, wasm MIME/loading, revision identity, card/library load, mixed frame drawing, and tutorial routing.
- [ ] 5.4 Build and test the downstream site, commit its refresh separately, and record both upstream and downstream commit ids in the release manifest or integration record.
- [ ] 5.5 Deploy with the previous artifact retained, run production smoke checks at the public Maku routes, and roll back on identity/load/render/tutorial failure.

## 6. Final Documentation and Demo Verification

- [ ] 6.1 Run all documentation example checks, upstream browser smoke tests, package link checks, and strict OpenSpec validation.
- [ ] 6.2 Verify that deployed docs/demo versions agree, no legacy dots/beams protocol remains, and Canvas/WebGPU/compute-backend terminology is accurate.
- [ ] 6.3 Confirm the old documentation stubs are no longer active sources of duplicate work and publish the canonical URLs/revision for benchmark reports.
