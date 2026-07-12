# RNG spawn-order independence

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

RNG is sequential splitmix, so replay determinism holds but spawn-order independence does not: reordering spawns changes every subsequent draw.

## What Changes

- To be scoped at pick-up (e.g. independently seeded entities / hierarchical seeding), preserving the replay-determinism contract.

## Capabilities

To be finalized at pick-up.

## Impact

- RNG stream contract; the round-22 capture-vector draw order (`draw_caps` mirrors `subst_rand`'s walk) is part of the same contract and would need re-deriving.
- Also a prerequisite for parallelizing compiled entity hot loops (parallel entity order changes RNG unless entities are independently seeded) — relevant to the JIT tier's data-parallelism (`jit-native-codegen`).
- Related decision: smooth noise should be a pure deterministic function of coords+seed, not sequential RNG state (`docs/notes/intrinsics.md`).
