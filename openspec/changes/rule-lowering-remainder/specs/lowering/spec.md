# lowering — rule-lowering-remainder delta

## MODIFIED Requirements

### Requirement: Programs classify all-or-nothing
A surface expression SHALL either lower to a complete compiled program or stay fully interpreted — no partial compilation with interpreter re-entry from inside a program. Runtime aborts (e.g. a numeric read hitting a keyword-valued field) stay at the driver level: abort the pass, rerun interpreted (per the determinism spec's fallback requirement).

One driver-level composition is permitted and is not partial compilation: an entities-where predicate whose post-expansion form is a short-circuiting conjunction chain MAY split into a fully-compiled prefix filter plus an interpreted residual, where the residual is evaluated by the interpreter at the driver level on prefix-surviving rows only. This split SHALL be exactly semantics-preserving: because the interpreter short-circuits the chain, rows the compiled prefix rejects would never have had their residual evaluated interpreted either. Non-short-circuiting conjunction shapes (the `*` product form) SHALL NOT split — they compile whole or fall back whole.

#### Scenario: Unlowerable subform
- **WHEN** a motion expression contains a form the lowerer does not cover
- **THEN** the whole expression evaluates on the interpreter path, bit-identically to pre-lowering behavior

#### Scenario: Mixed short-circuit predicate
- **WHEN** an entities-where predicate expands to a short-circuit conjunction whose leading conjuncts are recognized row tests and whose tail is an unrecognized form
- **THEN** the recognized prefix runs as a compiled row scan and the interpreted residual runs only on rows the prefix accepts, with match set, errors, and evaluation order identical to the fully interpreted scan

#### Scenario: Mixed product predicate
- **WHEN** an entities-where predicate is a `*` product conjunction with any unrecognized conjunct
- **THEN** the whole predicate evaluates interpreted per row (no prefix split), because the interpreter evaluates every product conjunct on every row

## ADDED Requirements

### Requirement: Cull rules compile
The tick-rule lowerer SHALL recognize the cull-rule expansion shape — a `map` whose function body is exactly a `cull` of the row parameter over an `entities-where` whose predicate compiles to row tests — and execute it as a compiled predicate scan followed by cull application per matched row in row-index order, through the same action-application path the interpreter uses for `cull` actions. Any deviation from the shape SHALL bail the whole form to the interpreter. Under the lowering oracle, the compiled scan's predicted match set SHALL be checked against the interpreted evaluation's produced cull actions (set and order) with the interpreted path as the single applier, so oracle runs never double-apply effects.

#### Scenario: Enemy hp cull rule
- **WHEN** a standing rule culls entities matching a compiled-recognizable predicate (e.g. team keyword equals plus an hp column comparison)
- **THEN** the rule runs as a compiled scan plus per-row cull with world effects identical to the interpreted rule, and the oracle dual-run confirms match set and action order

#### Scenario: Cull body deviation
- **WHEN** the map body is anything other than exactly a cull of the row parameter (extra forms, shadowed `cull`, different argument)
- **THEN** the whole form stays interpreted

### Requirement: Predicate recognition covers short-circuit conjunction expansion
The row-predicate recognizer SHALL recognize the post-expansion if-chain shape that short-circuit conjunction macros produce and fold it into the same conjunct list as the product form, keyed on the expansion structure (never on macro names). Disjunction if-chain shapes SHALL be recognized structurally only to fall back whole — no disjunction row tests. Because recognized conjuncts are pure and total by construction, evaluating all folded conjuncts is bit-identical to short-circuit evaluation.

#### Scenario: and-chain predicate compiles
- **WHEN** an entities-where predicate is the expansion of a short-circuit conjunction of recognizable row tests
- **THEN** it compiles to the same row-test conjunct scan the equivalent product form compiles to, with an identical match set to interpreted evaluation

#### Scenario: or-chain predicate bails
- **WHEN** an entities-where predicate expands to a disjunction if-chain
- **THEN** the predicate falls back whole to interpreted per-row evaluation

### Requirement: Registered rule bodies get the load-time rewrite
Macro expansion performed at `deftick` registration SHALL be followed by the same load-time AST rewrite applied to card forms (value-or intrinsic recognition and trivial-definition inlining), under the same shadowing discipline (names bound in the enclosing environment or card definitions suppress the rewrite), before tick-form lowering runs. Expansion shapes inside macro-generated rule bodies thereby become recognizable to the lowerer instead of retaining interpreted cost.

#### Scenario: Macro-generated rule body lowers
- **WHEN** a deftick body produced by a card macro contains a shape that rewrites to the value-or intrinsic (e.g. a default-column read)
- **THEN** after registration-time rewrite the tick form compiles under the existing recognizers, and evaluation is bit-identical to the unrewritten interpreted form

#### Scenario: Shadowed name suppresses rewrite
- **WHEN** a rule body binds a local name that shadows a rewrite-eligible head
- **THEN** the rewrite leaves that form untouched and evaluation uses the local binding
