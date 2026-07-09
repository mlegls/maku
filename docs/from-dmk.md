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

The old `spawn-*` macro names remain as aliases.

| DMK | here |
|---|---|
| `CreateShot2(x, y, speed, angle, style)` | `(bullet ((pose c[x y]) (linear p[speed angle])) {:style …})` |
| `sync(style, rv2, s(...))` | `(bullet dyn {:style …})` |
| engine-owned pools / player / BEH prefabs | `(import "touhou")` — the genre layer is a library card (spawn templates, `invuln`, the stock rig) over a bare `spawn` of dyn + explicit meta |
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
| `cull(y < -2)` | `(manip (fn [b] (< b.pos.y -2)) (fn [b] (cull b)))` |
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
| slow/filling lasers | `:fill (fill-linear warn d)` — full path telegraphs, the hitbox sweeps from the source |
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
| exposing boss state to UI | `(defchannel $boss {...})` plus `(bind-channel! $boss expr)` for structured state; `:expose {$chan :col}` is sugar for a single numeric column |

## Design philosophy (Tutorial 6)

DMK's t06 is a manifesto, not a mechanics tutorial — it introduces no
constructs. Its three arguments map at the level of principles, and each
lands somewhere specific here:

| DMK's pitch | here |
|---|---|
| movement is function-based: drop `3 + sine(4, 0.6, t)` into any velocity slot instead of falling back to manual update loops | the signal model *is* the language: every dyn/meta slot takes an expression over `t`/`u`, and closed-vs-integrated is typed (which DMK doesn't do — that's what buys scrubbing) |
| the showcase: 4 spiral bullets, `gsrepeat({ times(4), circle }, s(polar(2*t, 80*t)))` vs pages of LuaSTG/DNH update-loop code | `(bullet (circle 4 (polar m"2*t" m"80*t")))` — see `cards/translations/060_polar.maku` for a production-strength version of the same shot |
| modifiers/options (`scale`, `dir`, `opacity` on `simple`) extend entities without breaking signatures | meta maps merged per-key + render-signal tags (`:scale` `:facing` `:opacity` `:hue`); colliders, columns, triggers are the same open-map story |
| extensible by "writing a C# function and putting it somewhere" — engine-language extension, engine rebuild | extension is *in-language*: `defn`/`defmacro`/lib cards. The whole touhou genre layer (spawn templates, contact rules, phases) is card code over a bare engine — a user card extends the vocabulary with no engine involvement at all |

The divergence worth noting is the last row: DMK's extensibility story
bottoms out in its host language, ours bottoms out in the card language
itself (the engine keeps only detection, scheduling, and signals). t06
needs no tutorial port.

## Bosses, phases, script structure (Tutorial 7)

| DMK | here |
|---|---|
| `pattern { } { phase … phase … }` | `(phases (:label opts? body… (finally …)?) …)` — a `(import "touhou")` macro over the `states` FSM primitive |
| the boss BEH entity | `(boss $boss dyn meta machine…)` — library macro: an *enemy with a phase machine*. It owns the boss conventions: binds a map-valued `$boss` channel, holds the machine until the boss registers, binds `boss`/`boss-main` for the machine body; hp/hurtbox/triggers are ordinary meta |
| `hp(4000)` phase property | `{:hp n}` — desugars to `(until (<= (hp-of boss-main) n) body)`, reading the local boss handle; `{:until pred}` is the general gate |
| phase timeout `phase X {…}` | `{:timeout X}` — desugars to `(fork (seq (wait X) (goto)))`; bare goto = exit to successor |
| `root(0, 2)` phase property | `{:root c[0 2]}` — desugars to `(move-to boss-main …)` at the body head; the card knows its boss |
| `type(spell, "Name")` | card data: an exported cell written at state heads (or a card-level template macro) |
| `shiftphaseto(N)` (index) | `(goto :label)` — labels survive phase insertion; scoped to the innermost machine; labels are values, so routing may be computed (Markov chains) |
| the zeroeth setup phase convention | a routing state: `(:opening (goto :spell1))` — or nothing; setup is just code before the machine |
| phase-end task cancellation (token propagation) | the phase guard cancels the body's whole task subtree (`until` semantics) |
| end-of-phase item drops / cleanup | the clause's `finally` block — runs on every exit path |
| `vulnerable(false)` phase property | `(invuln boss dur)` in the previous phase's `finally` — a column both resolve paths honor |
| player invulnerability after death | automatic `iframe-until`; window duration = the `:iframes` column |
| boss HP bar / phase name on the HUD | `boss` binds a structured boss channel such as `$mima`; richer phase metadata is card-level state over `bind-channel!` |

