# The debug player

`crates/player` is the reference host: a sim+render **server** (the
sclang/scsynth split). Editor clients are thin
send-a-form-to-a-socket shims; `crates/editors/danmaku.nvim` is the reference
client (see its README for mappings).

```
cargo run --manifest-path crates/Cargo.toml -p maku-player -- [card.maku [pattern]]
```

With no card argument the player starts empty and waits for clients.
A CLI card argument auto-plays (explicit intent to watch it).

## Wire protocol

Newline-delimited EDN on `127.0.0.1:7777` — the wire format is the card
format. One form per line (clients strip `;` comments and join lines).

| command | effect |
|---|---|
| `(run <forms…>)` | run forms as an anonymous pattern with the current card's defs in scope; the **input tape replays through the new code** up to the current tick (pause → rewind → edit → re-run) |
| `(swap <forms…>)` | generational hot-swap: in-flight bullets keep their old code; new spawns get the new. Recorded on the **command tape** |
| `(add <forms…>)` | layer onto the running sim; the added pattern's clocks anchor at **this tick**. Command-taped |
| `(load "path")` | reload the card from disk (imports expanded) — refreshes defs/menu, does **not** play |
| `(load "path" "pattern")` | reload and play the named pattern |
| `(pattern "name")` | switch pattern in the current card |
| `(restart)` | re-instantiate and run the current pattern (fresh timeline) |
| `(clear)` | stop the running pattern; card stays loaded |
| `(seek N)` / `(step ±N)` | scrub the timeline (pauses) |
| `(snapshots N)` | snapshot cadence in ticks; `0` disables (soak runs) |
| `(resize-entities N)` | explicit host-side entity capacity change; recorded on the command tape |
| `(pause)` / `(resume)` | resume after a rewind **branches** (truncates the future) |

## Scrubbing and replays

Everything you do is recorded: inputs (including the keyboard) on one
tape, live code changes (`add`/`swap`) on another. Scrubbing to any past
tick is exact — even across a hot-swap — and resuming after a rewind
branches the timeline (the old future is discarded). The precise
contract lives in `openspec/specs/session/spec.md`.

## Controls (this host's contract)

| input | channel / effect |
|---|---|
| mouse | mock `$player` (mouse-rig cards) and mock `$nearest-enemy` fallback |
| WASD / arrows | merged `$move-x`/`$move-y`, plus `$p1-move-*` (WASD only) and `$p2-move-*` (arrows only) for co-op rigs (`cards/coop.maku`) |
| Shift | `$focus-firing` |
| X | `$bomb` |
| 1–9 | pattern menu |
| Space | pause/resume (resume after rewind = branch) |
| ←/→, ↑/↓ *(paused)* | scrub ±1 / ±30 ticks |
| r | reload card from disk and restart |
| c | clear |
| drag slider | scrub; orange marks = command-tape entries; faint notches = snapshots |

The host layers the stock player rig (the `player-rig` standard-library
shim; the implementation lives with the Touhou conventions in
`crates/core/lib/touhou.maku`)
into every fresh timeline via the command tape — swap in your own rig
live with the editor. The status line shows tick, entity count, graze, hits, lives.

## The web host

`crates/web` compiles the supported host facade to wasm (`crates/web/build.sh
serve`) and runs the same `Instance` in the browser. It builds the ordered
fixed-layout frame from `maku-render-touhou` and draws it with the bundled
**Canvas2D compatibility adapter**. Keyboard/mouse values become `Inputs`,
cards are fetched into a virtual filesystem, and the eval box speaks the same
wire protocol (`run`/`swap`/`add`). Touhou palettes, sprite dimensions,
ribbons, materials, and resources belong to the selected `TouhouProfile`, not
core. Canvas2D results are not native, WebGPU, or engine-only throughput
measurements; see [`renderer-api.md`](renderer-api.md).

## Bindings panel (native player)

Press **B** to open the bindings panel: the host contract as editable
data. Three kinds of rows:

- **Buttons** — one key, one channel, a mode: *hold* (1 while down),
  *tap* (1 for exactly one tick on press), *toggle* (flip on press).
- **Axes** — a negative/positive key pair yielding −1/0/+1. Movement is
  nothing special: `$move-x`/`$move-y` are just axis rows the rig
  integrates. Multiple rows may feed one channel (contributions sum,
  clamped); axis pairs sharing a stem (`foo-x`/`foo-y`) are
  vector-normalized so diagonals aren't faster.
- **Constants** — channels set directly to a number. `$rank` lives here
  (T/Y/U/I remain quick-sets).

Click a cell to edit: key cells capture the next key press, the mode
cell cycles, name/value cells take typed input (Enter commits). Rows
add/remove with the `+ button` / `+ axis` / `+ const` / `[x]` controls.
Bindings are host-side configuration: the input tape records the
resulting channel *values*, so replays and scrubbing are unaffected by
how a value was produced. (Bindings are per-session; a card can't see
them, only the channels.)
