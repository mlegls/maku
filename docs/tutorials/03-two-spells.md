# Tutorial 3: Two Spells

Runnable companion: **`cards/tutorials/t03.maku`**.

```sh
cargo run --release --manifest-path crates/Cargo.toml -p maku-player -- cards/tutorials/t03.maku
```

This tutorial builds two recognizable spell cards from the tools of the
first two tutorials — and factors them the way real cards get written:
parts earn names at the moment they'd otherwise be repeated.

## Spell 1: Miracle Fruit

Utsuho's **Miracle Fruit** (Touhou 11: Subterranean Animism, extra
stage). The design: slow "seed" bullets spread out in a ring; after a
moment, each seed bursts into rings of shots expanding from where it
died; repeat.

Seeds ripen on a fixed schedule, so this is timeline territory
(tutorial 2): each seed carries its own forked biography
(`ex1-fruit-basic`):

```clojure
(for [vol inf :every 3]
  (let [seeds (bullet (circle 8 (linear p[3 0])) {:style {:family :lellipse :color :red :variant :w}})]
    (for [b seeds]
      (fork
        (seq
          (wait 0.7)                          ; ripen
          ((pose (pos b))
            (bullet (circle 20 (vel p[(lerp 0.3 1.4 t 0 2.6) 0])) {:style {:family :ellipse :color :red :variant :w}}))
          (cull b))))))
```

One new thing: `(vel p[(lerp 0.3 1.4 t 0 2.6) 0])` is motion in the
*velocity* domain with a time-varying speed — `(lerp a b t from to)`
ramps as `t` goes `a → b`, so the burst eases in from a standstill.

### The burst grows — so name the parts

Miracle Fruit's signature is six rings unfolding over time at growing
radius. Inflating the timeline inline would bury the spell's shape —
this is the moment to factor.

**`defn`** names the burst as a function of *where* and *which color*.
Actions are values, so the function's result slots straight into the
timeline:

```clojure
(def palette [:red :pink :purple :blue :teal :green :yellow :orange])

(defn burst [at ci]
  ((pose at)
    (for [ring 6 :every (ticks 12)]
      (bullet (circle 20
               ((pose c[(* 0.4 ring) 0])
                 (vel p[(lerp 0.3 1.4 t 0 2.6) 0]))) {:style {:family :ellipse
                      :color (nth palette ci)
                      :variant :w}}))))
```

**`defpattern` takes parameters** — name/default pairs — so the whole
spell becomes tunable (`ex2-fruit-staged`):

```clojure
(defpattern ex2-fruit-staged [n 8 ripen 0.7]
  (for [vol inf :every 3]
    (let [seeds (bullet (circle n (linear p[3 0])) {:style {:family :lellipse :color palette :variant :w}})]
      (for [b seeds, ci (iota n)]
        (fork
          (seq (wait ripen)
               (fork (burst (pos b) 0))       ; one color for now
               (cull b)))))))
```

The inner `fork (burst …)` schedules the six-ring sequence as its own
child — the rings keep unfolding after the seed is culled. And note the
paired binding `[b seeds, ci (iota n)]`: each seed's timeline knows its
own index, lexically.

### Color inheritance costs one token

The index is already in scope; pass it through (`ex3-fruit-colors`):

```clojure
(fork (burst (pos b) ci))                    ; ← the whole diff
```

That `0` → `ci` is the entire difference between `ex2` and `ex3` — the
payoff of having factored: variations are one-token changes. (If a
*query* triggered the burst instead, the index would have to ride the
bullet as a column — `:cols {:ci (iota n)}` at spawn, `b.ci` in the
callback. Columns are how data crosses stage boundaries you don't hold
handles across.)

## Spell 2: Danmaku Chimera

Keine's **Danmaku Chimera** (Touhou 8: Imperishable Night). The design:
an emitter weaves side to side firing rings of long bullets; after a
second of flight, each long bullet freezes into a string of beads laid
along the path it just traveled.

The weave stands on its own — a frame whose position is a function of
time (`ex4-weave`):

