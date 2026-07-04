# danmaku.nvim

Thin editor client for the danmaku-player server (sclang/scsynth split):
newline-delimited EDN over 127.0.0.1:7777.

Start the player once:

    cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- translations/130_bowap.edn

Then from any `.edn` card buffer:

| mapping / command | effect |
|---|---|
| `<leader>dp` / `:DanmakuPlay` | save + load this file + play the `defpattern` under the cursor |
| `<leader>dl` / `:DanmakuLoad` | save + load this file (server-default pattern) |
| `<leader>dr` / `:DanmakuRestart` | restart the current pattern |
| `<leader>d<space>` / `:DanmakuPause` | toggle pause |
| `:DanmakuSend (pattern "bowap-fold")` | raw EDN command |

lazy.nvim spec (local dir):

```lua
return {
  dir = "/path/to/danmaku-engine/proto/editors/danmaku.nvim",
  name = "danmaku.nvim",
  opts = {},   -- { host = ..., port = ... }
}
```
