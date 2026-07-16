# GPU execution backend for typed kernel plans

## Why

Typed fixed-width kernel plans can execute over dense per-entity columns without interpreter values, making them suitable for GPU compute at the 100k–1M-row design ceiling. GPU execution needs its own contract for residency, dispatch, supported plan topology, deterministic math, fallback, and ordered readback rather than being treated as another native JIT target.

## What Changes

- Add a GPU executor for the typed `KernelProgram`/`KernelPlan` ABI produced by `ir-unification`, targeting the repository's web/native graphics direction selected at design time.
- Start with lane-local fixed-output motion/dyn and render-projection plans; admit fixed collider projection only after its output buffers and sampling bounds are explicit.
- Keep buffer residency, dispatch grouping, capability negotiation, state persistence, and readback visible in backend plans.
- Use the same declared widths, operation order, and deterministic math shims as the IR-loop oracle tier.
- Fall back at plan boundaries when a program, plan topology, width, resource limit, or host capability is unsupported.
- Keep reductions, compaction, collision contact generation, variable-length geometry allocation, and spawn/cull/remat/event application out of the initial backend; add them only as explicit deterministic plan templates in later changes.
- Preserve canonical tick and row ordering for every result returned to CPU-owned drivers and the replay/session fold.
- Exclude action/control execution, whole-card compilation, hidden GPU simulation state, and changes to `.maku` semantics.

## Capabilities

### New Capabilities

- `gpu-kernel-execution`: GPU plan eligibility, buffer/dispatch behavior, deterministic equivalence, fallback, readback, and initial supported surfaces.

### Modified Capabilities

None.

## Impact

- Blocked on `ir-unification`, `f32-hot-columns`, and the dense spec/program identity needed from `entity-representation-flip`.
- Kernel buffers and drivers in `crates/core/src/sim/`, typed program definitions/executors, web/native host capability negotiation, and backend-specific shader/module generation.
- Governing contracts: `openspec/specs/lowering/spec.md`, `openspec/specs/determinism/spec.md`, `openspec/specs/session/spec.md`, and `openspec/specs/perf/spec.md`.
- Collision contact streaming and variable-length render geometry remain separate domain-backend work.
