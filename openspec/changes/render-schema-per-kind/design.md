# render-schema-per-kind — design

## Context

The render field schema is one per-world key→kind map (`RenderFieldKind`
Num|Sym), accreted at runtime as keys appear. Three consequences drive this
change:

- Cross-card key poisoning: an imported card's `:width` sym claims the key
  world-wide, breaking another rule's numeric `:width` — the "one kind per
  key" contract has no scope.
- No contract at the pack boundary: mesh packs (mesh-touhou is the live
  case) ignore unknown columns and default missing ones — a card whose rows
  a pack can't meaningfully draw loads fine and renders wrong, the exact
  failure mode the host-channel manifest and style registry already solve at
  load time.
- Schema identity is runtime-emergent (`Rc::ptr_eq` memoized per rule,
  stable only "once dynamic-kind fields settle") — packs precompute layouts
  against something the card never declared.

Standing decisions this design builds on, not up for relitigation:
mesh renderers are hosts (render-rows spec, round 21); frame ordering /
row-expansion equivalence / staged batch abort semantics are settled;
`openspec/specs/load-time-schema/spec.md` says per-kind render schemas join
the one load-time pass "as their columns become statically declarable" —
declaration is what makes them declarable.

## Goals / Non-Goals

**Goals:**

- Named render kinds with registered row schemas; the key→fieldkind map
  scopes per kind.
- Load-time manifest negotiation: cards declare the kinds they emit, hosts
  declare the kinds they render, mismatch fails the load naming the kind.
- A builtin rename/pick adapter for composing cards/packs with conflicting
  field conventions.
- A declared sprite-batch kind in the touhou lib as the first consumer,
  negotiated by mesh-touhou.

**Non-Goals:**

- New structural geometry (`RenderData` stays Point/Polyline — a "mesh
  kind" as engine-side triangle transport waits for a consumer that needs
  it; sprite batching is a declared schema over Point, see decision 5).
- Frame-time interpolation (settled: tick-cadence snapshots).
- Style vocabulary ownership (stays host/library policy).
- JIT render kernels (drop in behind the column-fill seam later).

## Decisions

### 1. A kind is a declared name for a row schema

`(defrender-kind :sprite {:geometry :point :fields {:family :sym :color :sym :variant :sym :scale :num}})`
— card code (typically lib code), collected by the load-time pass like
`from-host` sites. A declared kind pins: geometry class (point|polyline),
field table (key → num|sym), and identity (the negotiation unit). Rows and
batches carry their kind; a rule's emissions check against the declared
table at registration time (compiled rules) or first emission (interpreted
path), failing with the same error surface as today's kind-surprise aborts.

Undeclared kinds keep today's behavior — runtime accretion, now scoped per
kind instead of per world — so existing cards load unchanged and ad-hoc
debug rows stay cheap. Declaration buys negotiation and load-time checking;
it is not mandatory.

### 2. The schema store scopes per kind

`kind → (key → RenderFieldKind)`, one kind per key *within* a render kind.
Rows without an explicit kind get `:default` (today's world map, renamed).
The staged batch-fill contract is unchanged — validation just indexes by the
rule's kind first. This alone kills cross-card key poisoning.

The row's kind is a distinguished slot on `RenderRow`/`RenderBatch`
(alongside geometry, not a keyed field): it is identity, not data — packs
dispatch on it before reading any column, and putting it in `cols` would
make every pack fish it back out of its own schema.

### 3. Manifest negotiation mirrors the host-channel contract

The card's render manifest = its `defrender-kind` declarations plus the
kinds its standing rules statically emit. Hosts pass their supported kind
set at load (`Sim::verify_render_kinds`, next to `verify_host_channels`);
a declared kind the host doesn't support fails the load naming the kind and
the declaring card. Undeclared (accretion) kinds are outside the manifest —
a load lint when a host provides a manifest, not an error, so scratch cards
keep working against strict hosts.

Negotiation gives packs stable identity: schema per declared kind is fixed
at load (no "settling"), so `Rc::ptr_eq` layout caches key off declarations,
and mesh-touhou's silent ignore/default behavior becomes a checked contract
for declared kinds.

### 4. The rename/pick adapter is registration-time key rewriting

`(render-adapt {:kind :their-sprite :as :sprite :fields {:col :color}} rules…)`
wraps imported rules: emitted kind and keys rewrite at rule registration, so
the adapted rule registers against the *local* kind's schema and the remap
folds into the rule's memoized `RenderSchema` — zero per-row cost on the
compiled path, one map lookup on the interpreted path. Pick = fields absent
from the map are dropped (schema-checked against the target kind after
rewrite). This is the render-side mirror of the collider meta namespace
adapter (language spec Reference §9) — same composition story, same
boundary.

### 5. Sprite-batch = a declared kind over Point, not new geometry

The touhou lib declares `:sprite` (point geometry + family/color/variant
fields it already emits); mesh-touhou negotiates for it and drops its
guess-and-default column probing for declared batches. This proves the whole
machinery on the live pack with zero new transport: instancing payload is
exactly what Point batches already carry as typed columns, and f32 packing
stays inside the pack (settled narrowing point). An engine-side triangle
transport (`RenderData::Mesh`) is deliberately deferred — "mesh renderers
are hosts" means the engine's obligation ends at typed columns, and no
current consumer needs card-authored triangles.

## Risks / Trade-offs

- [Per-kind scoping changes an observable error: cross-kind key conflicts
  stop erroring] → that error was the defect being fixed; the oracle
  suites re-baseline, and within-kind conflicts keep the exact staged-abort
  behavior.
- [Two schema regimes (declared vs accreted) to keep coherent] → one store,
  one validation path; "declared" only means the table is pre-filled and
  frozen at load. Divergence is structurally impossible — accretion into a
  declared kind's table is the same conflict check, it just always fails on
  new keys.
- [Manifest strictness could break existing host/card pairs] → hosts opt in
  by providing a manifest; hosts that don't (native player today) get
  current behavior plus lints.
- [Adapter adds a composition layer packs must reason about] → rewriting
  happens before registration, so downstream (schema store, packs, oracle)
  sees only the post-adapter world; nothing composes *with* the adapter.

## Migration Plan

1. Store + row/batch kind slot + per-kind scoping (behavior-preserving for
   single-kind worlds: everything lands in `:default`).
2. `defrender-kind` in the load pass; declared-kind validation.
3. `verify_render_kinds` + manifest lint; native player + wasm host wire it.
4. `render-adapt`.
5. Touhou lib declares `:sprite` / beam polyline kind; mesh-touhou
   negotiates and reads declared schemas.
6. Gates: core suite + oracle card suites; render-rows spec sync at archive.

Sequencing: implementation touches `interp/schema.rs` and `sim/` — starts
after scoped-channel-overrides lands (same reason as stdlib-touhou).
