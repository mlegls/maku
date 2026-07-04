# Translation exercise: conventions and findings

Companion to `dmk-corpus/README.md`. Surface syntax is EDN; all conventions here are
provisional and exist to make the translations writable — they are proposals, not spec.

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

**Units: one canonical unit per quantity + conversion functions; no unit-tagged
literals.** Time: seconds canonical, `(frames n)` converts at the fixed timestep.
Angle: radians canonical, `(deg x)` / `(rad x)` convert — ordinary functions,
usable inside `m""` as `deg(120*vol)`. (`#deg` is gone; a tagged literal was the
wrong shape — units are conversions, not syntax.)

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
  applies as `in-frame`: `((rot (deg base)) child)`, `(anchor child)` for a
  let-bound frame, `([p1 p2] child)` for a literal frame array. This is §4's
  frames-as-transformers made literal — a frame *is* a `Dyn → Dyn` (and
  `Action → Action`) transformer. Vector literals themselves stay pure data;
  only list forms apply.

The child slot is single; an array child multiplies per §5's root-to-leaf product.
Lint: point→pose promotion in head position warns (classic wrong-thing-applied). `in-frame` overloads to action trees:
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

Finite iteration sugar (`dotimes`-style) desugars to `loop`/`recur`; sugar is only
sugar. Adopted form: `(dotimes [i n :every dt] body)` — n iterations, `dt` wait
between (not after the last).

**`m"…"` — the math macro, scoped concretely (adopted):**

- **Parse-only.** Everything inside has an s-expr equivalent and parses to the
  same canonical tree; the macro adds zero semantics. `m"0.2*(i+1)*(i+2)"` and
  the nested s-expr are the *same* card data. Canonical/serialized form is
  always the parsed tree.
- **Grammar inside**: infix `+ - * / ^ %` and comparisons with PEMDAS;
  function-call syntax `f(a, b)` where `f` is any in-scope function and
  arguments are again math expressions (so `deg(120*vol)`, `sine(1, 0.2, t)`
  need no escape); array literals `[…]` and coordinate literals `c[…]`/`p[…]`;
  `$(…)` splices an arbitrary s-expression for anything else.
- **Free symbols resolve against the enclosing lexical scope** — no requirement
  to pre-bind; the macro is an alternate parse, not a binding boundary.
- **Broadcasts like everything else**: operators inside parse to the same `+`/`*`
  nodes, which broadcast per §5 — `m"[0 120 240] + 80*t"` fans out. Scalars and
  arrays/matrices alike; no separate math-mode semantics.
- **When to use it**: expressions with several *binary operators*, where infix
  genuinely reads more naturally. Single calls stay s-expr — `(lerp 0.4 0.8 t 12 2)`,
  `(inc i)`, `(mod vol 3)`, `(+ increment 0.4)` are not math-macro material.
- Backtick is *reserved* (quasiquotation for card macros), which is why the
  promotion is `m"…"` and not `` `…` ``.

**`(fork action)`** — run a child concurrently, attached to the nearest enclosing
scope for cancellation. Needed because DMK async repeaters do *not* wait for their
children (040's volleys overlap: 80-frame volley, 70-frame period); structured
concurrency's default is sequential, so the overlap must be explicit. See F8.

**Stock formation vocabulary**: `(arrow n back side)` — Array Pose,
{(−back·|j|, side·j) : j ∈ −(n−1)/2 … (n−1)/2}, canonical left-to-right order.
Image of DMK's `bindArrow` + `frv2(rxy(-a*aixd, b*aiyd))` idiom. More of these
will accumulate (§1: keep the vocabulary); they are library, not core.

**Broadcast zips cycle (adopted).** Shorter arrays cycle rather than error —
SC multichannel expansion (our §5 source) cycles, and DMK color lists cycle by
`i mod len`; 060/110 exploit it deliberately. Scalar lifting stops being a
special case: an atom is a length-1 array cycled — one rule subsumes lifting,
exact zip, and palettes. Constraint: cycling is **axis-aware, never flat** — a
7-vector against a 7×9 product cycles along the arm axis after leading-axis
alignment (F9); flat cycling over the 63 would stripe across sub-arrays and
silently produce garbage. Lint non-divisor lengths on finite axes (7 into 9 is
probably a bug; 3 into 8 is idiomatic).

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

Time units: bare numbers in `wait` are **seconds**; `(frames n)` converts at the
fixed timestep. (DMK mixes bare frame counts and `2.5s` suffixes; see finding F5.)

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

**F4 — the style/color merge DSL is real surface area.** `"gem-*/w"` × `"yellow"` →
`"gem-yellow/w"`; `colorf`, `colorx`, and wildcard variants appear in every corpus
script. Translated as `{:style "gem-*/w" :color [...]}` with zipwise broadcast
(§5 length-matching) and merge semantics **deliberately unspecified**. Needs an
open-decision entry in §7: styles as templates over a color axis vs. flat style
enum with a color tag the render contract consumes.

**F5 — time-unit hazard.** DMK waits are engine frames (120 fps) except when
suffixed (`2.5s`), and `paction` delays are seconds. Two corpus-adjacent bugs
waiting to happen. The language should have exactly one bare unit (seconds) and an
explicit `(frames n)` constructor; never context-dependent units.

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
`girepeat` starts its child and moves on (unless `waitchild`); 040's volleys
genuinely overlap. Under §8's structured trees, sequential is the default and the
overlap needs explicit `(fork ...)` into the enclosing cancellation scope — an
inversion of DMK's defaults (DMK: fork by default, `waitchild` to sequence). The
explicit form is better for cards (concurrency visible in the tree), but `fork` is
new §8 vocabulary that language.md doesn't have yet.

**F9 — leading-axis meta broadcast.** 080 spawns a 7×9 product (arms × sub-chevron)
but colors by *arm*: a 7-vector in `:color` must zip against the leading axis of
the product and broadcast within. §5 specifies length-matching for flat arrays;
the product case needs the k/APL leading-axis rule stated explicitly.

**F10 — DMK auto-bindings are formation combinators.** `bindArrow`/`bindLR`/
`bindUD` inject magic index-derived variables into scope (source: Patterner.cs
`PrepareIteration`, Math.cs `HMod`/`HNMod`); their entire content is a pure
function index → offsets/signs. In this language they are ordinary pose-array
constructors (`arrow`, and `[-1 1]`-style sign vectors for `bindLR`) — no binding
machinery survives. Also decoded from source while translating: short V2RV2
literal `<a;b:c>` = (rx, ry, angle); `spread(total)` increments by
total/(times−1); DMK float suffixes `s` (×120, seconds→frames) and `f` (÷120).

## Status

- `020_gsrepeat.edn` — complete. Everything has a clean image.
- `130_bowap.edn` — complete. Two versions (closed-form and fold); F3 is the finding.
- `040_spread.edn` — complete. Both repeater levels are time-sequential → nested
  control loops; `rv2incr`/`spread`/`hvar` all dissolve to index arithmetic; F8.
- `080_aimed.edn` — complete. First script to touch an injected signal (implicit
  snap, §3 class (a)); chevron idiom → `arrow` combinator (F10); F9; the
  decelerating `vel` is the F1 lint case in the wild.
- Next per README order: 060 (polar + signal-valued color), 070 (lasers / axis
  materialization).
