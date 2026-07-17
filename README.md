# Maku

An engine-agnostic bullet-hell ("danmaku") engine and pattern language,
derived from an audit of [Danmokou](https://github.com/Bagoum/danmokou)'s
semantics and SuperCollider's signal model. Patterns are data (EDN cards);
motion is signal composition with an explicit closed-form/integrated split;
the whole gameplay state is a deterministic fold over input tapes — so
replays, rewind, and live code-swap are exact by construction.

## Layout

| path | contents |
|---|---|
| `docs/README.md` | documentation map by audience and version |
| `docs/language-reference.md` | card-author lookup reference; capability specs remain authoritative |
| `docs/host-api.md` | supported Rust embedding lifecycle |
| `docs/renderer-api.md` | BYO transport, Touhou pack, Canvas2D, and WebGPU-ready ABI |
| `docs/player.md` | debug player: wire protocol, session/scrubbing, controls |
| `docs/tutorials/` | learn the language from scratch — runnable companions in `cards/tutorials/` |
| `docs/from-dmk.md` | mapping notes for readers coming from Danmokou/BDSL |
| `openspec/` | specs (settled contracts + design), changes (all open work — `openspec list`) |
| `crates/` | Rust workspace: `core` (engine/session/host), `render-touhou` (render pack), `player` (Macroquad host), `web` (wasm/Canvas host), `editors/danmaku.nvim` |
| `crates/js/maku/` | publishable browser package wrapping the wasm host |
| `cards/` | playable cards — start with `reimu_vs_mima.maku` |
| `cards/translations/` | the DMK translation corpus (validation exercise) + working records |
| `dmk-corpus/` | the upstream DMK scripts translated (MIT) |

## Quickstart

```sh
# play the demo fight: WASD move, Shift focus, X bomb
cargo run --manifest-path crates/Cargo.toml -p maku-player -- cards/reimu_vs_mima.maku

# core conformance, gameplay, and session/scrubbing tests
cargo test --manifest-path crates/Cargo.toml -p maku
```

Live editing: the player is a server (`docs/player.md`); install
`crates/editors/danmaku.nvim` and evaluate forms into the running game
(`<localleader>e` operator — run/swap/layer, all scrub-safe).

Browser: use the public [neen.ink Maku player](https://neen.ink/projects/maku/play.html)
or browse the [interactive tutorials](https://neen.ink/projects/maku/tutorials.html).
For local development, run `crates/web/build.sh serve` and open
`http://localhost:8000/crates/web/static/`. Both use the same wasm engine and
Canvas2D compatibility adapter, with the same controls and an in-page eval box
speaking the wire protocol. See the [benchmark reports](docs/benchmarks/), including the [July 2026 staged f64 baseline](docs/benchmarks/2026-07-f64-baseline.md)
for native/wasm p95/p99 headroom, memory, workload shape, and raw results.

## Development toolchain

The release toolchain is pinned in `rust-toolchain.toml` and `mise.toml`:
Rust 1.97 (the initial MSRV), Bun 1.3.14, Node 24.18.0, and wasm-pack
0.15.0. Run commands through `mise` or install matching versions manually.
The wasm target is installed by the Rust toolchain declaration.

Native player builds require the platform C/graphics toolchain used by
Macroquad. On macOS, install Xcode Command Line Tools; `crates/.cargo/config.toml`
selects the system Apple C driver to avoid incompatible third-party `ld64`
versions earlier in `PATH`. Linux hosts need their distribution's C compiler,
X11, OpenGL, and audio development packages.

`scripts/check.sh fast` is the normal local and pull-request gate.
`scripts/check.sh release` adds the lowering oracle, ignored card suites,
wasm artifact rebuild/smoke test, and clean-tree verification. The same entry
points are exposed as `mise run check` and `mise run release-check`.
