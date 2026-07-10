# Special forms / builtins / keywords audit

Method: every name the interpreter dispatches on was enumerated from the
tables in `interp/mod.rs` (specials), `interp/engine.rs` (engine specials),
`interp/builtins/{math,array,language,geometry}.rs` (pure builtins),
`interp/card.rs` (top-level forms), plus reserved keywords inside forms.
Each gets a verdict for the core-vs-lib stratification (TODO.md):

- **IR** — stays a special: binding/control semantics, action-tree
  construction, or a World/boundary effect. Specials are the IR.
- **INTRINSIC** — stays a pure builtin: deterministic-math contract or
  macro-layer support that lib code cannot provide.
- **LIB** — expressible today in `.maku` over more generic primitives;
  should move to `cards/lib/prelude.maku` (hot ones may keep a
  compiler-recognized lowering later).
- **LIB-AFTER(x)** — expressible once prerequisite x exists.
- **DROP/MERGE** — redundant surface.
- **FLAG** — needs a design decision; noted below the tables.

Per the recorded stance, easings and other hot pure functions stay
intrinsics *pending profiling*; the verdict marks semantic expressibility,
the profiling harness decides the final placement.

## Top-level card forms (`card.rs`)

| name | verdict | note |
|---|---|---|
| `def`, `defn`, `defmacro`, `defpattern`, `defchannel`, `deftick` | IR | load-time registration surface |
| `defcollider` | IR (sugar) | already `def` + `collider` |
| `import` | IR | load/resolve boundary |

## Control / binding specials (`interp/mod.rs`)

| name | verdict | note |
|---|---|---|
| `let`, `fn`, `if`, `cond`, `match`, `quote`, `quasiquote` | IR | core binding/branch/staging |
| `loop`, `recur` | IR | the iteration primitive (decided) |
| `for` / `dotimes` | LIB | decided: macros over `loop`/`recur` + `wait`; drop the `sf_dotimes` special |
| `map`, `filter` | FLAG(1) | special today; should become intrinsics over seq views — they carry no binding magic, they're in the special table only for dispatch convenience |
| `when`, `unless` | (already LIB) | prelude macros — the model to follow |
| `inf`, `phi` | LIB | constants; `(def inf ...)` needs a literal or one intrinsic constant table entry |

## Action-tree specials

| name | verdict | note |
|---|---|---|
| `seq`, `par`, `fork`, `wait`, `until`, `finally`, `race`, `goto`, `wait-for`, `event` | IR | scheduler node constructors |
| `states`, `stages`, `stages-action` | IR | FSM/staged-dyn cores; richer templates stay lib (TODO) |
| `inline` | IR | pattern-embedding scope adapter |
| `defvar`, `channel`, `bind-channel!`, `live`, `in-frame` | IR | cells/channels/frame ambience |
| `export` | IR | host boundary |

## Spawn / figure / dyn specials

| name | verdict | note |
|---|---|---|
| `spawn` | IR | the entity constructor |
| `collider`, `circle-collider`, `capsule-chain-collider` | IR | closed projector algebra (decided design) |
| `curve-samples`, `render` | IR | render boundary (decided design) |
| `curve`, `fields` | IR | figure constructor / element-seed carrier |
| `circle`, `fan`, `arrow` | LIB | formations = arrays of poses composed with the child: `(circle n)` ≈ `(map (fn [k] (rot (* k (/ 360 n)))) (iota n))`; array-of-figures already broadcasts at spawn. Move; keep nothing in core |
| `aim` | LIB-AFTER(ambient readable) | pure math over target and the ambient frame; it is a special only because `ctx.ambient` is not a readable value |
| `sample` | IR | pure dyn evaluation — the kernel the others reduce to |
| `pather`, `path`, `vel`, `slew`, `smooth`, `scan`, `clamp` | FLAG(2) | the dyn-motion kernel; `scan` is the integrator primitive, the rest are candidates to re-express over it. NB `clamp` is spatial — `(clamp c[lo] c[hi] dyn)` boxes a dyn's position, not numeric min/max |
| `rand`, `rand-int` | IR | deterministic RNG stream is engine state |
| `pos` (engine), `entity-pos` | MERGE | two accessors for the same read; keep one that takes handle or view |
| `on-curve` | LIB-AFTER(entity figure readable) | = `sample` of a live entity's curve; needs a way to read the figure off a handle/view |
| `nearest-entity` | LIB-AFTER(`best-by` intrinsic) | query + argmin |
| `entities-where`, `collisions`, `count-entities`, `sum-entities`, `matches`, `entity-col` | IR | SoA-native domain queries |
| `manip` / `manipulate` | IR / DROP alias | keep one name |
| `remat`, `cull` | IR | boundary effects (bare hostile `cull` default is a separate TODO) |
| `set-col` | IR | field write boundary |
| `set-style` | DROP after migration, FLAG(3) | style axes are ordinary sym fields since the render migration; `set-col` should cover it. Still used by `cards/tutorials/t02.maku` (and its tutorial doc) — migrate those first |

