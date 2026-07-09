# Maku Type System Notes

Status: target design. The interpreter only implements parts of this today.

The type system should separate meaning from execution. A source expression
first becomes a typed semantic expression; only after that should the runtime
choose whether to execute it as interpreter closures, dense SoA loops, cached
projectors, compiled code, or another backend-specific representation.

## Pipeline

```text
Form
  -> macro-expanded Form
  -> untyped AST
  -> typed / elaborated IR
  -> representation-classified IR
  -> backend lowering
```

The parser only knows source syntax. Macro expansion rewrites syntax. Type
elaboration assigns semantic meaning, applies expected-type coercions, and
infers semantic signal classes such as `Closed` vs `Scanned`. The later
representation pass classifies equivalent semantic programs by execution
strategy.

## Semantic Types

Core scalar atoms:

- `Num`: one numeric type. Predicates, counts, indices, and masks are numeric
  schemas, not separate runtime scalars.
- `Kw`: interned keyword/event/style atoms. String syntax is source or host
  boundary syntax; runtime comparison uses interned atoms.
- `Handle`: generation-checked entity handle.
- `Nothing`: explicit empty value. It is not implicit nullability; a slot that
  may be absent must say so with `Option<T>` or with a domain variant such as
  `ColliderData::none`.

Geometry:

- `Pose`: `(x, y, theta?)`, where `theta = none` means unspecified facing.
  This is `theta: Option<Num>`, not implicit `Nothing` inhabiting numeric
  slots.
- `Curve`: abstract 2D curve. Sampling is a projection choice.
- `Figure`: `Pose | Curve | ...`.

Structured values:

- `List<T>`: unstructured list data for macros, schemas, and ordinary sequence
  manipulation.
- `Array<T>`: homogeneous runtime sequence.
- `Vec<N, T>`: fixed-size homogeneous vector.
- `Mat<R, C, T>`: fixed-size homogeneous matrix.
- `Record{field: T, ...}`: finite known fields.
- `Option<T>`: explicit absence/presence. Optional record fields elaborate to
  this type or to a record-row schema with an explicit presence bit.

Engine boundary types:

- `Collider` / `ColliderData`: typed literal collision boundary rows,
  including `none`. These are emitted by extraction from collider projectors;
  they are not the opaque projector values themselves.
- `RenderData<K>`: the typed render boundary row for a registered render kind
  `K`. Unlike `ColliderData`, this is not a single closed schema: each kind
  selects its own typed record schema from the load-time render-kind registry.
  Source code may construct render rows directly when a renderer slot expects
  them. Each entity emits exactly one render row; nullable/no-render behavior is
  encoded by fields chosen by that schema, not by a language-reserved kind.
- `Meta`: finite record of entity fields. Field storage is selected at
  load/schema time, not allocated per tick.
- `EntityView<F>`: the same entity view shape used by query/manip callbacks,
  specialized by the entity's core figure variant `F`: handle identity plus
  entity-scoped meta and the figure-specific fields/getters for `F`.
- `MetaEnv`: lexical/projector view of an entity's meta. By default it is the
  entity's shared meta namespace, but higher-order adapters may rebind names or
  select a subrecord for an imported projector.
- `ProjectorContext`: non-entity execution context for projectors, including
  world tick, entity-local age/`t`, and extraction-pass parameters.
- `ColliderProjector<F>`: opaque source value produced only by registered
  primitive projector constructors and projector combinators. Extraction lowers
  it with `(EntityView<F>, ProjectorContext)` to `List<Collider>` each tick.
- `RenderProjector<F, K>`: typed pure function or projector expression lowered
  by extraction with `(EntityView<F>, ProjectorContext)` to one `RenderData<K>`
  each tick. Unlike `ColliderProjector<F>`, render rows are open schema data, so
  this does not have to be an opaque primitive-only value.
- `EntitySet`: ephemeral row-index view.
- `Action`: inert control-layer effect description.
- `Fn<A, B>`: pure function.

Time-varying values:

- `Dyn<T>`: value of type `T` over a slot-bound axis. The axis is not a free
  type parameter: `t`, `u`, tunnel `s`, and ancestor clocks are bound by the
  expecting slot, and named signal expressions rebind at the referencing slot
  as described in `language.md` section 3. A `Dyn<T>` value cannot be floated
  out with an unbound hidden axis.

