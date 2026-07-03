# Danmaku Pattern Language: Design Document

A language design for an engine-agnostic bullet-hell system, derived from an audit of Danmokou's (BDSL) semantics, SuperCollider's signal model, and array-language composition. Companion to *Engine-Agnostic Danmaku Core: Design Notes* (architecture/runtime doc); this document specifies the language itself.

Status: design sketch. Sections marked **[decide]** are open decisions; sections marked **[spec]** are settled by the design discussion.

---

## 1. Design stance

**Steal DMK's invariants, redesign its composition layer.** DMK is *array-ready but scalar-souled*: its runtime is SoA pools and its per-bullet functions are pure over `(t, env)` — so its semantics vectorize mechanically — but the language's unit is the individual bullet, and its composition layer (repeaters) is imperative accumulator mutation. Audit findings:

- Repeater modifiers (`spread`, `circle`, indexed color lists) are pure functions of the loop index wearing mutation clothes: `gsrepeat times(n)` *is* a map over `!n`. The only genuinely sequential elements — shared-stream RNG and wait-between-shots — dissolve into counter-based RNG and birth-time columns respectively.
- What DMK encodes implicitly and this language makes explicit: per-bullet local time with spawn capture (GCX environment frames), the spawn-time/flight-time evaluation split (GCXF vs bullet functions), closed-form vs integrated motion (`roffset` vs `rvelocity`), scoped cancellation (token hierarchy), and frame composition (V2RV2).
- What to keep wholesale: the *function vocabulary* (sine, polar, easings, cull/graze mechanics, aimed-modifier conventions) — years of ergonomic tuning, portable into any composition model. And the negative lesson: DMK's v9 dynamic-type period was removed in v11 in favor of a standard typed model. Interpretations must be types.

**Array structure in danmaku is ephemeral**: rings/rows/polygons are *birth* structures that dissolve in flight (per-bullet graze flags, culls, controls address predicates, not birth groups). Therefore: **array semantics at spawn, bag-of-rows semantics in flight.**

---

## 2. Core types

**[spec]** Small, closed universe for per-bullet data; types erase to flat SoA columns at runtime.

- `Float`
- `Vec2` with **tagged interpretation**: `Cart` (x, y) and `Polar` (r, θ) are distinct types over the same storage, with explicit conversion. Rationale: polar has a partial algebra (adding to θ = rotation, adding to r = radial push — both pattern-meaningful) but componentwise `+` of two polar values is not vector addition; broadcasting would do it silently if untagged.
- `Pose` = element of SE(2): position + orientation θ. **Points and poses are distinct**: points add (offsets, lerps, arithmetic), poses compose (frames, emission anchors). Cheap promotion point → pose (θ from context; see §5).
- `Tag` values for meta (color, style, render hints, gameplay flags, hp, …). Tags may be signal-valued (§7).
- `Bullet` — opaque handle exposed in manipulation callbacks (§9).
- Arrays of the above (§6).
- `Signal a` — the central abstraction (§3).
- `Pattern` / `Action` — the control layer (§8).

Per-bullet hot state draws only from {Float, Vec2, Pose, Tag-word} so scanned state packs into columns and steps vectorize pool-at-a-time. Control-layer signals (per-pattern, per-frame) may carry richer types.

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

### Time and clocks **[spec]**

Three clocks with explicit nesting rules:

1. Every action node gets a **local clock** zeroed at its activation (`seq`, `loop` iterations, etc. rebase).
2. Every bullet's dyn runs on **bullet-local time** `t − birth`. Birth time is a column; emission over time (spirals) is birth-time data, not phase-locked global-t functions. An explicit operator reaches an ancestor clock when phase-locking is wanted (ring vs spiral distinction).
3. **World time / world parametrization is host-injected.** Nothing is sacred about `t`: patterns may be parametrized on any monotone-or-not host signal (e.g. tunnel arc-length `s`). Closed signals evaluate at arbitrary parameter values, enabling backward evaluation when the parameter is player-controlled.

