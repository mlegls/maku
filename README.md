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
| `openspec/specs/language/spec.md` | **the language spec** (authoritative) |
| `docs/player.md` | the debug player: wire protocol, session/scrubbing, controls |
| `docs/tutorials/` | learn the language from scratch — runnable companions in `cards/tutorials/` |
| `docs/from-dmk.md` | mapping notes for readers coming from Danmokou/BDSL |
| `openspec/` | specs (settled contracts + design), changes (all open work — `openspec list`) |
| `proto/` | Rust prototype: `core` (interpreter/sim/session/host), `player` (macroquad host), `web` (wasm/canvas host), `editors/danmaku.nvim` |
| `proto/js/maku/` | publishable browser package wrapping the wasm host |
| `cards/` | playable cards — start with `reimu_vs_mima.maku` |
| `cards/translations/` | the DMK translation corpus (validation exercise) + working records |
| `dmk-corpus/` | the upstream DMK scripts translated (MIT) |

## Quickstart

```sh
# play the demo fight: WASD move, Shift focus, X bomb
cargo run --manifest-path proto/Cargo.toml -p maku-player -- cards/reimu_vs_mima.maku

# tests (52: conformance corpus + gameplay + session/scrubbing)
cargo test --manifest-path proto/Cargo.toml -p maku
```

Live editing: the player is a server (`docs/player.md`); install
`proto/editors/danmaku.nvim` and evaluate forms into the running game
(`<localleader>e` operator — run/swap/layer, all scrub-safe).

Browser: `proto/web/build.sh serve` then open
`http://localhost:8000/proto/web/static/` — the same engine as wasm, same
controls, plus an in-page eval box speaking the wire protocol.
