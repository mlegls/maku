# Blocking lasers / world-geometry extent

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

Blocking lasers / world-geometry extent from DMK §13.7 remains unported; cards cannot express bullets blocked by world geometry.

## What Changes

- Port the DMK §13.7 blocking-laser / world-geometry extent semantics to Maku (see `docs/from-dmk.md` for mapping conventions).

## Capabilities

To be finalized at pick-up.

## Impact

- Collision layers and figure/collider surface; interacts with `collision-streaming` (a new geometry class should fit the per-layer streaming design rather than the general AABB index).
