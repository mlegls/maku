# Intrinsics, arrays, and stdlib stance

Settled criteria for what becomes an intrinsic and where library code
lives. Moved from the old `docs/notes/TODO.md` "Intrinsics / Arrays"
and "Standard Library" sections (2026-07); these are decisions, not
open work. The kernel-shrink worklist and audit live in
`docs/notes/builtins-audit.md`; evolve semantics in
`docs/notes/evolve-design.md`.

## Intrinsic criterion

- Make an operation intrinsic only when it is hard to implement well in
  lib and is generically powerful. Everything else should start as lib
  code over `match` and seq views.
- Initial array/control candidates: `map`/each, `filter`, `fold`, `scan`,
  `each-prior`, `window`, `sort-by`, `best-by`, `count`, `nth`, `take`,
  `drop`, `concat`, and transpose/zip-style operations for tuple domains.
  Function argument destructuring reuses `match` pattern machinery, so
  collision pairs can be consumed as `(fn [[a b]] ...)` without a
  primitive `for-pairs`.
- K-inspired verbs/adverbs remain the direction, but the builtin set
  should be profiling-driven. Specialized operations such as binsearch,
  case, join/split, encode/decode, converge, and while-style adverbs can
  start in the prelude unless profiling proves they need lowering.
- Deterministic math/matrix intrinsics are part of this language, not
  delegated semantics. Native and wasm must replay identically;
  dependency upgrades must not silently change language behavior.
- Smooth noise should be a pure deterministic function of coords+seed,
  not sequential RNG state.
- Bullet-field image-processing ideas (rasterize query -> grid,
  FFT/filter, resample -> bullets) belong to a later intrinsic pass.

## Stdlib stance

- Keep Touhou/DMK/BDSL conventions in `cards/lib/touhou.maku` and
  related libraries. Core should remain a 2D graphing +
  collision/rule/render-row engine.
- Collision effects use `deftick` plus `(collisions ...)` domain
  expressions and ordinary `map`/destructuring. Keep Touhou
  hit/graze/shot rules in lib over opaque layers and fields; any
  ergonomic row-wise API should be lib/prelude sugar rather than a core
  special form.
- Governing principle (decided): NO sugar in lang. Minimize the surface
  to a semantic kernel; the surface vocabulary is lib macros over it,
  and optimization recognizes the macro EXPANSION SHAPE (AST patterns
  after expansion), never the name — hand-writing the same shape
  optimizes identically. Builtins return as AST-rewrite intrinsics
  driven by profiled bottlenecks.
