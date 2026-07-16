## Why

Maku's tutorials, host documentation, and deployed neen.ink demo describe an older core-owned dots/beams renderer and vendored wasm snapshot, while the current engine exposes a host facade and Touhou render pack with typed buffers and material manifests. Public documentation and the live demo must present one versioned, canonical experience before release claims are shared externally.

## What Changes

- Write a lookup-oriented Maku language reference from settled capability specs while keeping tutorials task-oriented and Maku-first.
- Document the supported `core::host::Instance` embedding boundary, lifecycle, inputs/channels/events, render-schema negotiation, session/replay behavior, and renderer integration without documenting storage or interpreter internals as stable APIs.
- Update canonical player, web, render-pack, and tutorial documentation to describe host/profile-owned rendering and bring-your-own-renderer use.
- Add a web render-pack showcase covering sprite family/variant/color axes, hue/alpha/scale/orientation, beam warning/active layers, and actionable fallback behavior.
- Produce a coherent versioned wasm/JavaScript/demo artifact and a downstream sync manifest containing package version, source revision, files, and integrity information.
- Coordinate the downstream refresh of `~/dev/neen-ink/projects/maku`: preserve site-specific UI, replace legacy `dots()`/`beams()` integration, synchronize cards/tutorials/manifests, and add route/wasm/render smoke checks.
- Keep Canvas2D as a clearly labeled compatibility frontend; document that the render-pack ABI is WebGPU-compatible and define the missing adapter boundary without conflating rendering WebGPU with the planned simulation compute backend.
- Supersede the active `host-api-docs` and `language-reference` backlog stubs with this implementation-ready release slice.

## Capabilities

### New Capabilities
- `public-documentation`: Canonical language, host API, renderer integration, package, and migration documentation requirements.
- `web-demo-delivery`: Version-pinned wasm/JavaScript demo artifacts, render-pack frontend behavior, downstream synchronization, and deployment verification.

### Modified Capabilities

## Impact

- `docs/`, package READMEs, `crates/web/static`, JavaScript/wasm release artifacts, tutorial prose, cards selected for the showcase, and host/render integration examples.
- Coordinated downstream work in the separate neen.ink repository; the Maku repository owns the versioned artifact and sync contract, while the site repository owns its customized UI and deployment.
- Existing planning stubs `openspec/changes/host-api-docs/` and `openspec/changes/language-reference/` become superseded rather than parallel sources of work.
- Documentation cites settled behavior from `openspec/specs/language/spec.md`, `openspec/specs/session/spec.md`, `openspec/specs/render-rows/spec.md`, and `openspec/specs/mesh-renderer-api/spec.md`.
