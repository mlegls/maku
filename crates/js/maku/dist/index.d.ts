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
} from "../wasm/maku.js";
import { Maku, type InitInput, type InitOutput } from "../wasm/maku.js";

export type InitMakuOptions = {
  moduleOrPath?: InitInput | Promise<InitInput>;
};

export declare const EXPECTED_MAKU_VERSION = "0.1.0";
export declare const EXPECTED_FRAME_ABI_VERSION = 1;
export declare function releaseIdentity(): {
  makuVersion: string;
  frameAbiVersion: number;
  sourceRevision: string;
};
export type ReleaseIdentity = ReturnType<typeof releaseIdentity>;
export declare function assertRuntimeIdentity(identity: ReleaseIdentity): void;
export declare function initMaku(options?: InitMakuOptions): Promise<InitOutput>;
export declare function playerRigSource(): string;
export declare function createMaku(rig?: string | null): Maku;
export default initMaku;
