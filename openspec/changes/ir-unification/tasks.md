## 1. Typed Program Core

- [x] 1.1 Add fixed-width kernel value/register metadata and explicit typed input/output descriptors around the existing `NumProgram` representation
- [x] 1.2 Add typed float, integer, symbol, handle, mask, conversion, and selection operations without interpreter-valued operands
- [x] 1.3 Add fixed multi-output execution and flatten pose components plus orientation presence into typed lanes
- [x] 1.4 Include typed inputs, outputs, widths, and op order in structural program interning and cache identity
- [x] 1.5 Generalize scalar and `run_lanes` IR-loop execution to the typed program while preserving existing motion operation order
- [x] 1.6 Extend focused executor tests and the lowering oracle for integer/mask lanes, optional pose orientation, multiple outputs, width identity, and driver-level aborts

## 2. Kernel Plan ABI

- [x] 2.1 Define common program ids and declared direct, indirect, capture, channel, tick/axis, state, output, and presence bindings
- [x] 2.2 Define motion, dyn-field, filter, render-projection, collider-projection, and masked-update plan records without embedding interpreter callbacks
- [x] 2.3 Route executor calls through plan-owned typed lanes and scratch while keeping iteration, validation, fallback, and deterministic merge in drivers
- [x] 2.4 Add focused tests proving undeclared world access is impossible, stale-handle gathers preserve semantics, and unsupported plans select fallback before execution

## 3. Motion and Dyn Cutover

- [x] 3.1 Migrate existing compiled motion programs to the typed program and motion-plan ABI without changing `DynNode` size or state ownership
- [x] 3.2 Migrate `DynNum` fixed-width evaluation to dyn-field plans with whole-plan interpreted fallback
- [x] 3.3 Dual-run migrated motion and dyn-field plans through their former semantic evaluators under `MAKU_LOWER_ORACLE`
- [x] 3.4 Remove obsolete numeric compiled call paths after motion/dyn cutover; retain `NumProgram` only as the specialized F64 CPU backend bridged from canonical `KernelProgram` identity

## 4. Predicate and Rule Cutover

- [x] 4.1 Lower schema-resolved numeric and symbol field reads, comparisons, conjunction masks, and supported handle reads into typed filter programs
- [x] 4.2 Replace compiled `ResolvedRowTest`/`ResolvedRowNum` execution with filter plans while preserving the specified short-circuit-prefix fallback
- [x] 4.3 Migrate recognized cull rules and fixed-width field-update values to filter/masked-update plans with canonical driver application order
- [x] 4.4 Extend the oracle to compare predicate masks, match order, update values, and produced actions without double-applying effects
- [x] 4.5 Remove private compiled row-predicate/value evaluators after supported callers cut over

## 5. Render and Collider Projection Cutover

- [x] 5.1 Lower fixed per-kind render fields, including numeric, interned-symbol, and presence outputs, into typed multi-output render-projection plans
- [x] 5.2 Batch render-projection plans over declared columns and verify interpreted row equivalence without boxed intermediate render records
- [x] 5.3 Lower fixed collider-projector fields and pose/curve-sample inputs into typed collider-projection plans
- [x] 5.4 Keep variable-length geometry allocation and collision contact generation driver-owned while batching the fixed projection programs
- [x] 5.5 Extend oracle coverage for render/collider output schemas, absence masks, symbols, geometry values, and row order
- [x] 5.6 Remove `ProjectorNum` and private render-row compiled evaluators once every supported projection caller uses the common program

## 6. Verification and Cleanup

- [x] 6.1 Run focused core tests for typed programs, each migrated plan family, deterministic fallback, and stale/optional boundary cases
- [x] 6.2 Run the four ignored release oracle card suites with `MAKU_LOWER_ORACLE=1` and resolve every interpreter-versus-kernel mismatch
- [x] 6.3 Run interleaved wall-only A/B measurements for the representative and scaled profiles; reject migrations that regress the governing perf thresholds
- [x] 6.4 Update lowering design/status text to describe the landed `KernelProgram`/`KernelPlan` boundary, including the retained specialized F64 backend, and remove superseded private-evaluator guidance
- [x] 6.5 Confirm no compiled hot-loop path contains an interpreter callback and no supported fixed-width surface retains a private compiled evaluator

## Completion Evidence

- Typed executor/lowering: `sim::kernel::tests` 7 passed; `interp::lower::tests` 19 passed.
- Migrated surfaces: `compiled_` 8, `dyn_` 10, `collider_` 23, `projection_` 3, `masked_update` 2, and the stale-handle boundary 1 all passed; compiled/update oracle reruns also passed.
- Release oracle corpus: `translations_run`, `tutorial_cards_run`, `reimu_vs_mima_plays`, and `duel_card_plays` each passed in release mode with `MAKU_LOWER_ORACLE=1`.
- Five-pair wall-only A/B against `b11ec85`: representative median 373.7→364.8ms (−2.38%); scaled fruit median 2344.2→2261.9ms (−3.51%).
- Audit: executable `KernelProgram`/`KernelPlan` data contains only fixed-width typed operations, layouts, bindings, and policies. No `Val`, `Form`, `Env`, or interpreter callback is stored in a kernel. The obsolete `ResolvedRowTest`/`ResolvedRowNum`, `ProjectorNum`, and private render-row evaluator paths are absent. Remaining `DynNum`/collider expressions are semantic fallback and cold installation sources; `NumProgram` is the typed motion plan's specialized F64 CPU backend.
