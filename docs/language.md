# Danmaku Pattern Language: Design Document

A language design for an engine-agnostic bullet-hell system, derived from an audit of Danmokou's (BDSL) semantics, SuperCollider's signal model, and array-language composition. Companion to *Engine-Agnostic Danmaku Core: Design Notes* (architecture/runtime doc); this document specifies the language itself.

Status: consolidated after the DMK translation exercise (`cards/translations/`), a first implementation pass (`proto/` — a Rust interpreter + player whose conformance suite runs the entire translation corpus verbatim, production boss card included), and a gameplay/host sprint (`cards/` — collision/colliders, columns/triggers, scope cancellation, the piloted-rig host contract, imports; `cards/reimu_vs_mima.maku` is the everything-at-once playable witness). Findings F1–F20 and the adopted conventions are folded in here; NOTES.md/SCANNED.md remain as the working record. The prototype's session layer additionally realizes design.md §11's tooling (input + command tapes, snapshots, scrubbing, live eval/swap/layer from the editor). Sections marked **[decide]** are open decisions.

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

**[spec]** Small, closed runtime universe; source syntax may be richer, but typed/load-time lowering assigns every live field to a fixed representation before the card runs.

See [types.md](types.md) for the target inference/elaboration pipeline. In
short: semantic types are assigned before representation classification, so
`Dyn<T>`, collider/render schema checks, and homogeneous vector/matrix
recognition are not properties of the parser or the SoA runtime layout.

- `Number` — one numeric type. Predicate masks, counts, indices, and enum-like words are schema uses of numbers/symbols, not separate source-level scalar types.
- `Symbol` — interned keyword-like atoms. String syntax is source/host-boundary syntax; runtime comparisons and storage use interned symbols.
- `Pose` = `(x, y, theta?)`. `theta = none` is a point whose facing is unspecified and may be derived by consumers that need one (curve tangents, motion direction); `theta = some angle` is an explicit frame orientation, including explicit `0`. Surface literals `c[x y]`, `p[r θ]` (§11) construct point-poses, while `(rot θ)` constructs an orientation. `(still)` names the identity pose, the unit of frame composition.
- `Figure` = `Pose | Curve | Composite ...`. A point bullet is not a primitive category; it is an entity whose figure currently evaluates to a pose. A laser/path is not a primitive category; it is an entity whose figure currently evaluates to a curve or projected samples. User-facing Touhou names such as bullet, shot, enemy, player, boss, and laser are library constructors.
- `Collider` / `ColliderData` — literal collision rows consumed by core collision: `none`, circle, capsule/polyline chains, and later analytic curve colliders. Collider layer is universal routing metadata on collider data, not gameplay state. A collider is the result of projection, not the projector itself.
- `RenderData<K>` — the single render row exposed for an entity to the host or a rendering crate for a registered render kind `K`. Rendering vocabulary is open: the kind selects a typed field schema. Core fixes the projection/transport boundary and registry/manifest mechanism, not sprite family/color semantics. If a schema needs nullable/no-render behavior, that is schema-owned data such as a host/profile-defined `:kind`, `:visible`, or `:enabled` field; the language does not reserve a no-render kind. Multiple visible parts are encoded as fields in a schema, not as multiple rows.
- `Meta` — finite typed fields attached to entity rows. Source field names are interned at load/reschema time into typed matrices, not allocated dynamically per entity.
- `EntityRef` — stable handle with generation. Returned by `spawn`, consumed by `manip`/`manipulate`, and safe across row reuse.
- `EntitySet` — ephemeral vector of live row indices from `entities-where`; stable only for the tick/view that produced it.
- Arrays/maps of the above (§5).
- `Signal a` / `Dyn<a>` — the central time-varying abstraction (§3). In target low-level APIs, static `a`, functions `t -> a`, and structures containing dyn-valued fields all coerce to `Dyn<a>` when that type is expected.
- `Pattern` / `Action` — the control layer (§8).

The target low-level entity model is:

```text
Entity = Dyn<Figure>
       * Dyn<Meta>
       * [ColliderProjector<F>]
       * RenderProjector<F, K>
```

This is the semantic model. The runtime representation is a row in dense SoA storage: dyn programs/function pointers, typed meta matrices, sampled pose buffers, collider/render work buffers, and cold projector/spec data. Per-entity hot state draws only from fixed typed storage so scanned state packs into columns and steps vectorize pool-at-a-time. Control-layer values may be richer, but anything retained across ticks must lower to the fixed runtime universe.

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

### Reserved Axis Parameters **[spec]**

`t` (and the materialization axis `u`, §6) are reserved free axis names: an expression referencing `t`/`u` denotes a dyn-valued expression, exactly BDSL's movement-function model made a typing rule. Such an expression type-checks where `Dyn<T>` is expected, or where code explicitly applies/samples it; it is not silently coerced into ordinary scalar slots. The `m"..."` reader macro does not change this rule: it is only alternate expression syntax, so `m"2 * ctx.t"` is a normal expression over `ctx`, while `m"2 * t"` is a dyn-valued expression. `t`/`u` (and any future axis parameters, e.g. ancestor-clock symbols) are **reserved** — not bindable by `loop`/`let`/params — so shadowing is unrepresentable. There are **no rate/time tags on expressions**: whether an expression is time-varying is determined by its free variables, not chosen (unlike SC's `.ar`/`.kr`, which annotates a genuine degree of freedom); the compiler infers constructor/rate, the REPL displays it, the reader greps for `t`.

Corollary (found by implementation): a `def`'d signal resolves **hygienically except reserved axis names pass through** — `(def swirl (lerpsmooth eoutsine 0 4 t 0 480))` referenced in a θ slot means a dyn over that receiving slot's axis. That is what "free `t` makes a dyn" means for named signals.

Additional slot bindings in derived domains: **vel/acc slots bind self-state** — `pos` and `dir` (and `vel` in acc slots); DMK precedent: velocity functions receive `bpi` including own location. Self-reference is feedback, and these signals are already scans, so the type story is unchanged. Homing is one line:

```edn
(vel p[10 (slew 60 90 (angle-of (- (live $nearest-enemy) pos)))])
```

### Time and clocks **[spec]**

Three clocks with explicit nesting rules:

1. Every action node gets a **local clock** zeroed at its activation (`seq`, `loop` iterations, etc. rebase).
2. Every bullet's dyn runs on **bullet-local time** `t − birth`. Birth time is a column (each slot's initial epoch, §9); emission over time (spirals) is birth-time data, not phase-locked global-t functions. Phase-locking (the ring-vs-spiral distinction) needs no operator — **resolved** (2026-07, the t09 audit): ancestor clocks are ordinary values. A pattern captures its epoch (`(let [t0 $tick] …)`, an `ir` constant) and any child signal reads the world clock live against it (`m"(live($tick) - t0)/120"`); a phase-locked ensemble is every bullet reading the same live clock instead of its own `t`. A `(live …)` read counts as time-dependence for signal deferral. Sugar naming the idiom, if wanted, is lib code.
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

