# Tutorial 2: Bullet Controls

Runnable companion: **`cards/tutorials/t02.dmk`** — press the number keys
to run each example.

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t02.dmk
```

Tutorial 1 created bullets; this one changes them in flight. The two tools
are **queries** (select live bullets) and **manipulate** (do something to
each match). Everything else — bouncing, cleanup rules, transformations,
bullets that spawn bullets — is built from those two plus the control flow
you already have.

## A control is a loop

Here is "bounce circles off an invisible ceiling at y = 3, for four
seconds" (`ex1-flip`):

```clojure
(fork
  (for [i 480 :every (ticks 1)]              ; four seconds, every tick
    (manip {:family :circle
                 :where (fn [b] (and (> b.pos.y 3) (> b.vel.y 0)))}
      (fn [b] (remat b (linear c[b.vel.x (- 0 b.vel.y)]))))))
```

- `(manip query callback)` runs the callback on every live bullet the
  query matches, *right now*. To keep watching, put it in a loop; to stop
  after a while, make the loop finite (480 iterations here) or wrap it in
  `(until pred …)` for an event-driven stop.
- The **query** selects by style fields plus an arbitrary predicate over
  the bullet's *view*: its `:pos`, `:vel`, `:t` (age), and any columns it
  carries. Dotted symbols are accessor chains — `b.pos.y` reads the y of
  the position — and they work on handles, views, and vectors alike. In
  `m"…"` strings you also get indexing: `xs.[0]`, `xs.[0 1]` (gather),
  `xs.[iota(3)]`.
- The bounce itself is `remat` (*rematerialize*): `(remat b dyn)` swaps
  the bullet's motion for a new one anchored at its current position, with
  a fresh local clock — here, straight-line motion with the vertical
  velocity negated (`b.vel.x` and `b.vel.y` read the live values at the
  moment of the swap). This is the standard way to change a bullet's
  course mid-flight. There is also a callback form, `(remat b (fn [exit]
  dyn))`, where `exit` is the snapped `{:pos :vel :t}` — equivalent here,
  but it matches the `stages` convention where the boundary state only
  exists in the future.

Note the predicate checks `(> b.vel.y 0)` — only bullets still moving up
get flipped, so a bullet doesn't re-flip forever while above the line.

One footgun: bullets spawned without `:style` have an empty family, so a
`{:family :circle}` query won't match them. Selection matches what the
record actually says.

## Selecting across styles

An array in a query field means *any of* (`ex2-select-cull`):

```clojure
(manip {:family [:circle :star]
             :color [:red :blue]
             :where (fn [b] (< b.pos.y -2))}
  (fn [b] (cull b)))
```

That reads: circles and stars, in red or blue, below y = −2 — delete them.
Green bullets of the same families sail past untouched; run the example
and watch.

## Several effects per match

The callback body is ordinary code, so several effects under one
predicate is just a `seq` (`ex3-restyle-at-age`):

```clojure
(manip {:family :circle :where (fn [b] (> b.t 1))}
  (fn [b]
    (seq
      (event :sfx "x-transform-1")
      (set-style b {:family :arrow :color :red :variant :b}))))
```

`(set-style b {...})` changes a live bullet's appearance. Order within the
`seq` matters in the obvious way — and it fails gently: any verb after a
`(cull b)` is a no-op on the dead handle rather than an error.

## Bullets that spawn bullets

Callbacks can spawn, anchored wherever you like (`ex4-burst`):

```clojure
(manip {:family :star :where (fn [b] (> b.t 1.1))}
  (fn [b]
    (seq
      ((pose (pos b))
        (spawn (circle 8 (linear p[3 0]))
               {:style {:family :gem :color :pink :variant :w}}))
      (cull b))))
```

`(pos b)` reads the bullet's current world position; wrapping the spawn in
`(pose …)` anchors the ring there. Callbacks always spawn in world
coordinates — a control firing under some rotated frame hierarchy won't
double-anchor its spawns.

## A note on cost

Controls run per matched bullet, every iteration of their loop. At
tutorial scale this is nothing; for heavy effects prefer tighter queries
(match on style fields first — they're cheap) and coarser loop periods
(`:every (ticks 5)` is usually indistinguishable from every tick).

Next: [Tutorial 3](03-two-spells.md) — putting both tutorials together
into two complete spell patterns.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
