# Maku 0.1 language reference

> **Version:** Maku 0.1. This is a user-facing reference for implemented
> behavior. [`openspec/specs/language/spec.md`](../openspec/specs/language/spec.md)
> is the semantic authority; active backlog changes are not current syntax.

Maku separates **dynamics**, pure or stateful values sampled over time, from
**actions**, inert descriptions executed by the scheduler. Genre gameplay and
semantic render rules—bullets, players, damage, and emitted sprite/beam rows—are
library code rather than engine primitives; the host-selected profile realizes
those rows as concrete resources and geometry.

Examples marked **whole card** can be saved and loaded directly. Other examples
are fragments intended for a pattern, rule, or callback. Release checks run the
whole-card reference fixtures in
[`cards/docs/language-reference.maku`](../cards/docs/language-reference.maku)
and every tutorial companion under [`cards/tutorials/`](../cards/tutorials/).

## Source and values

Maku uses EDN-like forms:

| Form | Meaning |
|---|---|
| `(f a b)` | call or special form |
| `[a b]` | array |
| `{:key value}` | map |
| `:red` | interned symbol |
| `c[x y]`, `p[r theta]` | Cartesian or polar pose |
| `$player` | stream read |
| `m"2 + 3*t"` | infix reader shorthand |
| `; text` | line comment |

There is one language-level number type. Time is in seconds and angles are in
degrees. Predicate APIs use numeric masks: zero is false and nonzero is true.
Symbols are interned values. Arrays and maps are immutable source/control data;
retained entity fields are flat numbers, symbols, handles, or supported numeric
dynamics.

`nothing` means absence. Use `(default value fallback)` to replace only
`nothing`; unlike boolean fallback, a legitimate numeric zero is retained.

```clojure
(default e.hp 10)
(nth [:red :green :blue] 4) ; cyclic: :green
```

Arithmetic broadcasts over arrays and cyclically zips shorter arrays. `iota`,
`range`, `map`, `filter`, `count`, `first`, `rest`, `drop`, `take`, and `concat`
provide sequence operations.

## Definitions, functions, patterns, and imports

```clojure
(def palette [:red :orange :yellow])
(defn aimed [target speed] ((aim target) (linear p[speed 0])))

(defpattern main [speed 2]
  (bullet (circle 12 (linear p[speed 0]))
          {:style {:family :gem :color palette :variant :w}}))

(import "touhou")
```

`def` names a value or dynamic expression. `defn` defines a lexical function.
`fn` creates an anonymous function. `defpattern` defines an action entry point;
its parameter vector contains name/default pairs. If a host does not select a
pattern, the first pattern is used.

The prelude is imported once automatically. `(import "touhou")` loads the
embedded Touhou library. Imports containing `/` or ending in `.maku` resolve
relative to the importing card through the host filesystem or VFS. Imports are
include-once, including diamond import graphs.

`let` bindings evaluate left to right. `defmacro`, quote, quasiquote, unquote,
and `forms` support source macros; macros are an advanced, currently
unhygienic facility.

## Poses, dynamics, and frames

A pose carries position and optional orientation:

```clojure
c[2 3]
p[4 90]
(rot 30)
(still)
```

Free `t` and `u` denote dynamic axes. `t` is local motion time; `u` is the
curve/materialization axis. A function parameter named `t` is an ordinary
number.

```clojure
(linear p[3 -90])
(fn [t] (cart (* 2 t) (sine 1 0.3 t)))
(vel c[2 0])
```

`linear` and time functions are closed dynamics. `vel`, `evolve`, and some
`stages` forms carry deterministic tick state. `(evolve initial step)` receives
a context with `:t`, `:dt`, and `:tick`. Stateful values advance once per tick;
they are not arbitrary wall-clock callbacks.

Frames compose by nesting:

```clojure
;; Offset rotates with the outer frame.
((rot 90) ((pose c[2 0]) child))

;; Offset remains in the outer/world orientation.
((pose c[2 0]) ((rot 90) child))
```