Signal class is an inferred semantic property of `Dyn<T>`:

- `Const`: independent of the slot axis.
- `Closed`: pure function of the slot-bound axes, evaluable at arbitrary axis
  values.
- `PiecewiseClosed`: static segment table of closed pieces.
- `Integrated`: stateful integration with fixed state slots.
- `Scanned`: general tick-advanced stateful signal with fixed state slots.

Closedness is semantic because it determines whether arbitrary-axis evaluation
is meaningful. Storage choices such as SoA layout, specialized linear motion,
or interpreter enum cases are representation details layered after this.

The target low-level entity model is:

```text
Entity = Dyn<Figure>
       * Dyn<Meta>
       * List<ColliderProjector<F>>
       * RenderProjector<F, K>
```

Storage may be SoA, AoS, compiled buffers, or interpreter objects. That choice
must not leak into the semantic type of `spawn`.

## Expected-Type Elaboration

Inference should be HM-like for pure expressions, but Maku also needs
expected-type elaboration. Some meanings are only valid because a slot says
what type is expected.

Important coercions:

```text
T                         => Dyn<T>
Pose                      => Figure
List<T>                   => Array<T> / Vec<N, T> where context requires it
```

These are typed rewrites, not Rust conversion traits. They should leave an
explicit elaborated IR node so diagnostics and compiler lowering can see what
happened.

Coercion must be coherent: every legal derivation from the same source
expression to the same expected type must denote the same value. The compiler
therefore uses one canonical elaboration order:

1. Apply non-dyn structural coercions required by the expected type, such as
   `Pose => Figure`, `List<T> => Array<T>`, `List<T> => Vec<N, T>`, and
   homogeneous vector/matrix recognition.
2. Recursively elaborate each field or element under its expected element type.
3. If `Dyn<S>` is expected, lift every non-dyn child to `Const`, then sequence
   the structure into one `Dyn<S>`.
4. Apply schema checks such as collider/render/meta conversion at the typed
   slot boundary.

So a `List<Num>` checked against `Dyn<Array<Num>>` is canonicalized as:

```text
list literal
  -> Array<Num>
  -> Dyn<Array<Num>> by lifting all elements to Const and sequencing once
```

Mixed dynamic structures are the same rule, not a union rule:

```text
[a b(t) c]
  -> [Const(a) b(t) Const(c)]
  -> sequence -> Dyn<List<T>>
```

Records follow the same rule: elaborate every field under its field type, lift
non-dyn fields to `Const` when a `Dyn<Record{...}>` is expected, then sequence
the whole record. The elaborated IR is deterministic even when a shorter proof
path would have existed.

The `spawn` slots provide the clearest example:

```text
(spawn figure meta colliders renderer)

figure    expects Dyn<Figure>
meta      expects Dyn<Meta>
colliders expects ColliderProjector<F> or List<ColliderProjector<F>>
renderer  expects RenderProjector<F, K>
```

The figure and meta slots are the dynamic slots. Meta is where non-positional
dynamic data lives; the meta slot binds the current figure as a reserved name
alongside `t`, so fields can depend on per-tick geometry without needing a
separate `Figure -> Dyn<T>` surface type. Collider and renderer slots choose
projector functions. A projector may read any typed meta field, the current
figure, and `ProjectorContext` each tick. Primitive projector override fields
expect concrete values of their declared field types in that already-bound
`e`/`ctx` environment: `Num` for `:radius`/`:r`, `Kw` for `:layer`, etc.
`(* e.hitbox 2)` is therefore a `Num`, not a hidden entity callback. There is
no keyword-as-field-access shortcut in override maps; use `e.hitbox`, not
`:hitbox`, when reading an entity field. Time-dependent projector arguments
normally use explicit context fields such as `ctx.t` or `ctx.age`. A free-`t`
expression can still be defined inside projector code, but it remains a
dyn-valued expression and must be explicitly applied/sampled before it can feed
one of those concrete fields. The `m"..."` reader macro remains available
inside projector code because it is only syntax. Direct dynamic collider/render
row lists are not the public low-level surface. Purely local temporal behavior
such as "this collider until age 0.5" can be expressed as a higher-order
projector combinator rather than as a public meta switch.

