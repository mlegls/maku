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
elaboration assigns semantic meaning and applies expected-type coercions. The
representation pass classifies equivalent semantic programs by execution
strategy.

## Semantic Types

Core scalar atoms:

- `Num`: one numeric type. Predicates, counts, indices, and masks are numeric
  schemas, not separate runtime scalars.
- `Kw`: interned keyword/event/style atoms. String syntax is source or host
  boundary syntax; runtime comparison uses interned atoms.
- `Handle`: generation-checked entity handle.
- `Nothing`: explicit empty value.

Geometry:

- `Pose`: `(x, y, theta?)`, where `theta = none` means unspecified facing.
- `Curve`: abstract 2D curve. Sampling is a projection choice.
- `Figure`: `Pose | Curve | ...`.

Structured values:

- `List<T>`: unstructured list data for macros, schemas, and ordinary sequence
  manipulation.
- `Array<T>`: homogeneous runtime sequence.
- `Vec<N, T>`: fixed-size homogeneous vector.
- `Mat<R, C, T>`: fixed-size homogeneous matrix.
- `Record{field: T, ...}`: finite known fields.

Engine boundary types:

- `ColliderData`: typed collision boundary rows, including `none`.
- `RenderData`: typed render boundary rows or host-facing render metadata.
- `Meta`: finite record of entity fields. Field storage is selected at
  load/schema time, not allocated per tick.
- `EntitySet`: ephemeral row-index view.
- `Action`: inert control-layer effect description.
- `Fn<A, B>`: pure function.

Time-varying values:

- `Dyn<T>`: value of type `T` over entity-local time or another bound axis.
  This is a semantic type. Closed/integrated/scanned is not normally a distinct
  surface type.

The target low-level entity model is:

```text
Entity = Dyn<Figure>
       * Dyn<List<ColliderData>>
       * Dyn<List<RenderData>>
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
Record{a: Dyn<T>, ...}    => Dyn<Record{a: T, ...}>
List<Dyn<T> | T>          => Dyn<List<T>>
Array<Dyn<T> | T>         => Dyn<Array<T>>
```

These are typed rewrites, not Rust conversion traits. They should leave an
explicit elaborated IR node so diagnostics and compiler lowering can see what
happened.

The `spawn` slots provide the clearest example:

```text
(spawn figure colliders renderers meta)

figure    expects Dyn<Figure>
colliders expects Dyn<List<ColliderData>>
renderers expects Dyn<List<RenderData>>
meta      expects Dyn<Meta>
```

A literal or computed list in the collider slot is first dyn-lifted as
ordinary structure. Then that typed structure is checked against the
`ColliderData` schema. The same list elsewhere remains ordinary list data.

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

## Representation Classification

After type elaboration, a separate pass chooses execution/storage classes.

For `Dyn<T>`:

- `Const`: same value for all `t`.
- `Closed`: pure function of bound axes, evaluable at arbitrary time.
- `Integrated` / `Scanned`: tick-advanced stateful signal with fixed state
  slots.
- `PiecewiseClosed`: static segment table of closed pieces.

For structures:

- homogeneous list literals may classify as `Array`, `Vec`, or `Mat`;
- unstructured lists remain list data;
- records lower to fixed field layouts;
- entity meta fields lower to typed matrices in `WorldFields`.

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
