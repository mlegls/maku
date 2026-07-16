# Renderer integration and frame ABI

> **Version:** Maku 0.1, Touhou frame ABI 1. This guide describes CPU/native
> and wasm render data. It does not define a WebGPU adapter or GPU simulation
> backend.

Rendering has three separately measurable stages:

1. `Instance::render_frame()` produces backend-neutral ordered rows/batches.
2. An optional pack such as `TouhouMesh` compiles semantic rows to geometry,
   materials, and resources.
3. The host uploads and draws through Macroquad, Canvas2D, WebGPU, or another
   backend.

Do not describe pack construction or host drawing time as simulation time.

## Bring your own renderer

The stable transport is `maku::render::{RenderItem, RenderBatch, RenderRow,
RenderSchema}`. Configure supported kinds before load, bind each declared
schema once, and consume each frame in stream order.

```rust
let mut instance = maku::host::Instance::new(None);
instance.set_render_kinds(["my-point", "my-ribbon"]);
// Add/load card, then advance fixed ticks.

for item in instance.render_frame() {
    match item {
        maku::render::RenderItem::Batch(batch) => {
            // Consume typed columns directly in lanes 0..batch.len.
            println!("{} x {}", batch.schema.kind, batch.len);
        }
        maku::render::RenderItem::Row(row) => {
            println!("{}", row.kind);
        }
    }
}
```

A batch occupies one position in the ordered stream. Expanding its lanes at
that position is exactly equivalent to the source rows. `RenderSchema::cols`
and `RenderBatch::cols` are parallel. Numeric columns may be constants or row
vectors; optional columns carry presence information. Stable declared-schema
identity lets a renderer cache validated column indices. Never globally sort
transparent rows or batches by kind/material.

`Instance::render()` and `RenderItem::expand_into` are convenient reference
paths. They allocate rows and are not required for a columnar renderer.

## Touhou pack

`maku-render-touhou` is optional policy over core transport. It supports
`:sprite` and `:beam` kinds. A profile owns:

- `(family, variant)` recipe selection and independent color palettes;
- sprite dimensions, radial/directional orientation, and hit/display policy;
- active/warning ribbon layers and fallback diagnostics;
- texture resources, material layouts, blending, samplers, and pipeline keys.

Core owns none of that Touhou policy.

```rust
use maku_render_touhou::{TouhouMesh, TouhouProfile};
use std::rc::Rc;

let mut pack = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
instance.set_render_kinds(TouhouMesh::RENDER_KINDS);
// Load card.
for kind in TouhouMesh::RENDER_KINDS {
    if let Some(schema) = instance.declared_render_schema(kind) {
        pack.bind_schema(kind, schema)?;
    }
}
let transport = instance.render_frame();
let frame = pack.build(&transport)?;
// Resolve frame.draws in order against pack.profile().materials()/textures().
# Ok::<(), maku_render_touhou::MeshError>(())
```

Sprite schemas require symbolic `family`, `color`, and `variant`. Beam schemas
also require numeric `width`; numeric `hue` is optional. Foreign kinds are not
reinterpreted. Fallback behavior emits deduplicated diagnostics rather than
silently changing semantic axes.

`MeshFrame` buffers are pack-owned and reused by the next `build`. Submit or
copy used ranges before another build. Replacing the immutable profile clears
bindings, output, and diagnostics; create a matching host resource set and keep
old GPU resources alive until in-flight work completes.

## Native fixed layouts

All v1 records are `#[repr(C)]` and validated by tests:

| Source | Stride | Fields |
|---|---:|---|
| Basic sprite | 40 bytes | center `f32x2`, half-size `f32x2`, rotation `f32`, UV rect `f32x4`, alpha `u8`, padding |
| Tinted sprite | 44 bytes | basic record plus tint `unorm8x4` |
| Recolor sprite | 48 bytes | basic record plus low/high colors, each `unorm8x4` |
| Ribbon vertex | 20 bytes | position `f32x2`, UV `f32x2`, color `unorm8x4` |
| Ribbon index | 4 bytes | absolute `u32` vertex index |

Native `DrawCommand` is a typed Rust value. Do not serialize its enum memory;
the browser ABI below uses explicit packing.

## Browser frame ABI 1

Wasm frame bytes are little-endian. Sprite fields have these byte offsets:

| Layout | Offset | Field |
|---|---:|---|
| all sprites | 0 | center `float32x2` |
| | 8 | half-size `float32x2` |
| | 16 | rotation degrees `float32` |
| | 20 | normalized `(u0,v0,u1,v1)` `float32x4` |
| | 36 | alpha `uint8` (bytes 37–39 padding) |
| tinted | 40 | tint RGBA `unorm8x4` |
| recolor | 40 | low RGBA `unorm8x4` |
| recolor | 44 | high RGBA `unorm8x4` |

