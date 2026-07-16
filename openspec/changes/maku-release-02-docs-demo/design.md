## Context

The canonical engine now exposes `core::host::Instance`, per-kind render schemas, and the backend-neutral Touhou render pack, but public prose still describes the earlier core-owned palette/dots/beams path. The deployed neen.ink project is a customized, vendored frontend with an older wasm ABI; replacing only its wasm file would break it. The site repository has independent UI and deployment ownership, so synchronization must be explicit rather than an unrecorded directory copy.

This change consolidates the old `host-api-docs` and `language-reference` stubs into one release-oriented documentation slice. Settled semantics remain owned by capability specs; user docs explain them without becoming a second normative authority.

## Goals / Non-Goals

**Goals:**
- Give card authors a complete lookup path from tutorial to language reference.
- Give host authors one supported embedding guide centered on `Instance` and render-pack/BYO-renderer boundaries.
- Make the upstream web demo and neen.ink deployment consume one coherent, revisioned wasm/JavaScript/render artifact.
- Demonstrate the Touhou pack's semantic axes and resource/material behavior.
- State exactly what is Canvas2D today and what a future WebGPU adapter can consume.

**Non-Goals:**
- Implement a WebGPU adapter or GPU simulation backend.
- Turn internal OpenSpec rationale into user documentation.
- Make neen.ink's site-specific UI canonical upstream UI.
- Automatically publish registry/npm releases.
- Document unsettled backlog syntax such as blocking lasers, state return routing, or future stages re-expression as existing behavior.

## Decisions

### Organize docs by audience and authority

`docs/language-reference.md` is lookup-oriented and generated/checked against settled headings and examples from `openspec/specs/language/spec.md`; tutorials remain narrative. `docs/host-api.md` documents the supported facade and lifecycle. Renderer/package guides explain transport, Touhou pack, Canvas, and BYO integration. Capability specs remain normative and docs link to source/version rather than copying internal implementation decisions.

### Treat the browser runtime as an atomic artifact set

A release manifest records engine/render package versions, source commit, frame ABI, wasm-bindgen/tool versions, hashes, and the exact wasm glue/JS modules/static assets to synchronize. The JS wrapper rejects incompatible frame ABI versions. The web build emits the manifest alongside artifacts.

### Keep upstream runtime logic separate from site chrome

Upstream owns wasm loading, VFS/card loading, input forwarding, render-frame decoding, Canvas render-pack drawing, errors, and smoke fixtures. Neen.ink owns page layout, drawer/modal behavior, links, and site routing. Downstream integration imports or synchronizes the upstream runtime module and applies a small site adapter rather than maintaining a fork of render protocol logic.

### Coordinate two repositories explicitly

The Maku repository produces and validates the release artifact plus a downstream checklist. The neen.ink repository consumes a pinned artifact in a separate coherent commit, records the Maku source revision, updates copied cards/tutorials only through the declared manifest, and runs route/browser smoke checks. The OpenSpec apply session must treat the second repository as coordinated downstream work rather than silently editing outside its declared root; completion records both commit ids.

### Demonstrate semantic rendering rather than raw throughput

A dedicated deterministic showcase card exercises family/variant independently from color, hue/alpha/scale/theta, radial versus directional orientation, active/warning ribbons, layered materials, and fallback diagnostics. It remains small enough for Canvas and is not used as the scale benchmark.

### Document WebGPU compatibility without promising an adapter

The web guide maps fixed v1 sprite layouts, strip vertices/indices, material manifests, and ordered commands to WebGPU concepts. It states that wasm views still require GPU upload and that command order is authoritative. The wasm manifest exports all sampler fields, including separate minimum and maximum filters, so future adapters do not rely on Canvas assumptions. WebGPU rendering is clearly distinct from the compute-focused `gpu-kernel-backend` plan.

### Test deployment behavior, not only build output

Smoke coverage loads the actual project route, verifies JS and wasm MIME/import behavior, loads a representative card and libraries, advances simulation, builds a frame, resolves every referenced resource/material, observes nonempty sprite and ribbon commands, and reaches tutorial routes. A source-revision assertion prevents deploying a mixed snapshot.

## Risks / Trade-offs

- [Documentation drifts from specs] → Link each section to governing capability names and add checks for examples/API symbols without treating docs as semantic authority.
- [Downstream customization forks protocol code again] → Keep site customization behind a narrow adapter and synchronize an upstream runtime module as a unit.
- [The larger render-pack wasm regresses load experience] → Report compressed/uncompressed size, cache immutable hashed artifacts, and measure load separately from steady-state rendering.
- [Canvas performance obscures engine performance] → Label Canvas as a compatibility frontend and link staged benchmark results rather than making aggregate claims.
- [Cross-repository work cannot be atomic] → Pin hashes/revisions and retain the prior deployed artifact for rollback.
- [Future APIs invalidate docs] → Document only settled current behavior and version examples against the release manifest.

## Migration Plan

1. Finish release artifact naming/version decisions from `maku-release-01-readiness`.
2. Write canonical language/host/render/package documentation and migrate stale player/web prose.
3. Update the upstream demo and showcase against the current render-pack ABI; add complete sampler exports and smoke coverage.
4. Build a revisioned artifact and downstream sync manifest.
5. In the neen.ink repository, preserve site chrome while replacing legacy runtime integration and synchronizing selected docs/cards.
6. Run local browser and deployment-route smoke checks, deploy, and verify production.
7. Record upstream/downstream revisions and retain the previous deploy artifact for rollback.

## Open Questions

- Whether neen.ink consumes the npm package directly or a hashed vendored release bundle; either path must use the same release manifest.
- Whether the first public demo defaults to Canvas for reach or conditionally selects a later WebGPU adapter.
- Final canonical URLs for API docs, source links, and downloadable package artifacts.
