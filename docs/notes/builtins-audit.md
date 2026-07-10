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

## Governing principle (decided): minimal kernel, structural lowering

No sugar in lang. The language surface is minimized to a semantic kernel;
everything else is lib macros over it. Optimization recognizes the MACRO
EXPANSION SHAPE (AST patterns after expansion), never the macro name — so
hand-writing the same shape optimizes identically. Contract boundary: the
semantic guarantee is that no name is magical; the performance guarantee
applies to code that normalizes to a recognized shape (lib expansions do by
construction; the recognizer must be at least alpha/let-normalization
robust, and arbitrary-equivalence recognition is explicitly NOT promised).
Builtins get added back as AST-rewrite intrinsics from profiled bottlenecks
first (array/entity-domain paths are the expected start).

Kernel-shrink direction from this principle (beyond the tables below):
- `defcollider` → lib macro (already `def` + `collider`).
- `map`/`filter` → `match` + recursion; the recognizer compiles seq vs
  vec/mat/entity-domain shapes differently. Domain typing
  (`EntitySet`/`CollisionSet`) must survive expansion so rule bodies keep
  the SoA loop.
- Everything expressible over `sample` (pure evaluation) leaves the kernel.
- Everything expressible over `scan` leaves the kernel — NOTE: the `scan`
  head is currently a RESERVED STUB ("not implemented in this milestone");
  the existing stateful builtins route through `scan_builtin_spec`. Building
  real `scan` is the kernel-design centerpiece, not a refactor.
- `rand-int`, `randpm1` (missed by the first audit pass) → lib over `rand`
  with identical draw counts; requires a `floor` intrinsic (math also lacks
  `sqrt` outside `mag` — the minimal math set needs completing).
- `wait-for` → lib macro for the poll loop
  `(loop [] (when (not pred) (wait tick) (recur)))`; compiled scheduling
  recognizes the shape as a wake condition. Not primitive.
- Array ops → `match` + recursion; dense lowering by shape.
- Form reflection (`forms`/`get`/`form-type`/`form-name`/...) → collapse
  into `match` over form structure plus quote; keep one or two.
- `remat` via `manip`/`spawn`/`cull`: BLOCKED on an identity-semantics
  decision — if remat preserves the handle/generation, cull+spawn cannot
  express it without an explicit identity-transfer primitive.
- `set-col` via `remat` ("rematerialize differing only in field f", with
  the recognizer reducing that shape to a store): coherent, but MUST land
  after the pattern-lowering exists — hp/graze writes are the hottest rule
  effects and would crater interpreted.
- Renames/merges: `defvar` → `defcell` (it creates the F16 pattern-scoped
  cell); `channel` is a read-with-default of `$name` — merge into `$name`
  syntax + an ordinary default combinator; `stages-action` is a reserved
  stub (action-level `stages` analogue) — decide or delete the reservation.
- `remat` (decided): handle-preserving remat IS the primitive,
  `(remat handle spec)` — single handle, single-ELEMENT spec (multi-element
  figures are an error in remat position; identity is 1:1). Figure KIND may
  change (handles are generation-safe indirection; a kind change is a slot
  move, not a semantic constraint). Batch remat is lib `map` over
  handle/spec pairs, and the recognizer lowers that shape to the masked-SoA
  pass. Contract (all decided, see TODO.md): PARTIAL spec (absent slots
  retain); PER-SLOT epochs (rematted slot restarts `t` and clears scan
  state; untouched slots keep both — field-only remats never disturb
  motion); writes land at/right before the NEXT tick (within-tick reads see
  pre-tick state, ephemeral indices stay stable; deterministic action
  order); new figures anchor at the entity's CURRENT world pose (store/
  pass the old parent frame explicitly if wanted). Field writes (decided):
  the primitive is functional — `(change-col e col f)` queues f, and a
  slot's queued updates COMPOSE in deterministic action order at the tick
  boundary. `set-col` becomes lib sugar for the constant function;
  aggregate-over-domain remains the preferred idiom (and the fusion
  target); update functions must be pure. Remat's partial spec admits
  values or update functions per field, so `change-col` is the
  single-field case of remat.
- `emit` unification (direction): `event` and `render` are the same
  operation — push an open schema-checked row onto a named host-facing
  stream. Kernel primitive `(emit :stream row)`; `render`, `event`, and
  probably `export` become lib macros over it. The one-pass schema decision
  covers all streams (event rows gain a schema for free); geometry
  expansion (`curve-samples` values) is a field-kind concern of whichever
  stream declares a geometry column, not an `emit` concern. Hot render
  emission relies on pattern-lowering like everything else.

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
| `seq`, `par`, `fork`, `wait`, `until`, `finally`, `race`, `goto`, `event` | IR | scheduler node constructors / observable effects |
| `wait-for` | LIB (pattern-lowered) | = poll loop over `loop`/`recur` + `wait`; compiled scheduling recognizes the shape as a wake condition |
| `defvar` | IR, RENAME `defcell` | creates the F16 pattern-scoped cell |
| `channel` | MERGE | read-with-default of `$name`; fold into `$name` + default combinator |
| `stages-action` | STUB | reserved, errors today; decide or delete |
| `states`, `stages` | IR | FSM/staged-dyn cores; richer templates stay lib (TODO) |
| `inline` | IR | pattern-embedding scope adapter |
| `bind-channel!`, `live`, `in-frame` | IR | channels/frame ambience |
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
| `pather`, `path`, `vel`, `slew`, `smooth`, `scan`, `clamp` | FLAG(2) | the dyn-motion kernel; `scan` is the intended integrator primitive but is currently a RESERVED STUB (stateful builtins route through `scan_builtin_spec`) — designing/building real `scan` is the centerpiece. NB `clamp` is spatial — `(clamp c[lo] c[hi] dyn)` boxes a dyn's position, not numeric min/max |
| `rand` | IR | deterministic RNG stream is engine state |
| `rand-int`, `randpm1` | LIB (needs `floor` intrinsic) | one draw each over `rand`; identical stream consumption |
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

## First moves — DONE (kernel shrink wave 1)

Landed: `defvar`→`defcell`; `manipulate` alias and `entity-pos` gone (`pos`
takes handle or view); `stages-action` stub deleted; `set-style` replaced
by `set-col` (now writes sym fields too); `for`/`dotimes`, `inc`/`dec`,
`circle`/`fan`/`arrow`, `rand-int`/`randpm1` (identical draw counts), and
`value-or` are prelude macros/defns; `floor`/`ceil`/`round`/`sqrt` added to
the math intrinsics. The `emit` unification (render/event → prelude macros
over `(emit :stream row)`) is a separate follow-up change.

Everything else waits on: profiling harness (easings, derived array verbs),
the schema/manifest pass (`channel` merge included — the
`(defchannel $x (channel $x default))` host-read idiom is manifest
territory), top-level macro expansion in the card loader (`defcollider` →
lib), the `scan` design (flag 2), or the remat implementation
(`change-col`).
