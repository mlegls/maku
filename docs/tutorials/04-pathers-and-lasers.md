# Tutorial 4: Pathers, Lasers, and Subfiring

Runnable companion: **`cards/tutorials/t04.maku`**.

```sh
cargo run --release --manifest-path crates/Cargo.toml -p maku-player -- cards/tutorials/t04.maku
```

Bullets so far have been points. This tutorial covers the two extended
shapes — ribbons that trail behind a moving point, and beams — plus firing
*from* them.

## Pathers

A pather is a trailing window of a trajectory, drawn as geometry. Give it
a window (in seconds) and a motion; the engine records where the head has
been and draws a ribbon through the last `window` seconds of it
(`ex1-pathers`):

```clojure
(bullet ((rot m"120 * iota(3)")
         (pather 1.2 (cart m"1.2 * t" m"sine(1, 0.35, t)"))) {:style {:family :pather :color [:red :teal :purple] :variant :w}
        :hue m"60 * t"})
```

- `(cart x-expr y-expr)` with time expressions is closed-form motion — a
  position for every `t`. `sine(period, amp, t)` is the stock wave.
- The trail **is** the hitbox: collision tests against the ribbon, not
  just the head.
- `:hue` is a signal-valued tag — `m"60 * t"` drifts each ribbon around
  the color wheel as it flies. Any style-adjacent number can animate this
  way.
- Keep windows modest; cost is proportional to the window.

## Lasers

A laser is two things composed: a **curve** figure (the geometry — where
the beam is) and the **laser spawn** (the gameplay — when it telegraphs,
when it hurts). `(curve {...})` builds the figure; `:u-max` is its
length. `(laser dyn meta)` spawns it as a beam, with the lifecycle in
the meta as ordinary fields: a **warn** phase (the telegraph — visible,
harmless) and an **active** phase (hot) (`ex2-lasers`):

```clojure
(laser ((pose c[-2 3]) ((rot -90) (curve {:u-max 7})))
       {:warn 1 :active 2 :style {:family :laser :color :red}})
```

By default a curve points along its frame's +x, so aiming is ordinary
frame rotation: `(rot -90)` fires it downward, and a time-varying
rotation — `((rot m"-120 + 30 * t") …)` — makes a sweeping beam. There is
no separate "rotating laser" concept; it's the same `rot` you already
know.

`:warn` and `:active` are ordinary entity fields like `:hp` or `:hitbox`.
`touhou.maku` owns the gameplay lifecycle and emits semantic beam rows; the
host-selected Touhou profile turns those rows into warning/active ribbon
layers and materials. Override the fields, animate them, or write your own
beam rules and renderer policy over the same transport.

## Shaped lasers

A curve's shape can be any signal over `(t, u)`, where `u` is distance
along the beam. Using only `u` gives a frozen curve; letting `t` in makes
it writhe (`ex3-shaped`):

```clojure
;; static: a frozen spiral
(laser ((pose c[-2 0])
         (curve (polar m"1.5 * u" m"sine(1.4, 60, u)")
                {:u-max 2.5 :width 0.5}))
       {:warn 1 :active 4 :style {:family :gdlaser :color :yellow}})

;; dynamic: the same spiral, alive
(laser ((pose c[2 0])
         (curve (polar m"1.5 * u" m"-30 * t + sine(1.4, 60, u + t)")
                {:u-max 2.5 :width 0.5}))
       {:warn 1 :active 4 :style {:family :gdlaser :color :pink}})
```

