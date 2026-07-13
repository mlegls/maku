## ADDED Requirements

### Requirement: Kernel programs use typed fixed-width lanes

Every lowered kernel SHALL declare the type of every input, register, and output. Kernel values SHALL be limited to backend-portable fixed-width floating-point lanes, integer lanes, predicate/presence masks, and fixed aggregates flattened to those lanes. Kernels SHALL NOT allocate or manipulate interpreter `Val` values, source strings, maps/lists, lexical environments, or actions.

#### Scenario: Symbol predicate
- **WHEN** a row predicate compares a schema-resolved symbol field with an interned keyword
- **THEN** the field and keyword enter the kernel as integer lanes and the equality result is a mask, with no resolved-row or interpreter callback

#### Scenario: Optional orientation
- **WHEN** a pose with optional orientation enters a kernel
- **THEN** its components and orientation-presence mask use declared fixed-width lanes and preserve the language distinction between unspecified orientation and explicit zero

### Requirement: Kernel programs support fixed multiple outputs

One kernel execution MAY expose multiple typed outputs. Fixed render rows, collider rows, poses, and state transitions SHALL bind those outputs directly to schema- or plan-declared columns/masks without constructing boxed intermediate records.

#### Scenario: Sprite render projection
- **WHEN** a compiled sprite render rule produces position, facing, scale, family, color, and alpha
- **THEN** one plan binds the numeric, symbol, and presence outputs to the registered render-kind columns with values identical to interpreted row construction

### Requirement: Kernel plans declare execution bindings

Every executable kernel SHALL be installed through a domain plan that declares its iteration/group domain, program identity, direct and indirect inputs, captures/channels/state inputs, fixed outputs and next-state bindings, masks, and driver-owned fallback or merge behavior. Programs SHALL NOT read or write undeclared world state.

#### Scenario: Masked field update
- **WHEN** a recognized tick-rule shape filters rows and computes a fixed-width field update
- **THEN** its plan declares the predicate and value programs plus the output field, while the driver applies queued writes in canonical row order

#### Scenario: Indirect handle read
- **WHEN** a kernel reads a field through a generation-safe handle
- **THEN** the plan declares the indirect column input and handle validation contract, and stale-handle behavior matches interpreted evaluation

### Requirement: Cross-row topology and effects remain driver-owned

Reductions, compaction, collision pair generation, variable-length allocation, and action/effect application SHALL remain explicit driver or plan-template operations. Their supported fixed-width predicate, key, value, or projection expressions MAY use kernel programs, but an independent-lane program SHALL NOT implicitly allocate, synchronize lanes, or mutate scheduler/world topology.

#### Scenario: Collider projection and contact generation
- **WHEN** collider projector expressions compile
- **THEN** a kernel plan materializes fixed collider columns, after which the collision driver produces contact facts through its separately specified deterministic algorithm

#### Scenario: Cull rule
- **WHEN** a compiled predicate selects rows for culling
- **THEN** the kernel emits the match mask or fixed result records and the driver applies culls through the ordinary action path in canonical row order

### Requirement: Hot fixed-width surfaces converge on KernelProgram

Supported fixed-width computation in motion/dyn evaluation, dyn fields, collider projection, render-row projection, row predicates, and field-update values SHALL lower to `KernelProgram` rather than private executable expression representations. A surface MAY retain domain-specific semantic data and interpreted fallback, but SHALL NOT keep a second compiled evaluator for operations covered by the common kernel IR.

#### Scenario: Symbol and numeric render fields
- **WHEN** a render projection reads numeric and symbol entity fields using supported operations
- **THEN** both field classes execute through the same typed kernel/program-plan boundary

#### Scenario: Unsupported variable-shape expression
- **WHEN** a projector contains an operation outside the fixed-width kernel contract
- **THEN** the relevant plan/kernel remains interpreted as a whole rather than introducing a private compiled evaluator

### Requirement: Type checking and kernel lowering are independent tracks

Ergonomic source type checking and typed semantic elaboration MAY annotate resolved forms for reuse by kernel lowering, but neither capability SHALL be required to implement or execute the other. Kernel backends SHALL depend only on the fixed-width program/plan contract, and source diagnostics SHALL be specified independently of optimization coverage.

#### Scenario: Type-correct but unlowerable expression
- **WHEN** a source expression passes load-time type checking but uses an operation unsupported by the kernel IR
- **THEN** it remains semantically valid and executes through the interpreter

#### Scenario: Kernel lowering without whole-program type IR
- **WHEN** an existing schema-directed recognizer can prove a hot expression total and fixed-width
- **THEN** it may produce a kernel plan without waiting for universal typed elaboration of the card

## MODIFIED Requirements

### Requirement: The executor boundary is plans, typed lanes, and scratch

Batch call sites SHALL hand executors a domain kernel plan, typed input lanes, typed output lanes, and scratch storage—never reaching into op internals or undeclared world state. Kernel ops SHALL be total and callback-free. This boundary is the seam IR-loop, native, wasm, SIMD, and GPU executors share; the domain driver retains iteration orchestration, runtime-input validation, fallback, and deterministic merge.

#### Scenario: Vel batch
- **WHEN** rows sharing one compiled integrand program run as a batch
- **THEN** they execute as lanes of one motion plan through the common executor boundary, writing declared output/state columns directly

#### Scenario: Unsupported backend
- **WHEN** the selected backend cannot execute a plan or program operation
- **THEN** the driver selects the IR-loop or interpreted plan path before execution rather than re-entering the interpreter from compiled code

### Requirement: Programs are structurally interned with typed identity

Kernel programs SHALL be interned by structural identity of their typed op stream, declared inputs and outputs, and lane widths. Rand draws and fixed-width environment captures SHALL lower to input slots filled by per-entity capture vectors, so sites differing only in captured or drawn values share one program. Domain plans MAY differ in column bindings while referring to the same structurally interned program when their executable shape is identical.

#### Scenario: Two spawn sites, same typed shape
- **WHEN** two spawn sites differ only in a captured numeric value and have the same typed inputs, outputs, widths, and operation order
- **THEN** they share one interned program and may fuse into one compatible batch group

#### Scenario: Different width
- **WHEN** otherwise identical programs operate on different declared storage widths
- **THEN** they have distinct program identities and executor artifacts
