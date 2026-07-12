# IR unification (JIT gap 1)

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The prototype has several numeric evaluation representations (ProjectorNum, ResolvedRow*, DynNum alongside NumProgram). The JIT/native tier compiles NumProgram per distinct program; every surface that stays on a private representation stays interpreted and off the compile cache. Folding them onto NumProgram is the biggest remaining chunk on the settled-semantics path to codegen.

## What Changes

- Fold projector bodies, resolved render-row programs, and remaining DynNum surfaces onto `NumProgram` + the `run`/`run_lanes` executors, behind the same (program, input lanes, scratch) boundary.
- Keep ops total and callback-free; the planned Interp fallback op is the one interpreter re-entry point and defines the JIT→interpreter ABI.

## Capabilities

Lowering-internal; oracle-gated equivalence.

## Impact

- `proto/core/src/interp/{lower,rulelower,specs,renderers}.rs` and executors in `sim/`.
- Blocks `jit-native-codegen`. Governing: `openspec/specs/lowering/spec.md` "JIT readiness" (gap list + sequencing).
