# Danmaku Pattern Language: Design Document

A language design for an engine-agnostic bullet-hell system, derived from an audit of Danmokou's (BDSL) semantics, SuperCollider's signal model, and array-language composition. Companion to *Engine-Agnostic Danmaku Core: Design Notes* (architecture/runtime doc); this document specifies the language itself.

Status: consolidated after the DMK translation exercise (`cards/translations/`), a first implementation pass (`proto/` — a Rust interpreter + player whose conformance suite runs the entire translation corpus verbatim, production boss card included), and a gameplay/host sprint (`cards/` — collision/colliders, columns/triggers, scope cancellation, the piloted-rig host contract, imports; `cards/reimu_vs_mima.dmk` is the everything-at-once playable witness). Findings F1–F20 and the adopted conventions are folded in here; NOTES.md/SCANNED.md remain as the working record. The prototype's session layer additionally realizes design.md §11's tooling (input + command tapes, snapshots, scrubbing, live eval/swap/layer from the editor). Sections marked **[decide]** are open decisions.

---

## 1. Design stance

**Steal DMK's invariants, redesign its composition layer.** DMK is *array-ready but scalar-souled*: its runtime is SoA pools and its per-bullet functions are pure over `(t, env)` — so its semantics vectorize mechanically — but the language's unit is the individual bullet, and its composition layer (repeaters) is imperative accumulator mutation. Audit findings:

- Repeater modifiers (`spread`, `circle`, indexed color lists) are pure functions of the loop index wearing mutation clothes: `gsrepeat times(n)` *is* a map over `(iota n)`. The only genuinely sequential elements — shared-stream RNG and wait-between-shots — dissolve into counter-based RNG and birth-time columns respectively.
- What DMK encodes implicitly and this language makes explicit: per-bullet local time with spawn capture (GCX environment frames), the spawn-time/flight-time evaluation split (GCXF vs bullet functions), closed-form vs integrated motion (`roffset` vs `rvelocity`), scoped cancellation (token hierarchy), and frame composition (V2RV2).
- What to keep wholesale: the *function vocabulary* (sine, polar, easings, cull/graze mechanics, aimed-modifier conventions) — years of ergonomic tuning, portable into any composition model. And the negative lesson: DMK's v9 dynamic-type period was removed in v11 in favor of a standard typed model. Interpretations must be types.

**Array structure in danmaku is ephemeral**: rings/rows/polygons are *birth* structures that dissolve in flight (per-bullet graze flags, culls, controls address predicates, not birth groups). Therefore: **array semantics at spawn, bag-of-rows semantics in flight.**

The translation exercise (every WebDemo script, a production boss spell card, and player-side homing) confirmed the stance and repeatedly found DMK subsystems dissolving into composition: guide channels → frames, auto-bindings → formation data, mode flags → rematerialization, summons → forked scoped actions.

---

## 2. Core types

**[spec]** Small, closed universe for per-bullet data; types erase to flat SoA columns at runtime.

- `Float`
- `Vec2` with **tagged interpretation**: `Cart` (x, y) and `Polar` (r, θ) are distinct types over the same storage, with explicit conversion. Rationale: polar has a partial algebra (adding to θ = rotation, adding to r = radial push — both pattern-meaningful) but componentwise `+` of two polar values is not vector addition; broadcasting would do it silently if untagged. Surface literals: `c[x y]`, `p[r θ]` (§11).
- `Pose` = element of SE(2): position + orientation θ. **Points and poses are distinct**: points add (offsets, lerps, arithmetic), poses compose (frames, emission anchors). Cheap promotion point → pose (θ from context; see §5). `(still)` names the identity pose, the unit of frame composition.
- `Tag` values for meta (color, style, render hints, gameplay flags, hp, …). Tags may be signal-valued (§7). **Style is a structured record** `{:family … :color … :variant …}` — see §7.
- `Bullet` — opaque, **fixed-layout** handle: built-in columns (pose, velocity, epochs, standard tags) plus one escape pointer to optional sidecar state. Returned by `spawn`, consumed by `manipulate` (§9).
- Arrays of the above (§6).
- `Signal a` — the central abstraction (§3).
- `Pattern` / `Action` — the control layer (§8).

Per-bullet hot state draws only from {Float, Vec2, Pose, Tag-word} so scanned state packs into columns and steps vectorize pool-at-a-time. Control-layer signals (per-pattern, per-frame) may carry richer types.

### The type discipline: Signal / Function / Action

`Signal a` is **not a function type** — a first-class time-varying value, composed pointwise and sampled (`snap`), never applied. *Both* constructors are pure: `Closed` is a pure function of t; `Scanned`'s step is a pure `(s, Inputs) → (s, a)` transition — a "procedural" signal waits by *state* (a countdown in s), never by `wait`. Functions are ordinary pure lambdas; there is no separate procedure arrow — a procedure is a function whose *codomain* is `Action` (e.g. manipulate callbacks). `Action` is an inert first-class effect description (`wait : Float → Action`; spawn/event/manipulate/fork construct them; seq/par/race/loop compose them); only the control-layer scheduler executes them.

Enforcement of signal purity is therefore **structural, not analytical**: no signal slot accepts an `Action`, no primitive evaluates an `Action` inside a signal, and possessing an `Action` does nothing (inertness backstop) — no effect system needed. Patterns are *not* signals-of-effects: signals have no privileged evaluation schedule (which is exactly why effects are banned in them — scrubbing/hoisting/plotting would fire them incoherently), while actions have order and extent, stepped once. The §10 statement is the correct nearby truth: input-independent patterns *denote* closed data. The layers meet at exactly two points: `snap` and `spawn`.

---

## 3. Signals: the two-constructor model

**[spec]** The unifying type. Everything time-varying — motion, meta, injected host data, extracted pattern data — is a `Signal`.

```
Signal a = Closed  (t → a)                       -- evaluable at arbitrary t
         | Scanned (s₀, (s, Inputs) → (s, a))    -- advanced tick by tick
```

- `Closed` signals are pure functions of time: scrub-safe, rewind-safe, plot-able, hoistable.
- `Scanned` is the **only** introduction rule for streamed behavior. It subsumes: integration (`integrate = scan (+·dt)`), per-tick procedural motion, stateful visual effects, host-injected live data.
- **Effect typing is constructor contagion**: any composition touching a `Scanned` signal is `Scanned`; `Closed` combinators preserve `Closed`. "Scrubbable" = `Closed`. There is no conversion `Scanned → Closed` (this is the no-un-integration theorem); the sanctioned exit is **rematerialization**: sample current values into fresh spawn-captured constants and swap to a closed signal (§9).
- `snap : Signal a → a` — evaluate *now* (at action/spawn time), yield a constant. The elimination rule back to constant-land; the formalization of spawn-time capture.

### Slot-bound parameters **[spec]**

`t` (and the materialization axis `u`, §6) never appear free: a **signal-typed argument slot binds them** — an expression referencing `t`/`u` in such a slot denotes the `Closed` signal `λt.(…)`, exactly BDSL's movement-function model made a typing rule. Outside signal slots, `t` is an unresolved-symbol error. `t`/`u` (and any future axis parameters, e.g. ancestor-clock symbols) are **reserved** — not bindable by `loop`/`let`/params — so shadowing is unrepresentable. There are **no rate/time tags on expressions**: whether an expression is time-varying is determined by its free variables, not chosen (unlike SC's `.ar`/`.kr`, which annotates a genuine degree of freedom); the compiler infers constructor/rate, the REPL displays it, the reader greps for `t`.

