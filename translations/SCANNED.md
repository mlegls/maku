# Scanned signals: surface design

The corpus translations barely exercised `Scanned` (§3) — everything DMK-demo
was `Closed` or an integrated constant. `ph_boss2_mima.bdsl` supplies the real
material: per-bullet mode flags (`reflected`/`vel` + `switch`), guide-riding
bullets that detach, pattern-level mutable gating (`isAccel` + `whiletrue`).
This file develops the Scanned surface against it.

## 1. The raw constructor

```edn
(scan init-state step)          ; step : (state, inputs) → [state' out]
```

`inputs` is the injected snapshot plus `:dt` — scans are the one place live
signals arrive unsnapped by construction (§3 class (d)). Per §5, a scan
constructed inline under broadcast is fresh state per element (own columns);
`shared(...)` for one instance. Steps are *pure transitions*: waiting is
state (a countdown), never `wait` — `Action` doesn't fit in the return type
and would be inert if smuggled.

Raw-scan homing, for reference (what the sugar below compiles to):

```edn
(def homing
  (scan {:pos c[0 0] :dir -90}
    (fn [{:keys [pos dir]} {:keys [player dt]}]
      (let [dir' (toward dir (angle-to (- player pos)) (* 120 dt))
            pos' (+ pos (* dt p[3 dir']))]
        [{:pos pos' :dir dir'} (pose pos' dir')]))))
```

## 1b. Derived domains: vel/acc with self-reference (adopted)

`(vel sig)` / `(acc sig)` are §4's integrate-1/2-times constructors, `Scanned`
by construction. Adopted extensions:

- **Vel/acc slots bind self-state** (F12 extended): `pos` and `dir` in vel
  slots, plus `vel` in acc slots — DMK precedent: velocity functions receive
  `bpi` including own location. Self-reference is feedback, and these signals
  are already scans, so the type story is unchanged.
- **Injected kinematics**: the input snapshot carries player vel/acc alongside
  pos; `(deriv sig)` differentiates any signal (finite difference, one
  prev-sample column — the same machinery §4 uses for heading).
- **`(slew rate sig)`** — angle-aware rate limiter (shortest-arc; SC `Slew`).

Homing becomes one line:

```edn
(vel p[3 (slew 120 (angle-to (- (live player) pos)))])
```

`live` stays explicit — a vel slot is a spawn argument, so snap-by-default
applies; plain `player` aims once at spawn, `(live player)` tracks. The
scrub-breaking choice is visible (§3 class (d), marked).

- **Base + correction emerges; no operator needed.** Signals are a vector
  space pointwise and integration is linear, so `(vel (+ ballistic (* 0.3
  correction)))` and cross-domain `(+ (polar …) (vel correction))` both just
  type-check. Implementation note: additive decomposition confines scan state
  to the correction term — the closed base stays hoistable; "mostly-ballistic
  with slight homing" costs a small scan, not a scanned trajectory.

## 2. `stages` — the synchronous-feeling surface

Raw `scan` is the assembly language. The common shape — "do this motion for a
while, then that one" — wants sequential reading without the Action `wait`:

```edn
(stages
  (stage 0.5  (linear c[3 0]))                  ; closed segment, 0.5s
  (stage 1.2  (fn [exit] (polar m"2*t" m"30*t")))  ; t REBASES at the boundary
  (until (fn [in] (< (:y (:pos in)) -3))        ; predicate-terminated segment
         wobble)
  (forever (linear c[0 -1])))
```

- Each segment runs on its own epoch: `t` rebases at every boundary — the
  per-slot epoch model (§9) verbatim.
- The optional `(fn [exit] …)` form receives the *snapped exit state* of the
  previous segment (pose, velocity, tag samples) — continuity is explicit
  initial-condition passing, the same philosophy as `remat`. Stock helpers
  (`then-straight` = fly straight from wherever you were) cover the C¹ cases.
- **`stages` is not literally `wait`, but reads like it.** Durations are data
  (`Float`), not Actions; the type discipline is untouched.

### Compilation: graceful degradation

- All durations constant + all segments `Closed` + no `until` ⇒ the whole
  signal is **piecewise-Closed**: a static segment table, evaluable at
  arbitrary t (find segment, evaluate at t − epoch). Scrub- and rewind-safe.
- Any `until`, input-dependent duration, or `Scanned` segment ⇒ the whole
  signal is `Scanned` with state = (segment index, segment-local state).
  Constructor contagion does the classification; no annotation.

This mirrors §8's pattern-timeline rule exactly: a segment boundary is either
at a time you can compute or at a tick you must reach — `stages : signals ::
seq/wait : actions`, with the closed/tick-emergent distinction appearing
identically in both layers.

### The unification

`stages` and `remat` are one mechanism viewed from two sides:

- **`stages` = statically-scheduled rematerialization.** The segment list is
  the §9 `(epoch, signal, constants)` history, known up front.
- **`remat` = event-driven stage transition.** An external event appends the
  next segment at an unpredictable tick.

A bullet's motion is always a segment sequence; the only question is whether
the boundaries are data (closed), predicates (scanned), or events (remat).
Mixing is natural: `(stages (stage 1.5 homing) (forever then-straight))` is
"home for 1.5 seconds, then fly straight" — a scanned segment handing its
snapped exit to a closed one. That is §3's fairness discretization (class (d)
→ (c)) expressed in five words.

## 3. Corpus contact: ph_boss2_mima's reflected bullets

DMK (Spell 1/2/3, recurring idiom):

```
preloop b{ hvar vel = pxy(0,0); hvar reflected = false }
s switch(reflected, nrvelocity(vel), nroffset(load "eloc" p))
```

Every bullet carries a mode flag and a spare velocity, and pays a `switch`
branch **per bullet per frame**, so that some control elsewhere can flip
`reflected` and write `vel`. This is hand-rolled rematerialization — the
eliminated state sneaking back in, exactly as design.md §2 predicted.

Ours: bullets ride the guide frame (F14, unexpressed dyn) with no flags; the
reflection control is an event-driven remat:

```edn
(manipulate (query (= :family :scircle) (colliding-with :reflector))
  (fn [b] (remat b :motion (linear (reflect (vel b) (surface-normal b))))))
```

One signal swap at the event tick. No per-bullet columns for the "maybe
later" state, no hot-path branch, no polling. Note the same remat is also the
*detach* operation: `nroffset(load "eloc" p)` → world-frame `linear` is
reparenting-by-rematerialization (§9), which is what reflection off a guide-
riding bullet actually is.

Similarly, `isAccel` (an `exec`-mutated var polled by `whiletrue` in two
concurrent laser loops) is mutable-flag emulation of *structure*: the two
laser behaviors are arms of successive stages of the attack loop — `seq`/
`race` scope expresses the gating, no shared flag.

## 4. What genuinely needs raw `scan`

After the above, the honest residue is small and matches §3's prediction:
- continuous feedback over live signals (homing, drift fields with memory);
- stateful visual effects (proximity flicker with hysteresis);
- blocking-laser extent (world geometry feedback, §6).

Everything else in the boss script is segments + events wearing state flags.
