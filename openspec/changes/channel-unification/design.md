<!-- Moved verbatim from docs/notes/channel-unification.md (dissolve-design-notes).
Picking up the channel-unification change is the ratification decision. -->

# channel/cell unification — streams as sigiled bindings

Status: DESIGN CONVERGED (2026-07 discussion), not yet implemented or
ratified in language.md. Supersedes the cell/channel split of
language.md §channels/§control-cells if adopted. Sequencing: after the
current compiled-dyn/live-evolve work; composes with the model/ split
(openspec/changes/model-split/design.md), where "live input" becomes one model concept.

## Motivation

Cells and channels have channel-shaped read semantics with two parallel
APIs. In-language capability differences are small and mostly accidental:

- cells only: imperative `set!`, per-instance scoping with adapter-gated
  `(inline)` sharing;
- channels only: `(channel $x default)` reads, first-class live frames
  (`DynNode::Live` only knows channel names — a pose-holding cell cannot
  anchor a tracking frame; wart, not design), host manifest/tape surface.

The dynamic cell scope also poisons static name resolution in signal
slots — the hazard that forced the live-only cell-read pin
(Ctx.signal_scope) and had silently disabled def inlining for 100% of
corpus lowerings. Design evolution considered and rejected:
sigiled-but-separate cells; positional privacy (`defchannel` legal in
pattern bodies); a `(channel init)` constructor with unsigiled handles
(read ergonomics: every read needs `snap`).

## The design: the sigil IS the stream marker

`$name` always names a stream. Binding position constructs; reference
position reads by today's channel conventions.

```clojure
(def $player)                                ; declare
(bind! $player (from-host :player))          ; produce (host is just a producer)
(export! $player)                            ; publish

(def $num-enemies)
(bind! $num-enemies (sum (entities-where ...)))
(export! $num-enemies)
                            ; (defchannel dissolves into def + bind! + export!)

(defpattern turret [$ammo]  ; sigiled param: receives the stream (handle)
  (set! $ammo (- $ammo 1))) ; unsigiled param would snap instead

(defpattern boss []
  (let [$shared 30]         ; local sigiled binding = fresh private stream
    (par (turret $shared) (turret $shared))))  ; explicit sharing
```

Reference semantics (unchanged from today's channels): bare `$x` in
control-layer/spawn-arg position reads the current value — snap falls
out of the evaluation site, not an annotation; `(live $x)` yields the
first-class tracking value; `(set! $x v)` writes; signal-body reads are
per-tick. Passing the stream itself = sigiled parameter or closure
capture. A free `$name` (neither bound in scope nor def'd) is a LOAD
ERROR — this is the channel-manifest check (TODO) falling out of
scoping.

## Lifecycle: everything starts private and assigned

Declare, produce, publish are three orthogonal ops:

- declare: `(def $x)` / `(def $x init)` top level, `(let [$x init] ...)`
  local;
- produce: `(bind! $x expr)` attaches a per-tick refresh producer. Host
  injection is NOT special syntax — `(from-host :name)` is just a
  producer, so the writer axis is simply set!-only (no producer) vs
  bind!ed;
- publish: `(export! $x)` registers the stream with the host/registry.

Today's cells = unbound+unpublished; today's channels = bind!ed+
published (injected ones bind!ed to `(from-host :name)`). The other
quadrants (bind!ed-private, set!-only-public) become expressible. The
channel manifest = the set of `(from-host ...)` SITES (bound or not),
checked at load against what the host provides.

Because `(from-host :name)` names the host input explicitly, it is a
stream-valued expression in its own right — usable standalone, not only
as a bind! producer. Injection and naming decouple: an anonymous
injected stream can be snapped at an eval site, wrapped in
`(live ...)`, or passed to a sigiled param directly; `(bind! $x
(from-host :name))` is just the case where you give it a local name.
This detaches the two things `defchannel` was conflating (declaring a
host input, and declaring a named stream).

`set!` vs `bind!` coexistence — resolved by the EXISTING defchannel
fallback rule rather than a seal, and keyed purely on bind!ed-ness (no
host special case): the producer runs at refresh and overwrites, unless
it yields `nothing`, in which case the last `set!` stands. An
always-writing producer — `(from-host)` included — effectively seals (a
`set!` is visible only until the next refresh — well-defined; refresh
order is already pinned: defs in order, then bound producers). A lint
for "set! on a stream with an always-writing producer" beats a hard
error.

## What each cell mechanism maps to

- defcell → `let` with sigiled name (fresh per instantiation — instance
  scoping becomes ordinary evaluation, no parallel scope machinery);
- adapter-gated `(inline)` cell sharing → explicit handle passing
  (sigiled params / closure capture) — capability-style;
- export → `(export! $x)`, a visibility flip into the public registry;
- (live cell) frame gap → closes: `DynNode::Live` holds a handle, local
  and global streams frame identically;
- lowering: `$x` is structurally a stream; captured handles classify as
  channel-input slots (compiled-dyn milestone B) with no name analysis.

## Kernel deletions unlocked

`CELLS_KEY` env threading, `cell_scope`, `fresh_cell_scope`, the
CallPattern caller_cells/fresh_cells adapter plumbing, the bare-sym
cell-read arm (already signal-gated). The id-keyed `sig.cells` backing
store can remain as the runtime representation (handles get
deterministic identity from deterministic re-execution; `set!` stays a
frame-stamped action — scrub story unchanged).

## [decide]

- Export/registration collisions across instances of one pattern:
  rename form (`(export! $vol :as $p1-vol)`) + collision as load error
  (lean), vs latest-wins like bound channels today.
- Naming: `from-host` vs `inject` (or similar) for the host-input
  producer form. (Name resolution itself is settled: explicit arg —
  inference is incoherent for the standalone/anonymous form.)
- Exact surface of `bind!` vs today's `bind-channel!` (local producer
  attachment vs global-name registration — likely one form once names
  are scoped).
- Migration: two corpus files use cells (cards/tutorials/t05.maku,
  cards/translations/ph_boss2_spell2.maku) plus stdlib defchannels
  rewritten as `def $name`.