### Injected signals **[spec]**

Host-provided values (player pos/vel/acc, boss position, rank, arbitrary channels) are `Scanned` signals. **Default capture semantics: injected signals appearing in spawn arguments are implicitly snapped** (spawn-time capture — the overwhelmingly common case: aimed fans, rings at last known position). Continuous tracking requires explicit `live(...)`. Rationale: reactivity decomposes as

- (a) spawn-time sampling → `snap` (the vast majority of "reactive" danmaku);
- (b) pointwise composition with a live signal (drift fields, boss-parenting, rank scaling) — stays scrub-evaluable given a recorded input tape, since `injected(t)` is a lookup;
- (c) event-time re-capture (fly straight, re-aim once) → rematerialization at event boundaries;
- (d) true continuous feedback (integration over a live signal: homing) — irreducibly `Scanned`, and small in practice because game design discretizes it into (c) for fairness (turn-rate caps, re-aim ticks, give-up times).

Only (d) breaks closed form. The typing rule: an expression is `Scanned` iff it is downstream of a `scan` — pointwise use of injected signals does not by itself stream a bullet.

**Channels are symmetric.** The host↔pattern surface is four constructs, all on named channels — raw engine-object access does not exist:

- **injected signals** in (player pose, rank, tunnel `s`, arbitrary host data);
- **exported signals** out — continuous pattern→host data, realized as signal-valued tags on entities (§7), no separate mechanism;
- **outbound events** out — discrete, frame-stamped, the dual of injected events; inputs tape in, events tape out, keeping the replay log symmetric;
- **host handoff** — a command plus `wait_for(host-event)` (e.g. run a dialogue scene). The only construct that makes a timeline tick-emergent across the boundary (§8), which is honest: the host's duration genuinely isn't statically knowable.

What DMK does with direct engine-object mutation (`mine<Enemy>().ReceivedDamageMult = …`) maps to exported signals/tags; display switches map to discrete tag writes or outbound events; VN execution maps to host handoff.

### Rates **[spec]**

Adopted from SuperCollider (`ir/kr/ar`), realized as inference over shape and constructors:

- `ir` — evaluated once at spawn: snapped values; a column per element.
- `kr` — pool-invariant per frame: `Closed` signals referencing no per-bullet columns; hoisted, computed once per pool per frame.
- `ar` — per bullet per frame: everything else.

Rate inference is shape inference; hoisting is automatic. The REPL uses inferred rate to label parameters: `kr` knobs affect all live bullets immediately, `ir` knobs affect new spawns only — and the UI can say so.

---

## 4. Dyn: motion as signal composition

**[spec]** A **dyn** is `Signal Pose` — the trajectory of *one* position (with orientation) over time. Not privileged: position is a signal the express-action hands to collision and rendering.

- Constructors: closed-form (`f(t) → Vec2/Pose`, with linear and polar variants; `pos`/`vel`/`acc` variants are `integrate` applied 0/1/2 times — vel/acc are scans by construction), and procedural per-tick (`scan` directly, with read access to own state via the bullet object).
- Static poses / pose arrays (e.g. `circle(8)`) are **not dyns** — they are values. Promotion `pose → Closed(λt. pose)` (constants are the unit of both the signal and broadcast algebras) lets them serve in frame slots without ceremony.

### Frames: `in-frame` **[spec]**

One ordinary binary function is the core:

```
in-frame : Signal Pose → Signal Pose → Signal Pose    -- pointwise SE(2) composition
```

