# Tutorial 5: Channels and the Host Boundary

Runnable companion: **`cards/tutorials/t05.dmk`**. In the native player,
**T / Y / U / I** set the difficulty (0.7 / 1.0 / 1.4 / 2.0).

```sh
cargo run --release --manifest-path proto/Cargo.toml -p danmaku-player -- cards/tutorials/t05.dmk
```

Everything so far lived inside the card. This tutorial is about the
boundary: how a card reads the outside world, and how it publishes
itself back. Both directions go through one mechanism — **channels**,
the `$name` values you've been using since `$player`.

## Reading: injected channels

The host writes a set of named values every tick — the *input tape*:
`$move-x`/`$move-y` (the movement axes), `$focus-firing`, `$bomb`,
`$player` (in the sandbox, the mouse), and `$rank`, the difficulty.
Because every input arrives on the tape, a card plus its tape replays
deterministically — scrubbing works through gameplay, not just around
it.

Difficulty is nothing special — it's a number you multiply by
(`ex1-rank-density`):

```clojure
(for [vol inf :every (/ 2.5 $rank)]
  (spawn (circle (* 24 $rank) (linear p[1.8 0]))
         {:style {:family :circle :color :red :variant :w}}))
```

Higher rank: denser rings, faster volleys. Press T/Y/U/I and rerun. Two
habits from the start:

- **Multiplicative** scaling (`* $rank`) for continuous quantities, with
  a power to soften it where a layer would overwhelm —
  `(pow $rank 0.3)` varies much less than `$rank`.
- **Additive** scaling for counts you want exact (`ex2-rank-additive`):
  `(+ 3 (* 2 (- $rank 0.7)))` — three beams at easy, one or two more
  per step.

`$player` reads the same way, and `(aim $player)` rotates a frame
toward it at spawn time (`ex3-aimed`) — aimed fire is a frame
operation, not a special bullet type.

## Publishing: expose and export

The boundary runs both ways. A card publishes state two ways:

**`:expose`** maps an entity's *column* to a channel — the value tracks
the live entity, and reads 0 once it dies (`ex4-expose`):

```clojure
(spawn ((pose c[0 2.5]) (still))
       {:team :enemy :hp 20
        :expose {:hp $dummy-hp}
        :style {:family :lstar :color :green}})
```

Anything can read `$dummy-hp` now: the host draws its boss bar from it,
and other patterns react to it —

```clojure
(seq
  (wait-for (> $dummy-hp 0))          ; registered
  (wait-for (<= $dummy-hp 0))         ; destroyed
  (spawn (circle 24 (linear p[2 0])) {…}))
```

**`(export cell)`** publishes card-level state that isn't any entity's
column (`ex5-export`):

```clojure
(defvar volleys 0)
(export volleys)
…
(set! volleys (+ volleys 1))
```

`$volleys` is now a channel like any other — the host can display it, a
sibling pattern can fire a bonus every fourth volley. Exposed columns
and exported cells arrive identically at the reader; the difference is
only where the value lives.

## The player is card content

There is no engine-level player. The player is an entity a card spawns —
which means characters, co-op, and custom movement are cards, not
engine changes (`ex6-rig`):

```clojure
(spawn (clamp c[-3.8 -4.4] c[3.8 4.4]
         ((pose c[0 -3])
           (vel c[(* (live $move-x) (- 4.5 (* 2.7 (live $focus-firing))))
                  (* (live $move-y) (- 4.5 (* 2.7 (live $focus-firing))))])))
       {:team :player-body
        :colliders [{:layer :player-hurt :r 0.05}]
        :cols {:lives 3 :pilot 1 :iframes 1.0}
        :triggers [{:col :lives :leq 0 :event :game-over}]
        :style {:family :circle :color :white :variant :w}})
```

Reading it piece by piece:

- Position is the *integral of the raw axes* (`vel` over `$move-x`/`-y`),
  slowed while `$focus-firing` is held, clamped to the field. `(live …)`
  marks a channel read that stays live inside a signal rather than
  snapping its value at spawn.
- The hurtbox is a collider; `:lives` is a column; the hit effect
  (engine side) is a column write plus an `iframe-until` mercy window —
  its duration is the `:iframes` column, so it's part of the rig too.
- Game over is the rig's own *trigger*, not engine policy.
- `:pilot 1` marks this entity as the source of the derived `$player`
  channel — card-integrated movement overrides the host's mock, and
  aimed fire elsewhere in the card follows automatically.

Run `ex6-rig` and dodge: WASD to move, Shift to focus, and the aimed
fans track you — through the same `$player` channel your rig now
drives.

**Try it:** make `ex1`'s color depend on `$rank` (palette by
difficulty); give `ex6`'s rig a shot pattern (spawn `:team :player`
bullets with `:damage` from an `(in-frame :world …)` loop); expose the
rig's `:lives` and fire a taunt ring when it drops.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum).
Coming from DMK/BDSL? See the [migration notes](../from-dmk.md).*
