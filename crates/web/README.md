# maku-web

`maku-web` is the Rust WebAssembly host layer for Maku. It owns a supported
`maku::host::Instance` and a stock `maku-render-touhou` pack. Browser
applications normally consume the versioned `@mlegls/maku` package built from
`crates/js/maku` rather than calling this Rust crate directly.

## Lifecycle

1. Initialize the wrapper; it checks engine version and frame ABI.
2. Create `Maku`, add every VFS card/path import, and boot a pattern.
3. Set typed inputs and call `step` at fixed 120 Hz simulation cadence.
4. Call `build_render_frame` once per displayed simulation snapshot.
5. Resolve packed commands in order against the cold material/texture manifest.

The wasm host supports semantic sprite and beam render kinds. It exposes basic,
tinted, and recolor sprite bytes; ribbon vertex and `u32` index views; and an
explicit packed `u32` command stream. Material accessors include pipeline,
texture, source layout, blend, fixed color, separate minification/magnification
filters, and U/V address modes.

## View lifetime

Geometry and builtin-texture getters return views into wasm linear memory. The
next frame build clears/reuses pack vectors, and any operation that grows wasm
memory invalidates previously acquired JavaScript views. Draw/upload every used
range immediately, or make an owned `.slice()` before another mutating call.
Holding a JavaScript typed-array view does not pin wasm memory.

## Renderer ownership

The reusable `static/canvas-renderer.js` module is a **Canvas2D compatibility
adapter** over the ordered frame ABI. `static/main.js` supplies page UI, input,
and card-selection policy through that narrow adapter. Canvas2D consumes the
same command and material/resource manifest as other hosts; it is not a WebGPU
implementation or an engine-throughput benchmark.

A WebGPU host can map the fixed records to vertex/index layouts, but must copy
used wasm ranges into host-owned GPU buffers, create textures/samplers/pipelines,
submit commands in order, and manage in-flight resource lifetime. This is GPU
rendering of a CPU/wasm-produced frame, not the planned GPU simulation backend.

Exact offsets, enum values, ordering, sampler behavior, and WebGPU mapping are
specified in [`docs/renderer-api.md`](../../docs/renderer-api.md). The generated
artifact identity and synchronization policy are documented in
[`crates/js/maku/wasm/README.md`](../js/maku/wasm/README.md).

Build and test through `build.sh` and `static/smoke.mjs`; release verification
uses `scripts/check-generated.sh` so wasm, bindgen glue, wrapper, and identity
manifest cannot drift independently.
