# @mlegls/maku

Versioned browser bindings for the Maku danmaku pattern engine.

```sh
bun add @mlegls/maku
```

```ts
import initMaku, {
  createMaku,
  releaseIdentity,
  EXPECTED_FRAME_ABI_VERSION,
} from "@mlegls/maku";

await initMaku(); // rejects a mixed package/wasm version or frame ABI
console.log(releaseIdentity(), EXPECTED_FRAME_ABI_VERSION);

const maku = createMaku();
maku.add_file("cards/example.maku", source);
maku.boot("cards/example.maku");
maku.input_vec2("player", 0, -3);
maku.step(1);
maku.build_render_frame();

// Consume or upload all required views before another frame build or a wasm
// call that can grow memory.
const commands = maku.draw_commands();
```

The package ships wasm-pack output under `wasm/`, a typed wrapper under
`dist/`, and `wasm/release.json`. The wasm binary, bindgen glue/declarations,
wrapper, and release manifest are one artifact; never update only the wasm
file. `releaseIdentity()` reports engine version, frame ABI, and source
revision. `assertRuntimeIdentity()` can validate an externally supplied
manifest before renderer initialization.

<!-- compatibility-migration -->
The wrapper exposes the complete semantic render-pack frame rather than legacy
`dots()`/`beams()` arrays. Geometry getters are zero-copy views into wasm linear
memory and are invalidated by frame reuse/reallocation or memory growth.

See the repository's [`crates/web/README.md`](../../web/README.md) for the wasm
host lifecycle and [`docs/renderer-api.md`](../../../docs/renderer-api.md) for
packed layouts, material/resource resolution, Canvas2D, and WebGPU mapping.
