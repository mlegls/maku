# f32 hot columns (mixed numeric width)

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Mixed numeric width is the contract, not blanket f64 (decided 2026-07). f64 was Val/EDN inheritance, never load-bearing: determinism means same ops/order/width per tier, and replay compatibility — both restate over f32. f32 halves bandwidth (the profile's top fixed cost), doubles SIMD lanes, and legalizes GPU tiers (WGSL has no f64). Needed for the 100k+ scale tier.

## What Changes

- Control plane (interpreter, Val, spawn-time math) stays f64; HOT columns (positions, integrator state, collider radii, render batches) go f32.
- Care points: large-angle trig argument reduction inside the shared math shims (possibly f64 internally); long-lived integrators. tau is already integer-tick-anchored, so no time accumulation hazard.

## Capabilities

Numeric-width contract change; determinism spec restated over f32 widths.

## Impact

- Gate: run the card corpus with f32 columns against the f64 interpreter via MAKU_LOWER_ORACLE and read the measured drift — the oracle is the precision meter, not a guess.
- Related: f32 narrowing inside the mesh pack (`proto/mesh-touhou`) is a small independent slice.
- Governing: scale-target decision (this stub + `docs/notes/perf-campaign.md`), `docs/notes/compiled-dyn-design.md` determinism contract.
