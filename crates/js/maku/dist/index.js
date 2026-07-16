// src/index.ts
import initWasm, {
  Maku,
  frameAbiVersion,
  initSync,
  makuVersion,
  sourceRevision,
  stdlibSource
} from "../wasm/maku.js";
var EXPECTED_MAKU_VERSION = "0.1.0";
var EXPECTED_FRAME_ABI_VERSION = 1;
function releaseIdentity() {
  return {
    makuVersion: makuVersion(),
    frameAbiVersion: frameAbiVersion(),
    sourceRevision: sourceRevision()
  };
}
function assertRuntimeIdentity(identity) {
  if (identity.makuVersion !== EXPECTED_MAKU_VERSION) {
    throw new Error(`Maku wrapper ${EXPECTED_MAKU_VERSION} loaded wasm ${identity.makuVersion}`);
  }
  if (identity.frameAbiVersion !== EXPECTED_FRAME_ABI_VERSION) {
    throw new Error(`Maku frame ABI ${EXPECTED_FRAME_ABI_VERSION} loaded wasm ABI ${identity.frameAbiVersion}`);
  }
}
async function initMaku(options = {}) {
  const output = options.moduleOrPath === undefined ? await initWasm() : await initWasm({ module_or_path: options.moduleOrPath });
  assertRuntimeIdentity(releaseIdentity());
  return output;
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
  sourceRevision,
  releaseIdentity,
  playerRigSource,
  makuVersion,
  initSync,
  initMaku,
  frameAbiVersion,
  src_default as default,
  createMaku,
  assertRuntimeIdentity,
  Maku,
  EXPECTED_MAKU_VERSION,
  EXPECTED_FRAME_ABI_VERSION
};
