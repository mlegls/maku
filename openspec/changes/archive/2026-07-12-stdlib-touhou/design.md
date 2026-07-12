# stdlib-touhou — design

## Context

Three accretions in `cards/lib/touhou.maku`, all library card code (no core
changes): family→hitbox data, spellcard-shaped phase sugar, and the `col-or`
alias. Ground truth as of pick-up:

- Templates resolve `:hitbox` at macro time via `primary-hitbox-radius*`
  over the literal meta forms; the classic family radii (star/gem 0.09,
  lstar/gglcircle 0.20) live only as a prose comment (touhou.maku:15) and
  are re-typed at every call site (duel, coop, reimu_vs_mima, tutorials).
- `phases` clause opts (:hp/:until/:timeout/:root) desugar to state-body
  code; `boss` binds `boss-main` (unhygienic by convention) and `bind!`s the
  card's boss channel to `{:hp … :pos …}`.
- `reimu_vs_mima.maku` hand-rolls `(event :spell)` between phase clauses —
  the in-the-wild shape a spellcard template should absorb.
- Strings intern to keywords (`Form::Str` → `Val::Kw`), entity fields are
  numbers/symbols only, `(event name pos?)` carries no payload — so spell
  names/types ride the boss *channel* (channels carry any Val, maps
  included), not entity fields or event payloads.
- `col-or` is an exact alias of `default` (both inline to `%value-or` by the
  round-7 rewrite — lowering recognizes shapes, not names, so the alias has
  zero semantic or perf content). The "possible rename" note from a prior
  round is not findable in the archive; treating alias-dissolution as the
  decision.

Timing: `cards/lib/` is compile-time embedded into proto/core, so these
edits rebuild the core test corpus — implementation waits until the
scoped-channel-overrides working tree lands, to keep its test loop clean.

## Goals / Non-Goals

**Goals:**

- Family radii as one lib data table consumed by the templates; call sites
  drop the repeated numbers with bit-identical collider output.
- A `:spell` clause opt on `phases` giving name/type/lifecycle to hosts via
  the boss channel + events, absorbing the hand-rolled `(event :spell)`.
- One defaulting spelling: `col-or` dissolves into `default`.

**Non-Goals:**

- Engine/HUD rendering of banners or hp bars (host policy; the card exports
  data).
- Phase-body return-value routing (`states-return-routing`, separate change).
- Capture/bonus scoring semantics (needs a design of its own; the lifecycle
  events this change adds are its future input).

## Decisions

### 1. `family-hitbox` is macro-time data, explicit `:hitbox` wins

`(def family-hitbox {:star 0.09 :gem 0.09 :lstar 0.20 :gglcircle 0.20})` plus
a resolution chain in the template radius helper: explicit `:hitbox` >
`family-hitbox` lookup of the literal `:style {:family …}` > the template's
flat default (bullet/shot 0.12, player 0.06 — unchanged). Resolution stays
macro-time over literal meta forms, like `primary-hitbox-radius*` today; a
non-literal `:style` form falls through to the flat default (same behavior
as today, documented in the lib header). Values are the classic ones already
in the comment, so migrated call sites are bit-identical.

Rejected: runtime resolution (a signal-valued `:style` deciding the collider
radius) — colliders take their radius at spawn; making it style-reactive is
new semantics, not data motion.

### 2. Spellcard surface = a `:spell` opt on `phases` clauses, data on the boss channel

`(:label {:spell :v-of-victory :type :survival :hp n …} body…)` desugars,
alongside the existing opts, to:

- entry: `(set! $spell {:name :v-of-victory :type :survival})` and
  `(event :spell-declared)`;
- exit (via the clause's `finally`): `(set! $spell nothing)` and
  `(event :spell-end)`.

`boss` allocates the local stream: `(let [$spell nothing] …)` and extends
its producer map to `{:hp … :pos … :spell $spell}` — hosts read
name/type/liveness off the channel they already consume, and the discrete
events key banner triggers. `$spell` joins `boss-main` as a macro-bound
convention name. `:type` defaults absent (plain spell); it is card data the
lib passes through, not an enum the lib owns.

Why the channel and not fields/events: names are keyword-valued (strings
intern to keywords), entity fields hold numbers/symbols but the HUD read
path is the boss channel already, and events carry no payload — the split
falls out of what exists. Richer templates stay cards-over-this, as the lib
comment (touhou.maku:335) says; this change only moves the *mechanism*
(lifecycle + data plumbing) into the lib, not a genre HUD.

Hp bars: no new mechanism. `:hp` gates already define the segment
boundaries, and the channel's `:hp` is live; a host drawing segmented bars
has everything (`reimu_vs_mima` phases at 60/0 = two bars). If phase-start
hp capture proves wanted for fractions, it's one more producer-map entry
later.

### 3. `col-or` dissolves into `default`

Two names for one inlined shape is vocabulary without semantics. Lib call
sites (`hp-of`, contact rules) switch to `default`; `col-or` is deleted
rather than deprecated — the lib is pre-1.0 card code with a known,
greppable call-site set (only touhou.maku itself uses it).

## Risks / Trade-offs

- [Macro-time family lookup silently misses non-literal `:style` forms] →
  same fallback as today (flat default); the lib header documents the rule.
  No card in the corpus passes a non-literal style to a bullet template.
- [`$spell` name capture collides with a card's own `$spell`] → same
  convention risk as `boss-main`, accepted there; documented next to it.
- [Dropping `:hitbox` at call sites changes colliders if the table and the
  old literals disagree] → they are the same numbers by construction;
  the oracle card suites re-verify lowered-vs-interp over the migrated
  corpus, and collider radii feed collision tests directly.
- [Embedded-cards rebuild breaks the concurrent change's test loop] →
  sequenced: implementation starts after scoped-channel-overrides lands.

## Migration Plan

Lib first (table + template chain + `:spell` opt + col-or dissolution), then
call-site sweep (drop family-default `:hitbox`, port `reimu_vs_mima`'s
hand-rolled `(event :spell)` to the `:spell` opt), then gates: core suite +
the 4 ignored oracle card suites (lib edits change every embedded card).
