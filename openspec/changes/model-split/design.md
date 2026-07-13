## Context

The original design proposed moving a backend-parametric `Dyn<E>` plus entity-spec and state-schema halves into `model/`, on the assumption that interpreter, compiled CPU, and GPU backends would consume the same semantic enum before choosing execution representation. Two newer boundaries make that assumption unnecessary:

```text
language-type-checking
    owns source semantic types and elaborated author-facing meaning

ir-unification
    owns typed fixed-width KernelProgram/KernelPlan execution

entity-representation-flip + sim
    own spec ids, captures, state/epoch columns, snapshots, and drivers
```

A GPU or native backend consumes kernel plans, not a generic semantic dyn tree. The interpreter still needs its own closures, `Val`, environments, state construction, and fallback behavior. The remaining reason to move a type into `model/` must therefore be stable domain sharing, not anticipated backend reuse.

## Goals / Non-Goals

**Goals:**

- Give stable domain data, source typing, interpreter execution, kernel execution, and runtime storage unambiguous owners.
- Move only types with two concrete consumers that need identical domain meaning.
- Avoid extracting soon-to-be-reexpressed `Vel`/`Stages` or interpreter/cache artifacts.
- Allow a no-op outcome when the existing owner is already correct.

**Non-Goals:**

- A generic semantic IR between type checking and kernel lowering.
- Making `model/` the dependency root for every backend-related type.
- Moving interpreter `Form`/`Env`/`Val` or kernel program/plan types into `model/`.
- Moving physical storage/schema slot ids, snapshots, epochs, or execution caches into `model/`.
- Blocking native/GPU code generation on a model refactor.

## Decisions

### 1. Ownership follows unchanged meaning across current consumers

A type belongs in `model/` only when at least two current subsystems consume the same domain value and neither owns its execution or storage policy. Candidates include:

- `Symbol`, field-name ids, and generation-safe handle values;
- `Pose`, `Figure`, and stable curve/geometry descriptors;
- literal `ColliderData` and typed render-boundary records/descriptors;
- stable schema descriptors that source checking and runtime validation must interpret identically.

A type does not move merely because it is generic over an expression type or could hypothetically be consumed by a future backend.

### 2. Source semantic types belong to the frontend

`language-type-checking` owns `Num`, `Symbol`, `Handle`, `Dyn<T>`, signal classes, functions, records, projectors, render rows, actions, expected-type coercions, and typed elaboration as author-facing meaning. Those types may reference stable model domain types, but they do not become runtime storage or backend IR.

### 3. Interpreter dyns remain interpreter execution representations

`DynNode`, `DynNum`, `EvolveDyn`, `Form`, `Env`, `Val`, interpreter closure adapters, node-local caches, and fallback state construction remain under `interp/` unless a later cut removes them entirely. Their variants encode interpreter evaluation strategy and migration history, not a backend-independent domain value.

`Vel` and `Stages` remain explicitly ineligible for extraction while their `evolve-followups` re-expression is pending.

### 4. Kernel plans are the compiled backend boundary

`KernelProgram`, `KernelPlan`, typed registers, column/state bindings, structural compile identity, generated code, and backend eligibility live in lowering/kernel modules. Native, wasm, SIMD, and GPU backends share this contract directly. Duplicating it behind `model::Dyn<E>` would add a translation layer without another semantic consumer.

### 5. Runtime spec/state identity belongs to entity/sim storage

The entity spec table may refer to stable model schemas and kernel plan ids, but spec ids, capture ranges, state slots, epochs, generations, row reuse, snapshots, cache policy, and driver bindings are runtime representation. `entity-representation-flip` owns that layout and the replacement of pointer identity with explicit ids.

A state-kind descriptor moves to `model/` only if source checking and more than one runtime executor require the exact same descriptor unchanged. Physical slot assignment never moves.

### 6. Reassessment precedes movement

At pick-up, inventory every proposed moved type and record:

```text
current owner
current consumers
meaning shared unchanged?
execution/storage policy embedded?
post-kernel/type-check target owner
move / split / keep / delete
```

Only then create implementation tasks. If no type passes the criterion, close/archive this change as a resolved no-op rather than performing organizational churn.

## Risks / Trade-offs

- **[Risk] Keeping interpreter dyn types under `interp/` preserves some coupling.** → Remove coupling at real plan/schema boundaries; do not move it into a generic layer without a second consumer.
- **[Risk] Frontend and runtime schemas drift.** → Share only stable schema descriptors/registries in `model/`; keep elaborated expressions and physical slots in their owning layers.
- **[Risk] The no-op criterion feels too conservative.** → Prefer evidence from current consumers; a later concrete backend can justify a narrow move without paying for a speculative abstraction now.
- **[Risk] `model/` becomes an incoherent grab bag.** → Require domain-value identity and two unchanged consumers for every addition; execution and storage types are categorically excluded.
- **[Risk] Evolve/model changes race.** → Retain the sequencing gate and reassess only after `Vel`/`Stages` target shapes are settled.

## Migration Plan

1. Land or stabilize `KernelProgram`/`KernelPlan` ownership and the entity spec-id target.
2. Complete the relevant evolve re-expression before assessing dyn variants as stable.
3. Produce the type ownership inventory and decide move/split/keep/delete for each candidate.
4. Move one coherent stable-domain cluster at a time, with no behavior changes and focused compile/tests.
5. Delete obsolete bridge types/imports after each cutover; do not retain aliases or duplicate schema authorities.
6. Close the change without code edits if no remaining candidate passes the ownership criterion.

## Open Questions

- Whether any motion state-kind descriptor is genuinely shared semantic data after kernel plans and typed source elaboration exist.
- Whether current `Figure<E>` generic evaluation descriptors remain stable domain structure or should split into a non-generic figure descriptor plus frontend/kernel evaluators.
- Which render/collider schema descriptors are already authoritative enough to share directly with the type checker.