## Pure builtins

| group | names | verdict | note |
|---|---|---|---|
| arithmetic/compare | `+ - * / mod pow quot = < > <= >= min max abs not` | INTRINSIC | deterministic-math contract |
| trig | `sin cos` | INTRINSIC | ditto |
| sugar | `inc dec` | LIB | trivial over `+` |
| rate boundary | `ticks` | IR-ish INTRINSIC | reads the tick rate; keep with the engine |
| waves/easings | `sine lssht lerp lerp3 lerpsmooth einsine eoutsine eiosine` | LIB (pending profiling) | all expressible over base math; hot inside per-tick dyns, so they move only when the compiled path or profiling says it's free |
| arrays | `iota range nth count first rest drop take concat` | INTRINSIC | seq-view core |
| arrays (derived) | `without stutter` | LIB (pending profiling) | expressible over the core verbs |
| macro support | `forms get form-type form-name nothing? num?` | INTRINSIC | macro-layer reflection |
| geometry constructors | `cart polar pose rot still` | INTRINSIC | data constructors; `polar` = `cart` + trig but stays for symmetry with the lowered node |
| geometry reads | `angle-of mag` | LIB-AFTER(pose field access in lib) | trivial math once `:x`/`:y` reads on poses are ordinary |
| `linear` | FLAG(4) | pos = v·t with STATIC v (recorded in TODO): not an integrator; candidate lib sugar over a `scan`-based integrator, lowering to `DynNode::Linear` |

## Reserved keywords / heads inside forms

`:else` (cond), `:world` (in-frame target), `as` (match binder),
`stage` / `until` / `forever` (stages segment heads), `&` (variadic params),
`$name` (channels), `m"..."` (math-expression strings), `c[..]`/`p[..]`
(pose literals). All IR-level syntax; no action needed beyond documenting.

## Flags — design decisions to make

1. **`map`/`filter` dispatch**: nothing special-form about them; moving them
   to intrinsics over seq views simplifies the specials table and makes the
   audit's "specials are the IR" claim true. Check nothing depends on their
   unevaluated-form position.
2. **The dyn-motion kernel** (`scan`, `vel`, `pather`, `path`, `slew`,
   `smooth`): decide the minimal primitive set. `scan` (stateful integrate/
   fold over a signal) looks like the generic core; `vel` is plausibly
   integrate-velocity, `slew`/`smooth` are stateful filters (scan instances),
   `pather`/`path` compose curve-following. If they reduce to `scan` +
   figure/dyn kernel, the rest become lib with compiler-recognized lowerings
   — same pattern as `linear`.
3. **`set-style`**: confirm nothing calls it (cards/corpus), then delete.
4. **`linear` / integrator**: recorded in TODO under stratification.

## Suggested first moves (safe now, no design blockers)

- `for`/`dotimes` → prelude macros; delete `sf_dotimes`.
- `inc`, `dec` → prelude.
- `circle`/`fan`/`arrow` → prelude over `iota`+`rot`/`cart` (verify
  element-seed and style-axis broadcast shapes survive: formations must
  still produce the same §5 shape paths for F15 axis rules — the nested
  array structure from `map` should reproduce it; test with the axis tests).
- Drop the `manipulate` alias; merge `pos`/`entity-pos`.
- Delete `set-style` after a corpus grep.

Everything else waits on: profiling harness (easings, derived array verbs),
the schema/manifest pass (nothing here), or flags 1/2.
