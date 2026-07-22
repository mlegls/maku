# Rust API boundary (0.2)

This inventory defines the compatibility intent used by release checks. It is
not a 1.0 stability promise.

## Installation

Maku has one public Rust SDK with an empty default feature set:

```toml
[dependencies]
maku = "0.2"                         # engine + BYO render transport
maku = { version = "0.2", features = ["touhou"] }
# or features = ["macroquad"], which also enables touhou
```

Browser applications install `@mlegls/maku`. Card authors normally download
`maku-player` from GitHub Releases. The player and wasm Cargo packages are
private build producers rather than additional SDK dependencies.

## Supported embedding surface

- `maku::host`: `Instance`, `Inputs`, `Event`, and `Timeline`; card loading,
  virtual files, host/render capability declarations, fixed-tick advancement,
  typed channel reads, events, render frames, status, and timeline controls.
- `maku::source`: embedded standard-library lookup and source/import expansion.
- `maku::render`: stable schema identity, ordered `RenderItem` transport, typed
  `RenderBatch` columns, rows, and render field/data types.
- `maku::touhou` with feature `touhou`: immutable profile/schema binding, cold
  resources and materials, fixed frame layouts, and ordered frame construction.
- `maku::macroquad` with feature `macroquad`: native resource resolution,
  prepared-frame conversion, and ordered Macroquad submission.

## Explicitly unstable implementation surface

`maku::interp`, `maku::sim`, `maku::session`, `maku::model`, and `maku::edn`
remain temporarily public for in-workspace backend development and migration.
They are hidden from generated package documentation and carry no source or
representation compatibility promise. Interpreter values, world/entity
storage, motion state, lowering programs/executors, collision indices, raw
sessions, and debug cell snapshots may change without a public API migration.

A package-level smoke consumer in `tests/public-api-smoke/` imports only the
supported modules from `maku`. Release checks compile it separately so
workspace-private visibility cannot make an accidental API appear usable.
