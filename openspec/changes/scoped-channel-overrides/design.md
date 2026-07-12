# scoped channel overrides — design

## Context

`(with {$chan v} body)` is specified (§3: "with is to channels what in-frame
is to poses") but unimplemented; §13.8 holds the residuals: nesting/shadowing,
which derived channels are overridable, signal-valued override values.

Post channel-unification, the ground truth is: every `$name` resolves
lexically to a stream handle `Val::Stream(id)`; reads go through the id-keyed
store (`sig.cells` / `stream_val(id)`); signal-time reads are
`DynNode::LiveStream { id }` (interp eval at motion.rs `dyn_node_*`, lowered
eval via `ChanRef::Stream(id)`); the scheduler already maintains ambient
context by folding the task stack (`TF::Frame` → `ambient()` in exec.rs), and
forked children inherit the live stack, which is exactly how in-frame's
"distributes over control combinators" is realized at execution time.

## Goals / Non-Goals

**Goals:**

- `(with {$chan v ...} body...)` as an action combinator: dynamic binding for
  the extent of the body's execution, including callees ("code you cause")
  and spawned signals for their lifetimes (`live` reads included).
- Settle the §13.8 residuals; resolve them in the spec delta.
- Bit-exact determinism and scrub-safety; no hot-path regression for cards
  that don't use `with`.

**Non-Goals:**

- The `:sealed` pattern-embedding adapter that *blocks* overrides (§10
  residual — stays open, but the design leaves it a one-line check).
- Lowering `with`-scoped signals beyond what streams already get (live-stream
  reads in lowered tiers go through `ChanRef::Stream`; no new tier work).
- Rank-modulation stdlib vocabulary over `with` (library work, separate).

## Decisions

### 1. Mechanism: fresh-id indirection, not save/restore mutation

At execution of a `with` node, allocate a **fresh override stream** X′ per
binding (id from the world counter — deterministic), initialize it to the
override value, and push an override frame `{base id X → X′}` onto the task
stack. Every read/write of a stream inside the extent resolves its id through
the composed ambient override map first.

Why not save/restore on the base stream (classic dynamic-wind): concurrent
tasks outside the extent must keep seeing the base value — the sim is full of
interleaved tasks, so temporal mutation is wrong, not just inelegant. The
fresh-id scheme is "dynamic *binding*, not mutation" literally: the base
stream and its producer keep running untouched (the tape is unaffected), the
override cell is an ordinary store cell (snapshot deep-copy → scrub-safe),
and nothing needs unwinding on exit — extent end is just the stack frame
popping.

### 2. Distribution = the task stack, same as frames

`ActionV::With { binds: Vec<(base_id, name, Val)>, inner }` mirrors
`ActionV::InFrame`. exec.rs pushes `TF::Overrides(Rc<[(u64, u64)]>)` (base →
fresh, allocated at execution so each loop iteration gets a fresh extent);
`ambient_overrides(stack)` folds like `ambient()`. Control combinators (seq,
par, loop, race, states, until) need no changes — they run inside the same
task or fork children that inherit the stack. `CallPattern` and `defn` calls
execute within the task, so overrides reach callees automatically — this is
the let-cannot-substitute payoff, and it is the same way ambient frames
already reach callee-produced actions at execution time. A future `:sealed`
adapter is "push an empty-barrier frame"; nothing else blocks.

Override values evaluate at `with`-form evaluation (action construction),
matching in-frame's frame-spec evaluation and the spawn-arg snap default.

### 3. Spawn capture: per-row override map, not node rewriting

`ActionV::Spawn` under a non-empty ambient map stores it on the spawned rows:
`Option<Rc<FxHashMap<u64, u64>>>` next to the motion schema (one word per
row, None in the common case). Signal-time reads apply it:
`DynNode::LiveStream { id }` and lowered `ChanRef::Stream(id)` look up
through the row's map before hitting `stream_val`. `MotionEvalCtx` carries
`overrides: Option<&FxHashMap<u64, u64>>` (default None; entity-bound
construction sites fetch from the row).

Rejected: rewriting the captured dyn tree (substituting `LiveStream{X}` →
`{X′}` at spawn). Zero read-time cost, but node identity is semantic —
scan-state keys and §5 shared instances key on `Rc::as_ptr`, so cloning
ancestor nodes silently unshares scan state across spawns referencing the
same def'd node. Not worth a correctness cliff for a hash lookup that only
happens on rows that used `with`.

`DynNode` is untouched — the ≤96-byte guard is safe.

### 4. Reads, writes, and who resolves through the map

Everything that turns a stream id into a value or a write target resolves
through the ambient map (control layer) or the row map (signal time):

- bare `$x` snap reads (interp mod.rs stream-read sites) — via ctx;
- `(live $x)` — the *node* keeps the base id; resolution happens at read
  time, so a def'd dyn built outside the extent still sees the override when
  spawned inside it (construction-time substitution would leak exactly here);
- `(set! $x v)` inside the extent writes X′ — the write is scoped with the
  binding, the outer stream never sees it, and the write stays a
  frame-stamped deterministic store write;
- `bind!` / `export!` inside an extent resolve through the map too
  (consistency over cleverness) — the override cell is a real store cell, so
  `bind!` attaches its producer there and the override value is just the
  initializer (the `def`-then-`bind!` lifecycle on a scoped cell). This adds
  nothing `let` + aliasing can't express; what matters is what it rules out:
  attaching to the *base* stream, where rebinding replaces the global
  producer in place and the mutation would outlive the extent — the exact
  cross-subtree spookiness `with` exists to prevent.

### 5. §13.8 residuals, settled

- **Nesting/shadowing**: innermost wins. The map is keyed by *base* id
  (lexical resolution always yields the base — an inner `with` on `$x`
  produces another `X → X″` entry), so the fold from outer to inner frames
  makes shadowing a plain map overwrite. No transitive chains exist by
  construction.
- **Which channels are overridable**: all of them, uniformly — injected,
  derived, and local streams are all just ids. The override never touches the
  base stream, its producer, or the replay tape; overriding `$player` pins
  what the *subtree* reads, while the world's `$player` keeps refreshing.
  No allowlist, no special cases.
- **Signal-valued override values**: snap by default, like every spawn-arg.
  The escape hatch is a stream-handle value — `(with {$rank $other} body)`
  aliases: reads deref the handle to the source's current value (precedent:
  bind! producer mirroring). `(with {$rank (live $other)} ...)` is NOT a
  supported form for now; `live` values are dyn-typed, and blessing them here
  would smuggle a second aliasing spelling.
