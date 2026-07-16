export {
  Maku,
  initSync,
  stdlibSource,
  type InitInput,
  type InitOutput,
  type SyncInitInput,
} from "../wasm/maku.js";
import { Maku, type InitInput, type InitOutput } from "../wasm/maku.js";

export type InitMakuOptions = {
  moduleOrPath?: InitInput | Promise<InitInput>;
};

export declare function initMaku(options?: InitMakuOptions): Promise<InitOutput>;
export declare function playerRigSource(): string;
export declare function createMaku(rig?: string | null): Maku;
export default initMaku;
