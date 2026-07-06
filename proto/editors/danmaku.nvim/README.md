# danmaku.nvim

Thin editor client for the maku-player server (sclang/scsynth split):
newline-delimited EDN over 127.0.0.1:7777.

Start the player once:

    cargo run --manifest-path proto/Cargo.toml --features player --bin maku-player -- cards/translations/130_bowap.dmk

Then from any `.dmk` card buffer:

| mapping / command | effect |
|---|---|
| `<localleader>e{motion}` | run the covered text as an anonymous pattern (`<localleader>ea(`, `gg<localleader>eG`) |
| `<localleader>e` (visual) | run the selection |
| `<localleader>ee` | run the innermost form enclosing the cursor |
| `<localleader>er` | run the root (top-level) form — e.g. the `defpattern` under the cursor |
| `<localleader>es` | hot-swap the root form: in-flight bullets keep their old code, new spawns get the new (§11 generational swap) |
| `<localleader>eA` | add the root form to the running sim in parallel — its clocks anchor at the add tick (`A` capital: lowercase would shadow `ea(`-style text objects) |
| `<leader>dl` / `:DanmakuLoad` | save + load this file — **does not play** (menu/defs refresh only) |
| `<leader>dr` / `:DanmakuRestart` | restart the current pattern |
| `<leader>dc` / `:DanmakuClear` | stop the running pattern (card stays loaded) |
| `<leader>d<space>` / `:DanmakuPause` | toggle pause (resuming after a rewind branches the timeline) |
| `{count}<leader>dj` / `{count}<leader>dk` | scrub forward / back by count ticks (default 1) |
| `{count}<leader>dg` | scrub to absolute tick (bare = tick 0) |
| `:DanmakuSeek {tick}` / `:DanmakuStep {±n}` | scrub to an absolute tick / by a relative amount |

| `:DanmakuPlay` | play the `defpattern` nearest the cursor, by name |
| `:DanmakuSend (pattern "bowap-fold")` | raw EDN command |

Anonymous forms sent with `e` run with the loaded card's `def`s in scope;
sending a whole `(defpattern …)` registers and runs it. Comments are stripped
line-wise before transport (the protocol is one form per line).

The session is a deterministic fold over two tapes. `er` (run) **keeps the
input tape**: the recorded timeline replays through the new code up to the
current tick — pause, rewind, edit, `<localleader>ee`, and watch the same
dodge against the revised pattern. `es` (swap) and `eA` (add) land on the
**command tape** at their tick, so scrubbing back across a swap/add boundary
and forward again replays the program change exactly (orange markers on the
player's timeline slider). Resuming after a rewind branches: future inputs
*and* future program changes are dropped.

lazy.nvim spec (local dir):

```lua
return {
  dir = "/path/to/Maku/proto/editors/danmaku.nvim",
  name = "danmaku.nvim",
  opts = {},   -- { host = ..., port = ... }
}
```