## Firing index and empty-guided fires (Tutorial 8)

| DMK | here |
|---|---|
| `p this` / `preloop b{ hvar itr = i }` | an ordinary binder: `(map (fn [k] …) (iota n))` per bullet, `dotimes` seq bindings per volley, `:cols` for indices the bullet carries |
| `p add` / `p mod` / `p invmod`, unpacking via `p1`/`p2`/`pm` | nothing — nested indices are separate named variables; there is no packing because the index never leaves scope |
| `bindArrow` (`axd`/`ayd`/`aixd`/`aiyd` autovars) | a formation is a function: four lines of card code mapping index → offset frame (tutorial 7) |
| `guideempty2(p, {("eloc", code(loc)), ("edir", code(dir))}, path, children)` | a level of the frame tree: `((vel path) children…)` — the guide is an unexpressed dyn, nothing spawns |
| `dtpoffset("eloc", "edir", p, offset)` = `eloc + rotatev(edir, offset)` | frame composition — every pose carries a heading, children rotate with it automatically |
| `p` as the unique child-to-guide key | lexical nesting — the association is structural |
| `dir2(load("edir", p))` bullet option | the rendered facing already follows the composed frame heading; `:facing` overrides when wanted |
| empty bullets (invisible, bomb-immune, hard-destroyed on clear) | spawn a guide entity only when you want it *seen*: `:team :scenery`, no colliders |
| SM riding the empty / shared guide | `let`-bound guide = the shared instance; `(spawn guide …)` expresses it, `(in-frame guide (fork …))` rides it |

## Repeater modifiers (Tutorial 9)

DMK's t09 is a reference page over the `GenCtxProperty` repeater
modifiers. There is no repeater here, so this table is the whole port:
each modifier exists to parametrize repeater state (rv2, loop counters,
timers), and maps to ordinary code — most rows are idioms the earlier
tutorials already teach.

| DMK | here |
|---|---|
| `bank <off>` / `bank0` ("inner repeats": shift rv2 into nonrotational coords) | frames nest — a child group composes around its parent's origin natively; rotational-vs-not is the inside/outside-`rot` choice (the V2RV2 row, Tutorial 1) |
| `bindArrow` / `bindLR` / `bindUD` | formation fns and `[1 -1]` seq bindings (Tutorial 8 table above) |
| `cancel(pred)` — checked every iteration | `(until pred loop)` — cancellation kills the loop's scope |
| `clip(pred)` — checked once at entry | `(when pred …)` |
| `whiletrue(pred)` — pause stepping while false | `(wait-for pred)` in the loop body |
| `unpause(sm)` — run on the resume edge | the code after `(wait-for pred)` runs exactly at resume |
| `rv2incr(<θ>)` | `(rot m"θ * iota(n)")` — arrays broadcast |
| `spread(<Θ>)` — endpoints inclusive | `(fan n step …)`, step = Θ/(n−1) |
| `circle` | `(circle n …)` |
| `color({…})` + wildcards | style-record arrays; axes target structurally (Tutorial 1) |
| `colorr` — reverse-direction merge | dissolves: records merge per-key, there is no string direction |
| `colorf(list, idx)` | `(nth palette expr)` |
| `delay(f)` | `(wait d)` before the loop |
| `wait(f)` | `:every` |
| `waitchild` | `for` waits its body by default; `fork` opts out |
| `root(pos)` | an explicit frame: `(in-frame :world ((pose at) …))` |
| `rootadjust` — root override compensating rv2 | nothing to compensate; positions compose explicitly |
| `start b{…}` / `preloop` / `postloop` / `end` | `let` at the head / per-iteration bindings (`dotimes` seq bindings, fn params) / fold state (`loop`/`recur`) / `finally` |
| `face(original\|velocity\|derot\|rotator)` | rendered facing follows the composed frame heading; `:facing` overrides (Tutorial 4) |
| `times(n)` / `maxtimes(n)` | loop bounds and array lengths; `maxtimes` dissolves with `p` packing (Tutorial 8 table) |
| `fortime(d)` — frame cap racing the count cap | `(race (wait d) (for [i n] …))` |
| `frv2(fn-of-i)` — offset as a function of iteration | offsets *are* expressions of the index (BoWaP: `cards/translations/130_bowap.maku`) |
| `noop` | — |
| `onlaser` | Tutorial 4 table |
| `p` / parametrization | Tutorial 8 table |
| `saoffset(SAAngle, θ, eq)` — summon along an equation | the offset is an expression; the `SAAngle` enum is where you place `rot`/`pose` in the tree — `banktangent` is a closed path's frame heading, free |
| `sequential` — children in sequence instead of parallel | `seq` vs `par`, always explicit |
| `sfx` / `sfxf` / `sfxif` | `(event :sfx …)`; index with `nth`, gate with `when` |
| `target(mode, Lplayer)` / `sltarget` | `((aim $player) …)` at whichever tree level you mean; the six control modes are arithmetic on explicit offsets (laser grids: the grid helpers in `cards/translations/ph_boss2_spell2.maku`) |
| `timer` / `newtimer()` — shared, resettable clocks | capture an epoch and read the world clock live: `(let [t0 $tick] … m"(live($tick) - t0)/120" …)`; resettable = the epoch in a cell, reset by `set!` |
| `timereset` + `st` (summoning time) | `st` is fixed at spawn — pass the emitting loop's elapsed time in as a binding; resetting is rebinding |

