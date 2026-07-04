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
| `<leader>dl` / `:DanmakuLoad` | save + load this file (no pattern change) |
| `<leader>dr` / `:DanmakuRestart` | restart the current pattern |
| `<leader>d<space>` / `:DanmakuPause` | toggle pause |
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