**Channels split by who can know the value.** Genuinely *injected* channels carry host-only knowledge: player pose/kinematics, buttons (`$focus-firing`), tunnel `$s`, rank/difficulty (DMK's `dl` — a channel, never a pattern parameter; misclassifying it forces pointless threading through lexically-scoped `defn`s). ***Derived* channels are sim-computed world facts** — `$tick` (the world clock, which is what lets deadline columns like `iframe-until` be plain library code), `$nearest-enemy` (a spatial query over `:enemy`-tagged expressed entities relative to `$player`), hp-fraction thresholds, another entity's pose — computed per tick by the sim, exposed *and recorded on the replay tape exactly like injected channels*, which is what lets signals read them without violating world-isolation while keeping scrubbing honest. Everything else that looks channel-shaped is not one: self-entity accessors are entity-state reads; `aim` and formations are library; `snap`/`live` and counter-based `rand` are core.

**Ambient context is three disciplined forms, not a shared read-write map** (which would be DMK's GCX environment again — unmarked cross-subtree writes are both spooky action and a card-algebra hazard):

1. read-only ambient = **channels** (single writer, taped, readable anywhere without threading);
2. read-write ambient = **control cells** (below; pattern-scoped, adapter-gated);
3. **scoped overrides**: `(with {$rank 0.5} body)` — dynamic *binding*, not mutation. **`with` is to channels what `in-frame` is to poses**: the same distribution law over the action tree (pushes through control combinators, lands on spawns, which capture it for their signals' lifetimes — including `live` reads, long after the body's evaluation), the same boundary (stops at pattern-embedding adapters), the same capture rules. The ambient frame is the special case for the "where am I" channel; `with` generalizes the mechanism. `let` cannot substitute: lexical scope reaches only text you *contain*, not code you *cause* (callees resolve channels in their own definitions; spawned signals outlive the body) — and the `$` namespace makes the let-vs-with ambiguity *unrepresentable* (a channel read is never a variable reference; `let` binds bare symbols only — strictly better than reserving names, which cannot scale to an open host vocabulary). Overrides are ordinary card data (tree nodes; they serialize). Residual **[decide]**: nesting/shadowing details and which derived channels are overridable (§13).

**The host↔pattern surface is four constructs, all on named channels** — raw engine-object access does not exist:

- **injected signals** in (player pose/kinematics, rank, tunnel `s`, arbitrary host data);
- **exported signals** out — continuous pattern→host data, realized as signal-valued tags on entities (§7), no separate mechanism;
- **outbound events** out — discrete, frame-stamped, the dual of injected events; inputs tape in, events tape out, keeping the replay log symmetric;
- **host handoff** — a command plus `wait-for(host-event)` (e.g. run a dialogue scene). The only construct that makes a timeline tick-emergent across the boundary (§8), which is honest: the host's duration genuinely isn't statically knowable.

**The export surface is declarative** (resolving the mechanism half of §13.9): **`(defchannel $name expr)`** is the top-level channel declaration/default. Its expression is re-evaluated once per tick during channel refresh (definition order; each sees host channels, `$tick`, and its predecessors), with the world-query vocabulary in scope (`(count-entities q)` / `(nearest-entity q to)` over §9's manipulate query maps). A `nothing` result leaves the channel untouched, so host mocks survive as fallbacks — `$player`, `$lives`, `$enemies`, and `$nearest-enemy` are stdlib defchannels in lib/touhou.maku, not engine code. **`(bind-channel! $name expr)`** is the runtime producer: an instant action that registers an instance-scoped derived channel whose expression closes over the current environment, including pattern cells and entity handles. Bound channels refresh after top-level defchannels, so local producers override defaults. Pattern-internal state can still be published by **`(export cell)`**, which exposes a cell as a read-only channel of the same name. Per-pilot families are library/card conventions, not engine-derived names.

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

- Each segment runs on its own epoch (per-slot epoch model, §9); the optional `(fn [exit] …)` form receives the snapped exit state of the previous segment — continuity is explicit initial-condition passing, the remat philosophy. (`remat` itself accepts a direct dyn — `(remat b (linear c[b.vel.x (- 0 b.vel.y)]))` — since handles expose the live view and the boundary is *now*; the callback form is the general mechanism, required where the boundary is deferred, i.e. stages.) Exit semantics (same for `remat`): **state as of the boundary tick** — `:pos` is the current-tick position, `:vel` the finite difference over the preceding tick, `:t` the segment-local age; nothing is predicted forward, and the successor anchors at the snapped pose so there is no positional jump at the boundary.
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

- Constructors: closed-form (`f(t) → Pose`, with cartesian and polar point variants; `pos`/`vel`/`acc` variants are `integrate` applied 0/1/2 times — vel/acc are scans by construction, with self-state slot bindings per §3), and procedural per-tick (`scan` directly). `lerp`-family speed profiles in vel slots are the common Scanned case; see the F1 lint in §9.
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
- **Entity motion is remat, not frame mutation.** The boss/option is an expressed entity (renders, collides, has hp); patterns anchor to its live pose signal (kr). Core supplies `remat` plus `path`: `(path curve progress)` samples a curve dyn at progress `u`, so library code can write `(move entity curve progress)` as `(remat entity (path curve progress))` and `(move-to entity dur ease dest)` as a blocking eased segment. Entity trajectories are ordinary piecewise-Closed segment histories; non-blocking movement is just `(fork (move-to …))`. There is no boss-anchor shortcut in core; stdlib `:root` sugar targets `boss-main` directly.
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
- Spawn slots (`figure`, `colliders`, `renderers`, `meta`) broadcast likewise; atoms lift.
- **Frame multiplicity is tree shape, not an operator.** Ring-of-fans = array of 8 frame dyns, each carrying a 3-element child array = 24 bullets; multiplicity per spawn = product of array sizes along the root-to-leaf path, statically readable. Under the desugaring this is `map (λf → in-frame f fan3) (circle 8)` — ordinary map. Pairing i-th parent with i-th child is ordinary `zipWith in-frame`. No special broadcasting regime for frames exists.
- **Meta arrays bind to the leading axis, period.** To target a deeper axis of a product spawn, write that axis's length explicitly — `(nth [:blue :green :teal] (iota 6))` is a 6-vector (cyclic `nth` broadcasts over `iota`) and binds to axis 1 by length. Length-based leading-first matching without this rule is ambiguous under cycling (a 3-vector meant for a 6-axis also matches a 3-axis). **Nested meta arrays resolve structurally**: depth in the value = axis along the element's root-to-leaf path, cycling at every level, scalars broadcasting to all deeper axes — `[[:red :blue] :green :purple]` over a 10×3 spawn gives group 0 an inner red/blue cycle, groups 1–2 solid colors, group 3 the wrap. Shape disambiguates what length cannot; flat arrays keep by-length targeting. Possible future sugar: `(on-axis k xs)` **[decide]**.
- Spawn combinators are arithmetic on pose arrays: `(circle n)` = θ column `(iota n) × 360/n`; spread = `+` on a θ column; aimed fan = `snap(angle-to player) + centered-offsets`. Formation vocabulary is stock, not core: `(arrow n back side)` (the image of DMK `bindArrow` + `frv2`), `(fan n step)` (centered), sign vectors `[1 -1]` (the image of `bindLR`) — DMK's auto-bindings are formation *data*.
- **Scan sharing is explicit in the canonical tree**: a scan is fresh state per element (own column; vectorizes naturally) unless wrapped in a `shared(...)` node marking one instance referenced by all elements. The surface convenience — a let-bound scanned signal referenced in multiple places reads as shared — *desugars to* `shared`; the lexical rule is sugar only, so tree rewrites cannot silently change state identity. (For `Closed` signals the distinction is moot — stateless per-element instances are indistinguishable from a shared one; identity is a `Scanned`-only concern.)
- RNG is **counter-based** (`rand(seed, path, k)`, Philox-style): element k's randomness independent of evaluation order — required for array spawning, scrubbing, and rewind to coexist. Surface `(rand lo hi)` / `(rand-int lo hi)` / `(randpm1)` key implicitly off spawn path + element index. DMK's unsafe-`rand` vs bullet-seeded-`brand` distinction does not exist here — all randomness is replay-safe by construction.

---

## 6. Spawning and expression

**[spec]**

- `(spawn figure meta colliders renderer)` is the **low-level entity constructor**. It is an action (never a signal), and it returns `EntityRef` handles in the same broadcast shape as the spawned elements.

  ```edn
  (spawn
    (pose c[0 0])
    {:team :enemy :hp 10 :radius 0.25 :style {:family :orb :color :red}}
    bullet-collider
    (touhou-renderer))
  ```

  Type targets:

  ```text
  figure   : Dyn<Figure>
  meta     : Dyn<Meta>
  colliders : ColliderProjector<F> | [ColliderProjector<F>]
  renderer : RenderProjector<F, K>
  ```

  Stored entity meta is a flat record of primitive fields. Field values are numbers, keywords/symbols, handles, and any other fixed-size primitive atom admitted by the typed field table; each field may later be constant or signal-valued when the meta slot expects `Dyn<Meta>`. Lists and maps remain ordinary source-level data for macros, option records, and reserved spawn directives, but they do not become retained entity fields. If namespace hygiene is needed, use explicit flat names or an adapter that maps one flat field convention to another.
- Current surface: `(spawn dyn meta/projector...)` accepts merged flat field maps plus explicit collider/renderer projectors. Multiple meta maps merge per-key with later maps winning. Collider projector slots accept a projector value, a one-level list of projector values, or `nothing`; lists concatenate their projector values, while nested lists or non-projector list elements are errors. Collider projectors are explicit arguments such as `(circle-collider ...)`, `(capsule-chain-collider ...)`, inline `(collider :pose [e ctx] body...)`, or named `defcollider` projector values; dynamic collider availability is expressed with ordinary control flow in projector bodies, not dynamic spec lists. Renderer compatibility maps remain transitional while the render boundary is finalized. Touhou library helpers own friendly names such as `bullet`, `shot`, `enemy`, `player`, `boss`, and `laser`.
- Figure values may carry per-element field seeds: a trailing map on a figure constructor (`(curve {:u-max 7})`, `(linear c[0 1] {:speed 3})`) or the general `(fields fig {...})` form attaches an open field map to that element. Frames and repeaters compose through the carrier untouched (composition acts on poses; element payload is transparent to it), and at spawn each element's seeds become its initial num/dyn/sym fields, winning over the broadcast spawn meta on conflict — the leaf map is the more specific site. Constructors read the geometry keys they need (`:u-max`, `:resolution`) from the same map, so one namespace serves the figure, its projectors, and gameplay reads alike.
- `(collider :pose|:parametric [e ctx] body...)` constructs a parameterized collider projector value; the kind keyword is optional and defaults to `:pose`. `defcollider` is top-level sugar for `(def name (collider :kind [e ctx] body...))`. A `ColliderProjector<F>` is an opaque value specialized to a core figure type: card code can get one only from registered primitive projector constructors such as `circle-collider`, `capsule-chain-collider`, or by wrapping existing projectors for the same `F` with `collider`. It cannot define a new primitive projector kind in `.maku`. Literal `Collider` rows are extraction output, not values card code manipulates. The surface form may name the figure type explicitly, e.g. `(defcollider :pose bullet-collider [e ctx] body)` or `(defcollider :parametric laser-collider [e ctx] body)`. That figure type changes the static type of `e`: pose projectors see pose fields/getters; parametric-curve projectors see curve/domain/sample helpers instead. The context is non-entity projector context such as world tick, entity-local age/`t`, and any extraction pass parameters. Extraction runs separate loops per figure type, so curve-specific fields do not bloat pointlike entities. The body is ordinary pure code over those parameters; `let` is the normal way to share computed values. Collider bodies return a projector value, a one-level list of projector values, or `nothing`; `[]` and `nothing` mean no colliders, and list-of-lists is an error rather than a deep flatten. Collider constructors are normal typed functions returning opaque projector values: their argument records must have statically known shape, and each override field expects a concrete value of that field's type in the current `e`/`ctx` environment, e.g. `Num` for `:radius`/`:r` and `Kw` for `:layer`. There is no keyword-as-field-access shortcut in overrides: write `{:radius e.hitbox}`, not `{:radius :hitbox}`. `(* e.hitbox 2)` is an ordinary numeric expression, not a hidden `(fn [e] ...)`; it type-checks as `Num`. Time-dependent arguments normally read `ctx.t`/`ctx.age`; a free-`t` dyn can still be defined in the body, but it must be explicitly applied/sampled before it can feed a concrete override field. `m"..."` remains usable as syntax for those expressions. Dynamic collider data that cards query, share, export, or manipulate still belongs in meta. Empty collision and disappearing collision are ordinary projector results; `none` remains a collider variant, not option/maybe semantics, so projector bodies can disable a branch without changing type.
- `defrenderer` is not as closed as `defcollider`: it is a typed definition whose body can directly return one render row in a schema-checked map format. Like colliders, it can be specialized to a figure type, e.g. `(defrenderer :pose sprite-render [e ctx] ...)` versus `(defrenderer :parametric beam-render [e ctx] ...)`, giving `e` the associated fields/getters and letting extraction run separate loops per figure type. A render row is kind-indexed: the kind (`:sprite`, `:polyline`, `:mesh`, etc.) selects a registered schema, and the remaining fields are checked against that schema. Core does not prescribe kind meanings beyond registry, manifest, extraction order, and typed transport. Stock renderer helpers such as `sprite-renderer` or `polyline-renderer`, if present, are conveniences that project figure/meta into rows for common registered kinds; they are not the only way to create render data. Render-affecting values such as style, scale, opacity, palette, or batch keys live in meta if cards or hosts need to observe/manipulate them. A renderer that needs several logical sprites encodes the maximum shape in one schema, for example `:aux-sprite-1`, `:aux-sprite-2`, or nested fields, rather than returning multiple rows. A renderer that needs to be nullable includes whatever nullable/visibility field its schema defines.
- Projectors intentionally read the shared flat entity meta namespace. This lets composed projectors coordinate by convention: hit and graze colliders can be ratios of the same `:radius`, and renderers can draw the same `:style`/`:scale` values queried by gameplay or host overlays. The language does not retain map-valued meta as a namespace mechanism; imported projectors that would collide on field names should be wrapped with an adapter that maps one flat field convention to another.
- Projectors are ordinary composable higher-order values at the authoring level. A Touhou bullet can use `(defcollider bullet-collider [e ctx] [hit-collider (graze-collider {:radius (* 2 e.hitbox)})])`; a laser can use a projector that samples a curve figure into capsule chains, with width/active state/layer read from meta. Projector constructors take an optional known-shape override record, not positional `e ctx` arguments; they elaborate to functions over the current projector input. Ordinary `cond` chooses between projector values, lists, or `nothing` in a projector body, e.g. `(cond (< ctx.age x) [] :else hot-collider)`. A namespace adaptation combinator can rebind flat field names in the meta environment seen by an imported projector, e.g. "evaluate `collider` with `env.radius = env.enemy-hit-radius`". This preserves the expressive power of direct dynamic collider row lists while keeping non-positional dynamic state in one place.
- Render kinds are negotiated like channels: the card's kind manifest is derivable from renderer specs, and the host/render stack declares support or degradation. The prototype now implements the field side of this: the `:style` spawn map flattens into ordinary flat sym fields (`:family`/`:color`/`:variant`), `:hue`/`:scale`/`:facing`/`:opacity` are ordinary (possibly dyn) entity fields, the stock Touhou sprite row is a library deftick rule reading them, and host-facing rows are open keyed num/sym fields over structural point/polyline geometry, checked against an accreted schema. Per-kind registered schemas and manifest negotiation remain future work.
- The anchor frame is the figure dyn's root composed with the lexically distributed ambient frame (§4). `let` defers action-valued bindings to scheduler reach-time: in `((pose P) (let [stars (spawn …)] …))` the spawn executes when the `let` is reached, inside the ambient frame the distribution law owes it; pure bindings are unaffected.
- Handles are generation-safe. The control layer may hold them; dead handles are no-ops for culling/manipulation and errors only for explicit reads that promise liveness. `manip` accepts a handle where it accepts an entity set/query. `entities-where` returns row indices, not handles; those are ephemeral views (§9).
- **Express only what renders/collides/exports runtime rows.** Emitter anchors, bases, guide trajectories live as unexpressed signal data; only expressed entities consume row slots, collision work, and render export buffers. DMK's `guideempty2` subsystem dissolves into `in-frame` with an unexpressed dyn — a level of the frame tree that renders nothing and consumes nothing. Extraction (§10) is only needed when a guide trajectory crosses an action-tree boundary.
- **Figures are abstract; sampling is a projector choice.** A curve can be retained as a parametric figure, projected to a polyline for collision, projected to a mesh for rendering, or sampled by card code. `sample` is pure figure/dyn evaluation. Laser-style fill/warn/hot behavior is library/projector data over `Dyn<Figure>`, not a scalar core shorthand on `:fill`. Blocking/nonpiercing lasers, where world geometry feeds back into extent, are necessarily `Scanned`.
- Lifecycle as signal (SC `doneAction`): cull conditions — lifetime elapsed, off-playfield, fade-complete — are done-action nodes on entity dyns/projectors, giving the compiler lifetime visibility for pool sizing. `(cull h)` is the low-level removal action; soft fade/cancel semantics are library/render projector conventions unless promoted by profiling.

---

## 7. Meta

**[spec]** `meta` is a finite typed record of flat primitive fields; **any field may be signal-valued** when the receiving slot expects `Dyn<Meta>`. Constant fields are the degenerate dyn. In the spawn meta slot, `t` and the current figure are reserved slot-bound names, so a field can be a function of entity-local time and per-tick geometry. Fields interact with capture like everything else (snap vs live), and gameplay-meaningful values (hp, team, graze-state, damage, layers, style ids) are ordinary entity fields addressable by query (§9).

- Source-level stored field names are flat and unified. There is no split between arbitrary `cols` and `meta`; storage may keep numeric, symbol, handle, and pose fields in separate typed matrices, but source field access is one mechanism. Maps and lists may appear as ordinary compile-time/source values and in option records such as `:style` or projector constructor arguments, but they are not retained as entity meta fields.
- Render-affecting fields are not core vocabulary by default. A renderer projector or host profile may interpret fields such as `:hue`, `:scale`, `:facing`, `:opacity`, `:family`, `:color`, or `:variant`, but those meanings live in renderer specs/library/host config. Core only preserves typed data and the render projection boundary.
- Style-like records are host/library policy. A Touhou library can define `{:family :gem :color :yellow :variant :w}` and a renderer can intern that record for pooling; core should not require those exact axes.
- Meta fields are also the **export surface** (§3): an entity's outward-facing continuous data (boss damage-mult, healthbar opacity, debug labels, host hooks) is data read by the host or other systems through declared render/meta channels.
- **Entity sounds are lifecycle-event data, not meta verbs.** The sim emits lifecycle transition events (spawn, warn→active, cull) regardless (§6, replay); host audio subscribes. Defaults bind family × transition → cue in host config (DMK `dSFX` = "use family defaults", zero language surface); custom audio is `{:cues {:spawn … :active …}}` — pure data decorating the entity's lifecycle events. Action-time sounds (per-volley fire inside a loop) remain ordinary outbound `(event …)`: cues are for entity lifecycle, events for control flow.

---

## 8. Patterns and the action layer

**[spec]** The language is two layers with different frequencies and disciplines:

- **Hot layer** (signals): pure, per-bullet-per-frame, loop-free ⇒ statically bounded frame cost (a hostile card can slow, not hang). Compiles to pool-at-a-time bytecode (dev/REPL) or AOT native (shipping).
- **Control layer** (actions): Turing-complete, per-event frequency, tree-walking interpreter with a per-frame fuel budget.

**Signals are pure; effects live only in the action tree** (see §2 type discipline for the enforcement story).

- `(defpattern name [param default …] body)` — patterns are named, parameterized (difficulty/rank as arguments), and exposed to the host (`engine.run(pattern)`).
- Combinators: `seq`, `par`, `race`, `(finally body cleanup...)`, `(wait dt)`, `(wait-for event-or-predicate)` (predicate-waits evaluate per tick; DMK's `whiletrue` is a pause = `wait-for` at a loop head), and **`(fork action)` — dynamic `par`**: start a child adopted by the nearest enclosing concurrency scope (`par`/`race`/phase); the scope's completion waits for adopted children, its cancellation cancels them. Static child list → `par`; dynamic branch count (from inside a loop) → `fork` (Trio's nursery `start_soon`). Needed because DMK async repeaters do not wait for their children.
- **Iteration trichotomy**: arrays (simultaneous fan-out — no loop at all) / `dotimes` (sequential indexed, pure per-iteration) / `loop`/`recur` (sequential fold, explicit carried state). A loop containing **no temporal actions is a pure fold and evaluates inline** when used for its value (rejection sampling, closed-form searches) — F3's fold-belongs-to-the-control-layer point, enforced: temporal actions inside a value-position loop are errors. `loop`/`recur` over `for` is semantic: fold and fan-out cannot be confused; loop state is explicit in the canonical tree (visible to serialization, card transformations, and the input-independence analysis); recur boundaries are the scheduler's cancellation/fuel/snapshot points — control-layer snapshot is "record the recur args," load-bearing for rewind.
- **`(for [i n, x xs, y ys … :every dt] body…)`** (surface name; `dotimes` remains an accepted alias) — first pair is the counter (`n` may be `inf`); `:every` is the inter-iteration wait, *between* iterations, not after the last (n−1 waits — DMK's own GCR semantics, which special-cases the final iteration; the difference is observable by anything sequenced after). Subsequent pairs are **seq bindings**: each iteration binds the i-th element of its source; arrays cycle (cyclic `nth`) — DMK's repeater-level `color({…})` modifier as a loop binding, restoring the which-loop-level information a spawn-attached meta map lacks. Sources are stream-shaped (an array is the trivial cycling stream, SC `Pseq`); the pattern algebra slots in here (`(dotimes [i inf, ang (pbrown 0 360 10)] …)`), as does `(stutter n xs)` (SC `Pstutter`; DMK `colorf(xs, i/2)`).
- **State machines** (revised twice: the primitive is a *bare FSM*, general enough for Markov chains and player-control rigs, not a boss template): **`(states (label process…) …)`** — ordered labeled states, label keyword as head (homogeneous clauses need no discriminating head-word; heterogeneous items like `stages`' do). Semantics: a trampoline — a state ends by goto or body completion; next = the goto target, defaulting to state order (DMK `shiftphase`); falling off the end completes the machine (which may return a value to its embedder). **State exit cancels the state's whole task subtree**: work forked in a body (a moveset, a turret rig) is guarded on the machine's *state generation*, bumped at every exit — so it dies when its state does, however the state ends. `(goto label?)` is a **scoped non-local exit**: cancel the enclosing state body, run any core `finally` scopes unwound past, then re-enter at the label; bare `(goto)` exits to the default successor. **Labels are values** (evaluated at the goto), so routing may be computed — `(goto (nth [:a :b] (rand-int 0 2)))` is a Markov chain, and a ground/air player rig is two states whose transitions read input channels. Goto is unambiguous by construction: it *exits structurally* and *enters only at state heads*, and it is **scoped strictly to the innermost lexical machine** — outer labels are not in scope, so an embedded card machine cannot hijack its host's flow; inner machines communicate by completing. Same-tick competing gotos resolve by tree order (the tie-break `race` needs anyway). Labels, not indices (DMK `shiftphaseto 4` breaks on phase insertion; labels survive tree transformations). Machines nest. **Everything DMK's phase props do is state-body code**: the hp race is `(until (<= (hp-of boss-main) n) attack)` inside `boss`, a timeout is `(fork (seq (wait d) (goto)))`, `root` is `(move-to boss-main …)` at the body head (the card knows who its boss is; the machine doesn't), and publishing the current phase is ordinary channel/cell code. **`phases` is the boss-shaped sugar over `states`, and it is a stdlib macro** (lib/touhou.maku, §10) — what a "phase" means is genre policy, so the engine doesn't define it. Clause opts `{:hp n :until p :timeout d :root pos}` desugar at macro time to exactly those body forms (`:hp n` ≡ `:until (<= (hp-of boss-main) n)`, reading the lexical boss handle supplied by `boss`); a `(finally …)` tail is sugar for wrapping the rooted phase body in core `(finally (seq …) …)`. Cleanup now runs before the state generation bump; for instant cleanup this is still within the same tick. Richer templates (`hpi`/`type`/names) are card-level macros over these.
- Phases are structured concurrency: `race(hp-depleted, timeout, attack)` *is* a DMK phase/spellcard; cancellation propagates scope-wise down the action tree (DMK's cancellation-token hierarchy, rediscovered — the `seq/par/loop/race` tree *is* the token tree). `race` forks all arms; the first arm to finish wins, losers are cancelled, and the parent resumes on its next step after the win (up to one tick of latency). `until` is the optimized two-arm case. `finally` is the paired cleanup operator: race decides who gets cancelled; finally decides what runs when a completed or cancelled scope unwinds. Losers of a `race` run `finally` blocks: soft-cull with fades, item spawns, end-of-phase bookkeeping.
- Triggered controls: `(par pattern (seq (wait-for (< hp 0.5)) (manipulate query f)))`. Event vocabulary: collisions, grazes, thresholds, host-injected events.
- **Pattern timelines inherit the closed/scanned split — the property is input-independence, not closed form.** A pattern whose waits are closed durations, with no event-waits and no injected-signal dependence, has an *input-independent* timeline — evaluable without an input tape (a pure control-layer fold is deterministic; evaluating it *is* static computation, so `loop`/`recur` accumulator patterns qualify). One `wait-for` on an event, or any injected dependence, makes the timeline tick-emergent / tape-relative. "When does the action happen" always has exactly two answers: at a time you can compute, or at a tick you must reach — never "whenever the evaluator looks."

---

### Scope cancellation **[spec]**

- **`(until pred body…)`** — structured cancellation: run body; the tick `pred` first holds, the body's entire task subtree (loops, `fork`s, nested `par`s — everything started under the scope) dies together. Forked tasks inherit the guard at fork time, so cancellation follows the dynamic tree, not the lexical text. This is the §8 phase-end semantics DMK gets from phase-token propagation: `(until (<= (hp-of boss-main) 0) (spell-2))` cancels the spell's guide rigs and turret forks the instant the health bar empties; in-flight bullets are inert data and persist (clear them explicitly with `(cull)` if the phase edge wants it). `until` is the degenerate case of `race` — `(race (wait-for pred) body)` — but remains its own optimized special. Guards are ordinary predicates over channels/cells: deterministic, scrub-safe, evaluated at a canonical point per task per tick.
- **`(finally body cleanup…)`** — scheduler-level unwind-protect. The cleanup forms run in order exactly once when `body` completes, when an enclosing cancellation guard fires, when a task dies through an inherited guard, or when `goto` unwinds past it. On task death the cleanup is protected by clearing guards first, so cleanup can finish before the task ends.
- **`(clamp lo hi dyn)`** — position clamp (playfield walls). Output-clamps the pose; for integrated children (`vel` under unrotated const frames) the **integrator state** is clamped after each step, so pushing a wall banks no phantom distance — you slide, and reversing moves away immediately. The piloted-rig companion: `(clamp c[-3.8 -4.4] c[3.8 4.4] (in-frame start (vel …axes…)))`.

---

## 9. Manipulation, queries, events

**[spec]**

- `(entities-where pred)` evaluates `pred : EntityView -> Number` over live rows and returns an `EntitySet`: an ephemeral vector of row indices. Nonzero predicate values are true; zero is false. The returned indices are stable only until the view changes by cull/remat/resizing. If an index disappears from a view and later reappears, the host/card must not assume it names the same entity unless it also holds and validates an `EntityRef` generation.
- `(matches :field value ...)` is convenience syntax for `(fn [e] (* (= e.field value) ...))`. It is not the only query language. Field access is flat at the source level (`e.team`, `e.render.style.color`, `e.pos.x`); storage may distinguish pose/state/meta internally.
- `(manip target callback)` (alias: `manipulate`) applies `callback` to each live target. `target` may be:
  - an `EntityRef` handle returned by `spawn`;
  - an `EntitySet` returned by `entities-where`;
  - a predicate function, as shorthand for `(entities-where predicate)`;
  - a compatibility map query while the prototype migrates.

  The callback receives an entity handle/view appropriate to action construction. It may return/execute instant actions (`set-col`, `remat`, `cull`, `spawn`, events) or, in a compiler, lower to masked SoA writes when it is pure and slot-local.
- Query views cut freely across birth structures. Rings/rows/fans are spawn-time array shapes only; after spawning, entities are rows filtered by predicates or referenced by handles.
- **`Entity` is a fixed semantic surface plus a finite field layout**: source fields are named (`:hp`, `:iframe-until`, `:ci`, `:team`), but load/reschema interns each field into a typed dense slot. Unknown fields are load/reschema errors at typed boundaries, not per-tick map allocation. The cost split is compiler-visible by inspection of the callback body: a callback touching only known numeric slots compiles to a masked in-place SoA update (`field[pred] = f(field[pred])`) — hot-layer, vectorized, no fuel; a callback spawning actions per entity runs on the control layer and bills fuel per matched entity. DMK's batchable-controls vs SM-per-bullet split, recovered as an inferred property of one API.
- **Rematerialization** is the blessed event mechanism: snap current values (pose, velocity, tag samples) into fresh spawn-captured constants, swap the signal. Uses: re-aim-once (class (c)), reparenting, reflection (the corpus `switch(reflected, vel, …)` per-bullet-flag idiom is hand-rolled remat paying a hot-path branch; ours is one event-driven signal swap), returning a bullet to `Closed`/scrubbable-land after an event, closure-splicing made explicit and cheap.
- **Epoch model** (remat clock semantics): every rematerializable slot carries an epoch column; birth time is just each slot's initial epoch. `(remat bullet slot new-signal)` writes `epoch := now`; the new signal runs on `τ = t − epoch`, starting at 0 at the event tick. Initial conditions are passed explicitly — the remat call snaps what it needs and hands it to the new signal as ordinary `ir` constants — so C⁰ continuity holds by construction; C¹ is a convention of stock helpers (`remat-straight = linear(snap pos, snap vel)`). Remat is **per-slot**: a half-finished fade keeps running on its own epoch when the motion slot is swapped. A bullet's history is a list of `(epoch, signal, constants)` segments — piecewise-`Closed` bullets remain fully scrubbable, and the segment record is exactly the replay-log entry. `stages` (§3) is this same model statically scheduled. Ancestor clocks stay orthogonal: remat moves only the local epoch.
- **The F1 lint — no silent strengthening**: velocity constructors with closed-form-integrable integrands (constants, piecewise-affine `lerp` profiles) are `Scanned` as written but have `Closed` equivalents; the compiler never rewrites silently (a scan stays a scan — predictability) but lints with the suggested closed rewrite. This matters compositionally: one `Scanned` guide contaminates every rider by contagion (the cradle: one `vel` makes 126 petals unscrubbable; the closed rewrite restores the whole tree).
- Scanned state is ordinary dense state + step functions ⇒ snapshots are memcpys; manipulation of scanned entities is writes to scanned state or signal swaps.

### Colliders and contact effects **[spec]**

- **Colliders are projector outputs, not entity kinds**: semantically an entity stores collider projectors specialized to its core figure type; extraction evaluates their source-level projector specs against `(EntityView<F>, ProjectorContext)` and emits literal `[Collider]` rows each tick. Card code composes projector specs such as `bullet-collider`; it does not construct raw collision rows directly. Most cards use projectors such as `bullet-collider`; expanding circles, warn/hot phases, disabling collision, and curve-sampled capsule chains are expressed by dynamic entity/meta inputs plus higher-order projector combinators over context such as age. Layer is universal routing metadata on every collider. An empty list or `none` collider means inert to the contact pass. Teams are ordinary meta fields only.
- **The engine supplies no genre defaults** — what a Touhou "bullet" carries (`:damage` core + `:graze` ring), an "enemy" (`:hurt`), or a "player" (`:player-hurt`) is library knowledge in `lib/touhou.maku`. `:hitbox r` is library sugar over the primary collider spec, not a core field.
- **Collision domains are card data plus card code**: `(collisions :a-layer :b-layer)` is a tick-domain expression over current collision facts. Collision detection is engine-side and hot: live entities are enumerated by row, materialized collider rows are tested by layer, and overlapping pairs are exposed as an ephemeral `CollisionSet`. Dead/posless entities and `i == j` are skipped; duplicate contacts from multiple colliders are allowed. Card code consumes the domain with ordinary array vocabulary, e.g. `(map (fn [[a b]] ...) (collisions :shot :hurt))` inside `deftick`.
- **Filters/latches are ordinary code and fields**: once-only and skip behavior are expressed in the callback/query with ordinary fields, e.g. checking `a.grazed` before setting it. Handles expose `:pos`, `:vel`, `:t`, `:tick`, `:kind`, style axes, `:team`, and flat fields such as `:damage`. `(event :name pos?)` emits an event; a point-pose second argument supplies the event position. Reactions stay control-layer (`wait-for`, derived channels such as `$graze`/`$enemies`); the engine knows detection, not damage/graze/shot semantics.
- **Invulnerability is a column**: `iframe-until` (a tick stamp) is honored by BOTH resolve paths — a hit inside the window doesn't land (player side), a shot is *absorbed* (dies, emits `absorbed`, no hp write — boss side). `(invuln b dur)` writes it — and is *library code*, not an engine verb: `(set-col b :iframe-until (+ $tick (* dur 120)))`, a deadline computed from the `$tick` channel. The automatic post-hit window reads its duration from an `:iframes` column (seconds) when present. Being columns, windows snapshot and scrub like everything else. `(set-col b :name v)` is the general write.
- **Shapes**: circles in the dense fast path; lasers derive capsule chains *per tick from the same curve figure the renderer draws* (the beam you see is the beam that hits — both projectors sample one figure, each producing its own static description per tick). Lifecycle is library code: the touhou `laser-collider` body returns no colliders during warn and sweeps the hot extent from `:warn`/`:active`/`:fill` fields while active, and a library cull rule ends the beam — core knows none of it. Beams persist through hits. Heterogeneity is confined to the rare shape.
- **hp and death are not special** — the same dissolution as hit/graze, one level down. hp is just a user-defined numeric field assigned to a dense slot: `:hp n` initializes it, and contact damage is nothing but a **field write** — the collision path does not know what zero means. Death is library/card `deftick` code over `(entities-where ...)`, with once-only latches represented by ordinary fields. The engine synthesizes no rules — the default `hp ≤ 0 → cull + event :died` is the standard library's Touhou rule. What this unifies: enemy death, **DMK's HP-gated boss phases**, enrage-at-50%, player lives — each a field plus a rule, none engine code.
- **The player is card content, not engine code**: Touhou's `player` template creates an entity with a `:player-hurt` collider, lives/bombs/graze/hits fields, library game-over logic, and a `$player` binding to its live pose. The stock `player-rig` is just the raw-input movement rig built on top. "The host mounts the player" means the host *layers a rig pattern in* (an `(add (player-rig))` riding the command tape, so card + tapes fully determine a replay — no hidden host state); characters, co-op rigs, options-as-satellites are different cards, not engine changes. More generally **the host/card boundary is a per-game contract, mediated entirely by channels**.
- **Everything writes World** (counters, events, latches, fields), so the whole gameplay layer snapshots and scrubs with the timeline; collision fact generation and tick-rule order are canonical so replays agree. Events carry positions; renderers may draw effect flashes statelessly from the event log — they replay under scrubbing for free.

### Runtime memory contracts **[spec]**

- A card/session declares or negotiates maximum entity capacity at construction time. Normal stepping, spawning within capacity, culling, queries, collision, render extraction, and channel refresh must not allocate on the hot path.
- Resizing capacity or changing the field/schema layout is an explicit host-side command, like loading/swapping a card. It is not an automatic response to overflow during a tick. If capacity is exhausted, spawn fails deterministically.
- Entity rows are stable while alive. Culling marks a row dead and bumps its generation; stale `EntityRef`s cannot target a reused row. Culled rows must be invisible to every query for at least the current refresh/tick boundary before they can be reused, so a disappearing row in a host view is not immediately ambiguous with a newly spawned entity.
- `EntitySet` values are row-index views, not ownership. They are cheap and ephemeral. For cross-time identity, hold `EntityRef`s or pair row indices with generations at the host boundary.
- Runtime storage target:
  ```text
  WorldFields:
    pose/current + pose/previous buffers, possibly parity-swapped
    dyn figure programs / state slots
    dyn collider programs / state slots
    dyn renderer programs / state slots
    numeric fields    : NumFieldId    x row -> f64
    symbol fields     : SymFieldId    x row -> Symbol
    handle fields     : HandleFieldId x row -> EntityRef
    presence/sentinel bitsets per typed matrix
  ```
- Source maps and keywords are load-time/typechecking artifacts. Per-tick field access is indexed matrix access; unknown fields are load/reschema errors at typed boundaries. Keyword/event names intern to compact symbols before hot execution.
- Closed-form dyns may be re-evaluated from `(row, local time)`; integrated/scanned dyns own dense state slots and optional trace/cache buffers. Trace retention is a performance/cache policy, not semantic state; shortening a trace should be indistinguishable from an entity that only existed for that long.
- Collision and render buffers are derived per tick from figures and projectors. Sampling rate/resolution belongs to collider/render projectors or host/rendering layers, not to the abstract figure unless a future analytic representation needs it for semantics.

---

## 10. Patterns as data

**[spec]**

- **Guide objects are first-class where needed, dissolved where not**: within one pattern, a guide is an unexpressed frame level (§6) — no extraction machinery. Extraction — positions (all, or by query) from a pattern as `Signal (Array Pose)` — is for trajectories crossing action-tree boundaries (consumed by other patterns or later actions).
- Extraction typing is derived, not legislated: extraction from an **input-independent pattern** (input-independent timeline + closed dyns) is itself `Closed` — a pure query over a timeline that exists as data (birth-time columns + closed motion), evaluable at arbitrary t, usable as a base for further closed patterns. Extraction from anything touching live injected signals or event-waits is `Scanned` (well-defined only relative to the input trace).
- Cards as trees: the canonical (desugared) s-expression form serializes; upgrades are tree transformations (macros); fusion/deck operations are tree composition; frames-as-transformers and pattern-transformers (the SC Patterns algebra) are the manipulation vocabulary.
- **Card macros**: `(defmacro name [params…] body…)` — arguments arrive *unevaluated* as forms; the body (typically a backtick template with `~`unquote/`~@`splice) returns a form, which evaluates in the caller's scope. Expansion happens at application, macro-first among unbound heads. Unhygienic in the classic way (templates introducing bindings should use unusual names) **[decide: gensym/hygiene]**. Most abstraction should NOT be a macro — frames, dyns, and actions are first-class values, so `defn` covers anything that doesn't invent binding forms or need arguments unevaluated.
- **Macro bodies are ordinary code, and forms are ordinary data.** `& rest` params (macros *and* fns) bind argument tails as arrays; the generic seq vocabulary (`count`/`first`/`rest`/`nth`/`drop`/`take`/`concat`/`map`/`filter`) sees a form list/vector as a sequence of subforms; `get` is total over map values *and map forms* (missing → nothing, probe with `nothing?`); `form-type`/`form-name` classify without pattern matching. Seq values are views: `rest`/`drop`/`take` are O(1) windows over shared immutable backing (fat-pointer semantics, the same representation a compiled backend keeps), so head/tail recursion is linear. A macro can walk a clause list, transform each clause with a helper `defn`, and splice the results — which is how the stdlib defines `phases` over `states` without any engine support.
- **`match` destructures forms and values:** `(match subject pat result …)` evaluates `subject` once, then tries flat pattern/result pairs in order; no match is an error. Patterns are `_`, binders, number/string/keyword/bool literals, `'sym` / `(quote f)` for exact form literals (`'f` reads as `(quote f)`), `(as n p)`, sequence patterns `[p…]` with `& rest` including mid-rest tails, and map patterns `{k p…}` whose literal keys discriminate by presence.
- **Imports**: `(import "relative/path.maku")` on its own line splices that card's text at that position — recursively, **include-once** (canonical-path dedup), so diamond imports yield one copy and the importing file's later definitions shadow imported ones by ordinary def ordering. Corpus-faithful (ph_boss2_mima imports ph_ref upstream). Expansion happens at file-load time, so the wire/card source stays self-contained — the tapes and live eval need no path context. Convention: imports go *after* the card's main `defpattern` so it stays the file's first (= default) pattern. This is deliberately a *textual* mechanism; namespaces wait until collisions hurt **[decide §13]**.
- **The standard library is a card, shipped inside the engine.** A *bare* import name — `(import "touhou")`, no slash, no `.maku` — resolves to a library card (`@lib/touhou.maku` is its include-once key) **embedded in the engine artifact at compile time**: authored as ordinary `.maku` files (cards/lib/), inlined via the build, identical on every host — native, wasm, headless — with no filesystem or fetch involved, and available to in-memory sources (tests, REPL, rig strings, swap/add) as well as files. Users import the library; they don't edit it. The genre layer lives there (the spawn templates, `player`/`player-rig`, `invuln`), and the direction of travel is that anything expressible as card code moves there — the engine keeps only what card code cannot express. **The prelude is the autoimported slice of the stdlib**: expansion prepends `@lib/prelude.maku` to every top-level source (a first-line sentinel keeps re-expansion and explicit imports idempotent). It holds *language-level* sugar only — `when`/`unless` are prelude macros over `if` (nothing coerces to the no-op action, so the one-armed if works in action position); anything a genre could disagree with goes in an importable lib instead. `for` remains engine for now (its `:every`/`inf`/array-iteration semantics live in the scheduler, not in a desugar).
- **Pattern embedding is scope-disciplined, with a safe default.** `(pattern arg…)` invokes a card pattern: arguments evaluate in the *caller's* scope as ordinary `ir` values, defaults fill the rest, and the instance gets **fresh cells** — two embeddings of the same pattern cannot share `defvar` state, so double-embedding is safe by default (every card written under the old share-everything prototype already conformed). The explicit adapter for the other behavior is **`(inline (pattern …))`**: the embedded pattern runs *in the caller's cell scope* — its `defvar`/`set!` bind into the embedding pattern's cells ("binds into the embedding pattern's scope"). Cells are *dynamic* pattern-scoped ambient: `defn`s called from a pattern body read and write that instance's cells (the guide-rig-reads-`mode` idiom) — hygiene excepts the cell scope exactly as it excepts slot-bound `t`/`u`. Read-only channels flow through embedding unconditionally. Adapter nodes are ordinary card data in the canonical tree — the composition-level analogue of the `shared` node (§5) — and ambient frames still do not cross (§4). Residual **[decide]**: a `:sealed` adapter blocking channel overrides, pending `with`.
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
| `(import "path.maku")` | textual include, include-once (§10) — own line, top level |
| `a.b.c` | accessor chain — reader sugar desugaring to keyword application `(:c (:b a))`; reads maps, Pose components (`:x`/`:y`/`:th`), and entity handles (the live view). The canonical tree never contains dots |

The vector/list division is load-bearing: `[0 120 240]` (data, broadcasts) is lexically distinct from `(circle 3)` (evaluated form returning an array), and every canonical tree is readable as pure data without an evaluator — what cards-as-data needs.

**`m"…"` — the math macro, parse-only.** Everything inside has an s-expr equivalent and parses to the same canonical tree; the macro adds zero semantics, and the canonical/serialized form is always the parsed tree. Grammar inside: infix `+ - * / ^ %` and comparisons with PEMDAS; postfix `.field` access and `.[idx]` indexing (`xs.[0 1]` and `xs.[iota(3)]` gather; dot-bracket, so bare `[` is unambiguously a literal — `c[…]`/`p[…]` coords and arrays — cyclic `nth` broadcasts, desugaring to `(nth xs i)`); function-call syntax `f(a, b)` for any in-scope function (`sine(1, 0.2, t)`, `iota(6)`, `live(mode)`); channel reads `$name`; array and coordinate literals; `$(…)` splices an arbitrary s-expression (`$` followed by `(` is a splice, otherwise a channel). Free symbols resolve against the enclosing lexical scope (an alternate parse, not a binding boundary). Operators broadcast per §5 — `m"[0 120 240] + 80*t"` fans out. Use it for expressions with several binary operators; single calls stay s-expr. Backtick is quasiquotation (card macros, §10): `` `form `` templates, `~e` unquotes, `~@e` splices arrays; `'form` is plain quote, reading as `(quote form)`.

**Units: one canonical unit per quantity + source-named conversion functions; no unit-tagged literals.** A conversion is named for its *source* unit: `(ticks 8)` = 8 physics ticks as canonical seconds; `(rad x)` = radians as canonical angle. The canonical unit has no function. **Angles are canonically degrees** (the entire corpus authors in degrees; DMK's `cossindeg` is fossil evidence — canonical degrees also deletes the parallel `*deg` function family, and angle→unit-vector is just `p[1 θ]`). **Time is canonically seconds** — unlike degrees/radians, tick-canonical would bake the timestep into card data, and continuous time is semantically load-bearing (`Closed (t → a)` scrubbing, tunnel arc-length); `(wait (ticks 8))` is exact on the grid, which is why control code authors in ticks.

Arithmetic `+ - * /` is variadic (n-ary fold; unary `-`/`/` negate/reciprocate) and broadcasts per §5 — `m"60*iota(6) + 120*t"` is six signals. Array builtins: `(iota n)`, `(range a b step)`, `(without x xs)`. Reading property preserved: `((rot base) (circle 6 (vel … (circle 7 (polar r θ)))))` reads outside-in as coarse-to-fine motion — anchor, ring, carrier, wiggle — matching how designers think.

---

## 12. 3D and alternative parametrizations

**[spec]** No dimensional lifting of the pattern language. 2D patterns + **emitter-frame embedding**: patterns execute in local oriented planes/cylinders/sphere-surfaces; a small vocabulary positions/orients/animates those frames in 3D (the NieR model: 2D patterns, 3D placement). Tunnel game: pattern space is `(θ, s)` on the unrolled cylinder; the player's tunnel pose (position + tangent) is a host-injected pose signal serving as the world frame; patterns parametrized on `s` remain closed ⇒ backward evaluation when the player backtracks; classes (a)–(c) reactivity survives non-monotone `s`; only class (d) needs monotone-section quarantine.

---

## 13. Open decisions **[decide]**

1. ~~Ancestor-clock operator design~~ **resolved** (2026-07, the t09 audit — see §4.2): no operator; clocks are ordinary values. Parents capture `$tick` into bindings, child signals read `(live $tick)` against them, and `(live …)` counts as time-dependence for deferral (so wall-clock signals don't constant-fold at spawn). The remat side was already settled by the epoch model; what interaction extraction needs is extraction's own question (§10).
2. Event vocabulary enumeration and the concrete channel API (injected/exported signal declaration, outbound event channels, host-handoff commands — §3 fixes the four-construct shape; the prototype has positioned events on a bounded shared log with per-snapshot cursors, but no declaration/manifest enforcement yet).
3. Exact finite field schemas and declaration surface. The target is load/reschema-time typed field matrices (§9); the open question is how much the author declares explicitly versus how much the loader infers from all field accesses and spawn meta.
4. ~~Angle representation for θ columns~~ **resolved** (2026-07): semantic/core poses store `(x, y, θ)` with canonical degrees. Backends may cache unit vectors for optimized projection/render paths, but wrapping-aware operations remain explicit where needed.
5. Facing override semantics: frame-relative vs absolute (§4 orientation policy; the cradle translation reads as frame-relative).
6. `(on-axis k xs)` meta-targeting sugar vs the explicit-length convention (§5).
7. Blocking-curve feedback contract (world geometry → extent; necessarily `Scanned`). Touhou blocking lasers are a library case over curve figures plus collider/render projectors.
8. `(with {channel value} body)` residual details (the core semantics are settled in §3 — the in-frame-for-channels distribution law): which *derived* channels are overridable, nesting/shadowing rules, override values that are themselves signals of taped inputs. Unimplemented in the prototype.
9. Derived-channel vocabulary and cost model (§3). Resolved enough for the prototype: `defchannel` declares top-level derived/default channels, `bind-channel!` covers instance-scoped handle/cell-derived state, and `(export)` covers cells by name. Core no longer owns genre channels; Touhou defines the DMK/BDSL scalar defaults, and co-op-style per-pilot families are library/card conventions.
10. ~~The interaction matrix as data~~ **resolved** (§9): collider projectors emit layer-tagged collision rows, `(collisions :a :b)` exposes current-tick collision pair domains, and `deftick`/array code owns effects/latches. Touhou hit/graze/shot rules live in lib/touhou.maku.
11. The `states` machine and general `race` are implemented. `states` is a trampoline over ordered states, scoped `goto` with evaluated labels and the bare exit-to-successor form (env-carried request cell; first write per state wins, realizing the tree-order tie-break), and generation-guarded state scopes (forked movesets die at state exit). Core `finally` runs on completion and cancellation, including fork task death; `phases` ships as a stdlib macro in lib/touhou.maku (`:hp`/`:until`/`:timeout`/`:root` → body code, `(finally …)` tail → core finally, at macro time). Phase-body return values as next-label (the full trampoline) await a forcing case: routing is goto-or-state-order.
12. ~~Pattern-embedding scope adapters~~ **resolved** (§10): fresh-cells default + `(inline …)`, arguments as caller-scope `ir` values, cells as dynamic ambient through `defn` application. Remaining: the `:sealed` channel-override adapter (waits on `with`, §13.8).
13. Rule/domain ergonomics (§9): primitive `deftick` plus array/domain values are in place; row-wise helper macros and richer tuple-domain operations remain open.
14. Import namespacing (§10): textual include-once suffices now; a namespace/alias story if cross-card collisions start hurting.

Settled since the first draft (see cards/translations/NOTES.md for the record): snap-by-default boundary + `live` marker; construction-vs-reference of scans (`shared` nodes); scanned-state limits (fixed runtime rows + indexed typed fields); entity semantics as `Dyn<Figure> * Dyn<Meta> * collider/render projector functions`; style/render vocabulary as host/library policy; phase transitions (`phases` + scoped goto); iteration/vocabulary surface (EDN, `m""`, units, `dotimes` seq bindings, formation/stream stock). Settled by the prototype: let-deferral of action bindings (F17); frames stop at lambdas (F18); difficulty is the rank channel + pure loops fold inline (F19); derived channels (F20); def-resolution hygiene under slot binding. Settled by the gameplay/host sprint: collider projectors + layer tags + collision domains (§9); fields plus `deftick` rules dissolving hp/death/lives/phase gates (§9); the player as card content and the channel-mediated host contract (§9); `until` scope cancellation and `clamp` with integrator-state semantics (§8); frames ambient at every level + variadic `in-frame` + `:world` (§4); imports (§10); raw-input channels on the tape (replays include the keyboard). Settled by the stdlib extraction: the engine's genre knowledge (bullet/enemy collider sets, the hp-1 default, the death rule, `invuln`, the stock rig, and Touhou hit/graze/shot collision rules) is *library card code* — authored in cards/lib/, compile-time embedded, imported by bare name (§10); `spawn` targets figure/meta/projector slots, with compatibility sugar during migration, and collision effects are ordinary `deftick` rules over `(collisions ...)`.

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