Porting this table doubled as the §13.1 *ancestor clocks* audit, and
closed it: a parent's clock is an ordinary value (`$tick` captured into
a binding), children read `(live $tick)` against it, and phase-locking
(ring-vs-spiral) is every bullet reading the same live clock instead of
its own `t`. No engine operator is needed — sugar naming the idiom, if
it ever feels heavy, is lib code. The one engine change the audit
forced was a bugfix, not a feature: `(live …)` reads now count as
time-dependence, so wall-clock-driven closed signals defer instead of
silently constant-folding at spawn.

## Boss configuration (Tutorial 10 / `tbosses`)

DMK's `tbosses` is mostly a Unity host tutorial: `BossConfig` and
`BossColorScheme` ScriptableObjects, sidebar portraits, spell stars,
cutins, background transitions, practice-selector registration, bottom
trackers, and localization strings. Those are not core-language
features here. The core publishes structured state; a host decides how
to render it.

| DMK | here |
|---|---|
| `boss("key")` pattern property | host/content metadata associated with the card or encounter; the core card publishes whatever state the host needs |
| `BossConfig.Key` / `GameUniqueReferences` registration | host asset registry / practice menu data, outside the sim core |
| boss names, replay names, tracker names, localization base keys | host metadata; cards may publish current phase/card ids, but text lookup is a host/content concern |
| `BossColorScheme` | host theme data; renderable entities still carry structured style records and signal tags (`:hue`, `:opacity`, etc.) |
| boss portraits, sidebars, hexagram overlays, spell-circle effects | host/UI/render-layer effects driven by boss channel state |
| default nonspell/spell backgrounds and transitions | host scene/background policy, usually keyed from published phase type |
| spell cutins and boss cutins | host timeline effects; card code can emit events such as `(event :spell)` or publish `{:type :spell}` |
| secondary HP display / bottom tracker / spell stars | host reads a map-valued boss channel such as `$mima` from `(boss $mima dyn meta …)` |
| `phaset` special timer | explicit phase-local state: use bullet-local `t`, or capture an epoch from `$tick` when a whole phase needs a shared clock |
| `type(non/spell, "Name")` as phase config | card-level phase metadata, commonly folded into a structured boss channel with `bind-channel!` |

The key design difference is direction of ownership. DMK uses a boss key
to pull a large bundle of Unity assets into the script. Here, the script
publishes a small, typed surface and the host chooses presentation:

