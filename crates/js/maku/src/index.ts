import initWasm, {
  Maku,
  frameAbiVersion,
  initSync,
  makuVersion,
  sourceRevision,
  stdlibSource,
  type InitInput,
  type InitOutput,
  type SyncInitInput,
} from "../wasm/maku.js";

export {
  Maku,
  frameAbiVersion,
  initSync,
  makuVersion,
  sourceRevision,
  stdlibSource,
  type InitInput,
  type InitOutput,
  type SyncInitInput,
};

export type InitMakuOptions = {
  moduleOrPath?: InitInput | Promise<InitInput>;
};

export const EXPECTED_MAKU_VERSION = "0.2.0";
export const EXPECTED_FRAME_ABI_VERSION = 1;

export function releaseIdentity() {
  return {
    makuVersion: makuVersion(),
    frameAbiVersion: frameAbiVersion(),
    sourceRevision: sourceRevision(),
  };
}

export type ReleaseIdentity = ReturnType<typeof releaseIdentity>;

export function assertRuntimeIdentity(identity: ReleaseIdentity): void {
  if (identity.makuVersion !== EXPECTED_MAKU_VERSION) {
    throw new Error(`Maku wrapper ${EXPECTED_MAKU_VERSION} loaded wasm ${identity.makuVersion}`);
  }
  if (identity.frameAbiVersion !== EXPECTED_FRAME_ABI_VERSION) {
    throw new Error(`Maku frame ABI ${EXPECTED_FRAME_ABI_VERSION} loaded wasm ABI ${identity.frameAbiVersion}`);
  }
}

export async function initMaku(options: InitMakuOptions = {}): Promise<InitOutput> {
  const output = options.moduleOrPath === undefined
    ? await initWasm()
    : await initWasm({ module_or_path: options.moduleOrPath });
  assertRuntimeIdentity(releaseIdentity());
  return output;
}

export function playerRigSource(): string {
  const src = stdlibSource("player-rig");
  if (src === undefined) {
    throw new Error("Maku stdlib is missing player-rig");
  }
  return `${src}\n(player-rig)`;
}

export function createMaku(rig: string | null = playerRigSource()): Maku {
  return new Maku(rig);
}

export default initMaku;
