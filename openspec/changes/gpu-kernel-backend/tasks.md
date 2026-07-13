## 1. Backend Contract and Eligibility

- [ ] 1.1 Define GPU capability records, typed bind layouts, dispatch descriptors, and all-or-nothing eligibility over the landed `KernelPlan` ABI
- [ ] 1.2 Define structural shader/pipeline cache keys including program types, widths, output shape, math-shim version, workgroup specialization, and device class
- [ ] 1.3 Add focused eligibility tests for unsupported ops, widths, bindings, resource limits, and hosts without GPU compute

## 2. Deterministic WGSL Emission

- [ ] 2.1 Emit WGSL for the initial fixed-width arithmetic, integer/symbol, mask, selection, conversion, and multi-output kernel operations
- [ ] 2.2 Implement or bind deterministic WGSL math shims matching the governing CPU operation and width contracts
- [ ] 2.3 Compile and cache generated modules/pipelines without specializing captures or per-row values
- [ ] 2.4 Add per-op IR-loop versus GPU conformance tests across representative values and boundary cases

## 3. Host and Buffer Integration

- [ ] 3.1 Add host-provided GPU device/queue capability integration without making GPU support mandatory
- [ ] 3.2 Bind f32 hot columns, integer/symbol columns, masks, captures, immutable tick/channel inputs, outputs, and next-state buffers from dispatch descriptors
- [ ] 3.3 Implement explicit synchronization, dirty/range-aware readback, backend switching, and session snapshot restoration for resident buffers
- [ ] 3.4 Add focused tests for snapshot/replay parity, fallback after residency, and absence of hidden future/device-only state

## 4. Initial Plan Families

- [ ] 4.1 Execute eligible motion plans on GPU with declared current/next state and three-way interpreter/IR-loop/GPU oracle comparison
- [ ] 4.2 Execute eligible dyn-field plans on GPU while preserving tick boundaries, presence masks, and fallback
- [ ] 4.3 Execute fixed render-projection plans on GPU and expose ordered typed render columns to existing host consumers
- [ ] 4.4 Add measured row-count thresholds so small eligible groups remain on the faster IR/native executor

## 5. Bounded Collider Projection

- [ ] 5.1 Define GPU eligibility for fixed bounded collider-projection outputs without moving contact generation or variable allocation onto the device
- [ ] 5.2 Execute eligible collider projections into declared columns and verify geometry/layer/presence parity against interpreted projection
- [ ] 5.3 Keep collision streaming/contact ordering CPU-owned and verify backend selection does not change collision facts

## 6. Verification and Cleanup

- [ ] 6.1 Run focused GPU conformance tests on every supported plan family, width, mask/presence boundary, and backend-switch path
- [ ] 6.2 Run representative card suites through the three-way oracle and verify session replay/scrub parity
- [ ] 6.3 Run wall-only end-to-end A/B measurements including upload, dispatch, synchronization, and readback at normal and scale-ceiling row counts
- [ ] 6.4 Remove temporary backend-specific duplication, document the landed capability/limit table in the owning design, and confirm every unsupported path falls back before execution
