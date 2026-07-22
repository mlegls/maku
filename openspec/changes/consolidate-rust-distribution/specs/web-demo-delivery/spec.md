## MODIFIED Requirements

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
