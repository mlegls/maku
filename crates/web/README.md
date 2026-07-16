# maku-web

`maku-web` is the Rust WebAssembly host layer for Maku. It owns a
`maku::host::Instance` and `maku-render-touhou` pack, and exports typed views of
fixed sprite, ribbon, index, material, resource, and ordered-command buffers.

Browser applications normally consume the versioned JavaScript package built
from `crates/js/maku` rather than calling this Rust crate directly. Typed views
borrow wasm linear memory and are invalidated by the next frame build or any
operation that grows memory.

See the [repository](https://github.com/mlegls/maku) for the build script,
Canvas frontend, and smoke test.