```clojure
(defpattern ex4-weave []
  (in-frame (cart m"sine(12.94, 2, t)" 0)
    (for [vol inf :every 2]
      (bullet ((rot m"13.6 * vol")
               (circle 16 ((pose c[1 0]) (linear p[2 0])))) {:style {:family :keine :color :purple :variant :w}}))))
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

Unlike the fruit, this spell genuinely **needs a query**: regeneration
(act 2, below) re-fires bullets that *re-enter* act 1's watch — the
watched set is open, growing with bullets no timeline is holding. That's
the rule of thumb in one sentence: **hold handles and write timelines
when you spawned it and the schedule is static; watch with queries when
the set is open or the trigger reads runtime state** (position,
velocity, proximity — like tutorial 2's ceiling bounce).

The spell itself runs in two acts.

**Act 1 — freeze.** Each long bullet becomes 13 beads laid along its
flight path. The launch point needs no saved state: for straight motion
*tail = pos − vel·t*, recoverable from the view alone. The beads anchor
at the volley origin and each one's motion is a `rot` frame whose angle
*eases over time* — alternating beads counter-rotate:

```clojure
(control (ticks 5)
         (where (* (= b.family :keine) (> b.t 1)))
         (fn [b]
           (let [th (angle-of b.vel)
                 r  (+ 1 (* (mag b.vel) b.t))   ; spawned at radius 1
                 tx (- b.pos.x (* r (cos th)))  ; the volley origin
                 ty (- b.pos.y (* r (sin th)))]
             (seq
               (bullet ((pose c[tx ty])
                        (map (fn [k]
                               ((rot (lerpsmooth eiosine 0 3 t
                                       th
                                       (+ th (* (- 1 (* 2 (mod k 2))) 33.75))))
                                 (pose c[(- r (* 0.2 k)) 0])))
                             (iota 13))) {:style {:family :gcircle
                               :color [:blue :purple]
                               :variant :w}
                       :cols {:k (iota 13) :ang (+ th 33.75)}})
               (cull b)))))
```

Three things to notice:

- The bead's motion is pure tutorial-1 vocabulary: a position inside a
  rotation rotates, and here the rotation's angle is a *signal* —
  `(lerpsmooth eiosine 0 3 t from to)` eases from `from` to `to` as the
  bead's age runs 0→3s.
- `(mod k 2)` partitions the beads: even beads swing +33.75°, odd beads
  −33.75°, and the two-color palette binds to the same axis, so the
  partition is visible. 33.75 = 1.5 × 360/16 — chosen so the sixteen
  rays interleave and *re-align* exactly when the ease settles.
- Each bead carries two columns: its index `k` and its settled angle
  `ang` — act 2 reads both.

**Act 2 — regenerate.** When the rotation settles, the *head* bead of
each ray re-fires a long bullet along its settled angle, and all beads
cull:

```clojure
(control (ticks 5)
         (where (* (= b.family :gcircle) (> b.t 3.2) (< (default b.k 1) 0.5)))
         (fn [b]
           ((pose (pos b))
             ((rot b.ang)
               (bullet (linear p[2 0]) {:style {:family :keine :color :purple :variant :w}})))))
(control (ticks 5)
         (where (* (= b.family :gcircle) (> b.t 3.2)))
         (fn [b] (cull b)))
```

`:k` only exists on beads, and every factor of `*` evaluates on every
live bullet — a bare `b.k` would error on the long bullets. `default`
gives the missing field a harmless default (1 is not `< 0.5`).

The re-fired bullet is `:family :keine`, so **act 1's control catches it
again**: the pattern rebuilds itself outward, generation by generation —
that's the chimera. Each generation starts ~2 units further out, so
chains walk off the field and die naturally; the weave keeps seeding new
ones.

## The abstraction ladder

The order things earned names in this card is the general rule:

1. **Inline** until something repeats.
2. **`defn`** for repeated *values* — and almost everything is a value
   here: frames, motions, whole action trees.
3. **`defpattern` parameters** for reusable, tunable spells — each
   invocation gets private local-stream state, so a pattern used twice
   never trips over itself.
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