Corollary (found by implementation): a `def`'d signal resolves **hygienically except the slot-bound parameters pass through** — `(def swirl (lerpsmooth eoutsine 0 4 t 0 480))` referenced in a θ slot means the *referencing slot's* `t`. That is what "the slot binds t" means for named signals.

Additional slot bindings in derived domains: **vel/acc slots bind self-state** — `pos` and `dir` (and `vel` in acc slots); DMK precedent: velocity functions receive `bpi` including own location. Self-reference is feedback, and these signals are already scans, so the type story is unchanged. Homing is one line:

```edn
(vel p[10 (slew 60 90 (angle-of (- (live $nearest-enemy) pos)))])
```

### Time and clocks **[spec]**

Three clocks with explicit nesting rules:

1. Every action node gets a **local clock** zeroed at its activation (`seq`, `loop` iterations, etc. rebase).
2. Every bullet's dyn runs on **bullet-local time** `t − birth`. Birth time is a column (each slot's initial epoch, §9); emission over time (spirals) is birth-time data, not phase-locked global-t functions. An explicit operator reaches an ancestor clock when phase-locking is wanted (ring vs spiral distinction) **[decide]**.
3. **World time / world parametrization is host-injected.** Nothing is sacred about `t`: patterns may be parametrized on any monotone-or-not host signal (e.g. tunnel arc-length `s`). Closed signals evaluate at arbitrary parameter values, enabling backward evaluation when the parameter is player-controlled.

### Injected signals and capture **[spec]**

Host-provided values (player pos/vel/acc, boss position, rank, arbitrary channels) are `Scanned` signals. **Default capture semantics: injected signals appearing in spawn arguments are implicitly snapped** (spawn-time capture — the overwhelmingly common case: aimed fans, rings at last known position). Continuous tracking requires explicit `(live …)` — the scrub-affecting choice stays visible. Channels are **role-relative**: boss patterns read `player`, player patterns read `nearest-enemy`; one mechanism pointed both ways. The snapshot carries kinematics (pos *and* vel/acc); `(deriv sig)` differentiates any signal (finite difference, one prev-sample column — the same machinery §4 uses for heading).

Reactivity decomposes as:

- (a) spawn-time sampling → `snap` (the vast majority of "reactive" danmaku);
- (b) pointwise composition with a live signal (drift fields, boss-parenting, rank scaling) — stays scrub-evaluable given a recorded input tape, since `injected(t)` is a lookup;
- (c) event-time re-capture (fly straight, re-aim once) → rematerialization at event boundaries;
- (d) true continuous feedback (integration over a live signal: homing) — irreducibly `Scanned`, and small in practice because game design discretizes it into (c) for fairness. Corpus note: give-up homing — `slew` with a rate signal decaying to zero — makes (d) *self-discretize* into (c) with no extra mechanism.

Only (d) breaks closed form. The typing rule: an expression is `Scanned` iff it is downstream of a `scan` — pointwise use of injected signals does not by itself stream a bullet.

**Channels have their own namespace: `$name`.** There may be any number of channels and none are privileged — `$player` and `$rank` are conventional names that make cards portable/combinable, nothing more. The sigil removes collision with card-local variables *structurally* (no reservation list to maintain against an open-ended host vocabulary), the host passes values by name, and a card's **channel manifest is derivable** — scan the canonical tree for `$` symbols — giving the same load-time contract check the style registry gets: a card reading `$wind` fails to load on a host that doesn't provide it. Pattern-internal cells (`defvar`) stay bare: they are card-declared, so their collisions are the author's own.

