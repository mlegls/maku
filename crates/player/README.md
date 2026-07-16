# maku-player

`maku-player` is Maku's native Macroquad reference host. It runs cards through
the supported `maku::host::Instance` facade and draws the backend-neutral
frames produced by `maku-render-touhou`.

Install or run it with a card path:

```sh
cargo run -p maku-player -- cards/reimu_vs_mima.maku
```

The player also exposes Maku's live-evaluation and timeline wire protocol. See
[`docs/player.md`](https://github.com/mlegls/maku/blob/main/docs/player.md) for
controls and editor integration.
