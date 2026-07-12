# Collapse the pose/figure asymmetry

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

`DynLike::Dyn(Pose)` is a typed dynamic value, not a data atom. The target is plain `Figure` values lifted through `Dyn<Figure>`, with `linear` and friends represented as optimized `Dyn<Pose>` constructors that lift to figure dynamics.

## What Changes

- Collapse the remaining pose/figure asymmetry onto `Dyn<Figure>`.
- Keep dyn coercions as explicit language-semantic branches while the interpreter is untyped: `interp::coerce` owns the value-level `DynLike` bridge; a future trait-style coercion surface should be over typed IR targets, not scattered Rust conversions over raw values.

## Capabilities

Value-model unification; semantics per `openspec/changes/entity-representation-flip/design.md`.

## Impact

- `interp::coerce`, figure/dyn kernel; interacts with `model-split` (likely the same or adjacent round).
