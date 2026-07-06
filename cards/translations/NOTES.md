# Translation exercise: conventions and findings

Companion to `dmk-corpus/README.md`. **Consolidated into language.md** — the
conventions and findings below are folded into the spec as of the consolidation
pass; this file remains as the working record of how each decision was reached
(with source citations). Where this file and language.md disagree, language.md
wins.

## EDN conventions

| Form | Meaning |
|---|---|
| `(f args…)` | evaluated form (function application / combinator / macro) |
| `[a b c]` | **array literal** — first-class, broadcasts per §5; never evaluated as a call |
| `{:k v}` | meta record (§7) / option map |
| `:keyword` | tag keys, channel names, enum-ish options |
| `c[x y]`, `p[r θ]` | coordinate literals — reader shorthand for `(cart x y)` / `(polar r θ)` (§2's tagged `Vec2`); elements are ordinary expressions: `c[(lerp 0.4 0.8 t 12 2) 0]` |
| `m"80*t + sine(1, 0.2, t)"` | infix math reader macro (§11's escape); scope defined below |
| `symbol` | binding reference (`t`/`τ` never appear free; stock dyn constructors close over bullet-local time internally) |

The EDN choice earns its keep exactly where hoped: `[0 120 240]` (array data,
broadcasts) is lexically distinct from `(circle 3)` (evaluated form that *returns* an
array), and every canonical tree is readable as pure data without an evaluator —
which is what cards-as-data needs.

**Units: one canonical unit per quantity + source-named conversion functions; no
unit-tagged literals.** A conversion function is named for its *source* unit and
converts to canonical: `(ticks 8)` = 8 physics ticks as canonical seconds;
`(rad x)` = x radians as canonical angle. The canonical unit has no function —
you never write `(seconds 2.5)`, you write `2.5`. **Angles are canonically degrees**: the
corpus authors 100% in degrees (DMK's `cossindeg` is fossil evidence of designers
refusing radians); a radians default would wrap every angle literal in every card.
Pattern-level trig takes canonical degrees (`sin(90) = 1`); θ-column storage
(radians / unit vectors, open decision 7) is the compiler's business — conversions
constant-fold. (`#deg` is gone twice over: a tagged literal was the wrong shape,
and the unit it tagged is now the default.) Canonical degrees also deletes
DMK's parallel degree-variant function family (`cosdeg`, `sindeg`,
`cossindeg`, …): `sin`/`cos` take canonical degrees, and angle→unit-vector
needs no function at all — it is `p[1 θ]`.

**Time stays seconds-canonical** (asked and answered): unlike degrees/radians — a
pure scale change — tick-canonical would bake the timestep constant into card
data, and the signal model makes *continuous* time load-bearing: `Closed (t → a)`
evaluates at arbitrary t (scrubbing), and the tunnel game reparametrizes on
continuous arc-length `s`. Ticks are the runtime's sampling grid, not the
semantic domain. DMK itself splits the same way (control waits in frames,
hot-layer `t` in seconds). Two amendments: the conversion is renamed `(ticks n)`
— "frame" implies rendering; the sim owns physics ticks — and note that
`(wait (ticks 8))` is *exact* on the grid while a seconds literal rounds to it,
which is the honest reason control code authors in ticks.

**Type discipline: Signal / Function / Action (clarified).** `Signal a` is not
a function type — a first-class time-varying value, composed pointwise and
sampled (`snap`), never applied; BOTH constructors are pure (`Closed` a pure
fn of t; `Scanned`'s step a pure `(s, Inputs) → (s, a)` transition — a
"procedural" signal waits by *state*, a countdown in s, never by `wait`).
Functions are ordinary pure lambdas; there is no separate procedure arrow —
a procedure is a function whose *codomain* is `Action` (manipulate
callbacks). `Action` is an inert first-class effect description
(`wait : Float → Action`; spawn/event/manipulate/fork construct, seq/par/
race/loop compose); only the control-layer scheduler executes them.
Enforcement of signal purity is structural, not analytical: no signal slot
accepts an Action, no primitive evaluates an Action inside a signal, and
possessing an Action does nothing (inertness backstop) — no effect system
needed. Patterns are NOT signals-of-effects: signals have no privileged
evaluation schedule (why effects are banned in them — scrub/hoist/plot would
fire them incoherently); actions have order and extent, stepped once. The
§10 statement is the correct nearby truth: input-independent patterns
*denote* closed data (spawn log + installed signals) — they evaluate TO
signal-like data without BEING signals. SC's Patterns ≠ UGens ≠ commands is
the same three-way split. The layers meet at exactly two points: `snap` and
`spawn`.

Control layer borrows Clojure shapes: `(defpattern name [param default …] body)`,
`(loop [binding init …] … (recur …))`, `seq`/`par`/`race`/`wait`. An infinite `loop`
is legal; it dies by scope cancellation (§8) when the enclosing phase/race ends.

**Frame sugar, generalized (adopted).** Two type-driven sugars, both desugaring to
the same canonical `in-frame` node (resolution is static, by unification — never
runtime dispatch, or "sugar is only sugar" breaks):

- *Trailing child*: any head-word whose return type unifies with `Signal Pose`
  (incl. `Array Pose`, incl. point→pose promotion) accepts one extra trailing
  dyn/action argument: `(circle 5 child)` ⇒ `(in-frame (circle 5) child)`.
  Collision rule: declared signatures win; the sugar overload is considered only
  when no declared overload unifies (protects functions with trailing pose-typed
  parameters).
- *Applicable frames*: a list whose head types to `Signal Pose`/`Array Pose`
  applies as `in-frame`: `((rot base) child)`, `(anchor child)` for a
  let-bound frame, `([p1 p2] child)` for a literal frame array. This is §4's
  frames-as-transformers made literal — a frame *is* a `Dyn → Dyn` (and
  `Action → Action`) transformer. Vector literals themselves stay pure data;
  only list forms apply.

The child slot is single; an array child multiplies per §5's root-to-leaf product.
Lint: point→pose promotion in head position warns (classic wrong-thing-applied).

**No global emitter; anchoring is lexical.** `in-frame` overloads to action trees:
`(in-frame <pose-signal> <actions…>)` scopes every `spawn` in the subtree. There is
no ambient emitter to `move-to` — DMK's `roott` (animate the boss into place) is the
boss *entity's* movement; patterns just anchor to a frame, constant or live (a boss
entity's pose signal is the `kr`-hoisted case from §4).

**Host effects are outbound events** (§3 channels): `(event :channel payload)` —
e.g. `(event :sfx "x-fire-burst-1")`. The language has no engine vocabulary like
`sfx`; the host interprets channels.

**Why `loop`/`recur` rather than `for`** — three gains, all semantic here:

1. *Sequential fold and parallel fan-out cannot be confused.* In a timed language,
   "iterate" means either simultaneous (array broadcast — no loop at all) or
   sequential-with-waits (control layer). A `for` comprehension is ambiguous
   between them; `loop`/`recur` is unmistakably sequential, and fan-out is arrays.
2. *Loop state is explicit in the canonical tree.* The recur bindings ARE the fold
   state — visible to serialization, to card transformations, and to the F3
   input-independence analysis (check: do recur args depend only on constants?).
   DMK's `hvar increment +=` hides the same state in a mutable environment.
3. *Recur boundaries are the scheduler's points.* Cancellation checks, fuel
   accounting, and snapshots all land at `recur`, where the loop's entire state is
   the binding vector — so control-layer snapshot/rewind is "record the recur args,"
   not "capture an arbitrary mutable environment." This is load-bearing for the
   replay/rewind commitments.

**`dotimes` — indexed sequential iteration (adopted form):**
`(dotimes [i n, x xs, y ys … :every dt] body…)`

- First pair is the counter: `i` over `0…n−1`; `n` may be `inf` (DMK `times(inf)`).
- `:every dt` is the inter-iteration wait — DMK's repeater `wait(x)`: between
  iterations, not after the last (that trailing `when`-guard is why it's an
  option, not a `(wait)` you write yourself).
- Subsequent pairs are **seq bindings**: each iteration binds the i-th element
  of the source; arrays cycle (cyclic `nth`). `col ["red" "green" "blue"]` is
  DMK's repeater-level `color({…})` modifier as a loop binding — restoring the
  which-loop-level information a spawn-attached meta map lacks, with no magic:
  it desugars to `(nth xs i)`.
- Binding sources are stream-shaped: an array is the trivial cycling stream
  (SC `Pseq`), so when the §8 pattern algebra lands,
  `(dotimes [i inf, ang (pbrown 0 360 10)] …)` slots in unchanged.

Control-layer trichotomy: **arrays** (simultaneous fan-out) / **`dotimes`**
(sequential indexed, pure per-iteration) / **`loop`/`recur`** (sequential fold,
explicit carried state). BoWaP Version A vs B is literally dotimes vs loop.

**`m"…"` — the math macro, scoped concretely (adopted):**

- **Parse-only.** Everything inside has an s-expr equivalent and parses to the
  same canonical tree; the macro adds zero semantics. `m"0.2*(i+1)*(i+2)"` and
  the nested s-expr are the *same* card data. Canonical/serialized form is
  always the parsed tree.
- **Grammar inside**: infix `+ - * / ^ %` and comparisons with PEMDAS;
  function-call syntax `f(a, b)` where `f` is any in-scope function and
  arguments are again math expressions (so `rad(1.5)`, `sine(1, 0.2, t)`
  need no escape); array literals `[…]` and coordinate literals `c[…]`/`p[…]`;
  `$(…)` splices an arbitrary s-expression for anything else.
- **Free symbols resolve against the enclosing lexical scope** — no requirement
  to pre-bind; the macro is an alternate parse, not a binding boundary.
- **Broadcasts like everything else**: operators inside parse to the same `+`/`*`
  nodes, which broadcast per §5 — `m"[0 120 240] + 80*t"` fans out. Scalars and
  arrays/matrices alike; no separate math-mode semantics.
- **When to use it**: expressions with several *binary operators*, where infix
  genuinely reads more naturally. Single calls stay s-expr — `(lerp 0.4 0.8 t 12 2)`,
  `(inc i)`, `(nth palette vol)`, `(+ increment 0.4)` are not math-macro material.
- Backtick is *reserved* (quasiquotation for card macros), which is why the
  promotion is `m"…"` and not `` `…` ``.

**Phase machines with labeled `goto` (adopted; revised).**

```edn
(phases
  (:opening (goto :spell1))                      ; routing (practice/difficulty)
  (:dialogue (handoff :vn "d2"))                 ; no goto: falls through
  (:spell1 {:name "Spell 1" :hp 42 :timeout 48}
    attack1
    (finally (event :spell-cleared {:bonus true})))
  (:spell2 {:name "Spell 2" :hp 38 :timeout 48}
    attack2))
```

`(label opts? process… finally?)`, as **ordered clauses** — the label keyword
heads the clause; a `phase` head-word would be redundant since everything
inside a `phases` is a phase (the rule: *heterogeneous* items need
discriminating heads — `stages`' `stage`/`until`/`forever` keep theirs —
homogeneous clauses don't; cf. `cond`/`case` clauses). The earlier map form
was a bug: EDN maps are unordered, and declaration-order fall-through (DMK
`shiftphase`) needs list order to be stable card data. The
optional opts map drives the implicit `race(hp, timeout, process)` AND
exports as host-facing card data (hp bar, timer, spell name — DMK's
`hpi`/`type` props do both jobs too).

`finally` and `race` are core because they touch scheduler cancellation:
`race` decides which task subtree loses, and `finally` decides what cleanup
survives the unwind. The `states` clause grammar is plain again; genre
sugar such as the `phases` `(finally …)` tail compiles to core
`(finally (seq …) …)`.

Semantics: trampoline — each phase evaluates to the next label, defaulting to
list order; falling off the end completes the machine (which may return a
value to its embedder). `(goto label)` is a scoped non-local exit: cancel the
enclosing phase body (finalizers run), re-enter at the label.

**Goto is scoped strictly to the innermost lexical `phases`.** Outer
machines' labels are *not in scope* — an embedded machine (a card) cannot
hijack its host's flow; inner machines communicate upward only by completing.
Combined with the earlier discipline (exit structurally, enter only at phase
heads, tree-order tie-break for same-tick competitors), goto's classical
pathology — jumping *into* structure, or *across* control regimes — is
unrepresentable, not merely discouraged.

Labels, not indices: DMK's `shiftphaseto 4` breaks when a phase is inserted;
labels survive tree transformations. Corpus contact (ph_boss2_mima):
`shiftphaseto 4` = routing goto; dialogue `shiftphase` = fall-through;
`hpi`/`type`/`root` = phase opts; the `isAccel` mode-flag attack is a nested
`phases` in a phase body.

**Revised again (implementation contact): the primitive is a bare FSM; the
opts map was a boss template in disguise.** `(states (label body…) …)` is
plain; `phases` may still accept `(label body… finally?)` as genre sugar.
Everything the opts did is expressible as state-body code
with two small generalizations of `goto`: labels are *values* (evaluated —
`(goto (nth [:a :b] (rand-int 0 2)))` makes the machine a Markov chain) and
bare `(goto)` exits to the default successor (state order). Then: the hp
race is `(until (<= (hp-of boss-main) n) attack)` *as* the body (scope cancellation
falls through), a timeout is `(fork (seq (wait d) (goto)))` (it can't name
what comes next; bare goto is exactly that), `root` is `(move …)` at the
body head — the card knows who its boss is, the machine doesn't — and
publishing the phase is an ordinary exported cell. `finally` became a core
concurrency operator: running on the *cancelled* path is the one thing body
code cannot do (something must survive the scope's unwind). The DMK spellcard
record (`hpi`/`type`/`root`/timer) becomes a card-level macro over this,
not engine surface.

**Named accordingly: `states` is the primitive, `phases` the boss sugar.**
The FSM is general control flow — a ground/air player rig is two states
whose per-state movesets are body forks and whose transitions read input
channels; nothing about it is boss-shaped, so it shouldn't carry a boss
name. `phases` remains as the shipped desugar (`:until`/`:timeout`/`:root`
→ the three body forms above; anything richer is a card macro). Building
the player-control case surfaced the real cancellation contract: state
scopes guard on a **state generation** (bumped at every exit), not on the
goto request — a request cell is cleared by the machine the same tick it
routes, before sibling forks ever step, and a state that ends by plain
body completion makes no request at all. Generation guards kill the
state's whole forked subtree *however* the state ends; that IS DMK's
phase-token semantics, rediscovered at the right grain.

**The genre layer is a standard library, not engine defaults.** Asking
"is hp/damage/invuln special?" gave the answer that finished §9's
dissolution: hp was already a column and invuln a deadline column — what
remained *engine* was the defaulting (team→collider sets, hp 1, the
died-trigger synthesis) and one verb (`invuln`). All of it is now card
code in cards/lib/touhou.dmk: `spawn` knows only dyn + explicit meta,
and `spawn-bullet`/`spawn-shot`/`spawn-enemy` are macros prepending a
defaults map (macros, not defns, so the caller's literal meta keeps its
unevaluated signal tags; `spawn` itself grew per-key multi-map merge as
the composition hook). `spawn-boss` is *an enemy with a phase machine* —
that is the whole difference — binding `boss` for its machine body.
`invuln` is `(set-col b :iframe-until (+ $tick (* dur 120)))` once
`$tick` exists as a derived channel. The lib is authored as .dmk files
but embedded in the engine at compile time (bare-name imports:
`(import "touhou")`), so distribution is single-artifact and every host
— native, wasm, tests, REPL strings — resolves it identically; the
stock `player-rig` ships the same way and hosts build their rig string
from it. Everything held: the whole corpus migrated with zero behavioral
drift (the old family-based hitbox radii became explicit `:hitbox` at
star/gem/lstar call sites; smoke stays tick-identical). What the engine
still knows by name: the interaction-matrix rows and the three
contact-resolution bodies (`lives`/`iframe-until`/`hp` writes) — the
next extraction, blocked on a cheap per-contact call story.

That extraction shipped as `defcontact`: contact rules are card code, and
layer names are opaque tags owned by the templates that create them. The
engine now provides hot overlap detection plus two data prefilters (`:once`
and `:skip-if`) and zero genre semantics; Touhou's hit/graze/shot behavior
lives beside `spawn-bullet`/`spawn-shot`/`spawn-enemy` in lib/touhou.dmk.

**`phases` is genre policy, so the lib defines it — and macros became
able to.** Mima showed the spawn-boss boundary sitting wrong: the card
was hand-writing hp channel exposure, the registration wait, and
`{:until (<= (hp-of boss-main) n)}` gates — pure boss *convention*, repeated at
every boss. Two moves fixed it. First, macro-time power became real
language surface: macro bodies were always full evaluations, so what
was missing was only vocabulary — `& rest` params (macros and fns), the
seq builtins reading form lists as sequences of subforms, total `get`
over map forms, `form-type`/`form-name`, and evaluator-backed
`map`/`filter`. With those, `phases` moved out of Rust entirely: a
touhou.dmk macro walking its clause list with a helper defn and
splicing `states` clauses. Second, `spawn-boss` now owns the boss
conventions: it binds a structured boss channel with `bind-channel!`,
holds the machine until the boss registers, and binds `boss`/`boss-main`;
`phases` gained `{:hp n}` as gate sugar reading that local handle.
The engine keeps only the bare FSM (`states`/`goto`) — what a
"phase" means is entirely the library's business. The prelude rode the
same wave: `when`/`unless` are autoimported stdlib macros over `if`
(nothing coerces to the no-op action), with the `;;@prelude` sentinel
keeping expansion idempotent. `for` stays engine — its `:every`/inf
semantics are scheduler behavior, not a desugar.

**`move` — entity motion is remat, not frame mutation (clarified).** There IS
a thing being moved: the boss (or player option) is an expressed entity — it
renders, collides, has hp — and patterns anchor to its live pose signal (the
`kr`-hoisted frame of §4). `(move dur ease dest)` is derived, not primitive:

    (move dur ease dest)
      ≡ (seq (remat self :motion (fn [exit] (ease-seg ease dur (:pose exit) dest)))
             (wait dur))

one frame-stamped remat event appending a closed eased segment (C⁰ by
construction — the segment starts from the snapped exit pose), then a
blocking wait. DMK's `movetarget` blocks the same way; its `~` prefix is our
`(fork (move …))`. Consequently the entity's trajectory is an ordinary
piecewise-Closed segment history — there is no mutable frame variable
anywhere, and scrub/replay ride the same segment log as every other remat.

**`(fork action)` — dynamic `par`.** Starts `action` concurrently as a child
*adopted by the nearest enclosing concurrency scope* (`par`/`race`/phase), then
continues immediately. The scope's completion waits for adopted children; its
cancellation cancels them (finalizers run per §8) — when a phase ends, in-flight
forked volleys die with it. Static child list → write `par`; dynamic number of
branches (forking from inside a loop) → `fork`. Precedent: Trio's nursery
`start_soon`. Needed because DMK async repeaters do *not* wait for their
children: 040's 70-tick volleys fire on a 70-tick cadence, so without fork the
sequential default would stretch the period to body+wait = 140 ticks. See F8.
`:every` is "between iterations, not after the last" — n−1 waits for n
iterations — which is DMK's own semantics (GCR special-cases the final
iteration to skip the trailing wait); the difference is observable by anything
sequenced after the loop and by par/race/phase completion.

**Stock formation vocabulary**: `(arrow n back side)` — Array Pose,
{(−back·|j|, side·j) : j ∈ −(n−1)/2 … (n−1)/2}, canonical left-to-right order.
Image of DMK's `bindArrow` + `frv2(rxy(-a*aixd, b*aiyd))` idiom. More of these
will accumulate (§1: keep the vocabulary); they are library, not core.

**Array builtins**: `(iota n)` = `[0 1 … n−1]` (APL `!n`, already the notation
language.md §5 uses for `circle`'s θ column); `(range a b step)` for the general
case; `(without x xs)` = xs minus elements equal to x. Usable inside `m""` as
`iota(6)`.

**`(still)`** — the constant identity pose, §4's monoid unit, named. Rarely
needed in practice: expressing a bare frame is `(spawn guide meta)` directly
(the rider IS the guide expressed), and `laser`'s shape argument is optional
(default: straight along frame +x, u in world units). Both Spell-2 uses of
`still` turned out to be these two cases and were removed.

**Stock stream vocabulary**: `(stutter n xs)` — each element repeated n times,
still cycling (SC `Pstutter`); image of DMK's `colorf(xs, i/2)` floored-index
idiom. Seq-binding sources compose as streams; this is the §8 pattern algebra
arriving one combinator at a time.

**Slot-bound time (F12, adopted).** `t` (and the laser axis `u`) never appear
free: a signal-typed argument slot *binds* them — an expression referencing
`t`/`u` in such a slot denotes the `Closed` signal λt.(…), exactly BDSL's
movement-function model. Outside signal slots, `t` is an unresolved-symbol
error. This is what makes `(polar m"2*t" m"lr*20*t")` well-formed with `lr` an
ordinary lexical capture and `t` the signal parameter. Two corollaries
(adopted): **`t`/`u` are reserved** — not bindable by `loop`/`let`/params, so
slot-binding is the only possible meaning and shadowing is unrepresentable
(any future axis parameters, e.g. ancestor-clock symbols, get the same
treatment); and **no rate/time tags on expressions** — whether an expression
is time-varying is determined by its free variables, not chosen, so a surface
tag could only be redundant or wrong (unlike SC's `.ar`/`.kr`, which annotates
a genuine degree of freedom). The compiler infers constructor/rate; the REPL
displays it (§3); the reader greps for `t`.

**Style is a structured record (adopted — resolves F4).** DMK decoded: a style
string names a pool generated at startup as family SO (sprite + collision
geometry + fade config, `SimpleBulletEmptyScript`) × palette color × gradient
variant (`/` color, `/w` light, `/b` dark — `DefaultColorizing`), interned on
demand (`GetMaybeCopyPool`). Style is NOT a render signal — it is static pool
identity bundling render class, collision class, and addressability. Ours:
`{:family :gem :color :yellow :variant :w}` — family from a host-declared
registry (render contract: unknown family fails at card load), color/variant
as keywords. **Pool identity = the interned record**; **style is `ir`, never
signal-valued** — it determines SoA residency, and residency changes are
events (manipulate/remat-level pool migration), not signals; animatable
appearance (`:hue`, alpha, scale) stays in separate signal tags (§7). Queries
become typed predicates over axes (`(= :family :star)` for `"star-*/w"`);
card recolor = `assoc` on the `:color` axis.

**Broadcast zips cycle (adopted).** Shorter arrays cycle rather than error —
SC multichannel expansion (our §5 source) cycles, and DMK color lists cycle by
`i mod len`; 060/110 exploit it deliberately. Scalar lifting stops being a
special case: an atom is a length-1 array cycled — one rule subsumes lifting,
exact zip, and palettes. Constraint: cycling is **axis-aware, never flat** — a
7-vector against a 7×9 product cycles along the arm axis after leading-axis
alignment (F9); flat cycling over the 63 would stripe across sub-arrays and
silently produce garbage. Lint non-divisor lengths on finite axes (7 into 9 is
probably a bug; 3 into 8 is idiomatic). Same principle for indexed access:
**`nth` is cyclic** (index mod length) — `(nth palette vol)`, no explicit mod;
strict bounds are the marked case (`nth-strict`). "Arrays are cyclic" is one
principle covering zip, index, and scalar lift. Note a bare meta array cannot
replace loop-indexed access in a control loop: a single-bullet spawn has no
axis to zip against, and under nested loops (vol/shot) the cycle axis would be
ambiguous — DMK dodges this by attaching `color` to a specific repeater level,
positional information a spawn-attached meta map doesn't carry. (If a finite
emission is instead expressed as one array spawn with a birth-time column —
§3's spiral idiom — a bare palette *does* cycle against the volley axis.)

**No `offset` constructor; pure translation is `+` (adopted).** Composing a
translation-only pose and adding a point to positions are the same operation, so
the two-op algebra (§4) covers it. Sharper than §4's current wording: `+` is
expressed in whatever frame it lexically appears in — add *inside* a rotation
frame and you have DMK's rotational `rx,ry`; add *outside* the `in-frame`
wrapper and you have nonrotational `nx,ny` (the world-frame gravity case).
V2RV2's rotational/nonrotational split is not an operator or data structure;
it is *where the `+` sits in the tree*. Pending language.md §4 amendment.

**Action-level `in-frame` is a distribution law, not new semantics (adopted).**
`(in-frame f (par a b)) ≡ (par (in-frame f a) (in-frame f b))` (same for
seq/loop/race); `(in-frame f (spawn d m)) ≡ (spawn (in-frame f d) m)`;
non-spawning actions ignore it. The frame pushes through control combinators
and lands on spawn dyn-roots, where it is the ordinary pose-typed `in-frame` —
so the action overload is macro-eliminable (kept as a canonical node for
compactness, *defined* by the law). Consequences: (a) a signal-valued frame
reaching a spawn is a spawn argument ⇒ snapped by default, `live` to track —
§3 needs no new capture rule; (b) distribution is lexical, so ambient frames do
not leak into embedded patterns — the scope adapter decides; (c) **patterns
don't self-anchor**: the caller applies the frame (`(boss-frame (bowap))`),
which is where DMK puts `roott` too (the boss script, not the pattern). Corpus
translations keep non-identity anchors only to mirror the demo scripts.


## Findings

**F1 — `rvelocity(const)` is `Scanned` as written; the translation wants `linear`.**
Both demo scripts use `s(rvelocity(px(v)))` — a constant integrand. Per §4, velocity
constructors are scans by construction, so the literal translation makes every
straight-line bullet `Scanned` (non-scrubbable) for no reason. Translated as the
`Closed` position form `(linear c[v 0])`. Proposed rule: **no silent
strengthening** (a scan stays a scan — predictability), but the stock vocabulary
covers the closed forms and the compiler lints "scan with closed-form-integrable
integrand" with the suggested rewrite. Candidate open-decision entry.

**F2 — the shared/fresh distinction is moot for `Closed` signals.** In
`(in-frame (circle 14) (linear …))` the inline child is nominally fresh-per-element
(§5), but `linear` is stateless — per-element instances of a `Closed` signal are
indistinguishable from a shared one. State identity (`shared(...)`, §5) is a
`Scanned`-only concern. Worth one clarifying sentence in §5.

**F3 — "closed pattern" should mean input-*independent*, not closed-*form*.**
BoWaP's honest translation is a control-layer fold (Version B): the accumulator
doesn't need to telescope, because the control layer is allowed to fold — the state
lives in the *pattern*, never in bullets. But §8/§10 define the closed/extractable
property as "statically computable timeline," which Version B technically fails
(you must run the fold). The property that actually matters for scrub/extraction is
**input-independence**: a pure fold over constants is deterministic and replayable
with no input tape — evaluating the timeline *is* static computation. Proposed
amendment: closed pattern = no event-waits, no injected-signal dependence; pure
folds included. (Version A shows this recurrence happens to telescope to
θ(i) = 0.2(i+1)(i+2)°, which a simplifier may exploit, but the semantics shouldn't
require it.)

**F4 — the style/color merge DSL is real surface area.** **RESOLVED** — see
"Style is a structured record": the merge DSL dissolves into a record with
family/color/variant axes; wildcards become predicates over axes; the DMK
startup pool product becomes interning of observed records.

**F5 — time-unit hazard.** DMK waits are engine frames (120 fps) except when
suffixed (`2.5s`), and `paction` delays are seconds. Two corpus-adjacent bugs
waiting to happen. The language should have exactly one bare unit (seconds) and an
explicit `(ticks n)` constructor; never context-dependent units.

**F6 — cohort rate (observation, not a language change).** A `Closed` dyn that
references only bullet-local τ (e.g. `linear`) evaluates identically for all
bullets born the same tick. Rate inference as specified (§3) classifies it `ar`,
but pool evaluation could compute it once per *birth cohort* per frame — a fourth
effective rate sitting between `kr` and `ar`, relevant for ring-heavy patterns
where cohorts are large. Pure backend optimization; noted for the runtime doc.

**F7 — repeater-modifier effects land cleanly.** DMK's `sfx(...)` is a repeater
modifier (fires per repeater invocation); under "effects live only in the action
tree" (§8) it becomes an ordinary outbound event (`(event :sfx …)`) sequenced
before the spawn, and the per-shot vs per-volley distinction falls out of *where
in the loop* it sits rather than which construct hosts it. No engine vocabulary
needed — the host interprets the channel.

**F8 — DMK's async repeaters implicitly fork; structured concurrency must say so.**
`girepeat` starts its child and moves on (unless `waitchild`); 040's 70-tick
volleys fire on a 70-tick cadence, impossible sequentially (the period would
become body+wait = 140). Under §8's structured trees, sequential is the default
and the concurrency needs explicit `(fork ...)` into the enclosing scope — an
inversion of DMK's defaults (DMK: fork by default, `waitchild` to sequence). The
explicit form is better for cards (concurrency visible in the tree), but `fork` is
new §8 vocabulary that language.md doesn't have yet.

**F9 — leading-axis meta broadcast.** 080 spawns a 7×9 product (arms × sub-chevron)
but colors by *arm*: a 7-vector in `:color` must zip against the leading axis of
the product and broadcast within. §5 specifies length-matching for flat arrays;
the product case needs the k/APL leading-axis rule stated explicitly.

**F11 — entity sounds are lifecycle-event data, not meta verbs (corrected).**
Source check: DMK's `hotsfx` is a one-shot `SFXService.Request` at the
cold→hot *transition* (collision activation, FrameAnimBullet.cs:109), re-fired
on hot re-entry — not a managed loop. Model: the sim already emits lifecycle
transition events for expressed entities (warn→active→off, §6; needed for
replay regardless); audio is the host subscribing to them. Defaults bind
family × transition → cue in host config (`dSFX` = "use family defaults",
zero language surface); a pattern wanting custom audio attaches
`{:cues {:spawn … :active …}}` — pure data decorating the entity's lifecycle
events, read off the event stream. Action-time sounds (per-volley fire inside
a loop) remain ordinary outbound `(event …)` — cues are for entity lifecycle,
events for control flow. An imperative `:sfx-loop`-style tag was the wrong
shape: audio is not a property of the bullet but a host reaction to its
lifecycle.

**F12 — slot-bound time formalized.** See the convention entry: `t`/`u` are
bound by signal-typed slots (BDSL's movement-function model as a typing rule).
Corpus forcing case: 060's `polar(2*t, lr*20*t)` mixes slot-bound `t` with
lexically captured `lr`. Pending language.md §3/§11 amendment.

**F13 — `spawn` returns Bullet handles (adopted).** The control layer may hold
them; `manipulate` accepts a handle where it accepts a query (a handle is a
degenerate predicate); dead handles are no-ops (generation-safe). This
dissolves DMK's hoist-index-into-bullet-state + persistent-control +
per-frame-predicate idiom whenever the trigger schedule is static (110: all
stars born one tick ⇒ explosion times known ⇒ control-layer `dotimes` over
handles). Queries remain the mechanism when triggers read per-bullet runtime
state (proximity, hp) — and they're also the vectorizable path.

**F14 — guide objects dissolve into unexpressed frames.** DMK's `guideempty2`
subsystem (invisible bullets + per-frame channel recording + `dtpoffset`/`@`
keyed reads) is `in-frame` with an unexpressed dyn: the guide is a level of
the frame tree that renders nothing and consumes no pool slot (§6's
express-only-what-renders, derived). 200_cradle: DMK spawns 18 invisible
bullets; we spawn 0. §10's first-class extraction is only needed when guide
trajectories cross action-tree boundaries — lexical nesting covers the
common case.

**F15 — meta axis-targeting is ambiguous under cycling.** 200's product is
3×6×7 with `:variant` a 3-vector for axis 0 and `:color` three values meant
to *cycle along axis 1*. Leading-axis-first binding would claim the 3-vector
:color for axis 0 (exact length match) — wrong. Rule adopted: **meta arrays
bind to the leading axis, period**; to target a deeper axis, write that
axis's length explicitly — `(nth [:blue :green :teal] (iota 6))` is a
6-vector (cyclic nth broadcasts over iota) and binds to axis 1 by length.
Possible future sugar: `(on-axis k xs)`. DMK avoids the ambiguity by
attaching modifiers to repeater levels — positional information our
spawn-level meta must encode by length or annotation.

**F16 — pattern-scoped control cells (adopted).** `(defvar name init)` +
`(set! name v)` actions + reads. The internal analogue of injected channels
(SC control-bus precedent): writes are frame-stamped events on the log, so
replay/scrub survives; the control layer reads the cell plainly (it owns it,
tick-synchronous); *signal* slots must mark `live(name)` — snap-by-default
applies to cells exactly as to injected channels. Dissolves ph_boss2's
`exec b{ hvar isAccel }` + `whiletrue` polling + mode-dependent render
functions. Structure is still preferred where the gating IS structure
(successive stages of a loop); cells are for state read *concurrently* by
long-lived signals and independent loops.

**F17 (from the prototype) — `let` defers action-valued bindings to
reach-time.** `((pose P) (let [stars (spawn …)] …))`: if the let executed its
spawn at evaluation time, the ambient frame — which the distribution law owes
the spawn — would not be applied yet (the frame wraps the let's *result*).
Rule: a `let` whose bindings include action values becomes an action; its
bindings execute when the scheduler reaches it, inside the ambient frame, and
their results (spawn handles) bind. Pure lets are unaffected. Pending
language.md §4/§6 amendment.

**F18 (from the prototype) — ambient frames do not cross `fn` boundaries.**
110's manipulate callback spawns at `(+ (pos b) …)` — world coordinates; if
the lexically enclosing anchor frame leaked into the callback, the explosion
would be double-anchored. Rule: lexical distribution stops at lambdas, the
same way it stops at embedded patterns (the adapter/caller decides). Verified
by test (`handles_and_manipulate`). Pending language.md §4 amendment.

**F19 (from the prototype) — difficulty must thread explicitly; pure loops
fold inline.** Two catches from running Spell 2: (a) `guide-rig` referenced
`spell-2`'s `factor` parameter free — `defn`s are lexically scoped, so
difficulty is passed explicitly (DMK's ambient `dl` has no analogue; if that
proves noisy, an ambient-constants story needs designing, cf. §8's rank
note). (b) `rand-cell-except`'s rejection loop is a *pure fold* used for its
value inside a defn — loops containing no temporal actions evaluate inline
(exactly F3's fold-belongs-to-the-control-layer point); temporal actions
inside such loops are errors.

**Ambient context: three disciplined forms, not one map (adopted).** A shared
read-write namespace "anything can write" is DMK's GCX environment again — the
hoisted-variable soup whose elimination made every translation clearer, and a
tree-rewrite hazard (cards can't see which names a subtree reads/shadows).
The ambient itch decomposes:

1. **Read-only ambient = channels** (injected or derived): rank/difficulty,
   player pose, tunnel `s`. Single writer, on the replay tape, readable
   anywhere without threading. F19(a)'s root cause was misclassification:
   DMK's `dl` IS rank, already an injected channel per §3 —
   `(def factor (pow rank 0.3))` and the threading disappears.
2. **Read-write ambient = cells** (`defvar`/`set!`, F16): already a shared
   namespace, but pattern-scoped; embedding adapters decide sharing.
3. **Scoped overrides** `(with {rank 0.5} body)` — dynamic *binding*, not
   mutation: channel values overridden for a lexical subtree. Delimited
   writes, tree-visible provenance, card-macro friendly ("this card at half
   rank" = wrap in `with`). NEW surface, pending spec entry **[decide]**.

**F20 — most "primitive" channels are derived; only true inputs are injected.**
`nearest-enemy` is not a host input: enemies are expressed entities carrying a
team/`:enemy` tag, and nearest-enemy = a spatial query over tagged entities
relative to `player` — a **derived channel**: computed by the sim per tick
from world state, exposed and *taped* like an injected channel (kr), so
signals may read it without violating world-isolation and scrubbing still
works. The same sorting applies across the assumed vocabulary:
- genuinely injected (host-only knowledge): player pose/buttons
  (`focus-firing`), tunnel `s`, rank;
- derived channels (sim-computed from world, taped): `nearest-enemy`, counts/
  thresholds (hp fractions), boss pose *as read by other patterns*;
- entity-state reads, not channels at all: DMK's `mine`/`OptionLocation`/
  `LaserLastActiveT` (self accessors), the boss frame a pattern is mounted on;
- pure library, not primitive: `aim` (sugar over `rot`∘`angle-of` + emitter
  origin), formations (`arrow`/`fan`), grid helpers, easings;
- genuinely core: counter-based `rand` (determinism contract), `snap`/`live`.

**F10 — DMK auto-bindings are formation combinators.** `bindArrow`/`bindLR`/
`bindUD` inject magic index-derived variables into scope (source: Patterner.cs
`PrepareIteration`, Math.cs `HMod`/`HNMod`); their entire content is a pure
function index → offsets/signs. In this language they are ordinary pose-array
constructors (`arrow`, and `[-1 1]`-style sign vectors for `bindLR`) — no binding
machinery survives. Also decoded from source while translating: short V2RV2
literal `<a;b:c>` = (rx, ry, angle); `spread(total)` increments by
total/(times−1); DMK float suffixes `s` (×120, seconds→frames) and `f` (÷120).

## Status

- `020_gsrepeat.dmk` — complete. Everything has a clean image.
- `130_bowap.dmk` — complete. Two versions (closed-form and fold); F3 is the finding.
- `040_spread.dmk` — complete. Both repeater levels are time-sequential → nested
  control loops; `rv2incr`/`spread`/`hvar` all dissolve to index arithmetic; F8.
- `080_aimed.dmk` — complete. First script to touch an injected signal (implicit
  snap, §3 class (a)); chevron idiom → `arrow` combinator (F10); F9; the
  decelerating `vel` is the F1 lint case in the wild.
- `060_polar.dmk` — complete. Every DMK modifier became a seq binding
  (`colorf`→`stutter`, `bindLR`→`[1 -1]`, parent-index `colorf`→outer binding);
  first nonlinear Closed dyn; slot-bound `t` formalized (F12).
- `070_dynamic_lasers.dmk` — complete. **Axis materialization survives first
  contact**: DMK dynamic laser = f(t, u) with u = lt, length = u-extent,
  stagger = render-resolution hint. Surface for open decision 3 proposed
  (`(laser shape {:warn :active :u-max :resolution})`); hueshift's hoisted
  index became one array-broadcast expression; F11.
- `110_exploding_stars.dmk` — complete. DMK's per-bullet state + pool control
  + polling dissolves into spawn handles + control-layer scheduling (F13);
  first facing override; `(cull b :soft)`; callback layer-audit exercised.
- `200_cradle.dmk` — complete. `guideempty2`/channels/`dtpoffset` dissolve
  into an unexpressed frame level (F14 — largest structural win yet: 18
  invisible bullets → 0); named signals replace BDSL function+$() idiom;
  Scanned-contagion poster child for the F1 lint; meta axis-targeting rule
  (F15).
- `SCANNED.md` — Scanned surface developed against ph_boss2_mima: raw
  `(scan s0 step)`; `stages` as the synchronous-feeling segment API
  (piecewise-Closed when segments close, Scanned otherwise); the unification
  `stages` = statically-scheduled remat / `remat` = event-driven stage
  transition; the boss script's `switch(reflected, …)` idiom decoded as
  hand-rolled rematerialization.
- `ph_boss2_spell2.dmk` — complete (the ceiling test). Exercises everything
  at once: `defvar` cells (F16), the first genuinely *shared* scan (the
  guide), summons-as-fork-in-frame, `whiletrue` = pause (verified) →
  `wait-for`, random-walk fold, rand/brand dissolution, macros→functions.
  ~60 lines vs ~100; the whole card except two parked guides is piecewise-
  Closed.
- `player_homing.dmk` — the Scanned/stages/live exercise (Reimu Home and
  Laser + Fantasy Seal motion core). `truerotatelerprate` == `slew` verbatim
  (source: "degrees of gap to close per second"); give-up homing = slew with
  a rate signal decaying to zero (class (d) self-discretizes, no stages
  needed); the tracking laser is `smooth |> slew` feeding a signal-valued
  frame; Fantasy Seal's `&ldelta` hand-threading (persistent control writing
  a per-bullet column) is the `stages` exit handoff verbatim. New stock:
  `(smooth k sig)` one-pole follower (SC Lag). Channels are role-relative:
  `player` / `nearest-enemy` are one mechanism pointed both ways.
- Remaining: the language.md consolidation pass.
