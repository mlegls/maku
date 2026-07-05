# Tutorial 3: Two Spells

Runnable companion: **`cards/tutorials/t03.dmk`**.

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t03.dmk
```

This tutorial builds two recognizable spell cards from the tools of the
first two tutorials — and factors them the way real cards get written:
parts earn names at the moment they'd otherwise be repeated.

## Spell 1: Miracle Fruit

Utsuho's **Miracle Fruit** (Touhou 11: Subterranean Animism, extra
stage). The design: slow "seed" bullets spread out in a ring; after a
moment, each seed bursts into rings of shots expanding from where it
died; repeat.

Start with everything inline — an emitter and a watcher
(`ex1-fruit-basic`):

```clojure
(par
  ;; the emitter: 8 seeds, every 3 seconds
  (for [vol inf :every 3]
    (spawn (circle 8 (linear p[3 0]))
           {:style {:family :lellipse :color :red :variant :w}}))
  ;; the watcher: seeds older than 0.7s burst and die
  (fork
    (for [i inf :every (ticks 5)]
      (manip {:family :lellipse :where (fn [b] (> b.t 0.7))}
        (fn [b]
          (seq
            ((pose (pos b))
              (spawn (circle 20 (vel p[(lerp 0.3 1.4 t 0 2.6) 0]))
                     {:style {:family :ellipse :color :red :variant :w}}))
            (cull b)))))))
```

One new thing: `(vel p[(lerp 0.3 1.4 t 0 2.6) 0])` is motion in the
*velocity* domain with a time-varying speed — `(lerp a b t from to)`
ramps as `t` goes `a → b`, so the burst eases in from a standstill.

### The burst grows — so name the parts

Miracle Fruit's signature is six rings unfolding over time at growing
radius. Upgrading the inline version would mean copying the emitter
verbatim and inflating the callback — this is the moment to factor.

**`defn`** names the burst as a function of *where* and *which color*.
Actions are values, so the function's result slots straight into the
callback:

```clojure
(def palette [:red :pink :purple :blue :teal :green :yellow :orange])

(defn burst [at ci]
  ((pose at)
    (for [ring 6 :every (ticks 12)]
      (spawn (circle 20
               ((pose c[(* 0.4 ring) 0])
                 (vel p[(lerp 0.3 1.4 t 0 2.6) 0])))
             {:style {:family :ellipse
                      :color (nth palette ci)
                      :variant :w}}))))
```

**`defpattern` with parameters** names the emitter. Parameters are
name/default pairs; each seed also gets its palette index as a *column*
(per-bullet data — `:cols` arrays bind per element, like style arrays):

```clojure
(defpattern seeds [n 8]
  (for [vol inf :every 3]
    (spawn (circle n (linear p[3 0]))
           {:style {:family :lellipse :color palette :variant :w}
            :cols {:ci (iota n)}})))
```

Now the staged version is just composition — patterns invoke like
functions (`ex2-fruit-staged`):

```clojure
(par
  (seeds)
  (fork
    (for [i inf :every (ticks 5)]
      (manip {:family :lellipse :where (fn [b] (> b.t 0.7))}
        (fn [b]
          (seq (fork (burst (pos b) 0))     ; one color for now
               (cull b)))))))
```

The `fork` inside the callback schedules the six-ring sequence as a
child task — the rings keep unfolding after the seed is culled.

### Color inheritance costs one character

The seeds already carry their palette index; pass it through
(`ex3-fruit-colors`):

```clojure
(fn [b]
  (seq (fork (burst (pos b) b.ci))
       (cull b)))
```

That `0` → `b.ci` is the whole diff between `ex2` and `ex3` — which is
the payoff of having factored: variations are one-line changes.

## Spell 2: Danmaku Chimera

Keine's **Danmaku Chimera** (Touhou 8: Imperishable Night). The design:
an emitter weaves side to side firing rings of long bullets; after a
second of flight, each long bullet freezes into a string of beads laid
along the path it just traveled.

The weave stands on its own — a frame whose position is a function of
time (`ex4-weave`):

```clojure
(defpattern ex4-weave []
  (in-frame (cart m"2 * sine(8, 1, t)" 0)
    (for [vol inf :every 2]
      (spawn ((rot m"22 * vol") (circle 16 (linear p[2 0])))
             {:style {:family :keine :color :purple :variant :w}}))))
```

`(cart x y)` with a time expression makes a moving frame; everything
inside tracks it. `(rot m"22 * vol")` turns each volley a little so the
rings interleave.

The full spell **reuses the previous example directly** — `(ex4-weave)`
is simply a part of `ex5-chimera`. And having now written the
fork-a-watcher shape three times in this card, the idiom earns a
**macro** — a named piece of *syntax*:

```clojure
(defmacro control [period query effect]
  `(fork (for [i inf :every ~period] (manip ~query ~effect))))

(defmacro where [expr]
  `(fn [b] ~expr))
```

Backtick quotes a code template; `~` splices arguments in. Macros
receive their arguments as unevaluated code — that's why `where` can
turn a bare expression into a predicate function.

```clojure
(defpattern ex5-chimera []
  (par
    (ex4-weave)
    (control (ticks 5)
             {:family :keine :where (where (> b.t 1))}
             (fn [b]
               (let [tx (- b.pos.x (* b.vel.x b.t))
                     ty (- b.pos.y (* b.vel.y b.t))]
                 (seq
                   (spawn (map (fn [k]
                                 (pose c[(lerp 0 12 k tx b.pos.x)
                                         (lerp 0 12 k ty b.pos.y)]))
                               (iota 13))
                          {:style {:family :gcircle :color :blue :variant :w}})
                   (cull b)))))))
```

The bead line needs no saved state: for straight motion the launch point
is recoverable from the view alone — *tail = pos − vel·t* — and
`(map f (iota 13))` lays 13 stationary poses along the segment.

## The abstraction ladder

The order things earned names in this card is the general rule:

1. **Inline** until something repeats.
2. **`defn`** for repeated *values* — and almost everything is a value
   here: frames, motions, whole action trees.
3. **`defpattern` parameters** for reusable, tunable spells — each
   invocation gets private `defvar` state, so a pattern used twice never
   trips over itself.
4. **`defmacro`** last, only for genuine notation — when arguments must
   stay unevaluated (like `where`) or the shape itself is the thing being
   named (like `control`).

**Try it:** make the beads start moving outward after they appear (a
second `control` watching `:family :gcircle`); parameterize `ex4-weave`'s
sway width and reuse it at two widths at once; write a `(ripe age)` macro
combining `where` with the age check.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
