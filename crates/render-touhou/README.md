# Touhou render pack

`maku-render-touhou` is a genre-facing host package, not an engine renderer or a
universal mesh abstraction. Hosts configure Touhou palettes, family/variant
recipes, local layers, materials, and resources through `TouhouProfile`. The
hot path binds stable render schemas, consumes rows or typed batches, and emits
one reusable ordered `MeshFrame`.

## Version 1 host ABI

Materials declare exactly one standard source layout:

- basic sprite transform/UV/alpha;
- tinted sprite transform/UV plus one RGBA tint;
- recolor sprite transform/UV plus low/high RGBA endpoints;
- indexed strip vertices and indices.

A custom pipeline key may select a host shader only when that shader consumes
the declared layout. Arbitrary per-instance attributes are not accepted.
Textures contain builtin immutable RGBA8 bytes or an external resource key;
the host owns loading, upload, sampler/blend state, pipeline creation, and GPU
lifetime.

Draw commands are authoritative frame order. Only adjacent commands with the
same material/layout and contiguous source ranges are coalesced. Hosts must not
globally regroup transparent commands.

## Effects

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
