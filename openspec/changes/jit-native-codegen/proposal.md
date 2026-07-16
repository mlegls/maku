# Native and wasm codegen for kernel plans

## Why

The typed `KernelProgram`/`KernelPlan` boundary from `ir-unification` makes hot row computation backend-independent, but the permanent IR-loop executor still pays op dispatch and cannot directly exploit target instruction selection. Native CPU and generated-wasm executors can remove that overhead without compiling or duplicating Maku's action/control semantics.

## What Changes

- Compile structurally interned typed `KernelProgram`s to native CPU code, initially in the Cranelift direction, behind the exact plan ABI used by the IR-loop executor.
- Emit generated wasm kernels at card load or publish time for hosts that cannot map native executable memory; generated kernels operate on declared linear-memory columns/state through the same plan contract.
- Cache generated artifacts by typed program identity, widths, deterministic math-shim version, and relevant target features.
- Add scalar codegen first and target SIMD only where it preserves declared lane order, operation order, widths, and deterministic merge behavior.
- Keep iteration orchestration, runtime-input validation, fallback selection, reductions/compaction, variable output, collision contact generation, and ordered effects in domain drivers.
- Keep the IR-loop executor and semantic interpreter as permanent fallbacks; unsupported programs or runtime input mismatches choose fallback before generated execution.
- Exclude whole-`.maku` JIT, action-tree/scheduler compilation, native-to-interpreter callbacks, fast-math, platform libm drift, future-tick precomputation, and GPU execution.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lowering`: Add native and generated-wasm executors behind the typed kernel-plan ABI with cross-tier oracle, fallback, cache-identity, and deterministic-math requirements.

## Impact

- Blocked on `ir-unification`: the stopping condition is one typed, interned, input-slotted program/plan ABI running all supported fixed-width hot surfaces through the IR-loop executor with oracle coverage.
- Kernel execution and driver seams in `crates/core/src/interp/lower.rs` and `crates/core/src/sim/`; codegen modules and build dependencies to be designed at pick-up.
- Platform policy includes macOS hardened-runtime `MAP_JIT` handling and generated wasm for wasm hosts.
- Governing contracts: `openspec/specs/lowering/spec.md` and `openspec/specs/determinism/spec.md`.
- GPU execution remains a separate `gpu-kernel-backend` change because buffer residency, dispatch, compaction, and readback have different prerequisites and acceptance criteria.