`in-frame` is the explicit spelling. `(in-frame :world action)` resets inherited
action framing. Arrays of frames create formations; spawn multiplicity is the
product of array sizes along the frame path. `circle`, `fan`, and `arrow` are
prelude functions over frame arrays, not engine primitives.

Curves use `u`:

```clojure
(curve (polar (* 1.5 u) (sine 1.4 60 u)) {:u-max 2.5})
(pather 1.5 (cart (* 2 t) (sin (* 90 t))))
```

`path`, `on-curve`, and `curve-samples` connect curve motion, queries, and
polyline rendering. `fields` adds flat leaf fields to a figure.

## Actions and scheduling

Actions execute only when reached by the action tree:

```clojure
(seq (wait 0.5) action)
(par left right)
(fork background-action)
(wait-for predicate)
```

`seq` orders actions; `par` starts branches together; `fork` creates an adopted
child task. `wait` uses simulation time. `until`, `finally`, and `race` provide
structured cancellation. Descendant tasks inherit frames and scoped streams.

`loop`/`recur` is the core recurrence form. The prelude supplies `for` and
`dotimes`:

```clojure
(for [i 8 :every 0.1]
  (bullet ((rot (* 12 i)) (linear p[2 0])) {}))
```

`states` defines a scoped state machine and `goto` selects its next state.
State exit cancels that state's task subtree. Nonblocking infinite control
work fails its deterministic fuel budget rather than hanging the host.

## Entities, fields, and manipulation

Low-level spawn syntax is:

```clojure
(spawn
  (linear p[2 0])
  (circle-collider {:layer :damage :r 0.12})
  {:team :enemy :hp 3})
```

`spawn` is an action and yields generation-checked handles when executed. An
array figure creates one entity per leaf and returns handles in spawn order.
Stale handles cannot affect a row that has been reused.

Entity queries expose pose/motion fields such as `e.pos`, `e.vel`, `e.t`, and
`e.handle`, plus retained flat fields. Missing fields produce `nothing`.
`entities-where`, `matches`, `count-entities`, `sum-entities`, and
`nearest-entity` query the current world. Query row sets are ephemeral; retain
handles for identity across ticks.

```clojure
(manip
  (fn [e] (* (= e.team :enemy) (<= (default e.hp 1) 0)))
  (fn [e] (cull e)))
```

`change-col` queues a functional field update. `set-col` is constant-assignment
sugar. `remat` replaces motion and may update fields while preserving the
handle. Writes apply at the next tick boundary; all current-tick reads see the
old state, and multiple writes compose in action order.

`deftick` installs a standing rule. Gameplay behavior—including hp, death,
graze, shots, and player policy—is implemented by such library rules.

## Collisions

`circle-collider` creates a point collider and `capsule-chain-collider` creates
a curve collider. `defcollider` defines a projector evaluated from entity and
context data. Layer names are symbolic routing data, not built-in teams.

```clojure
(deftick
  (map
    (fn [[shot target]]
      (seq
        (cull shot)
        (change-col target :hp
          (fn [hp] (- (default hp 1) (default shot.damage 1))))))
    (collisions :shot :hurt)))
```

Collision pairs are ordered handle pairs. Multiple collider pairs can create
multiple contacts; card/library policy owns any once-only latch.

## Streams, host inputs, and events

Every `$name` denotes a stream. A bare read snapshots the current value at a
control or spawn boundary. `(live $name)` keeps a dynamic expression connected
to the stream.

```clojure
(def $rank 1)
(def $wind)
(bind! $wind (from-host :wind 0))
(set! $rank (+ $rank 0.1))
(export! $rank)
```

`bind!` attaches a producer refreshed each tick. `export!` publishes a stream
to the host. `defchannel` combines declaration, producer, and export.
`from-host` contributes a required name to the card's load-time host manifest;
the host verifies it before tick zero when strict capabilities are configured.