- Associative, with the constant identity pose as unit ⇒ dyns form a monoid; deep hierarchies are folds; nesting *depth* is programmable with ordinary list code.
- Partial application `(in-frame boss-frame) : Dyn → Dyn` yields frames-as-transformers — the card-algebra building block ("this card, but mounted on the boss").
- **Surface sugar**: dyn constructors accept an optional last argument meaning "child dyn evaluated in this frame"; `(linear v (polar r ω child))` desugars to nested `in-frame`. Sugar is *only* sugar: the desugared application tree is canonical — it is what serializes, what card-upgrades transform, what the REPL prints, what conformance is defined over. The macro must never acquire capabilities the function lacks.
- Top level: the world frame is host-determined.
- **Two-operation algebra** (the full content of DMK's V2RV2, as syntax structure): `in-frame` composes *through* frames; `+` on point signals adds *across* them (world-frame additive terms — e.g. gravity staying world-down inside a rotating hierarchy). `translate-only(child)` / attach-to-point is the third citizen: inherit position but not rotation (options that shouldn't swing with a wobbling carrier).
- Reparenting (option released from carrier) = rematerialization: snap current world pose, swap to a world-frame dyn. Events and frames share one escape hatch.
- Cost note: bullets in a pool share tree shape ⇒ pool-at-a-time evaluation vectorizes each level; `kr` levels (e.g. the boss frame) are hoisted.

### Orientation policy **[spec]**

θ is **derived by default, materialized on demand**:

- Default pose θ = heading (direction of motion): analytic/finite-difference derivative for `Closed` (itself `Closed`), one extra prev-position column for `Scanned`. Derivation never changes a signal's constructor classification.
- **Spawn tick**: inherit θ from the emitter frame (snapped). For standard dyns, initial velocity points along emitter aim, so the inherited value is what the derivative converges to — the definition is continuous.
- Degenerate motion (zero/near-zero velocity): hold last well-defined heading (`Scanned`) or fall back to frame θ (`Closed`). Policy in five words: *inherit from parent, refine by motion*.
- Facing (sprite orientation) is **meta**, defaulting to pose θ, overridable — no definite relation between facing and motion is assumed.
- Storage: most bullets never parent anything and their facing is consumed only by rendering ⇒ compute heading in the render pass from the velocity column; pay the θ column only for poses used as frames. Conceptual model "every position is a pose"; memory model "θ on demand."
- Angle caution: lerp/average/smoothing on θ columns need wrapping-aware treatment (shortest-arc or unit vectors); raw `+` broadcasts fine.

---

## 5. Broadcasting and arrays

**[spec]**

- Standard array-language semantics: most functions broadcast elementwise over arrays; atoms broadcast against arrays; array-of-`f(t)→a` interchangeable with `Signal` of array where shapes agree.
- Spawn arguments (`dyn, meta`) broadcast with length-matching; atoms lift.
- **Frame multiplicity is tree shape, not an operator.** A dyn with a last-argument child cannot itself be an array — but it can be *in* an array, in which case each element broadcasts over its own children independently. Ring-of-fans = array of 8 frame dyns, each carrying a 3-element child array = 24 bullets; multiplicity per spawn = product of array sizes along the root-to-leaf path, statically readable. Under the desugaring this isn't even a rule: it's `map (λf → in-frame f fan3) (circle 8)` — ordinary map. Pairing i-th parent with i-th child is ordinary `zipWith in-frame`. No special broadcasting regime for frames exists.
- Spawn combinators are arithmetic on pose arrays: `circle(n)` = θ column `!n × 2π/n`; `spread` = `+` on a θ column; aimed fan = `snap(angle_to(player)) + centered_offsets`.
- **Scan sharing under broadcast** — explicit in the canonical tree, inferred at the surface. A scan is **fresh state per element** (own column; vectorizes naturally) unless wrapped in an explicit `shared(...)` node marking one instance referenced by all elements. One LFO on the bus vs an LFO per voice. The surface convenience — a let-bound scanned signal referenced inside a mapped function reads as shared — *desugars to* `shared`; the lexical rule is sugar only. The share node is what serializes and what card transformations see, so tree rewrites (inlining a binding, duplicating a subtree) cannot silently change state identity. This is a load-bearing instance of "sugar is only sugar" (§4).
- RNG is **counter-based** (`rand(seed, i, k)`, Philox-style): element k's randomness independent of evaluation order — required for array spawning, scrubbing, and rewind to coexist.

---

