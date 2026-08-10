# lowering — delta for f32-hot-columns

## ADDED Requirements

### Requirement: Physical hot columns are f32; compute width is a separate contract

The dense per-entity columns of the spec-table storage model —
integrator state cells, sampled/trace/root-frame pose columns, capture
vectors, numeric field cells, collider rows/AABBs, render batch
geometry columns — SHALL be physically f32, with accessor APIs that
keep f64 signatures (widen on load, round on store) so call sites and
kernels remain width-agnostic. Declared program *compute* width
remains F64 for all production lowering; F32 program emission and
width-narrowed executor arithmetic land only with the codegen/GPU
backends, which consume these dense f32 buffers rather than defining
another width policy. Trig shims for narrowed compute (argument
reduction in f64) are owed by the change that first emits F32 ops.

#### Scenario: Kernel lanes over f32 columns

- **WHEN** a batched kernel gathers lanes from f32 columns
- **THEN** lanes are widened to f64 at gather, the program executes at
  its declared F64 width, and results round once when stored back to
  an f32 column

#### Scenario: GPU backend consumes the buffers

- **WHEN** a later backend needs dense f32 state for residency
- **THEN** the columns are already physically f32 and no additional
  width migration of world storage is required

## MODIFIED Requirements

(none — the width-parametric determinism clause and the typed
program-identity requirements stand as written; this delta supplies
the physical storage-class classification they point at.)
