# Rust API boundary (0.1)

This inventory defines the compatibility intent used by release checks. It is
not a 1.0 stability promise.

## Supported embedding surface

- `maku::host`: `Instance`, `Inputs`, `Event`, and `Timeline`; card loading,
  virtual files, host/render capability declarations, fixed-tick advancement,
  typed channel reads, events, render frames, status, and timeline controls.
- `maku::source`: embedded standard-library lookup and source/import expansion.
- `maku::render`: stable schema identity, ordered `RenderItem` transport, typed
  `RenderBatch` columns, rows, and render field/data types.
- `maku_render_touhou`: immutable profile/schema binding, cold resources and
  materials, fixed frame layouts, and ordered frame construction.

The native player and wasm package are delivery hosts over this surface, not
extra engine dependencies.

## Explicitly unstable implementation surface

`maku::interp`, `maku::sim`, `maku::session`, `maku::model`, and `maku::edn`
remain temporarily public for in-workspace backend development and migration.
They are hidden from generated package documentation and carry no source or
representation compatibility promise. Interpreter values, world/entity
storage, motion state, lowering programs/executors, collision indices, raw
sessions, and debug cell snapshots may change without a public API migration.

A package-level smoke consumer in `tests/public-api-smoke/` imports only the
supported modules. Release checks compile it separately so workspace-private
visibility cannot make an accidental API appear usable.
