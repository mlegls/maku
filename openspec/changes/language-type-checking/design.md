## Context

Maku already has a semantic type system in practice: builtins have argument/return expectations; spawn slots distinguish figures, meta, and collider projectors; render kinds select record schemas; entity fields are interned into number/symbol/handle matrices; actions cannot inhabit signal slots; and dyns distinguish closed/scanned behavior. Today these checks are distributed across evaluator coercions, schema collection, spawn/projector construction, and simulation boundaries, so errors often lose source context or appear only when an execution path runs.

This change introduces a frontend-owned typed/elaborated representation for authoring ergonomics and tooling. It is deliberately separate from `KernelProgram`: source semantic types describe meaning, while kernel register types describe one executable representation of supported hot expressions. Either track may land first.

## Goals / Non-Goals

**Goals:**

- Report type and schema mistakes at card load with precise source and expansion provenance.
- Infer ordinary pure-expression and function types while checking domain slots against explicit expected types.
- Make implicit source coercions explicit and coherent in typed elaboration.
- Represent dyn element types and semantic signal classes without deciding storage or backend execution.
- Stage enforcement without breaking valid corpus programs because checker coverage is incomplete.
- Produce a stable typed frontend artifact usable by diagnostics, editor tooling, and later frontend consumers.

**Non-Goals:**

- A prerequisite or replacement for typed kernel lowering.
- Backend representation classification, optimization, storage layout, native/GPU code generation, or compile-cache identity.
- Changing valid `.maku` semantics or adding source syntax solely for the checker.
- Requiring every macro implementation to be typed before expansion.
- Eliminating all runtime checks for deliberately dynamic control data in the first milestone.

## Decisions

### 1. Check after expansion, with provenance

The pipeline is:

```text
source forms with spans
  -> imports and macro expansion, retaining expansion provenance
  -> resolved names and registered schemas
  -> typed / elaborated source representation
  -> existing interpreter and domain construction
```

Macros remain source transformers and are checked through their expanded result. Every expanded node retains its authored call-site span plus an expansion stack sufficient to name the macro/definition that introduced an invalid form.

Alternative: type macros before expansion. Rejected because Maku intentionally optimizes and gives meaning to expansion shapes, and typed macro systems would introduce a separate language feature.

### 2. Semantic types are frontend meaning, not machine layout

The initial semantic type universe includes:

```text
Num | Symbol | Handle | Nothing | Option<T>
Pose | Curve | Figure
List<T> | Array<T> | Vec<N,T> | Record<Row>
Fn<Args, Result>
Dyn<T, SignalClass>
EntityView<F> | EntitySet
ColliderData | ColliderProjector<F>
RenderData<K> | RenderProjector<F,K>
Action
```

Record rows carry known fields and types; open rows are allowed only where the governing source/schema contract permits them. `SignalClass` includes the language distinctions needed for legality and diagnostics (`Const`, `Closed`, `PiecewiseClosed`, `Integrated`, `Scanned`) but not storage choices.

Machine types such as `F32`, `U32`, and `Mask` do not appear here. Source `Num` may later lower to different physical widths; source `Symbol` may lower to `U32`; source `Pose` may flatten into lanes. Those are kernel/storage decisions.

### 3. Infer pure expressions; check expected domain boundaries

Ordinary literals, lexical bindings, pure function definitions/calls, arrays, records, and control expressions use unification-based inference. Domain construction supplies expected types:

```text
spawn figure       expects Dyn<Figure>
spawn meta         expects Dyn<Meta>
collider slot       expects ColliderProjector<F> or List<...>
render slot/rule    expects RenderData<K> / registered schema
query callback      expects EntityView<F> -> Num mask
manip callback      expects the action/value contract of its position
action tree slots   expect Action
entity fields       expect their collected schema kind
```

Builtin and special-form signatures are authoritative frontend data rather than reconstructed from Rust dispatch branches. Shadowing and lexical resolution run before signature use.

### 4. Expected-type coercion is explicit and coherent

The elaborator records coercion nodes for legal source conveniences. Canonical order:

1. apply non-dyn structural coercions required by the expected type, including `Pose -> Figure` and homogeneous list-to-array/vector recognition;
2. elaborate children under expected element/field types;
3. when `Dyn<S>` is expected, lift non-dyn children to `Const` and sequence structured dyn children once;
4. apply schema/projector/render/meta boundary checks.

Every legal derivation from one expression to one expected type must denote the same value. Diagnostics show the attempted coercion chain and the point that failed.

Alternative: mirror scattered runtime Rust conversion traits. Rejected because competing conversion paths produce inconsistent acceptance and poor diagnostics.

### 5. Typed elaboration is a frontend artifact, not the optimization IR

The checker produces typed nodes or an equivalent typed side table sufficient to:

- preserve resolved symbol/definition identity;
- record inferred and expected types;
- record explicit coercions;
- retain source/expansion provenance;
- expose schema field identities and signal classes.

