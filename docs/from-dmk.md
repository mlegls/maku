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
| `flipygt(4, _)` | `(remat b (linear c[b.vel.x (- 0 b.vel.y)]))` — the reflection written out |
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
| `bindLR` (lr/rl = ±1 by loop parity) | `(- 1 (* 2 (mod k 2)))` |
| `lerpsmooth($(eiosine), a, b, t, v1, v2)` | `(lerpsmooth eiosine a b t v1 v2)` — easings are values, no lambda-conversion syntax |
| one-shot `sm` control with predicate re-summoning | a control whose spawn re-enters an earlier control's query — self-rebuilding patterns |
| `move(inf, nroffset(px(sine(8p, 2, t))))` | `(in-frame (cart m"2*sine(8, 1, t)" 0) …)` — a moving frame |
| `sm` control running an async | `fork` inside the callback (adopted as a child task) |
| C#-defined reusable functions | `defn` (card-level, first-class values) |
| `pattern`-level reuse / script includes | parameterized `defpattern` + `(import …)` |
| — (no user macros in BDSL) | `defmacro` with backtick templates |

## Pathers, lasers, subfiring (Tutorial 4)

| DMK | here |
|---|---|
| `pather(maxRemember, rememberFn, path, opts)` | `(pather window dyn)` — the trail is the hitbox |
| `laser(path, cold, hot, opts)` | `(laser shape? {:warn cold :active hot :u-max len …})` |
| `straight(angle)` / `rotate(base, fn)` options | frame rotation: `((rot angle) laser)` / `((rot m"…t…") laser)` |
| `static(path)` / `dynamic(path)` with `lt` | one shape slot over `(t, u)`: u-only = static, t entering = dynamic |
| `s(width)` / `stagger(x)` options | `:width` (also scales the hitbox) / `:resolution` |
| `varLength` | signal-valued `:u-max` (grows the beam AND hitbox together) |
| slow/filling lasers | `:fill d` — full path telegraphs, the hitbox sweeps from the source |
| `hueshift(x*t)` option | signal-valued `:hue` tag |
| `scale(fn)` / `dir(fn)` / `opacity(fn)` SB options | signal-valued `:scale` / `:facing` / `:opacity` tags (`:scale` scales colliders too) |
| `sm` option (SM at the tip) | `let`-bound guide + `(in-frame guide (fork …))` |
| `onlaser(fn)` modifier | `(on-laser h u)` — pose with tangent heading at u |

## Difficulty and the host boundary (Tutorial 5)

| DMK | here |
|---|---|
| `dl` / `dn` / `dh` (ratio to a reference difficulty) | `$rank` — one channel, scale with `*` or soften with `pow` |
| `dc` (difficulty counter) | additive expressions over `$rank` |
| difficulty enum + reload keys (T/Y/U/I) | the host injects `$rank`; the sandbox binds T/Y/U/I |
| `target(ang, Lplayer)` | `((aim $player) …)` — a frame operation |
| `Lplayer` / engine-privileged player | the player is card content: an entity with `:pilot`, deriving `$player` |
| exposing boss state to UI | `:expose {:col $chan}` on the entity; `(export cell)` for card state |

## Bosses, phases, script structure (Tutorial 7)

| DMK | here |
|---|---|
| `pattern { } { phase … phase … }` | `(phases (:label opts? body… (finally …)?) …)` — ordered labeled clauses |
| `phase X { type(spell, "Name") hp(4000) root(0,2) }` | clause opts: `{:name "Name" :type :spell :timeout X :until (<= $boss-hp n) :root c[0 2]}` |
| phase timeout / hp race | the clause's implicit race: `:timeout` + `:until` + goto, guarding the body |
| `shiftphaseto(N)` (index) | `(goto :label)` — labels survive phase insertion; scoped to the innermost machine |
| the zeroeth setup phase convention | a routing clause: `(:opening (goto :spell1))` — or nothing; setup is just code before `phases` |
| phase-end task cancellation (token propagation) | the phase guard cancels the body's whole task subtree (`until` semantics) |
| end-of-phase item drops / cleanup | the clause's `finally` block — runs on every exit path |
| `vulnerable(false)` phase property | `(invuln boss dur)` in the previous phase's `finally` — a column both resolve paths honor |
| player invulnerability after death | automatic `iframe-until`; window duration = the `:iframes` column |
| boss HP bar / phase name on the HUD | `:expose {:hp $boss-hp}` on the boss entity; the machine exports `$phase` |

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
