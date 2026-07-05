# Tutorial 1: First Bullets

Runnable companion: **`cards/tutorials/t01.dmk`** — every example below is a
pattern in that card. Start the player and press the number keys (or click
the menu in the web host) to run each one:

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t01.dmk
```

The sequence of ideas follows [DMK's Tutorial 1](https://dmk.bagoum.com/docs/articles/t01.html)
(MIT, © Bagoum), re-taught for this language; where DMK introduces a
special mechanism, we show what it dissolves into here.

## Part 1: One bullet

```clojure
(spawn ((pose c[2 0]) (linear p[2 -90]))
       {:style {:family :fireball :color :red :variant :w}})
```

Reading outside-in:

- `(spawn dyn meta)` is an **action**: it expresses bullets whose motion is
  the *dyn* (a pose-valued signal) and whose appearance is the *meta*.
- `(pose c[2 0])` is a frame 2 units to the right of wherever the pattern
  is anchored. A frame followed by a child **applies** — `((pose …) child)`
  runs the child inside that frame.
- `(linear p[2 -90])` is straight-line motion. `p[r θ]` is a polar literal:
  speed 2, heading −90° (straight down). Angles are degrees everywhere.
- The style record `{:family … :color … :variant …}` picks the sprite. It
  is structured data, not a string — this matters in Part 4.

Polar and cartesian literals are two spellings of the same value:
`p[2 -90]` = `c[0 -2]`, so `ex2` fires the identical bullet with
`(linear c[0 -2])`.

**Try it:** make the bullet go left; make it go up, but faster. (Edit the
card and press `r` to reload — or, with the nvim client, put the cursor on
the pattern and re-run it live.)

## Part 2: Offsets that rotate — and offsets that don't

Danmaku patterns want rotational symmetry: "spawn at (2, 0), *rotated by
θ*." DMK dedicates a five-number type to this (V2RV2: a non-rotating pair,
a rotating pair, and an angle). Here the distinction needs no type at all —
**it is the position of the offset relative to the rotation in the tree**:

```clojure
;; offset INSIDE the rot — the offset turns with it: spawns at (0, 2)
((rot 90) ((pose c[2 0]) (linear p[2 -90])))

;; offset OUTSIDE the rot — position fixed at (2, 0); only the child
;; (the bullet's heading) turns
((pose c[2 0]) ((rot 90) (linear p[2 -90])))
```

Run `ex3` to see both at once (green vs blue). Everything V2RV2 encodes is
expressible by placing `pose`/`+` inside or outside `rot` frames, and the
"angle" slot is just the `rot` itself. Frames compose associatively, so
deep hierarchies are ordinary nesting — no dedicated offset algebra to
memorize.

## Part 3: Thirty bullets

A ring is not a loop — it's arithmetic. `(iota 30)` is the index array
`[0 1 … 29]`, arithmetic broadcasts over arrays, and a frame constructor
applied to an array of angles is an array of frames:

```clojure
(spawn ((rot m"10 * iota(30)")
         ((pose c[1 0]) (linear p[2 0]))))
```

Thirty rotation frames × one child = thirty bullets (`ex4`). The
`m"…"` reader macro is infix math for exactly this kind of expression; it
parses to the same tree as `(* 10 (iota 30))`.

Two stock formations cover the common cases (`ex5`, `ex6`):

- `(circle 30 child)` — evenly around the full circle.
- `(fan 30 6 child)` — centered fan, 6° between bullets.

These aren't engine primitives — `circle n` is just a θ column
`(iota n) × 360/n` worn as frames.

## Part 4: Nesting, and coloring the layers

Repeat-within-repeat is frame-within-frame. Ten groups of three (`ex7`):

```clojure
(spawn ((rot m"30 * iota(10)")        ; 10 group headings, 30° apart
         ((rot m"4 * iota(3)")        ; 3 bullets per group, 4° apart
           ((pose c[1 0]) (linear p[2 0])))))
```

Multiplicity is the product of array sizes along the root-to-leaf path —
10 × 3 = 30 — statically readable off the tree.

Now color each *group*. Meta arrays bind to the **leading axis** (the
outermost array — here the 10 groups) and shorter arrays **cycle**
(`ex8`):

```clojure
{:style {:family :arrow
         :color [:red :blue :green]   ; per group: r b g r b g …
         :variant :w}}
```

DMK does this with string wildcards resolved in order (`"arrow-*/w"` plus
a `color` modifier). Because our style is a structured record, there is
nothing to splice — you assign the axis you mean.

To target a *deeper* axis, write that axis's length explicitly — a
3-vector binds to the inner axis of 3 by length (`ex9`):

```clojure
{:style {:family :arrow
         :color [:red :blue :green]              ; leading axis: the 10 groups
         :variant (nth [:w :x :b] (iota 3))}}    ; inner axis: 3 per group
```

(`nth` is cyclic, so `(nth xs (iota n))` is "an n-vector cycling through
xs" — the idiom for both palettes and axis targeting.)

**Try it:** swap which property binds to which axis; make a 6-color
palette cycle over the 10 groups and predict which groups repeat colors.

## What DMK teaches here vs what you need here

| DMK Tutorial 1 concept | here |
|---|---|
| `CreateShot2(x, y, speed, angle, style)` | `(spawn ((pose c[x y]) (linear p[speed angle])) {:style …})` |
| V2RV2 `<nx;ny:rx;ry:θ>` | position of `pose`/`+` inside vs outside `rot` frames |
| `s(rvelocity(cr(2, -90)))` | `(linear p[2 -90])` |
| `cr(r, θ)` / `cxy(x, y)` | `p[r θ]` / `c[x y]` literals |
| `gsrepeat { times(30) rv2incr(<10>) }` | `((rot m"10 * iota(30)") child)` — broadcasting |
| `circle` / `spread` modifiers | `(circle n child)` / `(fan n step child)` |
| nested `gsrepeat` | nested frames; multiplicity = product of axis lengths |
| `color({…})` + `*` wildcards in style strings | arrays in the structured style record; leading-axis binding + cyclic `nth` for deeper axes |

Next: [Tutorial 2](02-firing-over-time.md) — firing over time (waits,
repeaters with periods, and the difference between simultaneous and
sequential fan-out).
