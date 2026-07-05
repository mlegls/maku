# Tutorial 2: Bullet Controls

Runnable companion: **`cards/tutorials/t02.dmk`** — press the number keys to
run each example.

```sh
cargo run --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t02.dmk
```

The concept sequence follows [DMK's Tutorial 2](https://dmk.bagoum.com/docs/articles/t02.html)
(MIT, © Bagoum). DMK introduces *bullet controls* — persistent objects
applied per-style every frame, with their own lifecycle commands. Here the
whole apparatus dissolves into two things you already have: **queries** and
**the control layer**.

## Part 1: A control is a loop

A "bullet control that flips circles at the ceiling for four seconds" is,
written honestly (`ex1-flip`):

```clojure
(fork
  (dotimes [i 480 :every (ticks 1)]              ; four seconds of control
    (manipulate {:family :circle
                 :where (fn [b] (and (> (:y (:pos b)) 3)
                                     (> (:y (:vel b)) 0)))}
      (fn [b] (remat b (fn [exit]
        (linear c[(:x (:vel exit))
                  (- 0 (:y (:vel exit)))])))))))
```

- `(manipulate query callback)` runs the callback on every live bullet the
  query matches, *right now*. Persistence isn't a property of the control —
  it's a loop. The "timer" that destroys the control after four seconds is
  the loop being finite (`480` iterations); an event-driven stop is
  `(until pred …)` around it.
- The **query** selects by style axes and an arbitrary pure predicate over
  the bullet's view: `:pos`, `:vel`, `:t` (age), plus any columns it
  carries. `(:x v)` / `(:y v)` read vector components.
- The **flip is a rematerialization** — the one blessed event mechanism for
  changing motion in flight. `(remat b (fn [exit] dyn))` snaps the bullet's
  state (`:pos`, `:vel`, `:t`), swaps its motion for the new signal (which
  anchors at the snapped pose and restarts its local clock), and clears its
  scan state. The flipped bullet is a *fresh closed signal* — it stays
  scrub-safe, piecewise. DMK implements flipping with per-bullet mutation
  flags inside the motion function; remat is that idea made a single
  explicit operation.

Note the predicate includes `(> (:y (:vel b)) 0)` — flip only while moving
up. A stateful "already flipped" flag is what DMK's control keeps
internally; here the condition is simply written out.

## Part 2: Selection

DMK selects styles with wildcard-string cross-products. Style here is a
structured record, so selection is a typed query — an array on an axis
means *any of* (`ex2-select-cull`):

```clojure
(manipulate {:family [:circle :star]
             :color [:red :blue]
             :where (fn [b] (< (:y (:pos b)) -2))}
  (fn [b] (cull b)))
```

That's "circles and stars, in red or blue, below y = −2" — the
cross-product is implicit in matching a record against per-axis
alternatives, and there are no wildcard strings to resolve. Green bullets
of the same families sail past untouched; run the example and watch.

## Part 3: Batches are just seq — and ordering is honest

Several effects under one predicate need no dedicated `batch` construct;
the callback body is a `seq` (`ex3-restyle-at-age`):

```clojure
(manipulate {:family :circle :where (fn [b] (> (:t b) 1))}
  (fn [b]
    (seq
      (event :sfx "x-transform-1")
      (set-style b {:family :arrow :color :red :variant :b}))))
```

- `(set-style b {...})` is restyle — legal as an *event-level* operation
  precisely because style is `ir` (pool identity), never a signal (§7).
- DMK documents a trap: putting the restyle before the sound silently
  drops the sound, because controls stop when the bullet transfers styles.
  Here the same ordering question is visible in the `seq` — and the failure
  mode is gentler: verbs after a `(cull b)` are dead-handle no-ops
  (generation-safe), not silent cross-control interactions.

## Part 4: Spawning from bullets

The last DMK control runs a whole StateMachine at a bullet's position.
Here a callback is control-layer code, so it can simply spawn — anchored
wherever you like (`ex4-burst`):

```clojure
(manipulate {:family :star :where (fn [b] (> (:t b) 1.1))}
  (fn [b]
    (seq
      ((pose (pos b))
        (spawn (circle 8 (linear p[3 0]))
               {:style {:family :gem :color :pink :variant :w}}))
      (cull b))))
```

`(pos b)` reads the bullet's current world position; wrapping the spawn in
`(pose …)` anchors the ring there. Callbacks always spawn in world
coordinates — ambient frames deliberately stop at lambdas (§4), so a
control firing under some rotated hierarchy doesn't double-anchor its
spawns. The cull afterward is DMK's `softcull` minus the fade effect
(spawn your own effect bullet first if you want one).

## Cost model, in one paragraph

Everything above runs on the control layer and bills fuel per matched
bullet — fine at tutorial scale, and the honest semantics. The §9 design
splits the same API by *inspection*: callbacks that only write built-in
columns compile to masked SoA updates (hot layer, no fuel); callbacks that
spawn or run actions stay control-layer. You write one thing; the split is
inferred.

| DMK Tutorial 2 concept | here |
|---|---|
| `bulletcontrol(persist, sel, ctrl)` | `(fork (dotimes [i inf :every (ticks 1)] (manipulate sel ctrl)))` |
| persistence predicate / timers | loop bounds, or `(until pred …)` |
| `poolcontrol(sel, reset)` | stop the loop (cancel its scope) |
| style selector `{{ "circle-*" }, { "red/w" }}` | query map: `{:family :circle :color :red}` — arrays = any-of |
| `flipygt(4, _)` | `(remat b (fn [exit] …))` with the flip written out |
| `cull(y < -2)` | `:where (fn [b] (< (:y (:pos b)) -2))` + `(cull b)` |
| `batch(pred, {…})` | a `seq` in the callback |
| `restyleeffect(style, fx, _)` | `(set-style b {...})`, plus your own effect spawn |
| `sm(_, sync …)` | spawn directly in the callback, anchored by `((pose (pos b)) …)` |
| `softcull("cwheel-…", _)` | effect spawn + `(cull b)` |

Next: [Tutorial 3](03-movement-functions.md) — movement functions (time in
expressions, easing, and the closed/integrated split).
