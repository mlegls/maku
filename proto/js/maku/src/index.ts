import initWasm, {
  Maku,
  initSync,
  stdlibSource,
  type InitInput,
  type InitOutput,
  type SyncInitInput,
} from "../wasm/maku.js";

export {
  Maku,
  initSync,
  stdlibSource,
  type InitInput,
  type InitOutput,
  type SyncInitInput,
};

export type InitMakuOptions = {
  moduleOrPath?: InitInput | Promise<InitInput>;
};

export async function initMaku(options: InitMakuOptions = {}): Promise<InitOutput> {
  if (options.moduleOrPath === undefined) {
    return initWasm();
  }
  return initWasm({ module_or_path: options.moduleOrPath });
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