## 6. Spawning and expression

**[spec]**

- `spawn(dyn, meta)` is an **action** (never a signal); the anchor frame is the dyn's root (typically a snapped constant pose). Arrays broadcast per §5.
- **Express only what renders/collides.** Emitter anchors, bases, guide trajectories live as unexpressed signal data; only expressed entities consume pool slots and collision. (DMK's simple-bullet vs BehaviorEntity split, derived rather than special-cased.)
- **Extended entities via axis materialization**: simple bullet = point sample of the pose signal; **pather** = trailing time-window of the trajectory materialized as geometry (procedural hitbox from remembered points); **laser** = materialization along a parameter axis `u` at fixed t of `f(t, u) → pose`, with lifecycle signals (warn → active window → off) and, for nonpiercing lasers, blocking (world geometry feeds back into extent — necessarily `Scanned`). Materialization-to-polyline is a core primitive (`spawn-extended(f(t,u), width(t), window(t), meta)`), not per-entity special casing. This dissolves the laser/pather geometry contract into the language.
- Lifecycle as signal (SC `doneAction`): cull conditions — lifetime elapsed, off-playfield, fade-complete — are done-action nodes on the entity's signals, giving the compiler lifetime visibility for pool sizing. Populations are dynamic (express appends, cull deletes): runtime arrays are compacting streams, not fixed shapes.

---

## 7. Meta

**[spec]** `meta` is a record of tags; **any tag may be signal-valued** — constant (snapped), `Closed` (`hueshift(120·t)`, scale-in envelopes, fades), or `Scanned` (proximity flicker). One evaluation story for motion and appearance; the render contract samples tag signals. Tags interact with capture like everything else (snap vs live). Gameplay-meaningful tags (hp, team, graze-state) are ordinary columns addressable by query (§9). Tags are also the **export surface** (§3): an entity's outward-facing continuous data (boss damage-mult, healthbar opacity) is a signal-valued tag read by the host or other systems.

---

## 8. Patterns and the action layer

**[spec]** The language is two layers with different frequencies and disciplines:

- **Hot layer** (signals): pure, per-bullet-per-frame, loop-free ⇒ statically bounded frame cost (a hostile card can slow, not hang). Compiles to pool-at-a-time bytecode (dev/REPL) or AOT native (shipping).
- **Control layer** (actions): Turing-complete, per-event frequency, tree-walking interpreter with a per-frame fuel budget.

**Signals are pure; effects live only in the action tree.** A closed signal has no privileged evaluation schedule (scrubbing, plotting, hoisting all evaluate it arbitrarily often at arbitrary t) — an embedded effect would fire incoherently. Actions fire at ticks reached by the control layer stepping the tree.

- `defpattern name(params) = …` — patterns are named, parameterized (difficulty/rank as arguments), and exposed to the host (`engine.run(pattern)`).
- A pattern is an action or a tree of actions under concurrency combinators: `seq`, `par`, `loop`, `race`, plus `wait(dt)` and `wait_for(event/predicate)`.
- **Phases are structured concurrency**: `race(hp_depleted, timeout(40), attack)` *is* a DMK phase/spellcard. Cancellation propagates scope-wise down the action tree (DMK's cancellation-token hierarchy, rediscovered — the `seq/par/loop/race` tree *is* the token tree). Losers of a `race` run `on_cancel`/`finally` blocks: soft-cull with fades, item spawns, end-of-phase bookkeeping.
- Triggered controls: `par(pattern, seq(wait_for(hp < 0.5), manipulate(query, f)))`. Event vocabulary: collisions, grazes, thresholds, host-injected events; predicate-waits evaluate per tick.
- **Pattern timelines inherit the closed/scanned split.** A pattern whose waits are closed durations and which contains no event-waits has a *statically computable timeline* (every spawn time derivable without execution) — a **closed pattern**. One `wait_for` makes the timeline tick-emergent. "When does the action happen" always has exactly two answers: at a time you can compute, or at a tick you must reach — never "whenever the evaluator looks."

---

## 9. Manipulation, queries, events

**[spec]**

- `manipulate(query, callback)` — queries are predicates over columns (style, tags, position, **bullet-local age**: `age > 1.5`), cutting freely across birth structures. Callback receives the `Bullet` handle.
- **`Bullet` is a fixed data type**: the built-in columns (pose, velocity, epochs, standard tags) plus one **escape pointer** keying optional custom state in a sidecar table. The cost split is compiler-visible by inspection of the callback body: a callback touching only built-in columns compiles to a masked in-place SoA update (`pool[pred] = f(pool[pred])`) — hot-layer, vectorized, no fuel; a callback dereferencing the escape pointer, or spawning actions per bullet, runs on the control layer and bills fuel per matched bullet. DMK's batchable-controls vs SM-per-bullet split, recovered as an inferred property of one API.
- **Rematerialization** is the blessed event mechanism: snap current values (pose, velocity, tag samples) into fresh spawn-captured constants, swap the signal. Uses: re-aim-once (class (c)), reparenting, returning a bullet to `Closed`/scrubbable-land after an event, closure-splicing made explicit and cheap.
- **Epoch model** (remat clock semantics): every rematerializable slot carries an epoch column; birth time is just each slot's initial epoch. `remat(bullet, slot, new-signal)` writes `epoch := now`; the new signal runs on `τ = t − epoch`, starting at 0 at the event tick. Initial conditions are passed explicitly — the remat call snaps what it needs and hands it to the new signal as ordinary `ir` constants — so C⁰ continuity holds by construction; C¹ is a convention of stock helpers (`remat-straight = linear(snap pos, snap vel)` is fly-straight-from-here in one call). Remat is **per-slot**: a half-finished fade keeps running on its own epoch when the motion slot is swapped; restarting a tag means rematting that slot too. A bullet's history is a list of `(epoch, signal, constants)` segments — a remated bullet is **piecewise-`Closed`**, still fully scrubbable (scrubbing across a boundary consults the previous segment), and the segment record is exactly the replay-log entry, so no new machinery. Ancestor clocks (§3) stay orthogonal: remat moves only the local epoch.
- Scanned state is ordinary columns + step functions ⇒ snapshots are memcpys; manipulation of scanned bullets is writes to scanned state or signal swaps.

---

## 10. Patterns as data

**[spec]**

- **Guide objects are first-class**: extract positions (all, or by query) from a pattern as `Signal (Array Pose)` and use them as bases/frames for other patterns. (DMK idiom: invisible bullets spawned purely for their trajectories — here, a primitive.)
- Extraction typing is derived, not legislated: extraction from a **closed pattern** (closed timeline + closed dyns) is itself `Closed` — a pure query over a timeline that exists as data (birth-time columns + closed motion), evaluable at arbitrary t, usable as a base for further closed patterns. Extraction from anything touching live injected signals or event-waits is `Scanned` (well-defined only relative to the input trace).
- Cards as trees: the canonical (desugared) s-expression form serializes; upgrades are tree transformations (macros); fusion/deck operations are tree composition; frames-as-transformers and pattern-transformers (the SC Patterns algebra) are the manipulation vocabulary.
- **Pattern embedding is scope-explicit.** A top-level pattern used as a subtree must be wrapped in an explicit scope adapter: one binds the embedded pattern's state and channels into the embedding pattern's scope, the other keeps them pattern-local; bare embedding is ill-formed. The adapter node lives in the canonical tree, so card transformations (which are tree rewrites) cannot silently change sharing or capture — the composition-level analogue of the `shared` node (§5).
- **The card subset is a type-level characterization**, not a convention: serializable/scrub-safe = closed timeline, no `wait_for` on host events, channel I/O limited to declared injected signals. Boss scripts are card trees plus channel I/O (§3) and may forfeit these properties; the compiler can say exactly where.

---

## 11. Syntax

**[spec]** S-expression canonical form (BDSL is an s-expr language in curly-brace cosplay: head-word + typed arguments; BDSL2's blocks-as-values is `progn`). Static type unification with overload resolution and implicit conversions over the tree (BDSL's actual innovation), retained over the s-expr surface. Infix escape for dense trigonometry (`math` macro or reader syntax) — `80*t + sine(1, 0.2, t)` must stay writable. Frame-nesting last-argument sugar per §4. Surface syntax is a pluggable skin; node types + typing rules are the spec.

Reading property worth preserving: `(linear v (polar r ω (aimed …)))` reads outside-in as coarse-to-fine motion — carrier, ring, wiggle — matching how designers think.

---

## 12. 3D and alternative parametrizations

**[spec]** No dimensional lifting of the pattern language. 2D patterns + **emitter-frame embedding**: patterns execute in local oriented planes/cylinders/sphere-surfaces; a small vocabulary positions/orients/animates those frames in 3D (the NieR model: 2D patterns, 3D placement). Tunnel game: pattern space is `(θ, s)` on the unrolled cylinder; the player's tunnel pose (position + tangent) is a host-injected pose signal serving as the world frame; patterns parametrized on `s` remain closed ⇒ backward evaluation when the player backtracks; classes (a)–(c) reactivity survives non-monotone `s`; only class (d) needs monotone-section quarantine.

---

## 13. Open decisions **[decide]**

1. Exact `snap`-by-default boundary: which argument positions of which constructors implicitly snap injected signals (proposal: all spawn/action-time arguments), and the surface marker for `live`.
2. Ancestor-clock operator design (reaching pattern/global time from bullet scope) and its interaction with extraction.
3. Extended-entity constructor surface: signatures for `spawn-extended`, width/window signal conventions, blocking-laser feedback contract.
4. Event vocabulary enumeration and the channel API surface (injected/exported signal declaration, outbound event channels, host-handoff commands — §3 fixes the four-construct shape; the concrete API remains).
5. ~~Construction-vs-reference marking~~ — settled: explicit `shared(...)` nodes in the canonical tree; lexical let-binding rule is surface sugar that desugars to them (§5). Pattern-level analogue: explicit scope adapters for embedding (§10).
6. Exact column set of the fixed `Bullet` struct. (The boundary *mechanism* is settled — built-in columns + escape pointer, with the hot/control cost split inferred from callback bodies, §9 — but which columns are built-in is not.)
7. Angle representation (wrapped float vs unit vector) for θ columns.

---

## 14. Provenance map (concept → source)

| This language | Source |
|---|---|
| Two-layer hot/control split | DMK GCXF/bullet-fn split; SC sclang/scsynth |
| `Closed`/`Scanned` constructors | DMK `roffset`/`rvelocity` + the no-un-integration theorem, reified |
| `snap` / spawn capture | DMK GCX environment frames, reduced to one operator |
| Rates `ir/kr/ar` + inference | SuperCollider, as shape inference |
| Broadcasting/MCE | SC multichannel expansion; k/APL leading-axis style |
| `in-frame` + `+` two-op algebra | DMK V2RV2 (rotational/nonrotational offsets + angle), as syntax structure |
| Frame sugar = function | SC nested-UGen graphs: graph construction *is* ordinary evaluation |
| Structured concurrency phases | DMK cancellation-token hierarchy; `race` + finalizers |
| doneAction lifecycle | SuperCollider envelopes |
| Patterns-as-data algebra | SC Patterns library; DMK guide-object idiom promoted |
| Counter-based RNG | replay/scrub determinism requirements |
| Per-slot epochs / piecewise-`Closed` remat | replay-log segment records; DMK closure-splicing made explicit |
| Symmetric channels (inject/export/events/handoff) | sclang/scsynth OSC symmetry; DMK engine-interop audit (thjam13 boss scripts) |
| Bullet/pather/laser as axis materialization | unification replacing DMK's special-cased entities |
| Typed trees over dynamic tags | DMK v9→v11 negative lesson (GCXU removal) 
