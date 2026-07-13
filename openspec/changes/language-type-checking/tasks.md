## 1. Source Provenance and Signatures

- [ ] 1.1 Preserve authored spans through parsed forms, imports, macro expansion, and load-time rewrites
- [ ] 1.2 Record expansion stacks linking generated nodes to macro/definition call sites
- [ ] 1.3 Centralize authoritative builtin, special-form, projector, render-schema, entity-field, and action-slot signatures without changing runtime dispatch
- [ ] 1.4 Add focused tests for spans, nested expansion provenance, lexical shadowing, and signature/runtime parity

## 2. Semantic Type Core

- [ ] 2.1 Define frontend semantic types for atoms, options, geometry, collections/records, functions, dyn/signal classes, entity views/sets, projectors, render rows, and actions
- [ ] 2.2 Implement type variables, unification, scoped environments, function generalization limited to the demonstrated pure-language needs, and structured type formatting
- [ ] 2.3 Define typed/elaborated nodes or side-table records carrying resolved identity, inferred/expected types, coercions, schemas, and source provenance
- [ ] 2.4 Add focused inference tests for literals, lexical bindings, functions/calls, arrays, records, branches, recursion boundaries, and mismatches

## 3. Expected-Type Elaboration

- [ ] 3.1 Implement canonical pose-to-figure and homogeneous-list structural coercions
- [ ] 3.2 Implement constant-to-dyn lifting, structured dyn sequencing, and signal-class annotations without selecting runtime storage
- [ ] 3.3 Check function arguments/returns and action versus value positions under explicit expected types
- [ ] 3.4 Produce failed-coercion diagnostics naming the inner field/element and attempted coercion path
- [ ] 3.5 Add coherence tests proving equivalent legal derivations elaborate to the same semantic value

## 4. Domain Boundary Checking

- [ ] 4.1 Check spawn figure/meta/collider slots against `Dyn<Figure>`, typed meta, and figure-specialized projector expectations
- [ ] 4.2 Check entity-field reads/writes and query/manip callback parameters/results against the collected field schema and entity-view type
- [ ] 4.3 Check collider/projector constructor records and figure-specific accessors through the authoritative projector registry
- [ ] 4.4 Check per-kind render rows through the authoritative render registry, including field presence and open-schema rules
- [ ] 4.5 Check host-channel and other load-time schema references without duplicating registry merge/default behavior
- [ ] 4.6 Add focused valid/invalid fixtures for every checked boundary and stale/unknown/schema-conflict case

## 5. Diagnostics and Staged Enforcement

- [ ] 5.1 Add structured diagnostic categories containing primary span, expected/found types, boundary context, related definition spans, expansion stack, and coercion failure
- [ ] 5.2 Add a diagnostic-only card-load mode that distinguishes proven violations, unchecked/dynamic forms, and checker limitations
- [ ] 5.3 Run the checker over libraries, tutorials, examples, and the full card corpus; resolve false positives and record genuinely ambiguous semantics in the owning specs
- [ ] 5.4 Make corpus-verified statically proven boundary violations load errors while retaining existing runtime checks for unchecked forms
- [ ] 5.5 Expose stable typed-query/diagnostic data for future editor hover and tooling without coupling it to kernel eligibility

## 6. Verification and Cleanup

- [ ] 6.1 Run focused checker tests for inference, coercion, schemas, diagnostics, macro provenance, and enforcement boundaries
- [ ] 6.2 Run the full card corpus in diagnostic and enforced modes and verify every previously valid unchecked program retains runtime behavior
- [ ] 6.3 Verify type-correct but kernel-unlowerable programs run interpreted and kernel-lowerable programs do not require whole-card elaboration
- [ ] 6.4 Remove duplicated signature/schema adapters, document the final checked versus dynamic boundary in the owning design, and confirm author-facing diagnostics contain no Rust or kernel-register terminology