```clojure
(defchannel $mima {:hp 0 :phase :none})

(boss $mima (live $boss) {:hp 100 ...}
  (phases
    (:nonspell {:hp 60} ...)
    (:spell2 {:hp 0} ...)))
```

`boss` binds `$mima` as structured boss state and `phases` gates on
the local `boss-main` handle, not a global hp channel. If a game wants
phase names, spell indices, capture timers, or active-boss ids, those are
ordinary cells/events folded into the same map with `bind-channel!` or a
small boss-template macro.

Multiple bosses follow from the same rule. The sim can spawn any number
of enemies and boss-like phase machines; the host still needs a policy
for which one is "the UI boss". DMK's legal multi-boss cases map as:

| DMK multi-boss case | here |
|---|---|
| one main boss plus invincible supports | spawn support entities without hurt colliders or death triggers; publish only the main boss channel |
| subbosses sharing one health pool via `diverthp` | expose/bind one shared hp value, or have shot contact write one designated entity's hp |
| subbosses with separate hp pools | supported at the sim level by separate entities/channels; host UI decides whether and how to display multiple bars |
| changing which boss drives UI by phase index | publish an active boss id/channel in phase code; host follows that value |

There is no standalone tutorial port for `tbosses`: the engine-facing
mechanics are already covered by Tutorial 6 (`boss`, `phases`,
channels), and the remaining material belongs in the first concrete host
integration guide.

## Stages and campaigns (Tutorial 11 / `tstages`)

Stage scripts are worth a runnable port because their engine-facing
semantics are card timelines: timed sections, enemy summons, boss
handoffs, announce/dialogue handoffs, and cleanup. Campaign construction
is host metadata and is not a core-language feature.

| DMK | here |
|---|---|
| LevelManager stage script | ordinary `defpattern`; the host runs it as a stage |
| `phase 8 { stage } { ... }` | scoped action: `(until timeout body...)` plus `(finally ... cleanup ...)`; host labels it as a stage practice segment if desired |
| stage phase timeout clearing bullets | explicit phase-edge policy: `(finally body (cull) (event :stage-section-clear))` |
| `stage` / `midboss` / `endboss` / `announce` / `dialogue` phase properties | published stage state and events, e.g. `(bind-channel! $stage {:section :dialogue})` and `(event :dialogue)`; timer freezing/UI visibility are host policy |
| stage script firing from LevelManager origin | direct `bullet` from the pattern's ambient frame |
| `summonr(root, saction, { hp ... })` | `(enemy ((pose root) dyn) meta)` returning handles; per-enemy `fork`ed timelines use `(pos e)`, `invuln`, `set-col`, and `cull` |
| enemy entrance movement + firing script | piecewise dyns such as `(stages (stage enter ...) (stage hover ...) (forever exit ...))` plus control-layer `seq`/`for` |
| `vulnerableafter` | `(invuln e dur)` or direct `:iframe-until` column writes |
| `boss "tutorial"` in a stage | host-selected boss card/pattern, or direct `(boss $boss dyn meta (phases ...))` inside the stage pattern |
| boss config's `State Machine` field | host registry mapping boss/stage ids to card patterns |
| `stageannounce` / `stagedeannounce` | host-facing events such as `(event :stage-announce)` / `(event :stage-end)` |
| `executevn ExampleVNScript "log-key"` | host handoff: emit an event or set a channel, then wait on a host-provided completion signal if needed |
| `GameDefinition` / `CampaignConfig` / `StageConfig` / `SceneConfig` | host campaign data: ordered stage patterns, scene/background ids, player choices, practice entries, endings |
| `CampaignConfig` stage order | host controller that runs patterns in sequence; nonlinear routing is just host code choosing the next pattern |
| `EndingConfig` predicates | host-side predicates over recorded campaign state and card events |
| practice unlocks based on campaign completion | host save/unlock policy; core can expose checkpoints/section ids but does not own menus |

The runnable port is `cards/tutorials/t08.maku` with
`docs/tutorials/08-stages-and-campaigns.md`. It demonstrates the core
half of `tstages`: a timed stage section, a fairy wave, a midboss
handoff, announce/dialogue events, and a compact full-stage timeline.
The Unity asset half of the upstream tutorial maps to the host shell.

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