The interpreter may continue executing existing semantic forms initially. `ir-unification` may consume resolved annotations when convenient, but its domain recognizers remain valid without whole-card elaboration. Type-correct code may remain unlowerable; lowerable code does not become valid merely because a kernel recognizer accepts its machine shape.

Alternative: make typed elaboration the mandatory optimizer input. Rejected because it couples ergonomic checker rollout to performance work and makes each track block the other.

### 6. Enforcement is staged by checker confidence, not optimization coverage

The first milestone runs over the corpus and classifies diagnostics:

- **proven violation**: incompatible known types or schemas;
- **unchecked/dynamic**: checker lacks sufficient information, existing runtime behavior remains;
- **checker defect**: accepted interpreter behavior the checker incorrectly rejects.

During parity rollout, proven violations are reported in diagnostic mode and corpus defects are fixed or the governing spec is clarified. Enforcement then makes only statically proven violations load errors. Unknown/checker-uncovered forms remain on existing runtime checks and do not become load errors merely because optimization cannot classify them.

### 7. Diagnostics are structured

A diagnostic contains:

- primary authored source span;
- concise expected and found semantic types;
- slot, field, schema, argument, or return context;
- relevant definition/call spans;
- macro expansion stack when generated;
- failed coercion step;
- stable diagnostic category/code for tooling and tests.

Messages name source types and source fields, never Rust enums or kernel registers.

### 8. Schema authorities remain single-source

The checker reads the same collected entity-field, render-kind, host-channel, collider/projector, and definition signatures used by load-time validation. It must not create a parallel registry with different merge/default rules. Owning semantics stay in `openspec/specs/language/spec.md`, `openspec/specs/render-rows/spec.md`, and `openspec/specs/load-time-schema/spec.md`.

### 9. Final checked versus dynamic boundary

The implemented checker owns ordinary source inference and the boundaries whose types are authoritative at load time: builtin/action signatures, spawn slots, projector fields, render rows, entity-view query predicates, host channels, and known entity-field accesses. It records typed nodes in a path-indexed side table; the interpreter continues to execute the original forms.

Enforced loading rejects only a `ProvenViolation`. Unknown callees, variadic callback shapes, keyword-led clauses, undeclared open render rows, host-supplied channel values, unresolved recursive types, and accesses whose entity schema is unavailable are reported as `Unchecked` or `CheckerLimitation` and retain their existing runtime validation. Dynamic numeric expressions may lift through pure source functions, but backend coverage and executable storage shape are never checker validity conditions.

The signature registry is the sole checker adapter for interpreter-dispatched vocabulary, while render/projector/entity schema checks read the collected runtime declarations rather than maintaining parallel merge or default logic. Corpus verification found no semantic ambiguity requiring a governing-spec change; the intentionally dynamic cases above are staged rather than rejected.

## Risks / Trade-offs

- **[Risk] Checker rejects valid dynamic idioms.** → Stage enforcement, retain an explicit unchecked state, and gate load errors on statically proven violations plus corpus parity.
- **[Risk] Typed nodes become a second runtime model.** → Keep them frontend-owned; interpreter and kernel backends consume only the annotations they need.
- **[Risk] Macro diagnostics point into generated code.** → Preserve call-site and expansion-stack provenance through every rewrite.
- **[Risk] Signature tables drift from runtime behavior.** → Generate or test signatures against the same builtin/special/schema authorities and add parity tests for every registered construct.
- **[Risk] HM-style inference becomes complex around records/dyns.** → Infer ordinary pure code, use expected types at domain boundaries, and avoid generalized row-polymorphism beyond demonstrated library needs.
- **[Risk] Checker and kernel lowering duplicate resolution.** → Share resolved ids/schema slots opportunistically, but keep separate semantic and machine type layers with no lifecycle dependency.

## Migration Plan

1. Preserve source spans and macro/import expansion provenance through the load pipeline.
2. Centralize builtin, special, projector, render, and field signatures without changing runtime dispatch.
3. Implement semantic types, unification, expected-type checking, and explicit coercion records for a pure core subset.
4. Add domain boundaries incrementally: functions, actions, dyn/figure slots, entity fields, projectors, render rows, queries/manip.
5. Run diagnostic-only over unit fixtures, tutorials, libraries, and the full card corpus; resolve checker defects and ambiguous governing semantics.
6. Make statically proven boundary violations load errors and retain runtime checks for explicitly unchecked forms.
7. Expose structured diagnostics and typed hover/query data to future tooling.

## Open Questions

- Whether typed elaboration is stored as a parallel side table over forms or a separate typed tree.
- The minimum record-row polymorphism needed by library adapters and generic helpers.
- Whether signal classes are inferred in the first enforcement milestone or initially attached only at known constructors/slots.
- Which intentionally dynamic forms receive an explicit source-visible annotation, if any, after corpus analysis.
