# The debug player

`proto/player` is the reference host: a sim+render **server** (the
sclang/scsynth split — design.md §11). Editor clients are thin
send-a-form-to-a-socket shims; `proto/editors/danmaku.nvim` is the reference
client (see its README for mappings).

```
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- [card.dmk [pattern]]
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
| `(pause)` / `(resume)` | resume after a rewind **branches** (truncates the future) |

## The session (core::session)

The sim is a deterministic fold over **two tapes**: inputs (one `Inputs` per
tick — mouse, axes, focus, bomb; replays include the keyboard) and program
commands (`add`/`swap`, stamped with their tick). Any tick is reachable as
nearest-snapshot + re-step; program changes re-apply at their recorded ticks,
so scrubbing across an add/swap boundary is exact. Snapshots are one per
second, auto-thinned to logarithmic density (recent scrubbing stays
fine-grained; the tick-0 baseline always survives). The event log is one
shared append-only structure; snapshots hold only a cursor.

## Controls (this host's contract)

| input | channel / effect |
|---|---|
| mouse | mock `$player` (mouse-rig cards) and mock `$nearest-enemy` fallback |
| WASD / arrows | `$move-x` / `$move-y` raw axes (piloted rigs integrate them) |
| Shift | `$focus-firing` |
| X | `$bomb` |
| 1–9 | pattern menu |
| Space | pause/resume (resume after rewind = branch) |
| ←/→, ↑/↓ *(paused)* | scrub ±1 / ±30 ticks |
| r | reload card from disk and restart |
| c | clear |
| drag slider | scrub; orange marks = command-tape entries; faint notches = snapshots |

The host layers the stock player rig (`cards/player-rig.dmk`) into every
fresh timeline via the command tape — swap in your own rig live with the
editor. The status line shows tick, entity count, graze, hits, lives.
