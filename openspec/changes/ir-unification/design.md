## Context

`proto/core/src/interp/lower.rs` already lowers a subset of motion expressions to an interned register program and executes it through scalar and lane-oriented IR loops. Other per-row surfaces retain private representations: collider fields use `ProjectorNum`, row predicates and values use resolved-row enums/evaluators, and numeric dyn columns use `DynNum`. The fragmentation prevents structural interning, capture/input sharing, batching, and future code generation from applying uniformly.

At the hot-loop boundary, source strings, maps, lists, lexical environments, and actions have already been resolved away. Supported values are fixed-width machine data: f32/f64 numbers, interned `Symbol` ids, handles, row ids/offsets, masks, and flattened fixed-width aggregates. Variable-shape figures, collection topology, and effects still need domain-specific orchestration.

The governing contracts are `openspec/specs/lowering/spec.md`, `openspec/specs/determinism/spec.md`, and `openspec/specs/language/spec.md`. Ergonomic load-time type checking and typed semantic elaboration remain useful, but belong to the separate `language-type-checking` track: kernel lowering must not wait for that frontend, and type checking must not be designed around optimization backend needs.

## Goals / Non-Goals

**Goals:**

- One typed fixed-width program representation for supported per-row computation across motion/dyn, collider projection, render projection, predicates, and fixed-width updates.
- One explicit plan ABI describing iteration, inputs, outputs, state, masks, and driver-owned merge behavior.
- Structural program identity and capture/input slots suitable for IR-loop, native, wasm, SIMD, and GPU executors.
- Total, callback-free programs with deterministic operation order and width.
- Incremental migration with interpreter parity under `MAKU_LOWER_ORACLE`.

**Non-Goals:**

- A universal typed IR for all `.maku` expressions.
- Whole-card or action-tree compilation.
- Replacing the scheduler, structured concurrency, states, live add/swap, or the source interpreter.
- Encoding reductions, compaction, collision pair generation, variable-length allocation, or effect application as ordinary lane programs.
- Native, wasm, or GPU emission in this change.
- Choosing physical f32 storage columns; `f32-hot-columns` owns that migration.
- Changing language-visible values or semantics.

## Decisions

### 1. Generalize `NumProgram` into a typed `KernelProgram`

A kernel program is a topologically ordered, register-addressed computation. Every input, register, and output has a fixed backend-portable type. The initial set is:

```text
F32 | F64 | U32 | U64 | Mask
```

`U32` covers interned symbols, row ids, small enum tags, and offsets. Generation-safe handles use either a `U64` canonical packing or two explicitly bound integer lanes; the implementation must choose one representation and keep stale-handle validation in declared ops/inputs. `Mask` is the backend predicate type and may be bit-packed in storage while remaining a logical lane value in the program.

Alternative: keep float-only `NumProgram` and leave symbol/handle/presence work in resolved evaluators. Rejected because it preserves the evaluator split and prevents a render/rule kernel from compiling as one unit.

Alternative: use interpreter `Val` registers. Rejected because boxed/tagged dynamic values prevent total callback-free kernels and transfer poorly to SIMD, wasm, and GPU backends.

### 2. Flatten fixed-width aggregates; specialize variable-shape values in plans

A pose lowers to numeric lanes plus orientation presence:

```text
x, y, theta, has_theta
```

Pose construction, composition, sampling, and selection may lower to primitive operations or deterministic convenience ops when preserving component order and shared math is clearer. Fixed render/collider records lower to multiple typed outputs bound by schema.

A general `Figure` is not a register type. Pose, parametric-curve, polyline, and later figure groups select specialized plans. A curve evaluator is a program over `(t, u, captures, frame inputs)`; polyline/composite storage and variable-size pools remain driver-owned.

Alternative: carry opaque figure objects through the program. Rejected because representation dispatch and variable-size ownership would leak interpreter/backend objects into every executor.

### 3. Programs are pure computation; `KernelPlan`s own execution bindings

`KernelProgram` answers “what fixed-width outputs follow from these inputs?” A domain plan answers “which rows run it, where are inputs/state, where do outputs go, and what does the driver do afterward?” Initial plan families are:

```text
MotionPlan
DynFieldPlan
ColliderProjectionPlan
RenderProjectionPlan
FilterPlan
MaskedUpdatePlan
```

A plan declares:

- structural program id(s);
- iteration/group domain;
- direct and indirect column inputs;
- capture, channel, tick/axis, and scan-state inputs;
- fixed output and next-state bindings;
- optional predicate/presence masks;
- driver-owned fallback and deterministic merge policy.

Plans may contain several programs during migration. Common-subexpression/fused multi-output lowering is allowed when one program can share setup and arithmetic without changing operation order.

Alternative: make each subsystem call programs ad hoc. Rejected because backend code generation needs a stable buffer/state ABI and because fallback, grouping, and effects would otherwise diverge by subsystem.

### 4. Cross-row and effect topology stays in drivers

Filter/reduction/compaction, collision contact generation, variable-length curve sampling, and ordered actions are not ordinary independent-lane expressions. Drivers or explicit plan templates own them. Their fixed-width predicate/key/value calculations still use `KernelProgram`.

