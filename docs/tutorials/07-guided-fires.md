# Tutorial 7: Indices, Formations, and Guided Fires

Runnable companion: **`cards/tutorials/t07.dmk`**.

```sh
cargo run --release --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t07.dmk
```

DMK calls its version of this material "on the harder side" and
"critical to a thorough understanding" of the engine. Here it's shorter,
because both of its subsystems — the firing index and empty-guided
fires — dissolve into things you already know: variables and frames.
The payoff is the same: complex shapes that move *as shapes*.

## Indices are just variables

DMK threads a "firing index" `p` through its repeaters — a single number
you pack loop indices into (`p this`, `p add`, `p mod`) and unpack with
`p1`/`p2`, with overflow rules and "awkward and easy to screw up"
retrieval past two layers. The problem it solves is real: bullets need
to know *which one they are*. The mechanism doesn't survive translation,
because here multiplicity is expressed with ordinary binders — the index
never leaves scope, so there's nothing to pack (`ex1-index`):

```clojure
(spawn-bullet ((aim $player)
         (map (fn [k] ((rot (- (* 6 k) 27))
                        (linear p[(+ 2 (* 0.2 k)) 0])))
              (iota 10))) …)
```

Ten bullets fanned at the player, each 0.2 faster than the last. `k` is
a lambda parameter; nested formations nest lambdas, and every level's
index has a name. The same job is done by seq bindings in `dotimes`
(one value per *volley*) and by `:cols` (an index the bullet carries
into later queries and contact callbacks) — three spellings of
"which one am I", all ordinary.

## Formations are functions

DMK maps indices to arrow-shape coordinates with `bindArrow`, an
engine-bound helper exposing magic variables (`axd`, `ayd`, `aixd`,
`aiyd` — "the best way to understand how they work is to play around
with them"). A formation here is a function from indices to offset
frames, and you write it yourself in four lines:

```clojure
(defn arrow-at [k dx dy]
  (let [w (quot (+ k 1) 2)              ; wing: 0 0 1 1 2 2 …
        s (- 1 (* 2 (mod k 2)))]        ; side: alternating ±1
    (cart (* dx w) (* dy s w))))
(defn arrow [n dx dy]
  (map (fn [k] (pose (arrow-at k dx dy))) (iota n)))
```

`ex2-arrow` fires it moving straight: `((linear c[2 0]) (arrow 11 -0.2 0.1))`
— eleven amulets in a chevron. `circle` and `fan` from the library are
the same species, just pre-written.

## The turn, and why it breaks

Now the tutorial's real problem. The arrow flies right; you want it to
bank downward over a second, *staying an arrow that points where it
flies*. The naive attempt gives every bullet the same turning velocity
(`ex3-unguided`):

```clojure
(map (fn [k] ((pose (arrow-at k -0.2 0.1))
        (vel c[(lerp 1 2 t 2 0) (lerp 1 2 t 0 -2)])))
     (iota 11))
```

Watch it: the formation translates rigidly — identical velocities keep
the offsets identical — but it *never rotates*. After the turn the
arrow still points right while flying down. The offsets were applied
outside the moving frame, in world orientation, once.

DMK solves this with dedicated machinery: spawn an invisible "empty"
bullet to fly the center, record its location and direction every frame
into a keyed public store (`guideempty2 p { ("eloc", code(loc)),
("edir", code(dir)) }`), and have every child compute
`load("eloc", p) + rotatev(load("edir", p), myOffset)` — with `p` as
the unique key tying children to their guide, which is the actual
reason the firing index exists.

## The guided turn: it's a frame

Here the entire subsystem is one edit — move the offsets *inside* the
turning frame (`ex4-guided`):

```clojure
((vel c[(lerp 1 2 t 2 0) (lerp 1 2 t 0 -2)])
  ((pose c[0.6 0]) (arrow 11 -0.2 0.1)))
```

Every frame's pose carries a heading (`linear`'s is its direction,
`vel`'s the instantaneous velocity direction, a closed path's the
tangent), and composition rotates child offsets by it. So the arrow
turns as a shape, and the amulets' rendered facing follows the composed
heading too — DMK's `dir2(load("edir", p))` option row simply
disappears. The `(pose c[0.6 0])` shim is DMK's final refinement
(`0.6 + -0.2 * aixd`): it shifts the pivot from the arrow's tip back
toward its center, so the bank looks like a body rotating rather than a
head dragging a tail.

Checklist of what didn't need to exist: no empty bullet (nothing
spawns for the guide — it's a level of the frame tree), no per-frame
recording, no keyed store, no unique identifier — the child-to-guide
association is *lexical nesting*. The translation notes call this the
largest structural win in the corpus; `cards/translations/200_cradle.dmk`
is a production example (18 guides, 126 petals, zero guide entities).

## Sharing a guide

One case remains: several *patterns* riding one trajectory — a visible
carrier with a turret loop, say. Let-bind the guide; the binding is the
shared instance (`ex5-rig`):

```clojure
(let [guide (vel p[(lerp 0.4 1.6 t 4 0) 45])]
  (par
    (spawn guide {:style {:family :lstar :color :teal} :scale 1.4
                  :team :scenery})
    (in-frame guide
      (for [i inf :every 0.7]
        (spawn-bullet ((aim $player) (fan 3 10 (linear p[2.5 0]))) …)))))
```

The launcher decelerates along its 45° line; the star rides it visibly
(`:team :scenery`, no colliders — bombs ignore it, like DMK's empties);
the aimed fans fire from wherever it currently is. Expressing the guide
as an entity is a *choice* — you spawn it when you want it seen, and
only then. `cards/translations/ph_boss2_spell2.dmk` uses exactly this
rig at production scale.

**Try it:** give `ex4`'s guide a `polar` path instead of the lerp and
watch the arrow orbit; write a `vee` formation (one wing) and race two
of them; fork a second turret in `ex5`'s frame gated on
`$focus-firing`.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum);
this chapter corresponds to DMK's t08.
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
