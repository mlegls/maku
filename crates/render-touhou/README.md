# Touhou render pack

`maku-render-touhou` is a genre-facing host package, not an engine renderer or a
universal mesh abstraction. Hosts configure Touhou palettes, family/variant
recipes, local layers, materials, and resources through immutable
`TouhouProfile`. The hot path binds stable render schemas, consumes rows or
typed batches, and emits one reusable ordered `MeshFrame`.

```rust
use maku_render_touhou::{TouhouMesh, TouhouProfile};
use std::rc::Rc;

let mut pack = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
for kind in TouhouMesh::RENDER_KINDS {
    if let Some(schema) = instance.declared_render_schema(kind) {
        pack.bind_schema(kind, schema)?;
    }
}
let transport = instance.render_frame();
let frame = pack.build(&transport)?;
// Resolve frame.draws in order against the profile's materials/textures.
```

The pack supports `sprite` and `beam`. Sprite schemas require symbolic
`family`, `color`, and `variant`; beam schemas also require numeric `width` and
may provide numeric `hue`. Foreign kinds are ignored rather than reinterpreted.

## Version 1 host ABI

Materials declare exactly one standard source layout:

- basic sprite transform/UV/alpha: 40-byte records;
- tinted sprite transform/UV plus one RGBA tint: 44-byte records;
- recolor sprite transform/UV plus low/high RGBA endpoints: 48-byte records;
- indexed ribbon vertices: 20-byte records with absolute `u32` indices.

A custom pipeline key may select a host shader only when that shader consumes
the declared layout. Arbitrary per-instance attributes are not accepted.
Textures contain builtin immutable RGBA8 bytes or an external resource key;
the host owns loading, upload, sampler/blend state, pipeline creation, render
targets, submission, and GPU lifetime.

Draw commands are authoritative frame order. Only adjacent commands with the
same material/layout and contiguous source ranges are coalesced. Hosts must not
globally regroup transparent commands.

The returned frame borrows pack-owned reusable vectors until the next `build`.
Submit or copy used ranges first. `replace_profile` clears output, schema
bindings, and diagnostics; rebuild host resources and retain old GPU objects
until in-flight work completes. Native `DrawCommand` is typed Rust data; the
browser uses a separate explicit packed command ABI rather than enum memory.

## Effects and policy

Material/layer and primitive-coupled geometry effects are cold profile data.
Adapters declare required primitive capabilities and compile to ordinary
ordered sprite or ribbon layers. The processors expose layouts, channels, and
composition operations rather than a catalog of named effects.

Group/offscreen and whole-frame effects are deliberately outside this API.
Render targets, screen-space bloom, color grading, and composition belong to
the native or web host render graph. They must not be smuggled into primitive
flags or custom payloads.

The crate keeps schema bindings, fixed recipe cardinality, output layouts, and
a separate variable-ribbon sizing seam visible so later CPU/SIMD/GPU executors
can plan or fuse compatible work without reconstructing semantic rows. No GPU
backend or one-kernel-per-tick promise is made here.

See [`docs/renderer-api.md`](../../docs/renderer-api.md) for BYO core transport,
exact browser offsets/enums, Canvas2D limitations, and WebGPU mapping.
