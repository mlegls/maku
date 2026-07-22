# maku-player

`maku-player` is Maku's private native Macroquad producer. Ready-to-run builds
are distributed through [GitHub Releases](https://github.com/mlegls/maku/releases/latest),
not as a public Rust SDK crate. It runs cards through
the supported `maku::host::Instance` facade, builds `TouhouProfile::stock()`
frames, and submits the pack's authoritative ordered commands.

```sh
cargo run --manifest-path crates/Cargo.toml -p maku-player -- cards/reimu_vs_mima.maku
```

The player demonstrates lifecycle, inputs, live evaluation, timeline/scrub,
material resolution, and native drawing. It is a reference adapter rather than
the renderer contract; hosts may consume core transport directly or use another
pack/backend.

The Macroquad adapter currently CPU-expands sprite instances, remaps `u32`
ribbon indices into backend-sized chunks, supports builtin stock textures and
pipeline keys, clamp addressing, and equal minification/magnification filters.
Unsupported profile requirements fail explicitly rather than changing policy.
For the portable contract and WebGPU-ready layouts, see
[`docs/renderer-api.md`](../../docs/renderer-api.md).

See [`docs/player.md`](../../docs/player.md) for controls, wire commands, and
editor integration.
