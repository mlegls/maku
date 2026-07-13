## Context

`ir-unification` defines typed fixed-width `KernelProgram`s plus domain `KernelPlan`s. The IR-loop and native/wasm executors can consume host memory directly, but a GPU backend must additionally define device residency, bind layouts, dispatch grouping, capability negotiation, state persistence, and deterministic readback. The mixed-width target in `f32-hot-columns` is a prerequisite because portable WebGPU-class hardware does not provide general f64 compute.

The first backend is WebGPU-compatible WGSL: it supplies one shader language and resource model for browser and native adapters. Core owns program-to-WGSL generation and plan descriptors; hosts own device/queue integration and presentation. This change does not move the action scheduler or session authority to the device.

## Goals / Non-Goals

**Goals:**

- Execute eligible typed fixed-output kernel plans on WebGPU-class devices.
- Preserve interpreter/IR-loop semantics, tick boundaries, widths, operation order, and deterministic CPU merge.
- Keep hot plan inputs, outputs, and state resident across ticks when the host can do so.
- Make eligibility and fallback explicit per program/plan/host.
- Start with motion/dyn and render projection, then admit bounded fixed collider projection.

**Non-Goals:**

- Whole-card or action/control execution on GPU.
- General reductions, compaction, sorting, collision contact generation, or variable-length allocation.
- GPU application of spawn/cull/remat/events or other ordered effects.
- Hidden device-only state that cannot participate in snapshots/replay.
- Fast-math, platform-dependent transcendental semantics, or approximate backend-specific rewrites.
- Requiring GPU support for card validity.

## Decisions

### 1. Target WebGPU-compatible WGSL first

The emitter lowers typed kernel ops to WGSL with explicit storage/uniform bindings and workgroup dispatch. Browser hosts can compile it directly; native hosts may integrate through a WebGPU-compatible adapter. A later SPIR-V/native-compute emitter may consume the same plan ABI but is not part of the initial contract.

Alternative: begin with a native vendor API. Rejected because it would not cover the web design ceiling and would introduce a second shader/resource model before the portable subset is known.

### 2. Eligibility is all-or-nothing per plan dispatch

A plan is GPU-eligible only when every program op, width, input/output binding, state transition, indirect access, and resource requirement is supported. Unsupported plans execute through the IR-loop/native path. Generated shaders never call back to CPU or the interpreter.

Eligibility is part of backend planning, not source validity. A card that passes semantic loading must run identically without a GPU.

### 3. Initial plans are lane-local and fixed-output

The initial eligible set is:

```text
MotionPlan
DynFieldPlan
RenderProjectionPlan
```

`ColliderProjectionPlan` joins only for fixed, bounded output layouts. Filter/masked-update plans remain CPU-owned initially because compaction and effect application need an explicit deterministic design. Variable-length curve/polyline output, pair generation, and reductions remain separate changes.

### 4. Buffer bindings mirror typed plan bindings

Each dispatch descriptor contains:

- program/shader identity;
- row range or group lanes;
- typed direct input buffers;
- typed output and next-state buffers;
- capture/state ranges;
- masks and presence buffers;
- immutable tick/channel constants;
- workgroup size and device limits.

Symbols, row ids, offsets, and masks use integer storage. Handles use the canonical representation selected by `ir-unification`. Poses use flattened hot columns. No boxed runtime values cross the device boundary.

### 5. Hot buffers may remain resident, but CPU remains session authority

Eligible hot columns and state may stay on device across ticks. Every resident buffer has a CPU-visible snapshot/readback contract sufficient for session snapshots, scrubbing, fallback, and host export. Backend selection and residency are cache/execution policy, not semantic state.

When CPU drivers need results—for render export, fallback, snapshots, or later effect application—the backend completes the dispatch and exposes declared buffers in canonical tick order. It never advances hidden ticks or precomputes future state.

### 6. Determinism uses explicit generated math

WGSL generation preserves program operation order and declared widths. Transcendentals and remainder semantics use shared deterministic implementations specified by the lowering/determinism contracts rather than unconstrained shader intrinsics where those can vary. No reassociation, contraction, denormal-mode drift, or fast-math is allowed unless the governing contract explicitly makes it equivalent.

The oracle compares semantic interpreter, IR-loop, and GPU results per output lane and state transition. f32 comparison uses the exact width-specific contract established by `f32-hot-columns`, not an ad hoc GPU tolerance.

### 7. Shader and pipeline identity follows kernel identity

The cache key includes typed program identity, plan output shape, math-shim version, workgroup specialization, and device capability class. Per-entity captures and row ranges are data, never shader specialization. Compatible sites share one shader/pipeline and dispatch grouping where their bindings permit it.

### 8. Ordered effects stay on CPU

If later plan kinds produce fixed action records, GPU output is staged into per-row records and CPU drivers apply them in canonical row/tick order. The GPU never mutates task queues, entity allocation, event logs, or command/session tapes directly.

## Risks / Trade-offs

- **[Risk] Device transfer hides arithmetic gains.** → Start only after hot columns can remain resident; measure wall-only end-to-end ticks including synchronization and readback.
- **[Risk] Shader math differs across vendors.** → Generate or import deterministic math shims and make three-way oracle coverage a landing gate.
- **[Risk] CPU snapshots force frequent full readback.** → Define dirty/range-aware readback and measure snapshot cadence; never weaken session semantics for residency.
- **[Risk] Small groups regress on dispatch overhead.** → Use measured row-count thresholds and fall back to IR/native execution below them.
- **[Risk] GPU limits reject valid plans.** → Make limits part of eligibility and retain universal fallback.
- **[Risk] Resource layouts couple core to one host.** → Core emits backend-neutral plan bindings plus WGSL/dispatch descriptors; hosts own device and presentation integration.
- **[Risk] Scope expands into collision/render infrastructure.** → Keep variable-output and cross-row algorithms in their owning changes until an explicit plan template is specified.

## Migration Plan

1. Land the typed kernel-plan ABI, f32 hot storage, and stable spec/program identities.
2. Add WGSL generation and eligibility for a minimal arithmetic motion program.
3. Add host capability/device integration and an IR-loop ↔ GPU executor switch.
4. Add resident motion/dyn state with explicit snapshot/readback.
5. Add render-projection plans and measure end-to-end frame/tick walls.
6. Add bounded fixed collider projection only after its plan contract is stable.
7. Keep every plan family independently disableable for rollback and oracle isolation.

## Open Questions

- Whether native host integration uses a Rust WebGPU implementation directly or consumes host-provided device handles.
- Which deterministic transcendental implementation meets both CPU and WGSL parity at acceptable cost.
- Snapshot/readback granularity once the session storage layout is finalized.
- The measured dispatch threshold and workgroup specialization policy per plan family.
