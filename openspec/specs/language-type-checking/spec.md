# language-type-checking Specification

## Purpose

Load-time semantic type analysis for expanded and resolved source forms, including authoritative domain-schema checks, explicit coercion elaboration, and structured source-oriented diagnostics independent of kernel optimization.

## Requirements

### Requirement: Type checking runs after expansion and name resolution

The checker SHALL analyze imported and macro-expanded forms after lexical/definition resolution and load-time schema collection have established the names and domain schemas visible at each site. Expansion SHALL retain authored source provenance sufficient to report generated errors at the originating call site.

#### Scenario: Macro-generated type error
- **WHEN** a macro expansion places a symbol value in a numeric field
- **THEN** the diagnostic identifies the authored macro call, the expected numeric field, and the relevant expansion step

#### Scenario: Shadowed builtin
- **WHEN** a lexical binding shadows a builtin name
- **THEN** the checker uses the resolved lexical binding's type rather than the builtin signature

### Requirement: Semantic types describe source meaning

The checker SHALL represent the source-level distinctions required by the governing language and schema contracts, including numbers, symbols, handles, nothing/options, poses/figures, homogeneous and unstructured collections, records, functions, dyn element/signal classes, entity views/sets, projectors, render rows, and actions. Semantic types SHALL NOT encode physical numeric width, GPU/CPU layout, kernel register classes, or optimization eligibility.

#### Scenario: Number stored in an f32 hot column
- **WHEN** a source `Num` later uses f32 physical storage
- **THEN** the author-facing type remains `Num` and no checker result depends on the selected backend width

#### Scenario: Type-correct unlowerable function
- **WHEN** a pure function is semantically well typed but unsupported by kernel lowering
- **THEN** the checker accepts it and execution may remain interpreted

### Requirement: Pure expressions infer types and domain slots provide expectations

The checker SHALL infer ordinary pure-expression and function types and SHALL check expressions against authoritative expected types at domain boundaries, including action positions, dyn/figure slots, spawn fields, collider/projector slots, render schemas, entity fields, query/manip callbacks, and registered function calls.

#### Scenario: Action in signal slot
- **WHEN** an action-valued expression is supplied where a dyn or figure value is expected
- **THEN** the checker reports a load-time type diagnostic naming the signal slot and the found `Action` type

#### Scenario: Query predicate returns symbol
- **WHEN** an `entities-where` predicate is statically known to return a symbol
- **THEN** the checker reports that the predicate requires a numeric mask

### Requirement: Expected-type coercion is coherent and explicit

Legal implicit source coercions SHALL follow one canonical elaboration order and SHALL be recorded explicitly in the typed result. At minimum, structural coercions such as pose-to-figure and homogeneous-list recognition occur before constant-to-dyn lifting and domain schema checks. Competing legal derivations to the same expected type SHALL denote the same value.

#### Scenario: Pose supplied to dyn figure slot
- **WHEN** a static pose is supplied where `Dyn<Figure>` is expected
- **THEN** elaboration records pose-to-figure followed by constant-to-dyn lifting and the checker accepts the expression

#### Scenario: Failed nested coercion
- **WHEN** one field of a structure cannot satisfy the expected dyn element type
- **THEN** the diagnostic identifies the field and the failed coercion step rather than reporting only the outer structure type

### Requirement: Schema checks use the authoritative registries

Entity-field, render-kind, collider/projector, host-channel, and other schema checks SHALL consume the same collected registries and merge/default rules used by load-time execution. The checker SHALL NOT maintain a parallel schema authority.

#### Scenario: Render field type mismatch
- **WHEN** a render row provides a field incompatible with its registered per-kind schema
- **THEN** the checker reports the kind, field, expected type, and found type using the authoritative render registry

#### Scenario: Unknown entity field
- **WHEN** a typed boundary reads or writes a field absent from the collected entity schema
- **THEN** the checker reports the authored field and boundary before simulation begins

### Requirement: Diagnostics are source-oriented and structured

Every checker diagnostic SHALL include a primary authored source location, expected and found semantic types, the relevant argument/slot/field/schema context, and a stable category. When definitions or macro expansion contribute to the mismatch, the diagnostic SHALL include the relevant call/definition or expansion provenance. Diagnostics SHALL NOT expose Rust implementation type names or kernel register types as the author-facing explanation.

#### Scenario: Function argument mismatch
- **WHEN** a call passes a figure to a parameter inferred as a handle
- **THEN** the diagnostic points to the argument, identifies the parameter/call context, and reports `Figure` versus `Handle`

### Requirement: Enforcement is staged and sound

Before checker coverage is authoritative, the implementation SHALL distinguish statically proven violations from unchecked/dynamic forms and checker limitations. Only statically proven violations covered by the governing language/schema contracts MAY become load errors. An unchecked form SHALL retain existing runtime checks and SHALL NOT become invalid solely because the checker or optimizer lacks coverage.

#### Scenario: Checker cannot infer a dynamic control value
- **WHEN** a valid form uses a deliberately dynamic value the current checker cannot classify
- **THEN** the form remains executable through existing runtime semantics and is recorded as unchecked rather than rejected as a type error

#### Scenario: Proven boundary mismatch after enforcement
- **WHEN** the checker proves a known symbol field is supplied to a numeric-only registered slot
- **THEN** card loading fails with the structured type diagnostic before the slot executes

### Requirement: Type checking and kernel optimization are independent

The checker MAY expose resolved ids, schema slots, or typed annotations for reuse, but kernel lowering SHALL NOT require whole-card type elaboration and type checking SHALL NOT depend on kernel operation coverage or backend availability. Neither change is a prerequisite for the other.

#### Scenario: Kernel recognizer lands first
- **WHEN** a schema-directed kernel recognizer proves an expression fixed-width before the type checker covers its enclosing card
- **THEN** the kernel may execute under the lowering oracle without changing checker behavior

#### Scenario: Checker lands first
- **WHEN** a card is fully type checked but no kernel backend recognizes its hot expression
- **THEN** it executes interpreted with the same valid semantics
