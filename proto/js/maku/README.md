# Maku

Browser bindings for the Maku danmaku pattern engine.

```ts
import initMaku, { createMaku } from "maku";

await initMaku();
const maku = createMaku();
maku.add_file("cards/example.maku", source);
maku.boot("cards/example.maku");
maku.step(1);
```

The package ships the wasm-pack output under `wasm/` and exposes a small
typed wrapper from `dist/index.js`.