**Channels split by who can know the value.** Genuinely *injected* channels carry host-only knowledge: player pose/kinematics, buttons (`$focus-firing`), tunnel `$s`, rank/difficulty (DMK's `dl` — a channel, never a pattern parameter; misclassifying it forces pointless threading through lexically-scoped `defn`s). ***Derived* channels are sim-computed world facts** — `$nearest-enemy` (a spatial query over `:enemy`-tagged expressed entities relative to `$player`), hp-fraction thresholds, another entity's pose — computed per tick by the sim, exposed *and recorded on the replay tape exactly like injected channels*, which is what lets signals read them without violating world-isolation while keeping scrubbing honest. Everything else that looks channel-shaped is not one: self-entity accessors are entity-state reads; `aim` and formations are library; `snap`/`live` and counter-based `rand` are core.

**Ambient context is three disciplined forms, not a shared read-write map** (which would be DMK's GCX environment again — unmarked cross-subtree writes are both spooky action and a card-algebra hazard):

1. read-only ambient = **channels** (single writer, taped, readable anywhere without threading);
2. read-write ambient = **control cells** (below; pattern-scoped, adapter-gated);
3. **scoped overrides**: `(with {$rank 0.5} body)` — dynamic *binding*, not mutation. **`with` is to channels what `in-frame` is to poses**: the same distribution law over the action tree (pushes through control combinators, lands on spawns, which capture it for their signals' lifetimes — including `live` reads, long after the body's evaluation), the same boundary (stops at pattern-embedding adapters), the same capture rules. The ambient frame is the special case for the "where am I" channel; `with` generalizes the mechanism. `let` cannot substitute: lexical scope reaches only text you *contain*, not code you *cause* (callees resolve channels in their own definitions; spawned signals outlive the body) — and the `$` namespace makes the let-vs-with ambiguity *unrepresentable* (a channel read is never a variable reference; `let` binds bare symbols only — strictly better than reserving names, which cannot scale to an open host vocabulary). Overrides are ordinary card data (tree nodes; they serialize). Residual **[decide]**: nesting/shadowing details and which derived channels are overridable (§13).

**The host↔pattern surface is four constructs, all on named channels** — raw engine-object access does not exist:

- **injected signals** in (player pose/kinematics, rank, tunnel `s`, arbitrary host data);
- **exported signals** out — continuous pattern→host data, realized as signal-valued tags on entities (§7), no separate mechanism;
- **outbound events** out — discrete, frame-stamped, the dual of injected events; inputs tape in, events tape out, keeping the replay log symmetric;
- **host handoff** — a command plus `wait-for(host-event)` (e.g. run a dialogue scene). The only construct that makes a timeline tick-emergent across the boundary (§8), which is honest: the host's duration genuinely isn't statically knowable.

**The export surface is declarative** (resolving the mechanism half of §13.9): entity state exports via spawn meta **`:expose {:col $channel}`** — the sim publishes that entity's column as a derived channel each tick, reading 0 once the entity is dead/absent (so hp gates fire; no stale values), and available the same tick the entity spawns. `$boss-hp` is not engine vocabulary: the boss's spawn declares `{:expose {:hp $boss-hp}}`. The `$name` there is a channel *designator*, not a read — `:expose` is an unevaluated tag (like `:hue`), the same convention as `(with {$rank 0.5} …)` keys. Pattern-internal state exports via the **`(export cell)`** action — the cell becomes a read-only channel of the same name; the pattern remains the single writer, hosts render it. Both are ordinary card data; what remains of §13.9 is the *systematic families* (`$player-k`/`$lives-k` per pilot, `$nearest-*` queries, counters), which stay engine-derived.

**Pattern-scoped control cells** are the *internal* analogue: `(defvar name init)`, written by `(set! name v)` actions (frame-stamped events → scrub survives), read plainly by the control layer (it owns them, tick-synchronous), and read in signal slots via `(live name)` — snap-by-default applies to cells exactly as to injected channels. SC control-bus precedent. Cells are for state read *concurrently* by long-lived signals and independent loops; where gating is structural (successive stages of a loop), structure is still preferred.

### The Scanned surface: `scan` and `stages` **[spec]**

Raw constructor: `(scan init-state step)`, `step : (state, inputs) → [state' out]`; `inputs` is the injected snapshot plus `:dt` — scans are the one place live signals arrive unsnapped by construction. Steps are pure transitions (waiting is a countdown in state, never `wait`).

`stages` is the synchronous-*feeling* surface — sequential segments that read like waits but whose durations are data, not Actions:

```edn
(stages
  (stage 0.5  (linear c[3 0]))                     ; closed segment, 0.5s
  (stage 1.2  (fn [exit] (polar m"2*t" m"30*t")))  ; t REBASES at the boundary
  (until pred wobble)                              ; predicate-terminated
  (forever (fn [exit] …)))                         ; exit = snapped prev state
```

- Each segment runs on its own epoch (per-slot epoch model, §9); the optional `(fn [exit] …)` form receives the snapped exit state of the previous segment — continuity is explicit initial-condition passing, the remat philosophy.
- **Compilation degrades gracefully**: all durations constant + all segments `Closed` + no `until` ⇒ the whole signal is **piecewise-Closed** (a static segment table, evaluable at arbitrary t — scrub/rewind-safe). Any `until`, input-dependent duration, or `Scanned` segment ⇒ `Scanned` with state = (segment index, segment-local state). Contagion classifies; no annotation.
- This mirrors §8's timeline rule exactly: a boundary is either at a time you can compute or at a tick you must reach. `stages : signals :: seq/wait : actions`.
- **`stages` and `remat` are one mechanism**: stages = statically-scheduled rematerialization (the segment list is the §9 `(epoch, signal, constants)` history known up front); remat = event-driven stage transition. A bullet's motion is always a segment sequence; boundaries are data, predicates, or events. Corpus witness: Fantasy Seal's orbit-then-chase hand-threads exit velocity through a per-bullet column — it is the `(fn [exit])` handoff verbatim.

Stock stateful combinators (Scanned by construction, no user state): `(slew rate init? sig)` — angle-aware rate-limited follower, rate may be a signal (SC `Slew`; DMK `truerotatelerprate` verbatim); `(smooth k sig)` — one-pole follower (SC `Lag`; DMK's beforeDraw-lerp01 idiom); `(deriv sig)`.

**Base + correction needs no operator**: signals are a pointwise vector space and integration is linear — `(vel (+ ballistic (* 0.3 correction)))` and cross-domain `(+ (polar …) (vel correction))` just type-check. Implementation note: additive decomposition confines scan state to the correction term; the closed base stays hoistable.

### Rates **[spec]**

Adopted from SuperCollider (`ir/kr/ar`), realized as inference over shape and constructors:

- `ir` — evaluated once at spawn: snapped values; a column per element.
- `kr` — pool-invariant per frame: `Closed` signals referencing no per-bullet columns; hoisted, computed once per pool per frame.
- `ar` — per bullet per frame: everything else.

Rate inference is shape inference; hoisting is automatic. The REPL uses inferred rate to label parameters: `kr` knobs affect all live bullets immediately, `ir` knobs affect new spawns only — and the UI can say so. (Backend observation: `Closed` dyns referencing only bullet-local τ are constant across same-tick cohorts — an effective rate between `kr` and `ar`, exploitable per birth cohort.)

---

## 4. Dyn: motion as signal composition

**[spec]** A **dyn** is `Signal Pose` — the trajectory of *one* position (with orientation) over time. Not privileged: position is a signal the express-action hands to collision and rendering.

- Constructors: closed-form (`f(t) → Vec2/Pose`, with linear and polar variants; `pos`/`vel`/`acc` variants are `integrate` applied 0/1/2 times — vel/acc are scans by construction, with self-state slot bindings per §3), and procedural per-tick (`scan` directly). `lerp`-family speed profiles in vel slots are the common Scanned case; see the F1 lint in §9.
- Static poses / pose arrays (e.g. `(circle 8)`) are **not dyns** — they are values. Promotion `pose → Closed(λt. pose)` (constants are the unit of both the signal and broadcast algebras) lets them serve in frame slots without ceremony.

### Frames: `in-frame` **[spec]**

One ordinary binary function is the core:

```
in-frame : Signal Pose → … → Signal Pose → Signal Pose   -- pointwise SE(2) composition
-- variadic: frames form a monoid, so (in-frame f1 f2 body) folds as
-- (f1 (f2 body)), outer to inner; the last argument is always the body.
-- The flat spelling of applicable-frame nesting; fewer than 2 args is an
-- arity error (never a silent drop).
```

- Associative, with `(still)` as unit ⇒ dyns form a monoid; deep hierarchies are folds; nesting *depth* is programmable with ordinary list code.
- Partial application — or directly, **frames are applicable** (below) — yields frames-as-transformers, the card-algebra building block ("this card, but mounted on the boss").
- **Frame sugar, two type-driven forms**, both desugaring to the same canonical `in-frame` node (resolution is static, by unification — never runtime dispatch, or "sugar is only sugar" breaks):
  - *Trailing child*: any head-word whose return type unifies with `Signal Pose` (incl. `Array Pose`, incl. point→pose promotion) accepts one extra trailing dyn/action argument: `(circle 5 child)` ⇒ `(in-frame (circle 5) child)`. Collision rule: declared signatures win; the sugar overload is considered only when no declared overload unifies.
  - *Applicable frames*: a list whose head types to `Signal Pose`/`Array Pose` applies as `in-frame`: `((rot base) child)`, `(anchor child)` for a let-bound frame, `([p1 p2] child)` for a literal frame array. Vector literals themselves stay pure data; only list forms apply. Lint: point→pose promotion in head position warns.
  - The child slot is single; an array child multiplies per §5's root-to-leaf product. The desugared application tree is canonical — it is what serializes, what card-upgrades transform, what the REPL prints.
- **Frames are ambient for their bodies at every level**: expression-level `in-frame` (and applicable-frame application) evaluates its frames left→right *extending the ambient frame*, then evaluates the body under the extended ambient, then composes values — so ambient-reading forms (`aim`) see the **lexical** frame composition, uniformly with the action-level distribution law below. Without this, `(in-frame (pose src) ((aim $player) …))` textually encloses the aim but the aim measures from the outer origin (the duel-card bug: stand under the source and it fires up). Signal-valued frames extend the ambient by their spawn-instant pose (aim snapshots at fire); array frames and trailing-child sugar do not extend (per-element ambients are genuinely ambiguous — aim there needs the element frame made explicit).
- **Action-level `in-frame` is a distribution law, not new semantics**: `(in-frame f (par a b)) ≡ (par (in-frame f a) (in-frame f b))` (same for seq/loop/race); `(in-frame f (spawn d m)) ≡ (spawn (in-frame f d) m)`; non-spawning actions ignore it. The frame pushes through control combinators and lands on spawn dyn-roots — macro-eliminable, kept as a canonical node for compactness. Consequences: a signal-valued frame reaching a spawn is a spawn argument (snapped by default, `live` to track); distribution is lexical, so ambient frames do not leak into embedded patterns (the scope adapter decides, §10) **nor into `fn` bodies** (manipulate callbacks spawn in world coordinates — a leaked frame would double-anchor; lexical distribution stops at lambdas, verified by test); and **patterns don't self-anchor** — the caller applies the frame (`(boss-frame (bowap))`), which is where DMK puts `roott` too. The converse escape is **`(in-frame :world body)`**: RESET the ambient composition instead of extending it — boss-side patterns anchor at the caller's anchor by default, and the player kit under the same card opts out explicitly (`(par (in-frame :world (reimu)) (mima))`).
- **Two-operation algebra — the complete dissolution of V2RV2**: `in-frame` composes *through* frames; `+` on point signals translates positions (θ untouched) **in whatever frame the `+` lexically appears** — add inside a rotation frame and you have DMK's rotational `rx,ry`; add outside the `in-frame` wrapper and you have nonrotational `nx,ny` (world-frame terms, e.g. gravity staying world-down inside a rotating hierarchy). No offset constructor exists — pure translation *is* point-addition; V2RV2's rotational/nonrotational split is nothing but the position of `+` in the tree. `translate-only(child)` / attach-to-point remains the third citizen: inherit position but not rotation.
- Reparenting (option released from carrier) = rematerialization: snap current world pose, swap to a world-frame dyn. Events and frames share one escape hatch.
- **Entity motion is remat, not frame mutation.** The boss/option is an expressed entity (renders, collides, has hp); patterns anchor to its live pose signal (kr). `(move dur ease dest)` is derived: `(seq (remat self :motion (fn [exit] (ease-seg ease dur (:pose exit) dest))) (wait dur))` — one frame-stamped remat appending a closed eased segment (C⁰ by construction), then a blocking wait; non-blocking is `(fork (move …))`. Entity trajectories are ordinary piecewise-Closed segment histories.
- Cost note: bullets in a pool share tree shape ⇒ pool-at-a-time evaluation vectorizes each level; `kr` levels (e.g. the boss frame) are hoisted.

### Orientation policy **[spec]**

θ is **derived by default, materialized on demand**:

- Default pose θ = heading (direction of motion): analytic/finite-difference derivative for `Closed` (itself `Closed`), one extra prev-position column for `Scanned`. Derivation never changes a signal's constructor classification.
- **Spawn tick**: inherit θ from the emitter frame (snapped). For standard dyns, initial velocity points along emitter aim, so the inherited value is what the derivative converges to — the definition is continuous.
- Degenerate motion (zero/near-zero velocity): hold last well-defined heading (`Scanned`) or fall back to frame θ (`Closed`). Policy in five words: *inherit from parent, refine by motion*.
- Facing (sprite orientation) is **meta**, defaulting to pose θ, overridable — no definite relation between facing and motion is assumed. Whether the override is frame-relative or absolute is **[decide]** (the cradle translation reads naturally as frame-relative).
- Storage: most bullets never parent anything and their facing is consumed only by rendering ⇒ compute heading in the render pass from the velocity column; pay the θ column only for poses used as frames. Conceptual model "every position is a pose"; memory model "θ on demand."
- Angle caution: lerp/average/smoothing on θ columns need wrapping-aware treatment (shortest-arc or unit vectors); raw `+` broadcasts fine. `slew`/`smooth` are angle-aware.

---

## 5. Broadcasting and arrays

**[spec]**

- Standard array-language semantics: most functions broadcast elementwise over arrays; array-of-`f(t)→a` interchangeable with `Signal` of array where shapes agree.
- **Zips cycle**: shorter arrays cycle rather than error (SC multichannel expansion cycles; DMK color lists cycle; the corpus exploits it deliberately). Scalar lifting is the length-1 case — one rule subsumes lifting, exact zip, and palettes. Cycling is **axis-aware, never flat**: after leading-axis alignment, cycling happens within an axis, never across (flat cycling over a product would stripe across sub-arrays and silently produce garbage). Lint non-divisor lengths on finite axes (7 into 9 is probably a bug; 3 into 8 is idiomatic).
- Same principle for indexed access: **`nth` is cyclic** (index mod length); strict bounds are the marked case (`nth-strict`). "Arrays are cyclic" is one principle covering zip, index, and lift.
- Spawn arguments (`dyn, meta`) broadcast likewise; atoms lift.
- **Frame multiplicity is tree shape, not an operator.** Ring-of-fans = array of 8 frame dyns, each carrying a 3-element child array = 24 bullets; multiplicity per spawn = product of array sizes along the root-to-leaf path, statically readable. Under the desugaring this is `map (λf → in-frame f fan3) (circle 8)` — ordinary map. Pairing i-th parent with i-th child is ordinary `zipWith in-frame`. No special broadcasting regime for frames exists.
- **Meta arrays bind to the leading axis, period.** To target a deeper axis of a product spawn, write that axis's length explicitly — `(nth [:blue :green :teal] (iota 6))` is a 6-vector (cyclic `nth` broadcasts over `iota`) and binds to axis 1 by length. Length-based leading-first matching without this rule is ambiguous under cycling (a 3-vector meant for a 6-axis also matches a 3-axis). Possible future sugar: `(on-axis k xs)` **[decide]**.
- Spawn combinators are arithmetic on pose arrays: `(circle n)` = θ column `(iota n) × 360/n`; spread = `+` on a θ column; aimed fan = `snap(angle-to player) + centered-offsets`. Formation vocabulary is stock, not core: `(arrow n back side)` (the image of DMK `bindArrow` + `frv2`), `(fan n step)` (centered), sign vectors `[1 -1]` (the image of `bindLR`) — DMK's auto-bindings are formation *data*.
- **Scan sharing is explicit in the canonical tree**: a scan is fresh state per element (own column; vectorizes naturally) unless wrapped in a `shared(...)` node marking one instance referenced by all elements. The surface convenience — a let-bound scanned signal referenced in multiple places reads as shared — *desugars to* `shared`; the lexical rule is sugar only, so tree rewrites cannot silently change state identity. (For `Closed` signals the distinction is moot — stateless per-element instances are indistinguishable from a shared one; identity is a `Scanned`-only concern.)
- RNG is **counter-based** (`rand(seed, path, k)`, Philox-style): element k's randomness independent of evaluation order — required for array spawning, scrubbing, and rewind to coexist. Surface `(rand lo hi)` / `(rand-int lo hi)` / `(randpm1)` key implicitly off spawn path + element index. DMK's unsafe-`rand` vs bullet-seeded-`brand` distinction does not exist here — all randomness is replay-safe by construction.

---

## 6. Spawning and expression

**[spec]**

- `(spawn dyn meta)` is an **action** (never a signal); the anchor frame is the dyn's root composed with the lexically distributed ambient frame (§4). Arrays broadcast per §5. **`spawn` returns Bullet handles**, and consequently **`let` defers action-valued bindings to scheduler reach-time**: in `((pose P) (let [stars (spawn …)] …))` the spawn executes when the let is *reached* — inside the ambient frame the distribution law owes it — and the handles bind then; pure bindings are unaffected. Handles (an array matching multiplicity): the control layer may hold them; `manipulate` accepts a handle where it accepts a query (a handle is a degenerate predicate); dead handles are no-ops (generation-safe). This dissolves the hoist-index-into-bullet-state + persistent-control + per-frame-predicate idiom whenever the trigger schedule is static; queries remain the mechanism when triggers read per-bullet runtime state — and are the vectorizable path.
- **Express only what renders/collides.** Emitter anchors, bases, guide trajectories live as unexpressed signal data; only expressed entities consume pool slots and collision. (DMK's simple-bullet vs BehaviorEntity split, derived rather than special-cased.) **Guides dissolve accordingly**: DMK's `guideempty2` subsystem (invisible bullets + per-frame channel recording + keyed reads) is `in-frame` with an unexpressed dyn — a level of the frame tree that renders nothing and consumes nothing. §10 extraction is only needed when a guide trajectory crosses an action-tree boundary. Likewise "summons riding a guide" is `(in-frame guide (fork …))`.
- **Extended entities via axis materialization**: simple bullet = point sample of the pose signal; **pather** = trailing time-window of the trajectory materialized as geometry (procedural hitbox from remembered points); **laser** = materialization along a parameter axis `u` at fixed t of a shape signal over `(t, u)` (`u` slot-bound like `t`, §3). Corpus-validated surface: `(laser shape? {:warn … :active … :u-max … :resolution … :width … :while …})` — shape optional (default: straight along frame +x, `u` in world units); `:warn`/`:active` are the lifecycle window; `:u-max` may be a signal (DMK `varLength`); `:resolution` is a render-contract sampling hint, not semantics (DMK `stagger` decoded: texWidth = length/stagger); `:while`/`:until` accept predicates over live signals. For nonpiercing lasers, blocking (world geometry feeds back into extent) is necessarily `Scanned`. Materialization-to-polyline is a core primitive; this dissolves the laser/pather geometry contract into the language.
- Lifecycle as signal (SC `doneAction`): cull conditions — lifetime elapsed, off-playfield, fade-complete — are done-action nodes on the entity's signals, giving the compiler lifetime visibility for pool sizing. `(cull b :soft)` is the fade-out variant. Populations are dynamic (express appends, cull deletes): runtime arrays are compacting streams, not fixed shapes.

---

## 7. Meta

**[spec]** `meta` is a record of tags; **any tag may be signal-valued** — constant (snapped), `Closed` (`:hue m"60*iota(6) + 120*t"`, scale-in envelopes, fades), or `Scanned` (proximity flicker). One evaluation story for motion and appearance; the render contract samples tag signals (DMK's own hueshift docs forbid `rand` in render functions — the same purity discipline, by comment where ours is by construction). Tags interact with capture like everything else (snap vs live). Gameplay-meaningful tags (hp, team, graze-state, damage records) are ordinary columns addressable by query (§9).

- **Style is a structured record, never a signal.** `{:family :gem :color :yellow :variant :w}` — family from a host-declared registry carrying collision class + render class (unknown family fails at card load: the render contract's load-time check); color/variant as keywords. **Pool identity = the interned record** (DMK's startup pool product, derived); **style is `ir`** — it determines SoA residency, and residency changes are events (remat-level pool migration), never signals. Queries become typed predicates over axes (`(= :family :star)` replaces wildcard strings); card recolor = `assoc` on the `:color` axis. Animatable appearance stays in separate signal tags.
- Tags are also the **export surface** (§3): an entity's outward-facing continuous data (boss damage-mult, healthbar opacity) is a signal-valued tag read by the host or other systems.
- **Entity sounds are lifecycle-event data, not meta verbs.** The sim emits lifecycle transition events (spawn, warn→active, cull) regardless (§6, replay); host audio subscribes. Defaults bind family × transition → cue in host config (DMK `dSFX` = "use family defaults", zero language surface); custom audio is `{:cues {:spawn … :active …}}` — pure data decorating the entity's lifecycle events. Action-time sounds (per-volley fire inside a loop) remain ordinary outbound `(event …)`: cues are for entity lifecycle, events for control flow.

---

## 8. Patterns and the action layer

**[spec]** The language is two layers with different frequencies and disciplines:

- **Hot layer** (signals): pure, per-bullet-per-frame, loop-free ⇒ statically bounded frame cost (a hostile card can slow, not hang). Compiles to pool-at-a-time bytecode (dev/REPL) or AOT native (shipping).
- **Control layer** (actions): Turing-complete, per-event frequency, tree-walking interpreter with a per-frame fuel budget.

**Signals are pure; effects live only in the action tree** (see §2 type discipline for the enforcement story).

- `(defpattern name [param default …] body)` — patterns are named, parameterized (difficulty/rank as arguments), and exposed to the host (`engine.run(pattern)`).
- Combinators: `seq`, `par`, `race`, `(wait dt)`, `(wait-for event-or-predicate)` (predicate-waits evaluate per tick; DMK's `whiletrue` is a pause = `wait-for` at a loop head), and **`(fork action)` — dynamic `par`**: start a child adopted by the nearest enclosing concurrency scope (`par`/`race`/phase); the scope's completion waits for adopted children, its cancellation cancels them. Static child list → `par`; dynamic branch count (from inside a loop) → `fork` (Trio's nursery `start_soon`). Needed because DMK async repeaters do not wait for their children.
- **Iteration trichotomy**: arrays (simultaneous fan-out — no loop at all) / `dotimes` (sequential indexed, pure per-iteration) / `loop`/`recur` (sequential fold, explicit carried state). A loop containing **no temporal actions is a pure fold and evaluates inline** when used for its value (rejection sampling, closed-form searches) — F3's fold-belongs-to-the-control-layer point, enforced: temporal actions inside a value-position loop are errors. `loop`/`recur` over `for` is semantic: fold and fan-out cannot be confused; loop state is explicit in the canonical tree (visible to serialization, card transformations, and the input-independence analysis); recur boundaries are the scheduler's cancellation/fuel/snapshot points — control-layer snapshot is "record the recur args," load-bearing for rewind.
- **`(dotimes [i n, x xs, y ys … :every dt] body…)`** — first pair is the counter (`n` may be `inf`); `:every` is the inter-iteration wait, *between* iterations, not after the last (n−1 waits — DMK's own GCR semantics, which special-cases the final iteration; the difference is observable by anything sequenced after). Subsequent pairs are **seq bindings**: each iteration binds the i-th element of its source; arrays cycle (cyclic `nth`) — DMK's repeater-level `color({…})` modifier as a loop binding, restoring the which-loop-level information a spawn-attached meta map lacks. Sources are stream-shaped (an array is the trivial cycling stream, SC `Pseq`); the pattern algebra slots in here (`(dotimes [i inf, ang (pbrown 0 360 10)] …)`), as does `(stutter n xs)` (SC `Pstutter`; DMK `colorf(xs, i/2)`).
- **Phase machines**: `(phases (label opts? process… finally?) …)` — ordered clauses, label keyword as head (homogeneous clauses need no discriminating head-word; heterogeneous items like `stages`' do). The opts map drives the implicit `race(hp, timeout, process)` *and* exports as host-facing card data (hp bar, timer, spell name; DMK's `hpi`/`type`). `finally` is the §8 finalizer as an explicit serializable slot. Semantics: a trampoline — each phase evaluates to the next label, defaulting to clause order (DMK `shiftphase`); falling off the end completes the machine (which may return a value to its embedder). `(goto label)` is a **scoped non-local exit**: cancel the enclosing phase body (finalizers run), re-enter at the label. Goto is unambiguous by construction: it *exits structurally* (cancellation semantics already define in-flight children, forks, finalizers) and *enters only at phase heads*, and it is **scoped strictly to the innermost lexical `phases`** — outer labels are not in scope, so an embedded card machine cannot hijack its host's flow; inner machines communicate by completing. Same-tick competing gotos resolve by tree order (the tie-break `race` needs anyway). Labels, not indices (DMK `shiftphaseto 4` breaks on phase insertion; labels survive tree transformations). Machines nest.
- Phases are structured concurrency: `race(hp-depleted, timeout, attack)` *is* a DMK phase/spellcard; cancellation propagates scope-wise down the action tree (DMK's cancellation-token hierarchy, rediscovered — the `seq/par/loop/race` tree *is* the token tree). Losers of a `race` run `finally` blocks: soft-cull with fades, item spawns, end-of-phase bookkeeping.
- Triggered controls: `(par pattern (seq (wait-for (< hp 0.5)) (manipulate query f)))`. Event vocabulary: collisions, grazes, thresholds, host-injected events.
- **Pattern timelines inherit the closed/scanned split — the property is input-independence, not closed form.** A pattern whose waits are closed durations, with no event-waits and no injected-signal dependence, has an *input-independent* timeline — evaluable without an input tape (a pure control-layer fold is deterministic; evaluating it *is* static computation, so `loop`/`recur` accumulator patterns qualify). One `wait-for` on an event, or any injected dependence, makes the timeline tick-emergent / tape-relative. "When does the action happen" always has exactly two answers: at a time you can compute, or at a tick you must reach — never "whenever the evaluator looks."

---

### Scope cancellation **[spec]**

- **`(until pred body…)`** — structured cancellation: run body; the tick `pred` first holds, the body's entire task subtree (loops, `fork`s, nested `par`s — everything started under the scope) dies together. Forked tasks inherit the guard at fork time, so cancellation follows the dynamic tree, not the lexical text. This is the §8 phase-end semantics DMK gets from phase-token propagation: `(until (<= $boss-hp 0) (spell-2))` cancels the spell's guide rigs and turret forks the instant the health bar empties; in-flight bullets are inert data and persist (clear them explicitly with `(cull)` if the phase edge wants it). `until` is the degenerate case of `race` — `(race (wait-for pred) body)` — which remains the general (not yet needed) form. Guards are ordinary predicates over channels/cells: deterministic, scrub-safe, evaluated at a canonical point per task per tick.
- **`(clamp lo hi dyn)`** — position clamp (playfield walls). Output-clamps the pose; for integrated children (`vel` under unrotated const frames) the **integrator state** is clamped after each step, so pushing a wall banks no phantom distance — you slide, and reversing moves away immediately. The piloted-rig companion: `(clamp c[-3.8 -4.4] c[3.8 4.4] (in-frame start (vel …axes…)))`.

---

## 9. Manipulation, queries, events

**[spec]**

- `(manipulate query-or-handle callback)` — queries are predicates over columns (style axes, tags, position, **bullet-local age**), cutting freely across birth structures; handles from `spawn` (§6) are degenerate queries.
- **`Bullet` is a fixed data type**: built-in columns plus one **escape pointer** keying optional custom state in a sidecar table. The cost split is compiler-visible by inspection of the callback body: a callback touching only built-in columns compiles to a masked in-place SoA update (`pool[pred] = f(pool[pred])`) — hot-layer, vectorized, no fuel; a callback dereferencing the escape pointer, or spawning actions per bullet, runs on the control layer and bills fuel per matched bullet. DMK's batchable-controls vs SM-per-bullet split, recovered as an inferred property of one API.
- **Rematerialization** is the blessed event mechanism: snap current values (pose, velocity, tag samples) into fresh spawn-captured constants, swap the signal. Uses: re-aim-once (class (c)), reparenting, reflection (the corpus `switch(reflected, vel, …)` per-bullet-flag idiom is hand-rolled remat paying a hot-path branch; ours is one event-driven signal swap), returning a bullet to `Closed`/scrubbable-land after an event, closure-splicing made explicit and cheap.
- **Epoch model** (remat clock semantics): every rematerializable slot carries an epoch column; birth time is just each slot's initial epoch. `(remat bullet slot new-signal)` writes `epoch := now`; the new signal runs on `τ = t − epoch`, starting at 0 at the event tick. Initial conditions are passed explicitly — the remat call snaps what it needs and hands it to the new signal as ordinary `ir` constants — so C⁰ continuity holds by construction; C¹ is a convention of stock helpers (`remat-straight = linear(snap pos, snap vel)`). Remat is **per-slot**: a half-finished fade keeps running on its own epoch when the motion slot is swapped. A bullet's history is a list of `(epoch, signal, constants)` segments — piecewise-`Closed` bullets remain fully scrubbable, and the segment record is exactly the replay-log entry. `stages` (§3) is this same model statically scheduled. Ancestor clocks stay orthogonal: remat moves only the local epoch.
- **The F1 lint — no silent strengthening**: velocity constructors with closed-form-integrable integrands (constants, piecewise-affine `lerp` profiles) are `Scanned` as written but have `Closed` equivalents; the compiler never rewrites silently (a scan stays a scan — predictability) but lints with the suggested closed rewrite. This matters compositionally: one `Scanned` guide contaminates every rider by contagion (the cradle: one `vel` makes 126 petals unscrubbable; the closed rewrite restores the whole tree).
- Scanned state is ordinary columns + step functions ⇒ snapshots are memcpys; manipulation of scanned bullets is writes to scanned state or signal swaps.

### Colliders and contact effects **[spec]**

- **Colliders are archetype data**: an entity owns a *set* of colliders `{shape, layer, radius}`, interned with the style/spawn like everything in §7 — per-instance storage is just the owner pose; world-space collider positions are generated during the collision pass, never stored. Defaults derive from team (`hostile → damage + graze ring`, `:team :player → shot`, `:team :enemy → hurt`); `:hitbox r` resizes the primary; `:colliders [{:layer … :r …} …]` replaces the set. No cap on multiplicity — cost scales with total collider count, and the layer matrix (not a spatial structure) keeps danmaku linear: the big set (hostile colliders, thousands) only ever tests against the player's few colliders.
- **The interaction matrix maps layer pairs to engine callbacks**: `damage × player-hurtbox → hit`, `graze × player-hurtbox → graze` (once-per-bullet latch), `shot × hurt → damage resolution`. Hit and graze are not special — they are rows of the matrix.
- **Three-tier frequency stratification** (the load-bearing observation): *checks* are per-pair-per-tick — hot, engine shape tests only. *Contacts* are rare — engine effects (decrement, kill, latch, counter, emit event) whose **parameters may be card-defined pure functions** evaluated at contact with both entities in scope: `:damage (fn [self other] …)` receives contact maps (`:pos`, contact `:vel` by finite difference, `:hp`, `:team`, `:family`), so damage-per-bullet-per-enemy is a function, not a layer explosion. *Reactions* are control-layer — cards respond asynchronously via events (`wait-for`, derived channels `$graze` / `$enemies`). The only thing banned at contact is control-layer actions; event emission is the sole crossing into card code.
- **Shapes**: circles in the dense fast path; lasers derive capsule chains *per tick from the same sampled curve the renderer draws* (the beam you see is the beam that hits) — active-window-gated, warn phase has no hitbox, beams persist through hits. Heterogeneity is confined to the rare shape.
- **hp and death are not special** — the same dissolution as hit/graze, one level down. hp is just the first *user-defined column* (§9's sidecar table): `:hp n` initializes it, `:cols {:armor 2 …}` adds more, and contact damage is nothing but a **column write** — the contact path does not know what zero means. Death is a **standing trigger**: an edge-fired rule over an entity's own columns, `col ≤ threshold → effects (event, cull)`, whose once-only latch is *itself a column* (so it snapshots and scrubs with everything else). `:hp n` synthesizes the default rule `hp ≤ 0 → cull + event :died`; explicit `:triggers [{:col :hp :leq 600 :event :phase-2} …]` replaces it. What this unifies: enemy death, **DMK's HP-gated boss phases** (a non-culling threshold feeding `phases` via `wait-for`), enrage-at-50%, player lives (a `lives` column decremented by the hit effect, gated at 0) — each a column plus a rule, none engine code. Predicates stay pure column comparisons (the masked-SoA query shape from above); Turing-complete reactions listen to the emitted event on the control layer. Trigger evaluation order is canonical (entity, rule).
- **The player is card content, not engine code**: a rig pattern — an entity whose motion is `(live $player)`, hurtbox a `:player-hurt` collider, lives a column, game-over its trigger. "The host mounts the player" means the host *layers a rig pattern in* (an `(add (player-rig))` riding the command tape, so card + tapes fully determine a replay — no hidden host state); characters, co-op rigs, options-as-satellites are different cards, not engine changes. More generally **the host/card boundary is a per-game contract, mediated entirely by channels**: the host may validate resources and inject only legal events (`$bomb-ok`), or inject raw input (`$bomb-pressed`) with the stock logic living in the rig; likewise movement — a host-integrated `$player` position, or raw axis channels integrated by the rig's own motion in the vel domain. The engine fixes the *mechanisms* (channels, layers, columns, triggers), never the split.
- **Everything writes World** (counters, events, latches, columns), so the whole gameplay layer snapshots and scrubs with the timeline; contact and trigger resolution order is canonical so replays agree. Events carry positions; renderers may draw effect flashes statelessly from the event log — they replay under scrubbing for free.

---

## 10. Patterns as data

**[spec]**

- **Guide objects are first-class where needed, dissolved where not**: within one pattern, a guide is an unexpressed frame level (§6) — no extraction machinery. Extraction — positions (all, or by query) from a pattern as `Signal (Array Pose)` — is for trajectories crossing action-tree boundaries (consumed by other patterns or later actions).
- Extraction typing is derived, not legislated: extraction from an **input-independent pattern** (input-independent timeline + closed dyns) is itself `Closed` — a pure query over a timeline that exists as data (birth-time columns + closed motion), evaluable at arbitrary t, usable as a base for further closed patterns. Extraction from anything touching live injected signals or event-waits is `Scanned` (well-defined only relative to the input trace).
- Cards as trees: the canonical (desugared) s-expression form serializes; upgrades are tree transformations (macros); fusion/deck operations are tree composition; frames-as-transformers and pattern-transformers (the SC Patterns algebra) are the manipulation vocabulary.
- **Imports**: `(import "relative/path.dmk")` on its own line splices that card's text at that position — recursively, **include-once** (canonical-path dedup), so diamond imports yield one copy and the importing file's later definitions shadow imported ones by ordinary def ordering. Corpus-faithful (ph_boss2_mima imports ph_ref upstream). Expansion happens at file-load time, so the wire/card source stays self-contained — the tapes and live eval need no path context. Convention: imports go *after* the card's main `defpattern` so it stays the file's first (= default) pattern. This is deliberately a *textual* mechanism; namespaces wait until collisions hurt **[decide §13]**.
- **Pattern embedding is scope-explicit.** A top-level pattern used as a subtree must be wrapped in an explicit scope adapter: one binds the embedded pattern's state and channels into the embedding pattern's scope, the other keeps them pattern-local; bare embedding is ill-formed. The adapter node lives in the canonical tree, so card transformations cannot silently change sharing or capture — the composition-level analogue of the `shared` node (§5), and the boundary ambient frames do not cross (§4).
- **The card subset is a type-level characterization**, not a convention: serializable/scrub-safe = input-independent timeline, no `wait-for` on host events, channel I/O limited to declared injected signals. Boss scripts are card trees plus channel I/O (§3) and may forfeit these properties; the compiler can say exactly where.

---

## 11. Syntax

**[spec]** EDN canonical form (BDSL is an s-expr language in curly-brace cosplay: head-word + typed arguments; BDSL2's blocks-as-values is `progn`). Static type unification with overload resolution and implicit conversions over the tree (BDSL's actual innovation), retained over the EDN surface. Surface syntax is a pluggable skin; node types + typing rules are the spec.

| Form | Meaning |
|---|---|
| `(f args…)` | evaluated form (function application / combinator / macro) |
| `[a b c]` | **array literal** — first-class, broadcasts per §5; never evaluated as a call |
| `{:k v}` | meta record (§7) / option map |
| `:keyword` | tag keys, channel names, style axes, phase labels |
| `c[x y]`, `p[r θ]` | coordinate literals — reader shorthand for `(cart x y)` / `(polar r θ)`; elements are ordinary expressions |
| `m"…"` | infix math reader macro (below) |
| `symbol` | binding reference; `t`/`u` reserved and slot-bound (§3) |
| `$name` | channel read (injected or derived, §3) — own namespace; snap/live rules apply |
| `(import "path.dmk")` | textual include, include-once (§10) — own line, top level |

The vector/list division is load-bearing: `[0 120 240]` (data, broadcasts) is lexically distinct from `(circle 3)` (evaluated form returning an array), and every canonical tree is readable as pure data without an evaluator — what cards-as-data needs.

**`m"…"` — the math macro, parse-only.** Everything inside has an s-expr equivalent and parses to the same canonical tree; the macro adds zero semantics, and the canonical/serialized form is always the parsed tree. Grammar inside: infix `+ - * / ^ %` and comparisons with PEMDAS; function-call syntax `f(a, b)` for any in-scope function (`sine(1, 0.2, t)`, `iota(6)`, `live(mode)`); channel reads `$name`; array and coordinate literals; `$(…)` splices an arbitrary s-expression (`$` followed by `(` is a splice, otherwise a channel). Free symbols resolve against the enclosing lexical scope (an alternate parse, not a binding boundary). Operators broadcast per §5 — `m"[0 120 240] + 80*t"` fans out. Use it for expressions with several binary operators; single calls stay s-expr. Backtick is *reserved* for quasiquotation (card macros).

**Units: one canonical unit per quantity + source-named conversion functions; no unit-tagged literals.** A conversion is named for its *source* unit: `(ticks 8)` = 8 physics ticks as canonical seconds; `(rad x)` = radians as canonical angle. The canonical unit has no function. **Angles are canonically degrees** (the entire corpus authors in degrees; DMK's `cossindeg` is fossil evidence — canonical degrees also deletes the parallel `*deg` function family, and angle→unit-vector is just `p[1 θ]`). **Time is canonically seconds** — unlike degrees/radians, tick-canonical would bake the timestep into card data, and continuous time is semantically load-bearing (`Closed (t → a)` scrubbing, tunnel arc-length); `(wait (ticks 8))` is exact on the grid, which is why control code authors in ticks.

Arithmetic `+ - * /` is variadic (n-ary fold; unary `-`/`/` negate/reciprocate) and broadcasts per §5 — `m"60*iota(6) + 120*t"` is six signals. Array builtins: `(iota n)`, `(range a b step)`, `(without x xs)`. Reading property preserved: `((rot base) (circle 6 (vel … (circle 7 (polar r θ)))))` reads outside-in as coarse-to-fine motion — anchor, ring, carrier, wiggle — matching how designers think.

---

## 12. 3D and alternative parametrizations

**[spec]** No dimensional lifting of the pattern language. 2D patterns + **emitter-frame embedding**: patterns execute in local oriented planes/cylinders/sphere-surfaces; a small vocabulary positions/orients/animates those frames in 3D (the NieR model: 2D patterns, 3D placement). Tunnel game: pattern space is `(θ, s)` on the unrolled cylinder; the player's tunnel pose (position + tangent) is a host-injected pose signal serving as the world frame; patterns parametrized on `s` remain closed ⇒ backward evaluation when the player backtracks; classes (a)–(c) reactivity survives non-monotone `s`; only class (d) needs monotone-section quarantine.

---

## 13. Open decisions **[decide]**

1. Ancestor-clock operator design (reaching pattern/global time from bullet scope) and its interaction with extraction. (The remat side is settled by the epoch model.)
2. Event vocabulary enumeration and the concrete channel API (injected/exported signal declaration, outbound event channels, host-handoff commands — §3 fixes the four-construct shape; the prototype has positioned events on a bounded shared log with per-snapshot cursors, but no declaration/manifest enforcement yet).
3. Exact column set of the fixed `Bullet` struct (the boundary mechanism — built-in columns + escape pointer, cost split inferred from callback bodies — is settled, and user columns exist in the prototype as an inline sidecar; the built-in inventory is not fixed).
4. Angle representation for θ columns (wrapped float vs unit vector) — storage-level; canonical-degrees is surface semantics either way.
5. Facing override semantics: frame-relative vs absolute (§4 orientation policy; the cradle translation reads as frame-relative).
6. `(on-axis k xs)` meta-targeting sugar vs the explicit-length convention (§5).
7. Blocking-laser feedback contract (world geometry → extent; necessarily `Scanned`); also collision width for lasers (the prototype uses a constant beam half-width; `:width` should feed it).
8. `(with {channel value} body)` residual details (the core semantics are settled in §3 — the in-frame-for-channels distribution law): which *derived* channels are overridable, nesting/shadowing rules, override values that are themselves signals of taped inputs. Unimplemented in the prototype.
9. Derived-channel vocabulary and cost model (§3). The prototype's de facto set — `$nearest-enemy`, `$graze`, `$enemies`, `$lives`, `$boss`, `$boss-hp` — works but is ad hoc; the `:boss`/`:pilot` column conventions especially want a principled "expose column of tagged entity as channel" rule.
10. **The interaction matrix as data** (§9): rows are engine-fixed; new pair-classes (bullet-cancel `shot × damage`, item pickup) and the hit effect's by-name knowledge of the `lives` column both want rows declared as card/host data `{layers, effect, params}`.
11. **`race` (general form) and the `phases` machine**: spec'd (§8) but not in the prototype — `until` covers phase-end cancellation; `phases`/`goto`/opts-as-card-data and same-tick goto tie-breaks are unexercised by running code.
12. **Pattern-embedding scope adapters** (§10): spec'd, but the prototype's callable patterns embed bare (defaults only, no arg passing, shared cells) — the adapter surface and its parameter story are undesigned.
13. Trigger predicate generality (§9): the prototype fires on single-column `≤` crossings only; upward crossings, multi-column predicates, and rate conditions are open.
14. Import namespacing (§10): textual include-once suffices now; a namespace/alias story if cross-card collisions start hurting.

Settled since the first draft (see cards/translations/NOTES.md for the record): snap-by-default boundary + `live` marker; construction-vs-reference of scans (`shared` nodes); scanned-state limits (fixed `Bullet` + escape); extended-entity surface (`laser` options, `:resolution` as render hint); style/color merge (structured records); phase transitions (`phases` + scoped goto); iteration/vocabulary surface (EDN, `m""`, units, `dotimes` seq bindings, formation/stream stock). Settled by the prototype: let-deferral of action bindings (F17); frames stop at lambdas (F18); difficulty is the rank channel + pure loops fold inline (F19); derived channels (F20); def-resolution hygiene under slot binding. Settled by the gameplay/host sprint: colliders as archetype data + layer matrix + contact-time pure functions (§9); columns + edge-triggers dissolving hp/death/lives/phases-gates (§9); the player as card content and the channel-mediated host contract (§9); `until` scope cancellation and `clamp` with integrator-state semantics (§8); frames ambient at every level + variadic `in-frame` + `:world` (§4); imports (§10); raw-input channels on the tape (replays include the keyboard).

---

## 14. Provenance map (concept → source)

| This language | Source |
|---|---|
| Two-layer hot/control split | DMK GCXF/bullet-fn split; SC sclang/scsynth |
| `Closed`/`Scanned` constructors | DMK `roffset`/`rvelocity` + the no-un-integration theorem, reified |
| `snap` / spawn capture | DMK GCX environment frames, reduced to one operator |
| Slot-bound `t`/`u`; vel/acc self-state | BDSL movement functions / DMK `bpi`, as a typing rule |
| Rates `ir/kr/ar` + inference | SuperCollider, as shape inference |
| Broadcasting/MCE + cycling | SC multichannel expansion; k/APL leading-axis style |
| `in-frame` + `+`-placement algebra | DMK V2RV2 (rotational/nonrotational offsets + angle), fully dissolved into tree position |
| Frame sugar = function; applicable frames | SC nested-UGen graphs: graph construction *is* ordinary evaluation |
| Structured concurrency phases; `fork` | DMK cancellation-token hierarchy; `race` + finalizers; Trio nurseries |
| `phases` + scoped `goto` | DMK `shiftphaseto`, disciplined: goto = exit + tail call (Steele) |
| `dotimes` seq bindings; `stutter` | DMK repeater modifiers; SC Patterns (`Pseq`, `Pstutter`) |
| `stages` / epoch segments | SC envelopes/doneAction + DMK closure-splicing, unified with remat |
| `slew`, `smooth` | SC `Slew`/`Lag`; DMK `truerotatelerprate` / beforeDraw-lerp01, verbatim |
| Control cells (`defvar`/`set!`/`live`) | SC control buses; DMK `exec`-hvar + `whiletrue`, event-logged |
| Scoped overrides `(with …)` | Clojure `binding` / React context, as card-visible tree nodes |
| Derived channels | DMK service lookups (`LNearestEnemy`), taped for determinism |
| doneAction lifecycle | SuperCollider envelopes |
| Patterns-as-data algebra | SC Patterns library; DMK guide-object idiom (dissolved into frames in-pattern, extraction across) |
| Counter-based RNG | replay/scrub determinism requirements |
| Bullet/pather/laser as axis materialization | unification replacing DMK's special-cased entities (corpus-validated: `lt` = `u`) |
| Per-slot epochs / piecewise-`Closed` remat | replay-log segment records; DMK closure-splicing made explicit |
| Symmetric channels (inject/export/events/handoff) | sclang/scsynth OSC symmetry; DMK engine-interop audit |
| Structured style records | DMK style-string pool product (SO × palette × gradient variant), interned |
| Typed trees over dynamic tags | DMK v9→v11 negative lesson (GCXU removal) |
| Collider layers/matrix; contact callbacks | physics-engine layer/mask contact systems, danmaku-specialized |
| `until` scope cancellation | Trio cancel scopes; DMK phase tokens |
| Session tapes/snapshots/command tape | design.md §11; deterministic-replay folklore (Bret Victor) |