Useful mental model: the shape traces where a bullet with that motion
would fly; the beam is all of those positions at once. With the stock Touhou library and render profile, `:width` scales both drawn
thickness and hitbox. `:resolution` is a geometry sampling hint, and a
signal-valued `:u-max` grows or shrinks the beam
over time. The map on `(curve ...)` seeds the same entity fields the
spawn meta does — writing geometry keys at the figure and lifecycle keys
at the spawn is convention, not a rule, and a field on the figure wins
over the same key in the spawn meta (it's the more specific site).

## Slow lasers

A classic shape: the *whole path* telegraphs at once, but the deadly part
sweeps out from the source. `:fill` is a signal returning the swept
fraction, clamped to 0…1. The stock linear helper is `fill-linear`
(`ex6-slow`):

```clojure
(laser ((pose c[-3 3]) ((rot -90) (curve {:u-max 7})))
       {:warn 0.8 :active 4 :fill (fill-linear 0.8 1.5)})
```

While filling, the full path renders dim (still a telegraph) and the
swept prefix renders bright; the hitbox covers only the prefix. Players
standing on the far end of a telegraphed line have exactly `warn +
dur·(u/u-max)` seconds to move — the fairness knob is explicit.

For a non-linear sweep, provide any expression over the laser's age `t`
returning the swept fraction. A fast start that decelerates toward the
tip:

```clojure
(laser ((pose c[-3 3]) ((rot -90) (curve {:u-max 7})))
       {:warn 0.8 :active 4
        :fill m"1 - (1 - (t - 0.8) / 1.5)^2"})   ; ease-out sweep
```

## Firing from a pather's tip

To fire from the moving tip, name the guide motion with `let` and use it
twice — once to draw the ribbon, once as a frame for the firing loop
(`ex4-tip-fire`):

```clojure
(let [guide (cart m"1.4 * t" m"sine(1.1, 0.3, t)")]
  (par
    (bullet (pather 1.5 guide) {:style {:family :lightning :color :blue :variant :w}})
    (in-frame guide
      (fork
        (for [i 40 :every (ticks 10)]
          (bullet ((rot 180) (linear p[2 0])) {:style {:family :amulet :color :pink :variant :b}}))))))
```

The `let` matters: both uses share *one* instance of the guide, so the
loop's frame is exactly the ribbon's head at every moment. This
guide-as-frame idiom generalizes — anything can ride anything.

## Firing along a laser

`(on-laser h u)` returns the pose — position *and tangent heading* — of
the point at distance `u` along a live laser. Firing normal to the beam
is the tangent plus 90° (`ex5-on-laser`):

```clojure
(let [h (laser ((pose c[-2.5 -2])
                 (curve (polar m"1.8 * u" m"-15 * t + sine(2.8, 40, u + t)")
                        {:u-max 3 :width 0.6}))
               {:warn 0.5 :active 5 :style {:family :gdlaser :color :red}})]
  (for [i 44 :every (ticks 8)]
    ((pose (on-laser (nth h 0) m"0.07 * i"))
      ((rot 90)
        (bullet (linear p[1.5 0]) {:style {:family :gem :color :green :variant :w}})))))
```

`laser` returns handles like every spawner; the loop walks `u` outward
each volley, so the gems peel off the beam from base to tip — and because
the pose is sampled live, they track the writhing curve.

The way to think about all of this: **points over time are bullets;
curves over time are lasers.** A curve is just a motion expression that
mentions `u`, and the `laser` spawn *expresses* one as an entity — the
same way `bullet` expresses a point motion. Which means curves don't need
an entity at all to be useful: `(sample curve t u)` evaluates one
anywhere, returning the pose (with tangent) at that point —

```clojure
(let [arc (polar m"1.8 * u" m"sine(2.8, 40, u + t)")]
  ((pose (sample arc 0.5 0.7))      ; a point on the curve, no laser
    ((rot 90) (bullet (linear p[1.5 0]) {}))))
```

`on-laser` is the entity-clocked convenience: the live laser's own age
supplies `t`. Use `sample` when the curve is data; use `on-laser` when
an actual beam is on screen and you want to stay synced to it.

## Lasers in lifecycle trees

Tutorial 3's per-bullet timelines compose with everything here: a
timeline can cull its bullet and spawn a laser in its place (a kind
change is a cull + spawn), hold the laser's handle, and fire off the
beam with `on-laser` a beat later. `ex7-lifecycle` in the companion card
runs a four-stage chain — ring → lasers → perpendicular shots →
parity-alternating explosions — with no queries at all.

**Try it:** make `ex5`'s gems fire from *random* points on the beam; give
`ex4`'s tip-fire a spread by replacing the single spawn with a `fan`;
give `ex7` a fifth stage.

Next: [Tutorial 5](05-channels.md) — channels, the host boundary, and
the player rig.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