Sigiled function/pattern parameters receive stream handles rather than snapped
values. `(with {$rank 0.5} body...)` creates a dynamically scoped stream cell;
it does not modify the base host-input tape.

Outbound events use:

```clojure
(event :exploded c[2 3])
;; primitive form: (emit :events {:name :exploded :pos c[2 3]})
```

Events are frame-stamped and replay with the session.

## Rendering

Rendering is ordered rule output, not a special spawn field. `(render row)` is
prelude sugar for `(emit :render row)`.

```clojure
(defrender-kind :orb
  {:geometry :point
   :fields {:family :sym :color :sym :size :num}})

(deftick
  (map
    (fn [e]
      (render {:kind :orb :shape :point
               :x e.pos.x :y e.pos.y :theta e.pos.th
               :scale (default e.size 1) :alpha 1 :hue 0
               :family e.family :color e.color
               :size (default e.size 1)}))
    (entities-where (fn [e] (= e.render :orb)))))
```

Point structural fields are `:x`, `:y`, `:theta`, `:scale`, `:alpha`, and
`:hue`. Polyline rows use `:points` or deferred `curve-samples`, plus numeric
`:active`. Direct compatibility aliases `:facing`, `:opacity`, and `:pts` are
not render-row fields; entity metadata with similar names may still be
translated by a library.

`defrender-kind` fixes geometry and extra field types. Hosts can reject
unsupported declared kinds before tick zero. Rows and compiled batches share
one authoritative stream order; batch lanes expand exactly at the batch's
position. Frontends must not globally regroup transparent output.

The Touhou library emits `:sprite` and `:beam` semantics. The optional
`maku-render-touhou` profile—not core or the card—maps family/variant/color to
textures, materials, palettes, orientation, radii, and layers. See
[`renderer-api.md`](renderer-api.md).

## Touhou library

**Whole card:**

```clojure
(import "touhou")

(defpattern main []
  (for [i 8 :every 0.15]
    (bullet
      ((rot (* 15 i)) (circle 12 (linear p[2 0])))
      {:style {:family :gem
               :color [:red :orange :yellow]
               :variant :w}})))
```

`"touhou"` provides ordinary card vocabulary including `bullet`, `shot`,
`enemy`, `player`, `boss`, `laser`, `laser-shot`, `player-rig`, and `phases`.
Template metadata merges with caller metadata, so explicit fields override
library defaults. Family-based hitbox defaults are library data.

## Errors

Failures are reported at the narrowest known boundary:

- reader/import errors include source context;
- the stream schema pass rejects undeclared `$name` references;
- the load-time checker rejects proven type, arity, collider, and declared
  render-schema mismatches;
- runtime checks cover dynamic projector results, schema accretion conflicts,
  remat errors, export collisions, fuel, and entity capacity.

The checker is conservative: inability to prove a form invalid does not by
itself reject it. There is no card-level exception mechanism. Operations on a
dead handle are safe no-ops; entity-capacity overflow is a deterministic error.

<!-- compatibility-migration -->
Removed pre-release forms report canonical replacements: use `default` instead
of `value-or`, and `bullet`/`shot`/`enemy`/`boss`/`player` instead of old `spawn-*` aliases.

## Determinism and replay

A run is a deterministic fold over card source, seed, ordered input samples,
ordered command/edit tape, and tick number. Observable ordering includes action
execution, queued writes, entity queries, collisions, tick rules, render rows,
and reductions.

Random draws consume a deterministic sequential stream. Replaying unchanged
code with the same seed and inputs is exact; reordering random sites or spawns
can intentionally change later draws. Stateful dynamics, tasks, streams,
events, pending writes, and entity generations are snapshot state. Render
output is a tick-cadence snapshot; display interpolation is host policy.

See [`host-api.md`](host-api.md) for tape/scrub behavior and the normative
[`determinism`](../openspec/specs/determinism/spec.md) and
[`session`](../openspec/specs/session/spec.md) capabilities for contracts.
