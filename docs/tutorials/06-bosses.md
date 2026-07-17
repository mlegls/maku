# Tutorial 6: Bosses, Phases, and Script Structure

Runnable companion: **`cards/tutorials/t06.maku`**. [Open Tutorial 6 in the
interactive player](https://neen.ink/projects/maku/play.html?card=t06).

To run the same card in the native player:

```sh
cargo run --release --manifest-path crates/Cargo.toml -p maku-player -- cards/tutorials/t06.maku
```

Every card so far was one pattern firing forever. Real fights have
*structure*: a boss opens with a non-spell, breaks into a spellcard at a
health bar, cleans up the field at each transition. This tutorial builds
that stack from the bottom: the bare state machine, the phase sugar over
it, and the boss template over both.

## A card is just code

Before the machinery, the frame around it. A card is a sequence of
top-level forms: `defpattern`s, `defn`s/`defmacro`s, imports, contact
rules. There is no privileged "script object" — the *first* `defpattern`
is the default pattern a host boots, and the number keys select others
(that's how these tutorial cards work). `(import "touhou")` splices the
genre library; `(import "path.maku")` splices another card's text, so a
full fight assembles from translation files and helpers by import
(see `cards/reimu_vs_mima.maku` for the worked example).

## The primitive: `states`

Structure is a state machine, and the machine is a *bare FSM* — not a
boss template:

```clojure
(states
  (:red
    (fork (seq (wait 1.2) (goto)))
    (for [i inf :every (ticks 20)]
      (bullet ((rot m"17*i") (circle 8 (linear p[1.8 0]))) …)))
  (:blue
    (fork (seq (wait 1.2) (goto :red)))
    (for [i inf :every (ticks 20)]
      (bullet ((aim $player) (fan 5 10 (linear p[2.4 0]))) …))))
```

Run `ex1-states`: rings for 1.2 seconds, then aimed fans, alternating
forever. The rules, in order of importance:

- **A state ends by `goto` or by its body completing.** Bare `(goto)`
  exits to the next state in order; `(goto :label)` routes anywhere.
  Falling off the last state completes the machine.
- **State exit cancels the state's whole task subtree.** The `for` loop
  above is infinite, and the forked timer still ends it — everything
  forked inside a state dies the tick the state exits. This is what
  makes movesets safe to fork: no orphaned turrets surviving into the
  next phase.
- **There is no timeout feature.** `(fork (seq (wait d) (goto)))` *is*
  the timeout — a timer racing the body. hp gates, repositioning, phase
  names: all state-body code too, as we'll see.
- **Labels are values**, evaluated at the goto. `ex2-markov` routes with
  `(goto (nth [:calm :burst] (rand-int 0 2)))` — a Markov chain over
  attack moods in one line.

And the machine is not boss-shaped. `ex3-control` is a *control* FSM:
two movesets whose transitions read an input channel (hold Shift to
switch). States + channels is the general tool; bosses are one use.

`goto` is scoped to the innermost lexical machine — an imported
pattern's machine can't hijack yours — and because targets are labels
rather than indices, inserting a state never re-points an existing
transition.

## The sugar: `phases`

Boss fights repeat four idioms — the hp gate, the timeout, the
repositioning, the cleanup — so the touhou library wraps them as clause
options over `states` (`ex4-phases`):

```clojure
(phases
  (:warmup {:timeout 1.2}
    (for [i inf :every (ticks 15)] …)
    (finally (event :warmup-done)))
  (:main
    (for [i inf :every (ticks 20)] …)))
```

| option | desugars to |
|---|---|
| `{:hp n}` | `(until (<= (hp-of boss-main) n) body)` — the local health-bar gate inside `boss` |
| `{:until pred}` | the same race with any predicate |
| `{:timeout d}` | `(fork (seq (wait d) (goto)))` |
| `{:root pos}` | `(move-to boss-main 0.9 eoutsine pos)` at the body head |
| `(finally …)` tail | core `(finally body cleanup…)` — cleanup on *every* exit path: gate, timeout, goto |

That table is the whole feature. `phases` is a macro in
`crates/core/lib/touhou.maku` — about thirty lines of clause-walking you can
read, and the desugared output is exactly the `ex1` shapes. If your
game means something different by "phase", you write a different macro;
the engine has no opinion.

## The template: `boss`

A boss is an *enemy with a phase machine* — that is the entire
difference. `boss` owns the conventions (`ex5-boss`):

```clojure
(defchannel $tutorial-boss {:hp 0})

(seq
  (boss $tutorial-boss (pose c[0 2.6])
              {:hp 40 :hitbox 0.45 :style {:family :lstar :color :purple} :scale 2}
    (phases
      (:opening {:timeout 2 :root c[0 2.2]}
        (for [vol inf :every (ticks 30)]
          (bullet ((aim $player) (fan 3 14 (linear p[2.2 0]))) …))
        (finally
          (event :spell)
          (cull)                          ; field clear at the phase edge
          (invuln (nth boss 0) 1.0)))     ; transition mercy window
      (:spell {:hp 0}
        (for [vol inf :every (ticks 24)]
          (bullet ((rot m"11*vol") (circle 16 (linear p[1.6 0]))) …))))))
```

The leading `move` is the legacy boss-anchor shortcut used before the
boss entity exists. Inside `phases`, `:root` now targets the local
`boss-main` handle directly.

What the macro does for you: binds `$tutorial-boss` as a map-valued
host channel (`{:hp … :pos …}`), holds the machine until the boss has
actually registered, and binds `boss` (the spawn's handles) plus
`boss-main` (the first handle) for the machine body. The `{:hp n}` gates
read that local handle, so multiple bosses do not fight over a global hp
channel. What it *doesn't* do is policy. Look at the `finally`
block: clearing the field at a phase break and granting a mercy window
are one `cull` and one `invuln` — card code, stated where it happens.
There is no phase *type*; a phase is characterized by which gate and
which cleanup you wrote. That keeps the genre conventions in reach of
the card: a capture bonus is a `wait`-vs-`(hp-of boss-main)` race (did
the health bar empty before the timer?) with an item drop in the winning
branch — three lines, next to the phase they judge.

Two structural notes:

- **There is no setup phase.** Setup is code before the machine — the
  `move` above, a local stream, whatever. While developing, point a `goto`
  at the state under test, or just reorder the clauses.
- **Explicit routing is rare.** Since a state's default successor is
  the next state in order, a linear fight writes no `goto` at all;
  labels are for skips, loops, and computed transitions.

Run `ex5-boss` and shoot the boss down (the sandbox rig fires with the
mouse): aimed fans until the timeout or your damage ends the opening,
then the field clears, the boss blinks through its mercy window, and
the spiral spell runs to `{:hp 0}`.

**Try it:** give `ex5`'s opening an `{:hp 20}` gate instead of the
timeout and race it; export a `$phase` cell written at each state head
and watch it from the host; make `ex2`'s Markov weights read `$rank`.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum);
this chapter corresponds to DMK's t07 (t06 is a design-philosophy essay
— see the [migration notes](../from-dmk.md) for the concept mapping).
Coming from DMK/BDSL? The [migration notes](../from-dmk.md) map every
construct.*
