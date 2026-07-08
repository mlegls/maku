# Tutorial 2: Changing Bullets in Flight

Runnable companion: **`cards/tutorials/t02.maku`** — press the number keys
to run each example.

```sh
cargo run --release --manifest-path proto/Cargo.toml --features player --bin maku -- cards/tutorials/t02.maku
```

Tutorial 1 created bullets; this one changes them mid-flight. There are
two tools, and the order matters:

1. **Lifecycle trees** — when *you* spawned the bullets and know the
   schedule, each bullet carries its own timeline. This is the default.
2. **Queries** — when the trigger depends on runtime state, or the set
   of bullets is open. More general, costs more.

## Lifecycle trees

The structural fact everything builds on: **`spawn-bullet` returns handles**,
one per bullet. `for` iterates them, and `fork` gives each its own
clock (`ex1-restyle`):

```clojure
(let [ring (spawn-bullet (circle 12 (linear p[1.2 0])) {:style {:family :circle :color :blue :variant :w}})]
  (for [b ring]
    (fork
      (seq
        (wait 1)
        (event :sfx "x-transform-1")
        (set-style b {:family :arrow :color :red :variant :b})))))
```

Read the forked `seq` as the bullet's *biography*: fly for a second,
announce, turn into a red arrow. `(set-style b {…})` changes a live
bullet's appearance in place.

A timeline can also end the bullet's life and start others
(`ex2-burst`):

```clojure
(for [s stars]
  (fork
    (seq
      (wait 1.2)
      ((pose (pos s))
        (spawn-bullet (circle 8 (linear p[3 0])) {:style {:family :gem :color :pink :variant :w} :hitbox 0.09}))
      (cull s))))
```

`(pos s)` reads the bullet's current world position; wrapping the spawn
in `(pose …)` anchors the ring there. Timelines always act in world
coordinates — a rig running under some rotated frame hierarchy won't
double-anchor.

Two properties make timelines robust:

- **Dead handles are no-ops.** If the player bombs a star before 1.2s,
  the rest of its timeline does nothing, harmlessly. You never check
  liveness.
- **Sleeping is nearly free.** A waiting timeline costs O(1) per tick;
  nothing scans anything.

## Queries

Now try to write "bounce off an invisible ceiling at y = 3" as a
timeline. You can't — *when* the bullet crosses the line depends on
where it's going, which is runtime state. For that there is
`(manip query effect)`: select live bullets matching the query, right
now, and run the effect on each (`ex3-flip`):

```clojure
(fork
  (for [i 480 :every (ticks 1)]              ; four seconds, every tick
    (manip (fn [b] (* (= b.family :circle) (> b.pos.y 3) (> b.vel.y 0)))
      (fn [b] (remat b (linear c[b.vel.x (- 0 b.vel.y)]))))))
```

- `manip` runs *once*; to keep watching, loop it. A finite loop (480
  iterations) is the control's lifetime; `(until pred …)` is the
  event-driven stop.
- The **query** is a predicate over the bullet's *view*: `:pos`, `:vel`,
  `:t` (age), style fields, and any columns it carries. Dotted symbols
  are accessor chains — `b.pos.y` — and work on handles, views, and
  vectors alike. In `m"…"` strings you also get indexing: `xs.[0]`,
  `xs.[0 1]` (gather), `xs.[iota(3)]`.
- The bounce is `remat` (*rematerialize*): `(remat b dyn)` swaps the
  bullet's motion for a new one anchored at its current position with a
  fresh local clock — here, straight-line motion with the vertical
  velocity negated, read live off the handle.

Note the predicate checks `(> b.vel.y 0)` — only bullets still moving up
flip, so nothing re-flips forever above the line.

Selection generalizes across styles with ordinary predicate composition
(`ex4-select-cull`):

```clojure
(manip (fn [b] (* (+ (= b.family :circle) (= b.family :star))
                  (+ (= b.color :red) (= b.color :blue))
                  (< b.pos.y -2)))
  (fn [b] (cull b)))
```

Circles and stars, in red or blue, below y = −2 — delete them. Green
bullets of the same families sail past untouched.

One footgun: bullets spawned without `:style` have an empty family, so a
predicate checking `(= b.family :circle)` won't match them. Selection
matches what the record actually says.

## Which tool, when

**Timelines** when you spawned it and the schedule is static — age
thresholds, staged transformations, multi-stage lifecycles. **Queries**
when the trigger reads runtime state (position, velocity, proximity),
when selecting across styles, or when the watched set is *open* —
bullets entering it that no timeline holds (tutorial 3's chimera
regenerates itself this way). Queries cost a population scan per poll;
timelines sleep. Tighter style fields and coarser poll periods
(`:every (ticks 5)`) keep controls cheap when you do need them.

Next: [Tutorial 3](03-two-spells.md) — putting both tutorials together
into two complete spell patterns.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
