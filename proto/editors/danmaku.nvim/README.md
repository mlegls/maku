# danmaku.nvim

Thin editor client for the danmaku-player server (sclang/scsynth split):
newline-delimited EDN over 127.0.0.1:7777.

Start the player once:

    cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- translations/130_bowap.dmk

Then from any `.dmk` card buffer:

| mapping / command | effect |
|---|---|
| `<localleader>e{motion}` | run the covered text as an anonymous pattern (`<localleader>ea(`, `gg<localleader>eG`) |
| `<localleader>e` (visual) | run the selection |
| `<localleader>ee` | run the innermost form enclosing the cursor |
| `<localleader>er` | run the root (top-level) form — e.g. the `defpattern` under the cursor |
| `<localleader>es` | hot-swap the root form: in-flight bullets keep their old code, new spawns get the new (§11 generational swap) |
| `<leader>dl` / `:DanmakuLoad` | save + load this file — **does not play** (menu/defs refresh only) |
| `<leader>dr` / `:DanmakuRestart` | restart the current pattern |
| `<leader>dc` / `:DanmakuClear` | stop the running pattern (card stays loaded) |
| `<leader>d<space>` / `:DanmakuPause` | toggle pause (resuming after a rewind branches the timeline) |
| `{count}<leader>dj` / `{count}<leader>dk` | scrub forward / back by count ticks (default 1) |
| `{count}<leader>dg` | scrub to absolute tick (bare = tick 0) |
| `:DanmakuSeek {tick}` / `:DanmakuStep {±n}` | scrub to an absolute tick / by a relative amount |

Sending a form with the eval operator **keeps the input tape**: the recorded
timeline replays through the new code up to the current tick — pause, rewind,
edit, `<localleader>ee`, and watch the same dodge against the revised pattern.
The player also has a timeline slider + play/pause button at the bottom.
| `:DanmakuPlay` | play the `defpattern` nearest the cursor, by name |
| `:DanmakuSend (pattern "bowap-fold")` | raw EDN command |

Anonymous forms sent with `e` run with the loaded card's `def`s in scope;
sending a whole `(defpattern …)` registers and runs it. Comments are stripped
line-wise before transport (the protocol is one form per line).

lazy.nvim spec (local dir):

```lua
return {
  dir = "/path/to/danmaku-engine/proto/editors/danmaku.nvim",
  name = "danmaku.nvim",
  opts = {},   -- { host = ..., port = ... }
}
```
