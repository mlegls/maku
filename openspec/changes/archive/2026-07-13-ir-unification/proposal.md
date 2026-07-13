# Typed kernel IR unification

## Why

Per-entity hot loops use several private evaluators even though macro expansion, symbol interning, and schema resolution reduce their supported values to fixed-width machine lanes. Motion alone reaches `NumProgram`; collider projection, render-row projection, row predicates, field updates, and dyn columns cannot share one compile cache or one CPU/wasm/GPU backend boundary.

## What Changes

- Evolve `NumProgram` into a typed, fixed-width `KernelProgram` over floating-point lanes, integer/symbol/handle lanes, predicate and presence masks, and fixed multi-output values.
- Add domain `KernelPlan`s that bind programs to iteration domains, input/output/state columns, captures, channels, masks, and driver-owned merge behavior.
- Re-express supported motion/dyn, collider-projection, render-projection, query/deftick predicate, and field-update recognizers as lowerings to `KernelProgram` and their domain plans.
- Flatten fixed-width aggregates such as poses into typed lanes; keep variable-shape figures and output pools under specialized plans and drivers.
- Keep every compiled kernel total, callback-free, and all-or-nothing. Unsupported kernels and runtime input mismatches fall back at the driver boundary; compiled code never re-enters the interpreter.
- Preserve the IR-loop executor as the permanent universal kernel backend and oracle tier.
- Exclude whole-card compilation, action scheduling, cross-row reductions/compaction, collision pair generation, variable-length output allocation, deterministic effect application, physical f32 storage migration, and backend code generation from this change.
- Keep ergonomic load-time type checking and typed semantic elaboration as a separate `language-type-checking` track; neither it nor kernel lowering is a prerequisite for the other.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lowering`: Replace the float-only/private-evaluator split with a typed fixed-width kernel-program and domain-plan contract while preserving interpreter parity, determinism, and driver-level fallback.

## Impact

- `proto/core/src/interp/lower.rs` and the private evaluator/recognizer paths in `interp/{dyn,projectors,rulelower,sem,specs}.rs` and `interp/mod.rs`.
- Kernel drivers and batch executors in `proto/core/src/sim/`.
- Governing contracts: `openspec/specs/lowering/spec.md`, `openspec/specs/determinism/spec.md`, and `openspec/specs/language/spec.md`.
- Unblocks `jit-native-codegen` and a separate GPU kernel backend without compiling the action/control plane.
