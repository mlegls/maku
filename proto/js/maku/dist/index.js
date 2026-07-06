// src/index.ts
import initWasm, {
  Maku,
  initSync,
  stdlibSource
} from "../wasm/maku.js";
async function initMaku(options = {}) {
  if (options.moduleOrPath === undefined) {
    return initWasm();
  }
  return initWasm({ module_or_path: options.moduleOrPath });
}
function playerRigSource() {
  const src = stdlibSource("player-rig");
  if (src === undefined) {
    throw new Error("Maku stdlib is missing player-rig");
  }
  return `${src}
(player-rig)`;
}
function createMaku(rig = playerRigSource()) {
  return new Maku(rig);
}
var src_default = initMaku;
export {
  stdlibSource,
  playerRigSource,
  initSync,
  initMaku,
  src_default as default,
  createMaku,
  Maku
};
