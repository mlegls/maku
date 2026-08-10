# Evolve/remat follow-ups

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

The `remat`/`change-col` contract is settled and landed (write queue, functional `change-col` composition, partial `(remat h spec-map)`, per-slot epochs, sited/live evolves, slew/smooth as prelude macros; semantics in `openspec/specs/language/spec.md` and `openspec/specs/evolve-semantics/spec.md`), but the track has open follow-ups.

## What Changes

- Per-dyn-field epochs (fades surviving motion remats), soft-cull fades, the F1 lint, and the masked-SoA fast path (the lowering target for batch `map`-remat shapes).
- `vel` re-expression — target shape settled 2026-08-10: `vel` becomes the ONE stock integrator evolve (`p += vel·dt`, spatial clamp composing inside the step) whose components are ordinary (possibly dyn) entity meta columns (`:vel-x`/`:vel-y`), like `:hue`/`:facing`. Position stays a per-slot stateful dyn — epochs, closed/live classification, and segment history are preserved; the fully-push variant (a tick rule accumulating pos) is rejected because it exits the signal model. Consequences: `b.vel` introspection and remat exit-state snap become field reads; sited evolves in components live in the existing dyn-field scan context; the `DynNode::Vel` keying in introspection/clamp_integrator/integrand programs collapses to one stock integrand shape (a kernel plan template), with per-entity variation handled by ordinary dyn-field compilation/grouping. Open at pick-up: pin tick ordering between vel-column evaluation and integrator advance in the determinism contract; decide materialized-column vs fused-plan execution at scale (interaction with `group-integrator-dedup`'s ConstFrame sharing).
- `stages` re-expression: own round, likely over `states` rather than raw evolve (its corpus sites use exit slots, `forever`, and `(fn [exit] ...)` handoff; see `states-return-routing`).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `evolve-semantics`: vel as the stock integrator evolve over component columns; single per-tick component evaluation owned by the integrator; per-dyn-field epochs (soft-cull fades expressible in lib); the F1 lint over component programs.
- `lowering`: batch `map`-remat shapes lower to masked SoA updates.

## Impact

- Known edge (recorded, not blocking): the player-hit iframe guard reads pre-tick state, so two damage contacts in ONE tick both pass the guard — the atomic multi-field remat spec covers it if a card ever needs it.
- Known limitation: cart/polar/rot capture guards (`contains_unbound_axis`) run on the RAW form, before expansion — a macro whose expansion introduces t-dependence is not recognized as a dyn expression. Revisit if a real card hits it.
- Governing notes: `openspec/specs/evolve-semantics/spec.md` (its "still open: vel — deferred to the model/ split" note is superseded by the settled target shape above; fold into the delta spec when this change's specs are generated), `openspec/changes/model-split/design.md`.
