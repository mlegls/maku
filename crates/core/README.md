# maku

`maku` is the single public Rust SDK for Maku. Its default feature set provides
the deterministic language/simulation engine, supported host lifecycle
(`maku::host`), embedded source helpers (`maku::source`), and backend-neutral
ordered render transport (`maku::render`) without a genre renderer, Macroquad,
or wasm dependency.

Optional integrations are compiled only when selected:

```toml
[dependencies]
maku = { version = "0.2", features = ["touhou"] }
```

- `touhou` exposes the bundled profile and fixed frame ABI through
  `maku::touhou`.
- `macroquad` enables `touhou`, adds the optional Macroquad dependency, and
  exposes the native adapter through `maku::macroquad`.

Browser applications install `@mlegls/maku`; card authors can download the
native player from GitHub Releases. The web and player Cargo packages in this
repository are private producers, not additional Rust SDK dependencies.

See the [repository](https://github.com/mlegls/maku) for language tutorials,
capability specifications, and complete examples.
