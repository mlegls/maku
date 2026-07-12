# JIT / native codegen tier

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The current tier is AOT-to-IR at card load (NumProgram, executed by the `run`/`run_lanes` interpreter loops); the destination is a JIT/native-codegen tier compiling the SAME NumProgram per distinct program, slotting in behind the same (program, input lanes, scratch) boundary, with the match-loop executors demoting to the fallback tier for cold/uncompiled programs. Cards known in advance also enable offline kernel codegen at publish (wasm module per card, same NumProgram lowering), with runtime channel binding.

## What Changes

- Native codegen (cranelift direction) + wasm AOT over interned NumPrograms.
- Hard requirement: bit-exact f64 semantics vs the IR interpreter (same op order, same libm, no fast-math) — the lowering oracle and replay/scrub determinism both depend on it.
- Data-parallelism comes nearly free at this tier (pure lanes over fixed scratch, deterministic merge points) — do NOT parallelize the interpreted hot loops instead (Rc-saturated values, ordered effects, sequential RNG; none of that work transfers).

## Capabilities

New execution tier; user-visible behavior unchanged (oracle-gated).

## Impact

- Blocked on `ir-unification`; stopping point before starting: "one interned, input-slotted IR runs all surfaces on the IR interpreter, three-way oracle scaffolding in place".
- Governing: `openspec/specs/lowering/spec.md` "JIT readiness" (cranelift/platform notes, no-Interp-op totality contract, batch seams).
- Precomputing future ticks is out: per-tick input/channel reads and the scrub/snapshot session model invalidate it.
- Decided: channel-free bullets recompute, never table — closed dyns ARE the precomputed form (program + captures in registers beat streamed position tables at scale), and bombs/cancels are cull masks, so channel-freeness survives interaction.