The `ExpectedType::Spawn*` names in the prototype are transitional spelling for
these compositional targets. The convergence target is ordinary expected types:
`Dyn<Figure>`, `Dyn<Meta>`, `List<ColliderProjector<F>>`, and
`RenderProjector<F, K>`.

## Schema Checking

Collider projectors are opaque source values, not parser forms and not runtime
maps. They are produced by normal typed function calls such as
`circle-collider`, but their result type cannot be constructed by user code
except through registered primitive constructors and combinators. Raw
`Collider` rows are boundary rows produced by extraction; they are not the
normal authoring surface.

Render rows are different: render kinds are open, host/library-registered
schemas, and card code may construct schema-checked map-like `RenderData<K>`
directly when a renderer slot expects it. A `defrenderer` can therefore be a
normal function over `e`/`ctx` that returns one `RenderData<K>` row.
Stock renderer helpers are conveniences for common projections from figure/meta
to those rows, not the only way to get render data.

Example:

```edn
(spawn fig
  {:radius m"0.1 + 0.02*t" :layer :enemy-hit}
  bullet-collider
  (touhou-renderer))
```

At the semantic boundary:

```text
meta source data
  -> Dyn<Meta>

bullet-collider
  -> ColliderProjector<Pose>
  -> extraction: source projector specs over (EntityView<Pose>, ProjectorContext)
  -> each tick: List<Collider>
```

The current interpreter still realizes dynamic bridge values per tick and
checks them at the simulation boundary. The target is to elaborate projector
functions directly, with dynamic data evaluated through figure, meta, and
context fields per tick; literal colliders are emitted by extraction rather
than returned as ordinary `.maku` values.

`Meta` itself is a flat typed record of primitive atoms. Source maps and lists
are still ordinary values for macros, option records, and reserved spawn
directives, but retained entity meta does not store arbitrary structures or
allocate cold per-entity records. Namespace hygiene is handled by field naming
or adapters that map one flat field convention to another.

Two projector output cases must classify differently:

- static projector shape with dynamic meta reads, e.g. one bullet circle whose
  radius is `meta.radius(t)`: fixed collider count, vectorizable per output column;
- dynamic projector output, e.g. a laser projector that returns no hot capsules
  during warn time and a sampled capsule chain during active time: row count
  changes, so the backend needs per-tick range realization or a lowered
  equivalent.

Both are produced by `ColliderProjector<F>`; the representation classifier
must preserve whether row count is fixed, bounded range-like, or truly dynamic.

Projectors compose at the authoring level. For example:

```edn
(defcollider bullet-collider [e ctx]
  (let [r e.hitbox
        graze (* 2 r)]
    (colliders
      (circle-collider {:radius r :layer :enemy-hit})
      (circle-collider {:radius graze :layer :enemy-graze}))))

(defcollider laser-collider [e ctx]
  (capsule-chain-collider {:width e.width :layer e.layer}))
```

`colliders` means concatenation/parallel projection of collider projector
specs. This recovers the expressiveness of directly composing collider
behavior while keeping all changing non-geometry inputs in meta.

`defcollider` is top-level only. Its body is pure code in a scope containing an
explicit entity view parameter such as `e` and an explicit non-entity context
parameter such as `ctx`. Because it cannot close over card-local mutable state,
ordinary `let` is enough for sharing computed values; no special binding syntax
is required. Semantically this is `defn` with an expected return type of
`ColliderProjector<F> | List<ColliderProjector<F>>`; the special form is
surface sugar for that typed definition. The optional figure type annotation on
the definition, e.g. `(defcollider :pose name [e ctx] ...)` or
`(defcollider :parametric name [e ctx] ...)`, selects the shape of `e` and the
extraction loop that will run it. User code can branch, parameterize, and
compose projectors, but it cannot define a new primitive collider projector kind
without registering a builtin.

Collider constructors are typed constructors inside that pure body. Their
argument records must have load-time-known shape so the elaborator can preserve
the projector algebra, and each field value must check against the field's
concrete type. These argument records are ordinary expressions over `e` and
`ctx`, not dyn-expecting slots, so `ctx.t` is the usual local time source. The
available figure accessors depend on `F`: a pose projector may read pose fields,
while a parametric-curve projector may read curve/domain/sampling helpers that
do not exist on pointlike entities.

```edn
(circle-collider {:radius m"2 * e.hitbox + 0.05 * ctx.t"
                  :layer :enemy-graze})
```

