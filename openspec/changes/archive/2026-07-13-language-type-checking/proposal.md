# Ergonomic language type checking

## Why

Maku already has semantic distinctions—numbers, symbols, handles, poses/figures, dyns, actions, entity views, projectors, render schemas, and typed entity fields—but many mismatches surface late through interpreter errors or subsystem-specific checks. A load-time checker with coherent expected-type elaboration can make authoring errors local and explainable without becoming a prerequisite for kernel optimization.

## What Changes

- Add a source-oriented type checker after import/macro expansion and name resolution, preserving source spans and expansion provenance for diagnostics.
- Infer ordinary pure-expression types and check expected types at language/domain boundaries such as function calls, dyn/figure slots, spawn metadata, collider projectors, render schemas, entity fields, queries, and action positions.
- Represent semantic types and explicit elaborations for author-facing meaning: numeric/symbol/handle atoms, `Nothing`/`Option`, poses/figures, lists/arrays/records, functions, dyn signal classes, entity views, projectors, render rows, entity sets, and actions.
- Apply one coherent expected-type coercion order, including constant-to-dyn lifting, pose-to-figure lifting, and list-to-homogeneous-structure coercion, and report the failed coercion path.
- Produce diagnostics with source location, expected/found types, relevant schema or slot, definition/call context, and macro expansion provenance.
- Stage adoption across the card corpus: begin by proving parity and reporting diagnostics, then make statically provable boundary violations load errors once the governing language requirements and corpus agree.
- Keep semantic typing independent from execution representation. `KernelProgram` lowering may reuse resolved annotations opportunistically, but type-correct code need not compile and kernel lowering need not wait for whole-card typed elaboration.
- Exclude native/GPU code generation, storage classification, optimizer coverage decisions, and changes to valid program meaning.

## Capabilities

### New Capabilities

- `language-type-checking`: Source semantic types, inference/elaboration, typed boundary checks, diagnostics, macro provenance, and staged load-time enforcement.

### Modified Capabilities

None.

## Impact

- Parser/form source spans, import/macro expansion provenance, definition environments, builtin signatures, and load-time schema resolution in `crates/core/src/interp/`.
- Semantic type/elaboration modules should remain frontend-owned rather than living in kernel/codegen modules.
- Governing language meaning remains `openspec/specs/language/spec.md`; render and schema boundaries remain governed by `openspec/specs/render-rows/spec.md` and `openspec/specs/load-time-schema/spec.md`.
- Independent of `ir-unification`; either track may land first, and neither is an implementation prerequisite for the other.
