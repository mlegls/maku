## Context

`crates/core/src/interp/lower.rs` already lowers a subset of motion expressions to an interned register program and executes it through scalar and lane-oriented IR loops. Other per-row surfaces retain private representations: collider fields use `ProjectorNum`, row predicates and values use resolved-row enums/evaluators, and numeric dyn columns use `DynNum`. The fragmentation prevents structural interning, capture/input sharing, batching, and future code generation from applying uniformly.

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

### 1. Make `KernelProgram` the typed contract and retain `NumProgram` as an F64 CPU specialization

A kernel program is the canonical, topologically ordered, register-addressed computation. Every input, register, and output has a fixed backend-portable type:

```text
F32 | F64 | U32 | U64 | Symbol | Handle | Mask
```

Symbols use `u32` storage, generation-safe handles use a distinct `Handle` register file with `u64` lane storage, and masks use a logical one-byte lane in the permanent SoA executor. Program identity covers the complete typed layouts, flattened output descriptors, and operation order.

The pre-existing `NumProgram` lowerer and `run_lanes` implementation remain as the optimized F64 CPU backend for motion programs. `kernel_program_for_num` builds the canonical typed program and declared input bridge; motion plans carry that typed identity while dispatching the proven numeric backend. `NumProgram` is therefore an executor specialization, not a second domain plan ABI or a representation available to predicates, rules, render projection, collider projection, or dyn-field drivers.

Alternative: keep float-only `NumProgram` as the cross-domain contract and leave symbol/handle/presence work in resolved evaluators. Rejected because it preserves the semantic evaluator split and prevents a render/rule kernel from compiling as one unit.

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

## Landed Migration

- `KernelProgram` construction validates typed layouts, operands, outputs, and structural identity; `KernelPlan` construction validates every declared input/output binding before execution.
- Motion keeps the specialized F64 `NumProgram::run_lanes` CPU backend behind a `NumKernelBridge`, while motion grouping and cache identity use the typed program/plan contract.
- Dyn fields, filters, fixed render rows, collider scalar projection, and fixed updates install domain plans over typed programs. Their CPU artifacts are derived and cached from those plans; the generic SoA executor remains the oracle/reference path where a specialized artifact is faster.
- Drivers gather or resolve all inputs before publishing output, own whole-plan fallback, and preserve canonical row/update/render/collider order. Variable geometry and collision contact generation remain driver-owned.
- `MAKU_LOWER_ORACLE=1` dual-runs every migrated surface against its semantic interpreter path without double-applying effects.
- Obsolete private compiled evaluators (`ResolvedRowTest`/`ResolvedRowNum`, `ProjectorNum`, and the private compiled render-row evaluator) were removed. Semantic `DynNum` and collider source expressions remain only to support interpreted fallback and cold plan installation.
- Final interleaved wall-only measurements passed the governing ±5% gate, and all four ignored release oracle card suites passed.

## Landed Decisions

- Handles use a distinct typed register/input/output class backed by `u64` lanes; stale generation policy stays explicit in indirect plan bindings.
- Registers and operations are type-specific, with no tagged interpreter value in a lane.
- Programs expose fixed flattened output-register descriptors; plans bind each descriptor to a column, state, presence, or driver target.
- Reductions, compaction, variable allocation, contacts, and effects remain explicit driver topology rather than ordinary kernel operations.