This elaborates to a circle-projector node whose `radius` expression is visible
to lowering, not to an opaque map-returning closure.

Projectors intentionally share the entity meta namespace. That lets hit,
graze, render, query, and host-exposed behavior agree on fields such as
`:radius`, `:style`, or `:scale`. To avoid collisions when importing
projectors from another library, authors can wrap the projector with an adapter
that rebinds flat names in the meta environment seen by the projector.
Colliders themselves do not have author-visible fields in this model; operators
compose or adapt opaque projector values.

Meta is the same kind of typed boundary. The current interpreter's
`SpawnMetaInput` still carries raw source forms because `:expose` channel
designators and legacy signal tags are directives, not ordinary `Meta` values.
Target elaboration should separate those directives from the `Dyn<Meta>` value
the entity stores.

Render schemas are the open counterpart to collider schemas. A render row is
typed by a kind discriminator plus that kind's registered record schema:

```text
RenderKind : Kw * RecordSchema
RenderData<K> = record checked against schema(K)
```

The kind is not an ordinary peer field such as `:mode`; it selects the schema
that gives the remaining fields meaning. A card's render-kind manifest is
derivable from renderer specs after macro/import expansion. Hosts and optional
rendering crates declare which kinds they implement; unsupported kinds fail at
load unless a declared degradation path is available.

Each entity has exactly one render row. If a schema needs no-render or nullable
behavior, it must define the field convention that expresses it, such as a
host/profile-owned `:kind`, `:visible`, or `:enabled` field. The language does
not reserve a no-render kind. To expose several visual parts, define a schema
with enough fields for the maximum shape, such as `:aux-sprite-1`/`:aux-sprite-2`
or a nested record, rather than returning multiple rows.

Render schemas merge by field key, not by key/type pairs: if two renderers
contribute the same key with incompatible types, card load fails. Imported
renderers with conflicting field names can be adapted by a builtin projection
operator that renames fields, selects a subset of fields, or both before schema
merge.

Core owns the render slot boundary, registry/manifest mechanics, typed row
transport, and deterministic extraction order. It does not own sprite family
semantics, texture/material binding, palette interpretation, or the meaning of
library-defined render fields. Stock kinds such as `:sprite`/`:dot` and
`:polyline` may ship with the engine for the prototype and debug hosts, but
they are registered profiles rather than universal language semantics.

Dynamic renderer inputs are ordinary dyn-valued meta fields. For example, the
current compatibility tags `:hue`, `:scale`, `:facing`, and `:opacity` should
become fields read by a stock `:sprite` renderer projector, sequenced through
the normal `Record{field: Dyn<T>} => Dyn<Record{field: T}>` meta coercion
instead of sampled by special render-side tags. The renderer projector itself
is not dyn and is not manipulated directly; changing render behavior over time
means changing figure/meta inputs or rematerializing with a different projector.

## Representation Classification

After type elaboration, a separate pass chooses execution/storage classes.
It may use the semantic signal class, but it does not decide whether a dyn is
closed, integrated, or scanned.

For structures:

- homogeneous list literals may classify as `Array`, `Vec`, or `Mat`;
- unstructured lists remain list data;
- records lower to fixed field layouts;
- entity meta fields lower to typed matrices in `WorldFields`.
- render rows lower to per-kind, per-field column buffers, optionally bucketed
  by an interned batch key. Because there is exactly one render row per entity,
  fixed-width fields can be entity-indexed directly. Variable-size payload
  fields such as mesh vertices or polyline points are represented as fields
  containing offsets/ranges into shared pools, not as multiple render rows.

This pass may choose optimized representations such as specialized linear
motion, dense motion-state slots, shared curve sampling caches, or compiled
loops. Those choices should preserve the typed semantic IR.

## Compiler Implications

An AOT compiler and the interpreter should share the typed semantic IR and as
much representation classification as practical. Backend lowering can differ:
the interpreter may lower to closures/enums, while a compiler lowers to vector
loops or codegen.

This suggests the implementation boundary:

```text
source parser / macros
  shared typed elaboration
  shared representation classification where possible
  backend-specific lowering
```

`sem.rs` in the current interpreter is the first narrow version of this
boundary. It should grow toward elaborated semantic slots, not toward a second
runtime model.
