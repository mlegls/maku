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

- `ColliderData`: typed collision boundary rows, including `none`.
- `RenderData<K>`: typed render boundary rows for a registered render kind
  `K`. Unlike `ColliderData`, this is not a single closed schema: each kind
  selects its own typed record schema from the load-time render-kind registry.
- `Meta`: finite record of entity fields. Field storage is selected at
  load/schema time, not allocated per tick.
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
       * Dyn<List<ColliderData>>
       * Dyn<List<RenderData<K>>>
       * Dyn<Meta>
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
(spawn figure colliders renderers meta)

figure    expects Dyn<Figure>
colliders expects Dyn<List<ColliderData>>
renderers expects Dyn<List<RenderData<K>>>
meta      expects Dyn<Meta>
```

A literal or computed list in the collider slot is first dyn-lifted as
ordinary structure. Then that typed structure is checked against the
`ColliderData` schema. The same list elsewhere remains ordinary list data.

The `ExpectedType::Spawn*` names in the prototype are transitional spelling for
these compositional targets. The convergence target is ordinary expected types:
`Dyn<Figure>`, `Dyn<List<ColliderData>>`, `Dyn<List<RenderData<K>>>`, and
`Dyn<Meta>`.

## Schema Checking

Collider and render construction are schema checks over typed structure, not
parser forms and not runtime maps.

Example:

```edn
(colliders {:layer :enemy-hit :shape [:circle {:r m"0.1 + 0.02*t"}]})
```

At the semantic boundary:

```text
Record/List source data
  -> Dyn<List<ColliderData>>
```

The current interpreter still realizes some dynamic specs per tick and checks
them at the simulation boundary. The target is to perform all static schema
work during elaboration, with only genuinely dynamic numeric fields evaluated
per tick.

Two cases must classify differently:

- static list shape with dynamic fields, e.g. one circle whose radius is
  `m"0.1 + t"`: fixed collider count, vectorizable per field;
- dynamic list shape, e.g. a function that returns zero, one, or many
  colliders over time: collider count itself changes, so the backend needs
  per-tick list realization or a lowered equivalent.

Both can have type `Dyn<List<ColliderData>>`; the representation classifier
must preserve the distinction.

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

Core owns the render slot boundary, registry/manifest mechanics, typed row
transport, and deterministic extraction order. It does not own sprite family
semantics, texture/material binding, palette interpretation, or the meaning of
library-defined render fields. Stock kinds such as `:sprite`/`:dot` and
`:polyline` may ship with the engine for the prototype and debug hosts, but
they are registered profiles rather than universal language semantics.

Dynamic renderer fields are ordinary dyn-valued record fields. For example,
the current compatibility tags `:hue`, `:scale`, `:facing`, and `:opacity`
should become fields of a stock `:sprite` render kind, sequenced by the normal
`Record{field: Dyn<T>} => Dyn<Record{field: T}>` coercion instead of sampled by
special render-side tags.

## Representation Classification

After type elaboration, a separate pass chooses execution/storage classes.
It may use the semantic signal class, but it does not decide whether a dyn is
closed, integrated, or scanned.

For structures:

- homogeneous list literals may classify as `Array`, `Vec`, or `Mat`;
- unstructured lists remain list data;
- records lower to fixed field layouts;
- entity meta fields lower to typed matrices in `WorldFields`.
- render rows lower to per-kind column buffers, optionally bucketed by an
  interned batch key. Range kinds such as polylines/meshes use shared
  vertex/index pools plus row ranges.

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
