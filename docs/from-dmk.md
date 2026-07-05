# Coming from Danmokou / BDSL

A mapping guide for readers who know [Danmokou](https://dmk.bagoum.com/)
(MIT, © Bagoum). This engine's design started from a close reading of
DMK's semantics — much of the vocabulary corresponds directly, expressed
through a different composition model (s-expressions, signals, and array
broadcasting rather than repeater state machines). The tables below map
DMK constructs to their equivalents here, in the order the tutorials
introduce them; `docs/language.md` covers the underlying model, and
`cards/translations/` contains full ports of DMK scripts with per-line
decode notes.

## Firing (Tutorial 1)

| DMK | here |
|---|---|
| `CreateShot2(x, y, speed, angle, style)` | `(spawn ((pose c[x y]) (linear p[speed angle])) {:style …})` |
| `sync(style, rv2, s(...))` | `(spawn dyn {:style …})` |
| V2RV2 `<nx;ny:rx;ry:θ>` | position of `pose`/`+` written inside vs outside `rot` frames; the θ slot is the `rot` itself |
| `rvelocity(v)` / `nrvelocity(v)` | `(linear v)` inside the rotation / outside it |
| `cr(r, θ)` / `cxy(x, y)` / `px(x)` | `p[r θ]` / `c[x y]` literals |
| `gsrepeat { times(30) rv2incr(<10>) }` | `((rot m"10 * iota(30)") child)` — arrays broadcast |
| `circle` / `spread(<rv2>)` modifiers | `(circle n child)` / `(fan n step child)` |
| nested `gsrepeat` | nested frames; bullet count = product of axis lengths |
| `color({ … })` + `*` wildcards in style strings | arrays in the structured style record; axes target by length, by cyclic `nth`, or by nested matrices |
| style strings `"arrow-red/w"` | records `{:family :arrow :color :red :variant :w}` |

## Bullet controls (Tutorial 2)

| DMK | here |
|---|---|
| `bulletcontrol(persist, sel, ctrl)` | `(fork (for [i inf :every (ticks 1)] (manip sel ctrl)))` |
| persistence predicate / `newtimer()` | loop bounds, or `(until pred …)` |
| `poolcontrol(sel, reset)` | cancel the control's scope |
| style selector `{{ "circle-*" }, { "red/w" }}` | query map `{:family :circle :color :red}`; arrays mean any-of |
| `flipygt(4, _)` | `(remat b (fn [exit] …))` with the reflected velocity written out |
| `cull(y < -2)` | `:where (fn [b] (< b.pos.y -2))` + `(cull b)` |
| `batch(pred, { … })` | a `seq` in the callback |
| `restyleeffect(style, fx, _)` | `(set-style b {...})`, plus an effect spawn if wanted |
| `sm(_, sync …)` | spawn directly in the callback, anchored by `((pose (pos b)) …)` |
| `softcull(fx, _)` | effect spawn + `(cull b)` |
| enforced control ordering (SM before cull) | explicit `seq` order; post-cull verbs are dead-handle no-ops |

## Spell assembly (Tutorial 3)

| DMK | here |
|---|---|
| `async … gcrepeat { wait(60) times(inf) }` | `(for [i inf :every t] …)` |
| `paction` / `saction` | `(par …)` / `(seq …)` |
| `gtrepeat { … waitchild }` | `for` + `fork` for children the loop shouldn't wait on |
| `lerpt(a, b, from, to)` in movement | `(lerp a b t from to)` in a `vel`/signal slot |
| `preloop b{ hvar colorIndex = i }` | `:cols {:ci (iota n)}` — column arrays bind per element |
| `&colorIndex` in a control | `b.ci` on the bullet view |
| `start b{ hvar rootloc = loc }` | recover from the view (`tail = pos − vel·t`) or save a column |
| `colorf(list, &i)` | `(nth palette i)` |
| `move(inf, nroffset(px(sine(8p, 2, t))))` | `(in-frame (cart m"2*sine(8, 1, t)" 0) …)` — a moving frame |
| `sm` control running an async | `fork` inside the callback (adopted as a child task) |

## Model-level notes

- **Repeaters vs arrays.** DMK expresses multiplicity through repeater
  state machines with modifiers; here simultaneous multiplicity is array
  broadcasting (frames × children) and only *sequential* firing uses a
  loop (`for`). If a `gsrepeat` has no waits, it's an array here.
- **Movement functions vs signals.** Both engines treat motion as pure
  functions of a bullet-local clock. Here that's typed: closed-form
  motion (evaluable at any `t`) and integrated motion (`vel`/`acc`,
  stepped each tick) are distinguished statically, which is what enables
  rewind/scrubbing in the tooling.
- **Per-bullet mutation vs remat.** Where DMK controls often mutate
  bullet state per frame (flip flags, hvars), the equivalent here is
  usually a one-shot `remat` — snap state, swap motion — or a column
  written at spawn.
- **Style strings vs records.** DMK's pool product lives in the style
  string (`"arrow-red/w"` + wildcards); here it's a record with axes,
  so selection and recoloring are structural operations.
- The full translation corpus (`cards/translations/`) ports several DMK
  scripts — including a production boss spell — with notes on each
  decoded idiom, and runs under the conformance suite.
