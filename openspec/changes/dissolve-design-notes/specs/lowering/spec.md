# lowering

## ADDED Requirements

### Requirement: The executor boundary is lanes plus scratch
Batch call sites SHALL hand executors a compiled program, input lanes, and scratch storage — never reaching into op internals — and compiled ops SHALL be total and callback-free. This boundary is the seam a JIT/native tier drops into behind the same signature.

#### Scenario: Vel batch
- **WHEN** rows sharing one compiled integrand program run as a batch
- **THEN** they execute as lanes of one program run through the `run_lanes`-shaped boundary, writing output columns directly

### Requirement: Programs classify all-or-nothing
A surface expression SHALL either lower to a complete compiled program or stay fully interpreted — no partial compilation with interpreter re-entry from inside a program. Runtime aborts (e.g. a numeric read hitting a keyword-valued field) stay at the driver level: abort the pass, rerun interpreted (per the determinism spec's fallback requirement).

#### Scenario: Unlowerable subform
- **WHEN** a motion expression contains a form the lowerer does not cover
- **THEN** the whole expression evaluates on the interpreter path, bit-identically to pre-lowering behavior

### Requirement: Programs are structurally interned
Compiled programs SHALL be interned by structural identity of their op stream (the compile-cache key). Rand draws and numeric environment captures SHALL lower to input slots filled by per-entity capture vectors, so sites differing only in captured or drawn values share one program.

#### Scenario: Two spawn sites, same shape
- **WHEN** two spawn sites differ only in a captured numeric value
- **THEN** they share one interned program and may fuse into one batch group

### Requirement: Hot node types stay small
`DynNode` SHALL stay ≤ 96 bytes (test-pinned by `dyn_node_stays_small`); new per-variant data goes behind a one-word `Option<Rc<..>>`. Pose-chain walks chase these enums on every hot path; the 88→120-byte draft cost ~60% wall.

#### Scenario: Adding variant data
- **WHEN** a change adds per-variant data to DynNode
- **THEN** the size-guard test fails unless the data is behind a one-word indirection

### Requirement: The IR interpreter is the permanent fallback tier
The IR-interpreter executor tier SHALL remain supported on every host as the universal fallback for cold/uncompiled programs, and the interpreted CONTROL PLANE (card loading, macros, scheduler/action tree, states/phases, live eval/swap) SHALL stay interpreted and user-facing permanently. Interpreted per-entity hot loops SHALL NOT be parallelized or pre-computed — that work does not transfer to the compiled form.

#### Scenario: Wasm host without codegen
- **WHEN** a host cannot run native codegen
- **THEN** every card still runs correctly on the IR interpreter tier with identical semantics
