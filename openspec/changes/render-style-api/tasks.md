## 1. Profile and resource contract

- [ ] 1.1 Add typed ids, palette shades, texture sources/regions, sampler/blend descriptors, material descriptors, sprite/beam styles, orientation policy, layer color bindings, unknown-style policy, and `TouhouProfile` builder types in `proto/mesh-touhou`.
- [ ] 1.2 Implement profile validation for duplicate keys, dangling ids, material primitive/layout compatibility, finite positive dimensions, known texture-region bounds, and complete explicit fallbacks.
- [ ] 1.3 Re-express the generated disc/ring atlas, stock palettes, family radii, two-layer dot recipe, beam widths, and alpha material as `TouhouProfile::stock()` data.
- [ ] 1.4 Replace `StyleTable` and direct core palette/radius calls with profile-owned cached `(family, variant)` and color resolution, including deterministic hue/alpha shade resolution and strict/fallback diagnostics.

## 2. Ordered geometry ABI

- [ ] 2.1 Add basic, tinted, and two-endpoint-recolor sprite instance structs; indexed strip vertices; versioned material/resource manifest views; source-layout-aware `DrawCommand`; and reusable `MeshFrame` buffers.
- [ ] 2.2 Implement ordered sprite-layer expansion into the matching fixed instance buffers, including UV regions, size multipliers, alpha/color bindings, additive materials, radial orientation, directional `theta`, and layer angle offsets.
- [ ] 2.3 Implement adjacent-only draw-command coalescing that requires identical material/source layout and contiguous backing ranges, preserving nonadjacent transparent command order.
- [ ] 2.4 Migrate polyline/beam output to indexed draw commands and profile beam styles, applying active/warning policy and multiplying profile thickness by row `width`.
- [ ] 2.5 Update render-batch expansion and schema/style caches so batch and row paths use the same emitters without per-row string/Rc allocation.

## 3. Native and web host cutover

- [ ] 3.1 Update `proto/player` to resolve the cold resource/material manifest and submit ordered instance/indexed commands; isolate any macroquad point-instance expansion/chunking in the player adapter rather than the mesh pack.
- [ ] 3.2 Turn the existing `proto/web` directory into a workspace Rust host crate depending on core and `mesh-touhou`, and update `proto/web/build.sh` to build that crate.
- [ ] 3.3 Move the wasm-bindgen engine/render wrapper above core, export zero-copy typed views for all fixed instance buffers, vertices, indices, and packed commands, and export the cold texture/material manifest plus builtin RGBA resource bytes.
- [ ] 3.4 Update the JavaScript web renderer to resolve opaque resource/pipeline ids, upload builtin or registered external textures, configure declared blend/sampler state, and submit the ordered commands without JSON geometry copies.
- [ ] 3.5 Delete core's Touhou-specific web mesh flattening and stock palette/hue/radius helpers after all native/web call sites use `TouhouProfile`; leave core with pack-neutral host APIs only.

## 4. Behavioral verification

- [ ] 4.1 Add profile-validation and unknown-style tests covering every rejected reference/invariant, strict errors, explicit fallbacks, and cache invalidation on profile replacement.
- [ ] 4.2 Add stock-compatibility fixtures covering family sizes, palette/hue/alpha colors, fill/outline ordering, active/warning beam widths, beam row-width scaling, and custom texture/material registration.
- [ ] 4.3 Add fixed-instance-layout tests proving fixed/tint styles do not populate recolor buffers, directional/radial `theta` behavior, additive layered commands, and material/layout mismatch rejection.
- [ ] 4.4 Add row-versus-batch byte-equivalence tests for every instance buffer, indexed geometry, ids, and command order, including the `A, B, A` non-coalescing boundary.
- [ ] 4.5 Add an allocation-counting warmed-build test that covers schema, style, palette, hue, layered sprite, beam, and command paths within reserved capacities.
- [ ] 4.6 Add focused native and wasm host smoke tests that resolve every emitted resource/material, submit mixed sprite/beam frames in order, and verify exported typed-view bounds remain valid until the next build.
