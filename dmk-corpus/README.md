# DMK translation corpus

Scripts pulled from https://github.com/Bagoum/danmokou (master, 2026-07).
Copyright (c) Bagoum, MIT-licensed (see `Assets/Danmokou.LICENSE.md` upstream);
vendored here unmodified as reference/test material for the translation exercise. The WebDemo
scripts (`0NN_*.bdsl`) are the author's graded feature demos — each isolates one
mechanism. The two boss scripts are full production spell-card fights. Each entry
below names the language.md claim it stress-tests.

| Script | DMK features | language.md claim under test |
|---|---|---|
| `020_gsrepeat.bdsl` | `gsrepeat times/circle`, `rvelocity` | §1/§5: repeater = map over `!n`; §4 `rvelocity` = `integrate` (Scanned by construction — is that acceptable for a straight-line bullet, or does it want the Closed `linear` form?) |
| `030_gcrepeat.bdsl` | `gcrepeat wait(...)` | §3 clocks: wait-between-shots dissolves into birth-time columns |
| `040_spread.bdsl` | `girepeat`/`gcrepeat` nesting, `rv2incr`, `spread`, indexed `color`, hoisted `hvar loop = i` | §5: modifier stack = arithmetic on θ columns; nested repeat = nested map; per-loop hoisting = closure over index |
| `060_polar.bdsl` | `polar(2t, ±20t)`, `bindLR`, `colorf(..., i/2)` | §4 Closed polar dyns; §2 Polar tag; §7 signal-valued color tags |
| `080_aimed.bdsl` | `target(ang, Lplayer)`, `frv2`, `bindArrow`, double-nested gsr | §3 injected signals: aimed = implicit `snap`; §4 frame composition through two nested repeats |
| `130_bowap.bdsl` | `preloop b{ increment += 0.4; rv2.angle += increment }` | §1's central claim, hardest form: sequential accumulator across an *infinite* repeater. Dissolves to θ(i) = 0.2·(i+1)(i+2) deg — pure function of shot index, but only because the recurrence has a closed form. What's the rule when it doesn't? |
| `070_dynamic_lasers.bdsl` | `laser ... dynamic(polar(2t, f(t, lt)))`, `hueshift(60·loop + 120t)` | §6 axis materialization: `lt` is exactly the `u` axis of `spawn-extended(f(t,u))`; §7 Closed color signals |
| `090_pathers.bdsl` | pather trails | §6: pather = trailing time-window materialization |
| `110_exploding_stars.bdsl` | `bulletcontrol(persist, batch(t > 0.6 + 0.3·&nStar, { sm(_), softcull }))` | §9: age-predicate query + explode-into-children + cull = manipulate + rematerialization. Note DMK spawns a *state machine per bullet* here — which layer does the callback run in? |
| `200_cradle.bdsl` | `guideempty2` binding `loc`/`dir` of invisible guides, `tmmod`, consumers via `dtpoffset`/`@` | §10: guide objects as first-class extraction — DMK's version is stringly hoisted channels; the claim is this becomes `Signal (Array Pose)` |
| `ingame-tutorial.bdsl` | phases, `hpi`, dialogue, practice blocks | §8: phase = `race(hp, timeout, attack)` with finalizers |
| `thjam13_mima.bdsl` | full boss: `phase 34 { hpi 19000 }`, `summon`, movement, `exec b{ mine<Enemy>()... }` C# interop, imports | ceiling test: how much of a production script is *pattern* (serializable card tree) vs *host scripting* (engine-object mutation that must become API commands) |
| `ph_boss2_mima.bdsl` | second full boss, different idioms | same, plus cross-script `import` (§ card composition) |
| `GTR2_Test.txt` | `gtrepeat` timing semantics (BDSL1 syntax) | regression oracle: DMK's own test for repeater clock nesting |

Suggested order: 020 → 040 → 060 → 130 → 080 → 070 → 110 → 200, then one
boss-script spell card. Each translation should state: (1) the canonical desugared
tree, (2) the inferred rate/constructor (`Closed`/`Scanned`, `ir`/`kr`/`ar`) of every
subexpression, (3) anything expressible in DMK that had no clean image (that's a
finding, not a failure).