- **`with` is a binder, not a producer form** — it does not accept
  bind!-style per-tick expressions. The three tiers compose from existing
  machinery: constant for the extent (snap), tracking (handle alias — the
  analogue of in-frame's signal-valued `FrameSpec::Node`), and computed per
  tick via `bind!` — either on a local stream that the map then aliases, or
  directly on the override cell (decision 4). Per-tick refresh semantics
  (attachment order, `nothing` fallback) stay in one place; an ergonomic
  `with*` over `with` + `bind!` is lib-macro territory.

### 6. Load-time schema pass

`with` binding keys are ordinary in-scope stream references: a free `$name`
in the map is the existing load error. The schema pass (interp/schema.rs)
learns the form's shape (map in binding position, body is action code) —
same treatment `let` sigil bindings already get, except `with` *references*
rather than constructs, so no declaration is added to scope.

## Risks / Trade-offs

- [Per-read hash lookup on override-carrying rows] → only rows spawned under
  `with` carry a map; the common case stays a None check. ~10k-bullet scale
  is unaffected unless a card blankets everything in `with`, which is the
  card author's explicit choice.
- [Lowered tiers must apply the row map at `ChanRef::Stream` reads] → the
  oracle (MAKU_LOWER_ORACLE=1) catches any interp/lowered divergence; this
  change adds card-level tests that exercise `with` + lowered live reads.
- [Aliasing via stream-handle values adds a deref at read] → one branch on a
  Val discriminant, only on override cells holding handles.
- [bind!/export! inside extents are near-untested territory] → covered by a
  unit test each; semantics are the boring compositional ones (they act on
  the override cell).
- [A producer bind!ed to an override cell keeps refreshing after the extent's
  last reader dies; a loop re-executing the `with` attaches a fresh producer
  per iteration] → harmless per attachment (the cell goes unread) but
  unbounded under re-execution; test pins the behavior, and producer teardown
  at extent exit waits for a card that actually hits it.

## Migration Plan

Purely additive — no existing card uses `with` (it never loaded). Land core +
tests, then the language-spec Reference paragraph flips from "[decide]" to
settled at archive time.