Examples:

```text
FilterPlan(predicate) -> row mask/entity set
MaskedUpdatePlan(predicate, values) -> deterministic queued writes
ColliderProjectionPlan(programs) -> collider columns
collision driver(collider columns) -> ordered contact facts
```

Spawn, cull, remat, and event results may eventually be fixed per-row action records produced by a plan, but canonical application stays driver-owned and row/tick ordered.

Alternative: introduce a universal control/dataflow IR covering reductions and actions. Rejected until a measured backend requires more than a small set of explicit driver templates.

### 5. Lowering is schema-directed and all-or-nothing per plan kernel

Macro expansion and the existing load-time rewrite run first. The domain recognizer resolves bindings, field schemas, projector/render schemas, and expected slot kinds, then lowers supported fixed-width expressions. Optimizations recognize expansion shapes, never source macro names.

A supported kernel contains no interpreter callback. An unsupported expression rejects the relevant plan/kernel and retains the semantic interpreter path. If a runtime input violates the declared type/presence assumptions, the driver abandons that batch and reruns the semantic operation interpreted.

This resolves the old `Interp`-op ambiguity in favor of the current JIT-readiness contract: there is no native-to-interpreter callback ABI.

### 6. Width is part of program and plan identity

Programs declare lane widths. Physical hot columns may later be f32 while control-plane values remain f64. Conversions are explicit ops/bindings, and structural cache identity includes types and widths. Executors use the same operation order and shared math shims; no backend may select fast-math or platform libm behavior that breaks the oracle.

`ir-unification` supplies typed width support. `f32-hot-columns` decides which physical storage classes narrow and measures corpus drift.

### 7. The IR-loop executor remains permanent

The existing scalar/lane IR executor evolves to execute typed programs and plans. It is the universal backend on every host, the cold fallback for uncompiled programs, and one side of the interpreter ↔ IR-loop ↔ generated-code oracle.

Native, wasm, SIMD, and GPU backends consume the same program/plan contract but are separate changes. Backend availability never changes card validity or semantics.

### 8. Surface migration order follows increasing orchestration complexity

1. Generalize the program, registers, outputs, interning, executor, and oracle without changing existing motion behavior.
2. Migrate `DynNum` and motion/pose fixed-width paths.
3. Migrate row predicates and symbol/field tests to `FilterPlan`.
4. Migrate fixed render-row projection to typed multi-output plans.
5. Migrate fixed collider projection.
6. Migrate supported masked field updates while retaining deterministic driver application.
7. Remove private evaluator variants only after every caller has cut over or deliberately remains control-plane interpreted.

Each migration keeps its old semantic evaluator available for all-or-nothing fallback and oracle comparison; obsolete private compiled representations are removed at cutover.

## Risks / Trade-offs

- **[Risk] The typed program grows into a second general interpreter.** → Admit only fixed-width total ops needed by measured hot plans; keep collections, actions, allocation, and arbitrary calls outside.
- **[Risk] Program fusion changes floating-point order.** → Preserve source/interpreter operation order and oracle every migrated plan; fusion may share inputs/setup but not reassociate arithmetic.
- **[Risk] Handle gathers introduce aliasing and stale-row bugs.** → Declare indirect inputs explicitly, validate generations through one driver/kernel contract, and prohibit undeclared world access.
- **[Risk] Multi-output programs increase scratch pressure.** → Measure register counts and permit several shared-input programs when fusion regresses wall time or GPU occupancy.
- **[Risk] Driver plans duplicate semantic decisions.** → Plans contain bindings and execution topology only; source meaning and fallback remain in the existing domain interpreter.
- **[Risk] Early GPU concerns overcomplicate the CPU IR.** → Require backend-portable fixed-width values and explicit bindings, but add GPU-only orchestration only in `gpu-kernel-backend` when measured.
- **[Risk] Type checking and kernel lowering duplicate some resolution work.** → Give `language-type-checking` ownership of ergonomic source types and diagnostics, keep `KernelProgram` ownership limited to executable fixed-width lanes, and permit shared resolved annotations later without making either track depend on the other.

## Migration Plan

- Introduce typed program metadata and typed executor paths alongside the current numeric path.
- Dual-run each migrated surface against its existing evaluator under `MAKU_LOWER_ORACLE`.
- Migrate one domain plan family per coherent change-set and retain whole-plan interpreted fallback.
- Remove each private compiled evaluator only after its corpus coverage and focused tests pass through the common program.
- Keep rollback local: disabling a plan family restores the existing interpreted evaluator without changing card or storage semantics.
- After all target surfaces use the common ABI, `jit-native-codegen` and `gpu-kernel-backend` may add executors without changing source semantics.

## Open Questions

- Canonical handle lane representation: packed `U64` versus explicit row/generation lanes.
- Whether the first implementation uses typed register metadata or type-specific op variants.
- Whether multi-output programs expose output registers directly or explicit store ops to bound columns.
- Which reductions deserve reusable plan templates after the fixed-output migrations are measured.
