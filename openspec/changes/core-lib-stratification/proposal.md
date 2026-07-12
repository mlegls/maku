# Core-vs-lib builtin stratification

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The semantic kernel should keep shrinking before the compiler pass: specials are the IR, pure builtins are intrinsics, and anything expressible in `.maku` without hot-path or boundary semantics should move toward lib code. Wave 1 of the audit is done.

## What Changes

- Continue the kernel-shrink worklist in `docs/notes/builtins-audit.md`: easings and derived array verbs wait on profiling; `map`/`filter` intrinsic-ification and the `channel` merge wait on their flags.
- Current interpreter categories (math/array/language/geometry builtins vs engine specials) are recorded in the audit note.

## Capabilities

Surface-vocabulary stratification; language semantics unchanged.

## Impact

- `proto/core/src/interp/builtins/*`, `interp/engine.rs`, `cards/lib/`.
- Governing principle (no sugar in lang, expansion-shape optimization) and intrinsic criteria: `docs/notes/intrinsics.md`; worklist: `docs/notes/builtins-audit.md`.
