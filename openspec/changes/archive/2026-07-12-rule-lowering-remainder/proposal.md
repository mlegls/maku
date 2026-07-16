# Rule lowering remainder

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Milestone C's renderer half is done (compiled deftick render rules + numeric row predicates, `interp/rulelower.rs`), but interpreted rule scans remain ~8% of step on the round-21 sample (`evaluate_list_inner` — beam/cull/hp rules).

## What Changes

- Lower the remaining interpreted rule scans (beam/cull/hp shapes).
- Partial prefiltering for mixed entities-where predicates: recognized-plus-residual conjunctions currently fall back whole.
- Teach the compiled row-predicate recognizer the and/or if-chain expansion shape.
- If it shows in profiles: a per-batch symbol-id table so compiled tick passes stop cloning `Rc<str>` per row in sym-column reads.
- Load-time AST rewrite follow-ups (`interp/rewrite.rs`): (b) macro-expansion output is not rewritten (expansion is lazy per-eval; shapes inside macro-generated forms keep interpreted cost); (c) purity edge — a pure higher-order builtin applying an impure user fn passed BY NAME is classified pure; conservative table fix if it ever bites.

## Capabilities

Lowering-internal; oracle-gated.

## Impact

- `crates/core/src/interp/{rulelower,rewrite}.rs`, `sim/exec.rs`.
- Governing: `openspec/specs/lowering/spec.md`; walls in `openspec/specs/perf/spec.md`.
