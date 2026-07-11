# Tutorial 8: Stages and Campaigns

Runnable companion: **`cards/tutorials/t08.maku`**.

```sh
cargo run --release --manifest-path proto/Cargo.toml -p maku-player -- cards/tutorials/t08.maku
```

DMK distinguishes boss scripts, stage scripts, and campaign assets. This
engine draws the line differently: a stage script is just a card pattern
whose timeline owns enemy waves, boss handoffs, announce events, and
cleanup. A campaign is host metadata: which stage cards run, in which
scene/background, with which player choices, unlocks, endings, and save
menus.

So this tutorial has a runnable half and a mapping half. The runnable
half is the stage timeline; the host half is the campaign shell around
it.

## Stage phases are scopes

The smallest stage phase is a timed action with cleanup (`ex1-stage-phase`):

```clojure
(let [end (+ $tick (ticks 2))]
  (finally
    (until (>= $tick end)
      (for [i inf :every (ticks 12)]
        (bullet ((rot m"17*i")
                 (circle 10 (linear p[1.6 0]))) ...)))
    (cull)
    (event :stage-section-clear)))
```

There is no separate "stage phase" primitive here. The important pieces
are already the same ones boss phases use:

- `until` gives the section its lifetime and cancels everything forked
  inside the section.
- `finally` states the phase-edge policy. If this stage segment should
  clear the field, write `(cull)` there. If it should not, leave it out.
- `(event ...)` is the prelude macro for host-facing event emits: logs,
  UI messages, music changes, or practice markers.

DMK's `stage` phase property combines timer, UI label, practice segment,
and cleanup convention. Here those are card code plus host policy.

## Enemy waves

Most stage scripts do not fire from the invisible stage origin. They
summon enemies, and those enemies run short local timelines. DMK writes
this as `summonr(..., saction ...)`; here the equivalent is spawn
handles plus forked timelines (`ex2-fairy-wave`):

```clojure
(let [fairies (enemy formation {:hp 12 ...})]
  (for [e fairies]
    (fork
      (seq
        (invuln e 1.0)
        (wait 1.0)
        (for [v 4 :every (ticks 24)]
          ((pose (pos e))
            (bullet ((aim $player) ...))))
        (cull e)))))
```

The handle `e` is the fairy. `pos` samples it at firing time, so each
volley comes from wherever that fairy currently is. `invuln` is just a
column deadline used by the contact rules, so entry invulnerability is
not special to bosses.

The fairies' entrance motion uses signal `stages`:

```clojure
(stages
  (stage 1.0 (vel c[0 -3.0]))
  (stage 2.0 (still))
  (forever (vel c[0 1.5])))
```

Do not confuse the names: this `stages` is a motion-segment signal, not
a campaign stage. It is useful here because "enter, hover, leave" is a
piecewise trajectory.

## Boss handoffs

DMK stage scripts summon bosses by key, and the key resolves through
`BossConfig`. This engine keeps the sim side direct: a stage can embed a
boss pattern or a host can choose which pattern to run for a stage slot.
The runnable midboss (`ex3-midboss`) is just `boss` inside the
stage timeline:

```clojure
(defchannel $stage-boss {:hp 0})

(boss $stage-boss (live $boss) {:hp 30 ...}
  (phases
    (:midboss {:timeout 3 :root c[0 2.1]}
      ...
      (finally
        (cull)
        (cull (nth boss 0))
        (event :midboss-clear)))))
```

The public `$stage-boss` channel is what a host can draw as a boss bar.
The hp gate is local to `boss-main`; there is no global `$boss-hp`, so a
stage can have support enemies, midbosses, or multiple boss-like
entities without corrupting one shared channel.

## Announce and dialogue phases

DMK's `announce` and `dialogue` phase properties mostly affect UI and
gameplay timers. Here they are host handoffs (`ex4-announce-dialogue`):

```clojure
(seq
  (bind-channel! $stage {:section :announce})
  (event :stage-announce)
  (wait 0.7)
  (bind-channel! $stage {:section :dialogue})
  (event :dialogue)
  (wait 0.8)
  ...)
```

A real host might freeze score decay while `$stage.section` is
`:announce` or `:dialogue`, run a VN scene on `:dialogue`, and then
resume the card when the scene completes. The core mechanism is the
same: publish state, emit events, wait for a known duration or a
host-provided channel/event.

## A complete stage pattern

`ex5-full-stage` puts the pieces in sequence:

```clojure
(seq
  (event :stage-start)
  (wait 0.5)
  (ex2-fairy-wave)
  (wait 0.8)
  (ex3-midboss)
  (event :stage-end)
  ...)
```

That is the stage script. The host can run this pattern as "Stage 1",
mount a player rig, choose a background, record checkpoints, and expose
practice entries. None of that changes the card language.

## Campaigns live in the host

DMK campaigns are Unity assets: `GameDefinition`, `CampaignConfig`,
`StageConfig`, `SceneConfig`, ending configs, replay-save scenes,
practice unlock logic, build-profile scene lists. In this engine, those
belong to a host frontend or game shell. The core contract is small:

| campaign concern | core/card surface |
|---|---|
| stage list | ordered list of card patterns to run |
| stage scene/background | host metadata keyed by stage id |
| player choices | host mounts a player rig or injects raw input channels |
| boss/stage practice | host starts a pattern/checkpoint directly |
| stage completion | card emits an event or completes its top-level task |
| endings | host chooses another card/pattern after the campaign state is known |
| nonlinear routing | host campaign controller chooses the next stage pattern |

The card should publish enough state for the host to make those
decisions: stage section, active boss, checkpoint id, score/rank
channels, and outbound events. The campaign controller should not need
to know how a fairy wave is authored.

**Try it:** in `ex5`, swap the midboss and fairy wave; remove the
`(cull)` in the midboss finalizer so bullets carry into the next
section; add a second `$stage` field such as `:checkpoint` and watch it
from the host.

---

*The topic sequence of this tutorial series follows the
[Danmokou](https://dmk.bagoum.com/) engine's tutorials (MIT, © Bagoum);
this chapter corresponds to DMK's `tstages`. Coming from DMK/BDSL? See
the [migration notes](../from-dmk.md).*
