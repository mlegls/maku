# Tutorial 7: Indices, Formations, and Guided Fires

Runnable companion: **`cards/tutorials/t07.maku`**.

```sh
cargo run --release --manifest-path proto/Cargo.toml --features player --bin maku -- cards/tutorials/t07.maku
```

This tutorial builds up to one skill: making a *complex shape* of
bullets — an arrow, a ring of rings, a carrier with escorts — move as a
shape: turning together, staying rigid, pointing where it flies. On the
way it settles two smaller questions that every pattern eventually
asks: how does a bullet know which one it is, and where do formations
come from?

## Indices are just variables

Bullets in a volley usually differ by *index* — the third bullet is
faster, the seventh is offset further. Whenever you create multiplicity
with `map` over `iota`, the index is simply in scope (`ex1-index`):

```clojure
(spawn-bullet ((aim $player)
         (map (fn [k] ((rot (- (* 6 k) 27))
                        (linear p[(+ 2 (* 0.2 k)) 0])))
              (iota 10))) …)
```

Ten bullets fanned at the player, each 0.2 faster than the last. `k` is
a lambda parameter; nested formations nest lambdas, and every level's
index has its own name — no numbering scheme to manage. Two other
spellings cover the other lifetimes of "which one am I":

- `dotimes` seq bindings — one value per *volley* (`[vol inf lr [1 -1]]`
  alternates a sign every shot);
- `:cols` — an index the bullet *carries*, readable later in queries
  and contact callbacks.

## Formations are functions

A formation is nothing but a mapping from indices to offset frames, so
it's a function you write once. Here is an arrowhead — a tip plus two
staggered wings:

```clojure
(defn arrow-at [k dx dy]
  (let [w (quot (+ k 1) 2)              ; wing: 0 0 1 1 2 2 …
        s (- 1 (* 2 (mod k 2)))]        ; side: alternating ±1
    (cart (* dx w) (* dy s w))))
(defn arrow [n dx dy]
  (map (fn [k] (pose (arrow-at k dx dy))) (iota n)))
```

`ex2-arrow` fires it moving straight: `((linear c[2 0]) (arrow 11 -0.2 0.1))`
— eleven amulets in a chevron. The library's `circle` and `fan` are the
same species, just pre-written; when a shape recurs across your cards,
give it a `defn` next to them.

## The turn, and why the obvious attempt fails

Now the real problem. The arrow flies right; you want it to bank
downward over a second, *staying an arrow that points where it flies*.
The obvious attempt gives every bullet the same turning velocity, with
the offsets applied around each bullet's spawn point (`ex3-unguided`):

```clojure
(map (fn [k] ((pose (arrow-at k -0.2 0.1))
        (vel c[(lerp 1 2 t 2 0) (lerp 1 2 t 0 -2)])))
     (iota 11))
```

Watch it: the formation translates rigidly — identical velocities keep
the offsets identical — but it *never rotates*. After the turn the
arrow still points right while flying down. The offsets were fixed in
world orientation at spawn, and nothing ever revisits them.

## The guided turn: it's a frame

The fix is one structural edit: move the offsets *inside* the turning
frame (`ex4-guided`):

```clojure
((vel c[(lerp 1 2 t 2 0) (lerp 1 2 t 0 -2)])
  ((pose c[0.6 0]) (arrow 11 -0.2 0.1)))
```

This works because every frame's pose carries a *heading* along with
its position — `linear`'s is its direction of travel, `vel`'s the
instantaneous velocity direction, a closed path's the tangent — and
frame composition rotates child offsets by the parent's heading. The
outer `vel` level is a *guide*: it renders nothing and collides with
nothing (it's a level of the frame tree, not an entity), it just flies
the turn, and the arrow rides it as a rigid body. The bullets' rendered
facing follows the composed heading too, so the amulets point along the
bank without further ado.

The `(pose c[0.6 0])` shim is worth pausing on. Without it the guide
sits at the arrow's *tip*, and the turn reads as a head dragging a tail.
Shifting every offset forward moves the pivot back toward the shape's
center, and the bank looks like a body rotating. Tune the pivot the way
you'd tune an easing — by eye.

The pattern generalizes to any rigid ensemble: put the shared motion in
one frame level, the shape below it, and nest further for shapes within
shapes. `cards/translations/200_cradle.maku` runs it at production scale
— three volleys × six guides × seven petals, every level a frame.

## Sharing a guide

One case remains: several *patterns* riding one trajectory — say a
visible carrier with a turret loop. Let-bind the guide; the binding is
the shared instance (`ex5-rig`):

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
(`:team :scenery`, no colliders — field clears pass over it); the aimed
fans fire from wherever it currently is. Spawning the guide as an
entity is a choice you make *when you want it seen* — the mechanism
never requires it. `cards/translations/ph_boss2_spell2.maku` uses this
rig at production scale.

**Try it:** give `ex4`'s guide a `polar` path instead of the lerp and
watch the arrow orbit; write a `vee` formation (one wing) and race two
of them; fork a second turret in `ex5`'s frame gated on
`$focus-firing`.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
