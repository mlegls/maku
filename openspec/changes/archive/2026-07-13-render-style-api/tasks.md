## 1. Pack, profile, and schema contracts

- [x] 1.1 Organize `crates/mesh-touhou` around a genre-facing pack, immutable `TouhouProfile`, schema bindings, shared output arena, resource registry, and internal sprite/ribbon processors without extracting a generic crate.
- [x] 1.2 Add typed ids, palette shades, texture sources/regions, sampler/blend descriptors, material descriptors, sprite recipes, layered beam recipes, orientation policy, layer color bindings, unknown-style policy, and primitive capability descriptors.
- [x] 1.3 Implement profile validation for duplicate keys, dangling ids, material primitive/layout compatibility, effect capability requirements, finite positive dimensions, known texture-region bounds, and complete explicit fallbacks.
- [x] 1.4 Bind declared `:sprite` and `:beam` schema identities once to batch-aware handlers, validating required field names/kinds and compiling cold semantic configuration to stable numeric ids and primitive recipes.
- [x] 1.5 Re-express the generated disc/ring atlas, stock palettes, family radii, two-layer dots, layered beam widths, and alpha materials as `TouhouProfile::stock()` data.
- [x] 1.6 Replace `StyleTable` and direct core palette/radius calls with profile-owned cached `(family, variant)` and color resolution, deterministic hue/alpha shade resolution, and strict/fallback diagnostics.

## 2. Shared primitive output and local effects

- [x] 2.1 Add basic, tinted, and two-endpoint-recolor sprite instance structs; indexed strip vertices; versioned primitive/material/resource manifest views; source-layout-aware `DrawCommand`; and one reusable pack-owned `MeshFrame`.
- [x] 2.2 Implement ordered sprite recipe emission into shared fixed-layout buffers, including UV regions, size multipliers, alpha/color bindings, radial orientation, directional `theta`, and layer angle offsets.
- [x] 2.3 Implement ordered layered ribbon emission, including per-layer material/texture, width and alpha multipliers, active/warning policy, joins/caps, and row `width` scaling.
- [x] 2.4 Implement primitive effect-surface validation and compile compatible sprite/ribbon adapters such as additive halos into ordinary ordered recipe layers; reject missing channels and arbitrary custom payloads.
- [x] 2.5 Implement adjacent-only command coalescing across the shared stream, requiring identical material/source layout and contiguous ranges while preserving cross-handler frame order.
- [x] 2.6 Update row and batch paths to use the same bound handlers and primitive emitters without batch expansion or per-row string/Rc allocation.
- [x] 2.7 Keep fixed batch bindings, compiled recipe cardinality, source layouts, and variable-ribbon sizing seams explicit for future CPU/SIMD/GPU execution planning without implementing a GPU mesher in this change.

## 3. Native and web host cutover

- [x] 3.1 Update `crates/player` to resolve the shared cold resource/material manifest and submit the pack's ordered instance/indexed commands; isolate macroquad point-instance expansion or chunking in the adapter.
- [x] 3.2 Turn the existing `crates/web` directory into a workspace Rust host crate depending on core and `mesh-touhou`, and update `crates/web/build.sh` to build that crate.
- [x] 3.3 Move the wasm-bindgen engine/render wrapper above core and export zero-copy typed views for all fixed instance buffers, vertices, indices, packed commands, and builtin resource bytes.
- [x] 3.4 Update the JavaScript renderer to resolve opaque resource/pipeline ids, upload builtin or registered external textures, configure declared blend/sampler state, and submit the shared ordered stream without JSON geometry copies.
- [x] 3.5 Keep group/offscreen and frame effects in the host compositor; document that compatible custom pipeline keys consume standard pack layouts and do not inject arbitrary attributes.
- [x] 3.6 Delete core's Touhou-specific web flattening and stock palette/hue/radius helpers after all native/web call sites use the pack; leave core with pack-neutral host APIs only.

## 4. Behavioral verification

- [x] 4.1 Add profile, schema-binding, material/layout, and effect-capability validation tests covering every rejected invariant, strict errors, explicit fallbacks, and cache invalidation on profile replacement.
- [x] 4.2 Add stock-compatibility fixtures covering family sizes, palette/hue/alpha colors, fill/outline ordering, active/warning beam layers, row-width scaling, and custom texture/material registration.
- [x] 4.3 Add fixed-instance-layout tests proving fixed/tint styles do not populate recolor buffers, directional/radial `theta` behavior, compatible sprite halos, layered beam halos, and missing-channel rejection.
- [x] 4.4 Add row-versus-batch byte-equivalence tests for every instance buffer, indexed geometry, ids, and commands, proving compatible batches are consumed without row expansion.
- [x] 4.5 Add mixed-handler ordering tests covering sprite A, beam B, sprite A; adjacent-only coalescing; and one shared resource registry/output arena.
- [x] 4.6 Add an allocation-counting warmed-build test covering schema bindings, style/palette/hue caches, compiled recipe dispatch, layered sprites, layered ribbons, and shared commands.
- [x] 4.7 Add execution-planning tests proving fixed sprite output cardinality is discoverable from a bound batch/recipe and variable ribbon sizing remains a separate deterministic pass.
- [x] 4.8 Add focused native and wasm smoke tests resolving every emitted resource/material, submitting mixed sprite/beam frames in order, and verifying typed-view bounds remain valid until the next build.
