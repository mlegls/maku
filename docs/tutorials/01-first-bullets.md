# Tutorial 1: First Bullets

Runnable companion: **`cards/tutorials/t01.maku`** — every example below is a
pattern in that card. Start the player and press the number keys (or click
the menu in the web host) to run each one:

```sh
cargo run --release --manifest-path crates/Cargo.toml -p maku-player -- cards/tutorials/t01.maku
```

## One bullet

The examples import `"touhou"` for its bullet templates: `bullet`
wraps the bare `spawn` primitive with the hostile-bullet defaults.
Use the canonical `bullet`, `shot`, `enemy`, `boss`, and `player` templates.
<!-- compatibility-migration -->
Pre-release `spawn-bullet` and related `spawn-*` aliases are no longer part of
the library API.

```clojure
(import "touhou")
(bullet ((pose c[2 0]) (linear p[2 -90])) {:style {:family :fireball :color :red :variant :w}})
```

Reading outside-in:

- `(bullet dyn meta)` creates bullets. The first argument describes motion;
  the second describes appearance.
- `(pose c[2 0])` is a *frame* — a position (and orientation) that things
  happen inside of; here, 2 units right of wherever the pattern is
  anchored. A frame followed by a child **applies**: `((pose …) child)`
  runs the child inside that frame.
- `(linear p[2 -90])` is straight-line motion. `p[r θ]` is a polar
  coordinate literal: speed 2, heading −90° (straight down). Angles are in
  degrees everywhere; time is in seconds.
- The style record carries semantic family, color, and variant axes. The
  host-selected Touhou profile maps those axes to sprite resources and
  orientation; core does not own a palette or sprite table.

Polar and cartesian literals are two spellings of the same value:
`p[2 -90]` equals `c[0 -2]`, so `ex2` fires the identical bullet with
`(linear c[0 -2])`.

**Try it:** make the bullet go left; make it go up, but faster. Edit the
card and press `r` to reload — or, with the nvim client, put the cursor on
a pattern and re-run it live.

## Offsets that rotate — and offsets that don't

Danmaku patterns are full of rotational symmetry: "spawn at (2, 0),
rotated by θ." Whether an offset rotates is decided by *where you write
it* relative to the rotation:

```clojure
;; offset INSIDE the rot — the offset turns with it: spawns at (0, 2)
((rot 90) ((pose c[2 0]) (linear p[2 -90])))

;; offset OUTSIDE the rot — position stays at (2, 0); only the child
;; (the bullet's heading) turns
((pose c[2 0]) ((rot 90) (linear p[2 -90])))
```

Run `ex3` to see both at once (green vs blue). Frames compose
associatively, so hierarchies of any depth are ordinary nesting — there
is no separate offset system to learn: position inside a rotation
rotates, position outside doesn't.

## Thirty bullets

A ring is not a loop — it's arithmetic. `(iota 30)` is the index array
`[0 1 … 29]`, arithmetic broadcasts over arrays, and a frame constructor
applied to an array of angles gives an array of frames:

```clojure
(bullet ((rot m"12 * iota(30)") ((pose c[1 0]) (linear p[2 0]))) {:style {:family :arrow :color :red :variant :w}})
```

Thirty rotation frames × one child = thirty bullets (`ex4`). The `m"…"`
form is infix math shorthand; it parses to the same tree as
`(* 12 (iota 30))` — use whichever reads better.

Two stock formations cover the common cases (`ex5`, `ex6`):

- `(circle 30 child)` — evenly around the full circle.
- `(fan 30 6 child)` — a centered fan, 6° between bullets.

These aren't special: `circle n` is just a θ column `(iota n) × 360/n`
worn as frames.

## Nesting, and styling the layers

Repeat-within-repeat is frame-within-frame. Ten groups of three (`ex7`):

```clojure
(bullet ((rot m"36 * iota(10)")        ; 10 group headings, 36° apart
         ((rot m"4 * iota(3)")        ; 3 bullets per group, 4° apart
           ((pose c[1 0]) (linear p[2 0])))) {:style {:family :arrow :color :red :variant :w}})
```

The bullet count is the product of the array sizes along the path —
10 × 3 = 30 — readable straight off the code.

To color each *group*, put an array in the style. Arrays in meta bind to
the **leading axis** (the outermost array — here the 10 groups), and
shorter arrays **cycle** (`ex8`):

```clojure
{:style {:family :arrow
         :color [:red :blue :green]   ; per group: r b g r b g …
         :variant :w}}
```

To target a *deeper* axis, write that axis's length explicitly — a
3-vector binds to the inner axis of 3 by length (`ex9`):

```clojure
{:style {:family :arrow
         :color [:red :blue :green]              ; leading axis: the 10 groups
         :variant (nth [:w :x :b] (iota 3))}}    ; inner axis: 3 per group
```

(`nth` is cyclic — `(nth xs (iota n))` means "an n-vector cycling through
xs", the standard idiom for palettes and axis targeting.)

Meta values can also be **matrices**: a *nested* array resolves
structurally — depth in the value corresponds to axis in the spawn,
cycling at every level, and a scalar reached early applies to everything
deeper (`ex10`):

```clojure
{:style {:family :arrow
         :color [[:red :blue] :green :purple]}}
;; group 0: red, blue, red   (inner array cycles the inner axis)
;; group 1: all green        (scalar covers its whole group)
;; group 2: all purple
;; group 3: wraps to [red blue] …
```

Rule of thumb: flat arrays target an axis by length; nested arrays follow
the axis tree by shape.

**Try it:** swap which property binds to which axis; make a 6-color
palette cycle over the 10 groups and predict which groups repeat colors.

Next: [Tutorial 2](02-bullet-controls.md) — selecting and transforming
bullets in flight.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
