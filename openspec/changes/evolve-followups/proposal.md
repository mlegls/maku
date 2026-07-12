# Evolve/remat follow-ups

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The `remat`/`change-col` contract is settled and landed (write queue, functional `change-col` composition, partial `(remat h spec-map)`, per-slot epochs, sited/live evolves, slew/smooth as prelude macros; semantics in `openspec/specs/language/spec.md` and `openspec/specs/evolve-semantics/spec.md`), but the track has open follow-ups.

## What Changes

- Per-dyn-field epochs (fades surviving motion remats), soft-cull fades, the F1 lint, and the masked-SoA fast path (the lowering target for batch `map`-remat shapes).
- `vel` re-expression: pure surface change, but deferred to the model/ split — b.vel introspection, clamp_integrator, and the compiled integrand programs all key on `DynNode::Vel`, so recognition is semantically mandatory (see `model-split`).
- `stages` re-expression: own round, likely over `states` rather than raw evolve (its corpus sites use exit slots, `forever`, and `(fn [exit] ...)` handoff; see `states-return-routing`).

## Capabilities

To be finalized at pick-up per slice.

## Impact

- Known edge (recorded, not blocking): the player-hit iframe guard reads pre-tick state, so two damage contacts in ONE tick both pass the guard — the atomic multi-field remat spec covers it if a card ever needs it.
- Known limitation: cart/polar/rot capture guards (`contains_unbound_axis`) run on the RAW form, before expansion — a macro whose expansion introduces t-dependence is not recognized as a dyn expression. Revisit if a real card hits it.
- Governing notes: `openspec/specs/evolve-semantics/spec.md`, `openspec/changes/model-split/design.md`.
