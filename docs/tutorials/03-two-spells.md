# Tutorial 3: Two Spells

Runnable companion: **`cards/tutorials/t03.dmk`**.

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t03.dmk
```

This tutorial builds two recognizable spell cards from the tools of the
first two tutorials — both classics of the "bullets that become other
bullets" family.

## Spell 1: Miracle Fruit

Utsuho's **Miracle Fruit** (Touhou 11: Subterranean Animism, extra
stage). The design: slow "seed" bullets spread out in a ring; after a
moment, each seed bursts into rings of shots expanding from where it
died; repeat. Two moving parts — an emitter, and a control watching for
ripe seeds (`ex1-fruit-basic`):

```clojure
(par
  ;; the emitter: 8 seeds, every 3 seconds
  (dotimes [vol inf :every 3]
    (spawn (circle 8 (linear p[3 0]))
           {:style {:family :lellipse :color :red :variant :w}}))
  ;; the control: seeds older than 0.7s burst and die
  (fork
    (dotimes [i inf :every (ticks 5)]
      (manipulate {:family :lellipse :where (fn [b] (> b.t 0.7))}
        (fn [b]
          (seq
            ((pose (pos b))
              (spawn (circle 20 (vel p[(lerp 0.3 1.4 t 0 2.6) 0]))
                     {:style {:family :ellipse :color :red :variant :w}}))
            (cull b)))))))
```

New here: `(vel p[(lerp 0.3 1.4 t 0 2.6) 0])` — motion in the *velocity*
domain, with a time-varying speed. `(lerp a b t from to)` ramps from
`from` to `to` as `t` goes `a → b`, so the burst rings ease in from a
standstill.

### Bursts that unfold over time

One ring per seed is flat — Miracle Fruit's signature is six rings
unfolding over time at growing radius: a *timed sequence per seed*. Fork
it from inside the callback (`ex2-fruit-staged`):

```clojure
(fn [b]
  (seq
    (fork
      ((pose (pos b))
        (dotimes [ring 6 :every (ticks 12)]
          (spawn (circle 20
                   ((pose c[(* 0.4 ring) 0])
                     (vel p[(lerp 0.3 1.4 t 0 2.6) 0])))
                 {:style {:family :ellipse :color :red :variant :w}}))))
    (cull b)))
```

A `fork` inside a callback schedules the work as a child task of the
pattern — the six rings keep appearing on their 12-tick cadence after the
callback (and the seed) are long gone. The `(pose c[(* 0.4 ring) 0])`
inside the circle offsets each bullet outward, so successive rings start
at larger radii.

### Data that travels with a bullet

Give each seed a color from a palette, and make its burst match. Columns
are per-bullet data: array values in `:cols` bind per element, exactly
like style arrays (`ex3-fruit-colors`):

```clojure
(def palette [:red :pink :purple :blue :teal :green :yellow :orange])

(spawn (circle 8 (linear p[3 0]))
       {:style {:family :lellipse :color palette :variant :w}
        :cols {:ci (iota 8)}})          ; seed k carries ci = k
```

The callback reads the column back off the bullet view and uses it to
pick the burst color:

```clojure
(fn [b]
  (let [ci b.ci]
    …
    {:style {:family :ellipse :color (nth palette ci) :variant :w}}
    …))
```

Columns are how any per-bullet fact crosses from spawn time to
control time.

## Spell 2: Danmaku Chimera

Keine's **Danmaku Chimera** (Touhou 8: Imperishable Night). The design:
an emitter weaves side to side, firing rings of long bullets; after a
second of flight, each long bullet freezes into a string of beads laid
along the path it just traveled.

The weave is a frame whose position is a function of time
(`ex4-weave`):

```clojure
(in-frame (cart m"2 * sine(8, 1, t)" 0)
  (dotimes [vol inf :every 2]
    (spawn ((rot m"22 * vol") (circle 16 (linear p[2 0])))
           {:style {:family :keine :color :purple :variant :w}})))
```

`(cart x y)` with a time expression makes a moving frame; everything
inside — including where each volley spawns — tracks it. `(rot m"22 * vol")`
turns each volley a little, so the rings interleave.

The transform recovers each bullet's launch point from its view alone:
straight-line motion means *tail = pos − vel·t*. Thirteen beads are laid
along the segment from tail to head (`ex5-chimera`):

```clojure
(manipulate {:family :keine :where (fn [b] (> b.t 1))}
  (fn [b]
    (let [tx (- b.pos.x (* b.vel.x b.t))
          ty (- b.pos.y (* b.vel.y b.t))]
      (seq
        (spawn (map (fn [k]
                      (pose c[(lerp 0 12 k tx b.pos.x)
                              (lerp 0 12 k ty b.pos.y)]))
                    (iota 13))
               {:style {:family :gcircle :color :blue :variant :w}})
        (cull b)))))
```

`(map f (iota 13))` builds an array of 13 poses; spawning an array of
poses makes 13 stationary bullets. `lerp` here interpolates positions —
`(lerp 0 12 k tx hx)` walks from tail-x to head-x as `k` runs 0 to 12.

**Try it:** make the beads start moving outward after they appear (hint:
a second control watching `:family :gcircle`); make the burst-fruit seeds
themselves home slowly toward the bottom of the screen.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
