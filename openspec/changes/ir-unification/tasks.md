## 1. Typed Program Core

- [ ] 1.1 Add fixed-width kernel value/register metadata and explicit typed input/output descriptors around the existing `NumProgram` representation
- [ ] 1.2 Add typed float, integer, symbol, handle, mask, conversion, and selection operations without interpreter-valued operands
- [ ] 1.3 Add fixed multi-output execution and flatten pose components plus orientation presence into typed lanes
- [ ] 1.4 Include typed inputs, outputs, widths, and op order in structural program interning and cache identity
- [ ] 1.5 Generalize scalar and `run_lanes` IR-loop execution to the typed program while preserving existing motion operation order
- [ ] 1.6 Extend focused executor tests and the lowering oracle for integer/mask lanes, optional pose orientation, multiple outputs, width identity, and driver-level aborts

## 2. Kernel Plan ABI

- [ ] 2.1 Define common program ids and declared direct, indirect, capture, channel, tick/axis, state, output, and presence bindings
- [ ] 2.2 Define motion, dyn-field, filter, render-projection, collider-projection, and masked-update plan records without embedding interpreter callbacks
- [ ] 2.3 Route executor calls through plan-owned typed lanes and scratch while keeping iteration, validation, fallback, and deterministic merge in drivers
- [ ] 2.4 Add focused tests proving undeclared world access is impossible, stale-handle gathers preserve semantics, and unsupported plans select fallback before execution

## 3. Motion and Dyn Cutover

- [ ] 3.1 Migrate existing compiled motion programs to the typed program and motion-plan ABI without changing `DynNode` size or state ownership
- [ ] 3.2 Migrate `DynNum` fixed-width evaluation to dyn-field plans with whole-plan interpreted fallback
- [ ] 3.3 Dual-run migrated motion and dyn-field plans through their former semantic evaluators under `MAKU_LOWER_ORACLE`
- [ ] 3.4 Remove obsolete numeric compiled representations and call paths after all motion/dyn callers cut over

## 4. Predicate and Rule Cutover

- [ ] 4.1 Lower schema-resolved numeric and symbol field reads, comparisons, conjunction masks, and supported handle reads into typed filter programs
- [ ] 4.2 Replace compiled `ResolvedRowTest`/`ResolvedRowNum` execution with filter plans while preserving the specified short-circuit-prefix fallback
- [ ] 4.3 Migrate recognized cull rules and fixed-width field-update values to filter/masked-update plans with canonical driver application order
- [ ] 4.4 Extend the oracle to compare predicate masks, match order, update values, and produced actions without double-applying effects
- [ ] 4.5 Remove private compiled row-predicate/value evaluators after supported callers cut over

## 5. Render and Collider Projection Cutover

- [ ] 5.1 Lower fixed per-kind render fields, including numeric, interned-symbol, and presence outputs, into typed multi-output render-projection plans
- [ ] 5.2 Batch render-projection plans over declared columns and verify interpreted row equivalence without boxed intermediate render records
- [ ] 5.3 Lower fixed collider-projector fields and pose/curve-sample inputs into typed collider-projection plans
- [ ] 5.4 Keep variable-length geometry allocation and collision contact generation driver-owned while batching the fixed projection programs
- [ ] 5.5 Extend oracle coverage for render/collider output schemas, absence masks, symbols, geometry values, and row order
- [ ] 5.6 Remove `ProjectorNum` and private render-row compiled evaluators once every supported projection caller uses the common program

## 6. Verification and Cleanup

- [ ] 6.1 Run focused core tests for typed programs, each migrated plan family, deterministic fallback, and stale/optional boundary cases
- [ ] 6.2 Run the four ignored release oracle card suites with `MAKU_LOWER_ORACLE=1` and resolve every interpreter-versus-kernel mismatch
- [ ] 6.3 Run interleaved wall-only A/B measurements for the representative and scaled profiles; reject migrations that regress the governing perf thresholds
- [ ] 6.4 Update lowering design/status text to describe the landed `KernelProgram`/`KernelPlan` boundary and remove superseded `NumProgram`/private-evaluator guidance
- [ ] 6.5 Confirm no compiled hot-loop path contains an interpreter callback and no supported fixed-width surface retains a private compiled evaluator
