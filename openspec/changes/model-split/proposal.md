# Model boundary after kernel and type-checking split

## Why

The former plan assumed a backend-parametric semantic `Dyn<E>` in `model/` would be the shared interpreter/compiler/GPU boundary. The architecture now gives source typing to `language-type-checking` and executable hot loops to typed `KernelProgram`/`KernelPlan`, so moving the interpreter dyn enum wholesale risks creating a redundant intermediate representation rather than a stable model layer.

## What Changes

- Reassess every proposed `model/` move against a stricter criterion: a type moves only when at least two current consumers need the same domain meaning unchanged, not merely because a hypothetical backend might.
- Keep stable domain data and schemas in `model/`: symbols/handles, poses/figures and curve descriptors, literal collider/render boundary data, and genuinely backend-independent schema descriptors.
- Keep source semantic types/elaboration in the `language-type-checking` frontend; they are not runtime model types.
- Keep interpreter execution representations (`Form`, `Env`, `Val`, `DynNode`, evaluator caches and closures) under `interp/`.
- Keep typed compiled execution (`KernelProgram`, `KernelPlan`, generated artifacts) in the kernel/lowering layer.
- Keep physical columns, state slots, epochs, snapshots, row/spec ids, and driver bindings in runtime/sim storage; `entity-representation-flip` owns that cut.
- Do not introduce a generic runtime `model::Dyn<E>` unless the post-`ir-unification` code shows two concrete consumers need the same enum and state-schema meaning unchanged.
- Retain the existing evolve sequencing warning: do not extract `Vel` or `Stages` as stable model nodes before their planned re-expression.
- Permit the change to close with no move if the reassessment finds the remaining types already have the correct owner.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None.

## Impact

- Code-structure/design reassessment across `proto/core/src/{model,interp,sim}/`; semantics unchanged.
- Sequenced after the kernel-plan boundary in `ir-unification` is concrete enough to test ownership against real consumers, and after the relevant `evolve-followups` re-expression for any dyn nodes considered stable.
- Coordinated with `language-type-checking`, `entity-representation-flip`, and `pose-figure-unification`, but no longer a prerequisite for native/GPU code generation.
- Governing semantics remain in `openspec/specs/language/spec.md`; this change must not create a second semantic authority.
