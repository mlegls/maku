## ADDED Requirements

### Requirement: GPU execution is optional and plan-scoped

A host MAY execute an eligible typed kernel plan on a GPU, but GPU availability or eligibility SHALL NOT affect card validity or language semantics. Eligibility SHALL be all-or-nothing per dispatch: every program op, width, binding, state transition, and resource requirement must be supported before GPU execution begins.

#### Scenario: Host without GPU compute
- **WHEN** a valid card runs on a host without the required GPU capability
- **THEN** every plan executes through the IR-loop or another supported executor with identical semantics

#### Scenario: Unsupported operation
- **WHEN** a plan contains one operation the GPU backend cannot emit
- **THEN** the complete plan dispatch uses a fallback executor without partial GPU execution or interpreter callback

### Requirement: Initial GPU plans are fixed-output and lane-local

The initial backend SHALL support only explicitly admitted fixed-output plan families. Motion/dyn and render projection MAY be admitted first; collider projection SHALL require a fixed bounded output layout. Reductions, compaction, collision contact generation, variable-length allocation, and ordered effects SHALL remain outside the initial GPU executor.

#### Scenario: Fixed render projection
- **WHEN** a render-projection plan has supported fixed typed inputs and outputs
- **THEN** it may execute over GPU-resident columns and produce the same ordered render columns as the IR-loop plan

#### Scenario: Variable-length polyline output
- **WHEN** a projection requires data-dependent output allocation or compaction
- **THEN** it is GPU-ineligible until a separate deterministic plan template specifies allocation and ordering

### Requirement: GPU bindings mirror kernel-plan bindings

Every GPU dispatch SHALL use explicit typed bindings derived from the kernel plan, including row/group domain, direct inputs, outputs, next-state columns, captures, masks, and immutable tick/channel inputs. Generated shaders SHALL NOT access undeclared world state or boxed interpreter values.

#### Scenario: Symbol field predicate
- **WHEN** an eligible plan reads an interned symbol field
- **THEN** the shader reads the declared integer column and preserves exact symbol equality semantics

#### Scenario: Stateful motion
- **WHEN** an eligible motion plan advances fixed per-row state
- **THEN** current state and next state use distinct declared bindings consistent with the canonical tick boundary

### Requirement: GPU residency is execution policy, not semantic state

Hot inputs, outputs, and state MAY remain GPU-resident across ticks, but the backend SHALL provide CPU-visible snapshot/readback behavior sufficient for fallback, session snapshots, scrubbing, and host exports. The backend SHALL NOT advance hidden future ticks or retain semantic state absent from the session model.

#### Scenario: Session snapshot
- **WHEN** the session captures a snapshot while motion state is GPU-resident
- **THEN** the captured state is sufficient to restore and replay identically on a GPU or fallback executor

#### Scenario: Backend switch
- **WHEN** a resident plan becomes ineligible or the host switches executors
- **THEN** declared state and outputs transfer to the fallback path without changing the next tick's result

### Requirement: GPU math and widths preserve cross-tier determinism

GPU emission SHALL preserve each program's declared lane widths and operation order and SHALL use deterministic math behavior compatible with `openspec/specs/determinism/spec.md`. It SHALL NOT enable fast-math, reassociation, unconstrained contraction, or platform shader intrinsics whose specified results violate the cross-tier contract.

#### Scenario: Three-way oracle
- **WHEN** GPU oracle mode executes an eligible plan
- **THEN** semantic-interpreter, IR-loop, and GPU outputs and next-state lanes agree under the governing width-specific contract

#### Scenario: Device cannot provide required math
- **WHEN** a device/compiler cannot implement a required operation with the specified deterministic behavior
- **THEN** programs containing that operation are GPU-ineligible on that device

### Requirement: Generated artifact identity is structural

Shader/pipeline cache identity SHALL include typed program identity, plan output shape, declared widths, deterministic math-shim version, relevant workgroup specialization, and device capability class. Captures, row ranges, and ordinary per-entity values SHALL remain data and SHALL NOT create distinct shaders.

#### Scenario: Capture-only difference
- **WHEN** two sites have the same typed plan/program shape but different capture values
- **THEN** they share the generated shader/pipeline and provide captures through bound data

### Requirement: CPU drivers preserve canonical effect order

The GPU backend SHALL NOT directly mutate action queues, entity allocation, events, or session tapes. If a future admitted plan emits fixed result/action records, CPU-owned drivers SHALL apply them at the specified tick boundary and in canonical row order.

#### Scenario: GPU-produced update records
- **WHEN** an admitted GPU plan produces per-row records that cause world effects
- **THEN** the CPU driver reads the records and applies effects through the ordinary deterministic path without GPU-side reordering

### Requirement: GPU performance includes transfer and synchronization

GPU adoption and dispatch thresholds SHALL be judged by end-to-end wall-only measurements that include upload, synchronization, readback, and fallback costs. A plan family SHALL remain on the faster supported executor when GPU dispatch regresses the representative workload.

#### Scenario: Small batch
- **WHEN** GPU dispatch overhead makes an eligible small row group slower than IR/native execution
- **THEN** backend selection uses the measured fallback threshold without changing plan semantics