Ribbon vertices use position at byte 0, UV at byte 8, and RGBA at byte 16.
Indices are absolute `uint32` values into the complete used vertex buffer.

### Packed commands

`draw_command_stride()` is **8 `u32` words** (32 bytes):

| Word | Meaning |
|---:|---|
| 0 | material id |
| 1 | source tag: 0 basic, 1 tinted, 2 recolor, 3 indexed ribbon |
| 2 | sprite start or indexed vertex start |
| 3 | sprite count or indexed vertex count |
| 4 | indexed index start; zero for sprites |
| 5 | indexed index count; zero for sprites |
| 6–7 | reserved zero |

Commands are authoritative. The pack may merge only adjacent compatible
contiguous ranges; an adapter must not regroup them globally.

### Material and sampler manifest

| Material layout | Value |
|---|---:|
| basic sprite | 0 |
| tinted sprite | 1 |
| recolor sprite | 2 |
| indexed ribbon | 3 |

| Blend | Value |
|---|---:|
| opaque | 0 |
| alpha | 1 |
| additive | 2 |
| soft additive | 3 |

| Filter | Value |
|---|---:|
| nearest | 0 |
| linear | 1 |

| Address | Value | WebGPU |
|---|---:|---|
| clamp | 0 | `clamp-to-edge` |
| repeat | 1 | `repeat` |
| mirror | 2 | `mirror-repeat` |

The manifest exports separate **minification** and **magnification** filters,
U/V address modes, blend, layout, pipeline key, texture id, and optional fixed
color. `material_fixed_color()` packs R in bits 0–7 through A in bits 24–31.
Layout determines whether fixed color applies; zero also represents no fixed
color.

Builtin textures are immutable tightly packed RGBA8 with declared width and
height. External textures expose an opaque resource key for host resolution.
Stock pipeline keys are `touhou.sprite.v1` and `touhou.ribbon.v1`; custom keys
must still consume the declared fixed source layout.

## Canvas2D compatibility adapter

The bundled browser frontend is a **Canvas2D compatibility renderer**. It
loads the same cold texture/material manifest, reads the same packed buffers,
and submits the same ordered command stream as any other host. It is not a
WebGPU implementation and its frame rate is not evidence of engine-only or
native renderer throughput.

Canvas has no direct equivalent for every GPU blend/sampler combination. An
unsupported material must fail or diagnose explicitly rather than silently
changing pack policy.

## Mapping ABI 1 to WebGPU

ABI 1 is WebGPU-compatible input data, not an implemented adapter and not GPU
zero-copy.

Recommended vertex mappings:

- basic/tinted/recolor: instance strides 40/44/48 with attributes at offsets
  0, 8, 16, 20, 36, and optional color attributes at 40/44;
- ribbon: vertex stride 20 with `float32x2`, `float32x2`, `unorm8x4`;
- ribbon index format: `uint32`.

Use vertex attributes rather than mirroring packed records as WGSL storage
structs, whose alignment rules differ. A sprite pipeline also supplies static
quad corners or derives them from vertex index.

A host should:

1. resolve the cold manifest and create host-owned textures, samplers, and
   pipelines;
2. allocate reusable `VERTEX|COPY_DST` and `INDEX|COPY_DST` buffers;
3. call `build_render_frame()`;
4. acquire and upload only each used byte range;
5. iterate commands in order, binding each command's material;
6. retain old resources until submitted work that references them completes.

Wasm geometry/texture getters return views into linear memory. A subsequent
frame build reuses or reallocates vectors, and any memory growth invalidates
old JavaScript views. Upload immediately or make an owned `.slice()` before the
next mutating Maku call. `GPUQueue.writeBuffer` performs a wasm-to-GPU copy; it
does not bind wasm memory directly.

WebGPU rendering here still runs simulation, rules, collision, render
transport, and pack construction on the CPU/wasm side. The planned
`gpu-kernel-backend` concerns compute execution and is a separate future
capability.

## Resource and error policy

The frontend owns asset I/O, decoded images, GPU/Canvas objects, swapchain,
submission, device loss, and teardown. The pack owns immutable semantic
resource descriptions and per-frame CPU buffers. Resolve every material and
texture referenced by a command before drawing. Reject mismatched frame ABI,
layout, pipeline, or sampler metadata before reading buffers.

For the wasm typed-view lifetime and package identity checks, see
[`../crates/web/README.md`](../crates/web/README.md). For native reference-host
limitations, see [`../crates/player/README.md`](../crates/player/README.md).
