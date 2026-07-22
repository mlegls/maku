# Web Demo Delivery Specification

## Purpose

Define versioned browser artifacts, renderer-neutral frame delivery, explicit bundled-pack capabilities, synchronized downstream integration, and deployed-route verification for Maku demos.

## Requirements

### Requirement: Atomic web artifact manifest
The web build SHALL emit a manifest that identifies the public Maku/npm version, bundled render-pack identities and contract versions, source revision, frame ABI, wasm/tool versions, artifact paths, and integrity hashes for the wasm binary, bindgen glue, JavaScript wrapper, renderer module, and required static resources. Private Cargo producer package versions MUST NOT be exposed as independently supported browser components. A frontend MUST reject incompatible frame ABI or missing required-pack combinations before reading typed buffers.

#### Scenario: Complete synchronized artifact
- **WHEN** the upstream demo or a downstream site consumes a release manifest
- **THEN** every loaded runtime component and hash belongs to that one declared release and its bundled pack set includes Touhou

#### Scenario: Partial file copy
- **WHEN** new wasm bytes are paired with legacy `dots()`/`beams()` host JavaScript
- **THEN** initialization or deployment verification fails before the demo is published

#### Scenario: Unsupported pack requirement
- **WHEN** a card or adapter requires a render pack absent from the manifest
- **THEN** loading fails with an explicit pack-capability diagnostic rather than silently treating the pack as universally available

### Requirement: Render-pack web showcase
The upstream demo SHALL include a deterministic card/profile showcase for every render pack declared by the browser distribution. The initial Touhou showcase SHALL visibly exercise independent sprite family, variant, and color resolution; hue, alpha, scale, and orientation; radial and directional recipes; active and warning ribbon layers; and explicit fallback diagnostics. Each showcase SHALL resolve all emitted resources and materials through the cold manifest.

#### Scenario: Showcase frame
- **WHEN** the Touhou showcase reaches its declared representative tick
- **THEN** the frame contains valid sprite and ribbon commands covering the documented semantic axes and every command resolves to an available material and texture

#### Scenario: Frontend pack declaration
- **WHEN** another pack is compiled into a future browser distribution
- **THEN** the manifest and showcase coverage add that pack explicitly without changing the promise for frontends that omit it

### Requirement: Backend-neutral browser frame contract
The wasm host SHALL expose all data needed by Canvas2D and WebGPU adapters, including fixed sprite/strip layouts, indices, ordered commands, material pipeline/layout/blend/fixed-color data, texture resources, and complete sampler minimum/maximum filter and address modes. Typed views SHALL remain subject to documented wasm-memory lifetime and GPU upload requirements.

#### Scenario: WebGPU adapter inspection
- **WHEN** a browser frontend maps the manifest and frame ABI to WebGPU
- **THEN** it can create faithful vertex/index layouts, pipelines, textures, and samplers without relying on undocumented Canvas defaults

#### Scenario: No direct wasm GPU binding
- **WHEN** a frontend uploads a wasm typed view with `GPUQueue.writeBuffer`
- **THEN** documentation identifies the wasm-to-GPU copy and limits uploads to used ranges rather than claiming GPU zero-copy

### Requirement: Canvas frontend is labeled and ordered
The bundled Canvas2D frontend SHALL consume the same ordered render-pack commands and material/resource manifest as other hosts, preserve command order, and identify itself as a compatibility renderer rather than a WebGPU or engine-throughput measurement.

#### Scenario: Mixed command order
- **WHEN** a frame alternates sprite and ribbon materials
- **THEN** Canvas submission follows the authoritative command stream without global material regrouping

### Requirement: Downstream demo synchronization
The Maku repository SHALL produce a downstream sync manifest and checklist. The neen.ink Maku project SHALL record the consumed Maku release/source revision, preserve site-owned UI through a narrow adapter, replace legacy runtime protocol code, and synchronize selected cards/tutorials through declared paths rather than undocumented copies.

#### Scenario: neen.ink refresh
- **WHEN** a new Maku artifact is integrated into `projects/maku`
- **THEN** the downstream commit records the upstream revision and passes the same runtime/frame smoke fixture while retaining site navigation and project styling

### Requirement: Deployed-route verification
Deployment verification SHALL request the Maku project and player routes, validate JavaScript and wasm loading/MIME behavior, load libraries and a representative card, advance simulation, build and draw a nonempty mixed frame, and open tutorial documentation. The prior artifact SHALL remain available for rollback until production verification succeeds.

#### Scenario: Production smoke
- **WHEN** neen.ink deployment completes
- **THEN** an automated browser check confirms the declared source revision and successful mixed sprite/beam rendering at the public route
