# Tutorial 4: Pathers, Lasers, and Subfiring

Runnable companion: **`cards/tutorials/t04.dmk`**.

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t04.dmk
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
(spawn ((rot m"120 * iota(3)")
         (pather 1.2 (cart m"1.2 * t" m"sine(1, 0.35, t)")))
       {:style {:family :pather :color [:red :teal :purple] :variant :w}
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

A laser has a lifecycle: a **warn** phase (the telegraph — visible,
harmless) and an **active** phase (hot). `:u-max` is its length
(`ex2-lasers`):

```clojure
(spawn ((pose c[-2 3]) ((rot -90) (laser {:warn 1 :active 2 :u-max 7})))
       {:style {:family :laser :color :red}})
```

By default a laser points along its frame's +x, so aiming is ordinary
frame rotation: `(rot -90)` fires it downward, and a time-varying
rotation — `((rot m"-120 + 30 * t") …)` — makes a sweeping beam. There is
no separate "rotating laser" concept; it's the same `rot` you already
know.

## Shaped lasers

A laser's shape can be any signal over `(t, u)`, where `u` is distance
along the beam. Using only `u` gives a frozen curve; letting `t` in makes
it writhe (`ex3-shaped`):

```clojure
;; static: a frozen spiral
(laser (polar m"1.5 * u" m"sine(1.4, 60, u)")
       {:warn 1 :active 4 :u-max 2.5 :width 0.5})

;; dynamic: the same spiral, alive
(laser (polar m"1.5 * u" m"-30 * t + sine(1.4, 60, u + t)")
       {:warn 1 :active 4 :u-max 2.5 :width 0.5})
```

Useful mental model: the shape traces where a bullet with that motion
would fly; the beam is all of those positions at once. `:width` scales
both the drawn thickness and the hitbox; `:resolution` is a sampling
hint for rendering; a signal-valued `:u-max` grows or shrinks the beam
over time.

## Slow lasers

A classic shape: the *whole path* telegraphs at once, but the deadly part
sweeps out from the source. That's `:fill` — seconds for the hot front to
travel source→tip once the warn ends (`ex6-slow`):

```clojure
(laser {:warn 0.8 :active 4 :u-max 7 :fill 1.5})
```

While filling, the full path renders dim (still a telegraph) and the
swept prefix renders bright; the hitbox covers only the prefix. Players
standing on the far end of a telegraphed line have exactly `warn +
fill·(u/u-max)` seconds to move — the fairness knob is explicit.

## Firing from a pather's tip

To fire from the moving tip, name the guide motion with `let` and use it
twice — once to draw the ribbon, once as a frame for the firing loop
(`ex4-tip-fire`):

```clojure
(let [guide (cart m"1.4 * t" m"sine(1.1, 0.3, t)")]
  (par
    (spawn (pather 1.5 guide)
           {:style {:family :lightning :color :blue :variant :w}})
    (in-frame guide
      (fork
        (for [i 40 :every (ticks 10)]
          (spawn ((rot 180) (linear p[2 0]))
                 {:style {:family :amulet :color :pink :variant :b}}))))))
```

The `let` matters: both uses share *one* instance of the guide, so the
loop's frame is exactly the ribbon's head at every moment. This
guide-as-frame idiom generalizes — anything can ride anything.

## Firing along a laser

`(on-laser h u)` returns the pose — position *and tangent heading* — of
the point at distance `u` along a live laser. Firing normal to the beam
is the tangent plus 90° (`ex5-on-laser`):

```clojure
(let [h (spawn ((pose c[-2.5 -2])
                 (laser (polar m"1.8 * u" m"-15 * t + sine(2.8, 40, u + t)")
                        {:warn 0.5 :active 5 :u-max 3 :width 0.6}))
               {:style {:family :gdlaser :color :red}})]
  (for [i 44 :every (ticks 8)]
    ((pose (on-laser (nth h 0) m"0.07 * i"))
      ((rot 90)
        (spawn (linear p[1.5 0])
               {:style {:family :gem :color :green :variant :w}})))))
```

`spawn` returns handles; the loop walks `u` outward each volley, so the
gems peel off the beam from base to tip — and because the pose is sampled
live, they track the writhing curve.

**Try it:** make `ex5`'s gems fire from *random* points on the beam; give
`ex4`'s tip-fire a spread by replacing the single spawn with a `fan`.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
