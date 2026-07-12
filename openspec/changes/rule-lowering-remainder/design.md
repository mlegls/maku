# rule-lowering-remainder — Design

## Context

Milestone C's renderer half is done: `deftick` bodies are macro-expanded once
at registration, `interp/rulelower.rs` recognizes the point/dot render-rule
expansion shape, and `Sim::run_compiled_tick_form` executes it with numeric
row predicates (`RowPredicate`/`RowTest::NumCmp`). What remains interpreted
on the round-21 sample (~8% of step, attributed to `evaluate_list_inner`) is
the residue of the standing-rule surface:

- **Cull rules** — `(map (fn [e] (cull e)) (entities-where pred))` (beam
  end-of-life, enemy hp≤0 in `cards/lib/touhou.maku`). Their predicates
  already pass `row_predicate` (the `default`/`col-or` calls are inlined to
  `%value-or` by the round-7 load-time rewrite), but the form as a whole has
  no compiled path, so every tick re-evaluates the `map`/`fn`/`entities-where`
  scaffolding and builds a per-match `ActionV::Cull` through the interpreter.
- **The beam render rule** — polyline output (`:shape (curve-samples …)`),
  an if-chain on `laser-hot-fraction`, and a two-emission `seq` arm. Fully
  outside the recognized emission grammar.
- **Mixed predicates** — a conjunction with one recognized conjunct and one
  unrecognized (e.g. a user-fn call) bails the whole query to per-row
  interpreted fn evaluation today.
- **Recognizer gaps** — `and`/`or` macro output is an if-chain
  (`(if a b false)` shapes), which `row_predicate` does not recognize; only
  the `(* …)` product-conjunction shape compiles.
- **Rewrite gaps** (`interp/rewrite.rs`) — (b) macro-expansion output is
  never rewritten, so `%value-or`/trivial-def shapes inside macro-generated
  forms keep interpreted cost; (c) a purity edge where a pure higher-order
  builtin applying an impure user fn by name is misclassified pure.
- **Sym-column reads** clone `Rc<str>` per row in compiled tick passes
  (suspected, profile-gated).

Standing constraints: bit-exact tier equivalence (oracle-gated, dual-run
compare), all-or-nothing recognition per form (any deviation bails whole),
DynNode ≤ 96 bytes, no name-based recognition (expansion shapes only).

## Goals / Non-Goals

**Goals:**

- Compile the cull-rule shape end-to-end (predicate scan + action apply)
  with oracle coverage.
- Recognize `and` if-chain expansion output in `row_predicate` so
  `and`-predicates compile like `*`-conjunctions.
- Partial prefiltering: a conjunction whose prefix compiles and whose tail
  does not runs the compiled prefix as a scan filter and the interpreted
  residual only on survivors — where that is exactly semantics-preserving.
- Rewrite follow-up (b): run the load-time rewrite over `deftick`'s
  macro-expanded output at registration so expansion shapes inside
  macro-generated rule bodies compile.
- Measure; only then decide the per-batch symbol-id table and any beam-rule
  work.

**Non-Goals:**

- Full lowering of the beam render body (curve-samples deferred geometry,
  multi-binding `let`, if-chains over lib-fn results). It stays interpreted;
  this design only removes the scaffolding cost around it. A polyline
  emission grammar is future work, re-scoped once the rest lands and the
  profile is re-read.
- `or`-disjunction compilation. `or` if-chains are recognized only to bail
  cleanly (documented), not compiled — no disjunction support in `RowTest`
  this round. (Recognize-and-compile is a cheap extension later if a card
  ever profiles hot on one.)
- Rewrite purity edge (c): deferred until it bites, per the proposal.
- Projector-body lowering, rayon batches (jit-native-codegen change).

## Decisions

### D1. Cull rules compile to a second `CompiledTickForm` action kind

`lower_tick_form` currently hard-requires an `(emit :render {…})` body.
Generalize the compiled artifact to

```rust
struct CompiledTickForm { predicate: RowPredicate, action: CompiledTickAction }
enum CompiledTickAction {
    Render { needs_pose, fields, schema },   // today's payload
    Cull,                                    // body is exactly (cull e)
}
```

Recognition for `Cull`: `map` fn body is exactly `(cull entity-param)` with
`cull` unshadowed. Execution: run the shared predicate scan (same
resolve-once, bail-on-fallible machinery), then for each matched row apply
the cull through the same code path `exec_tick_value` uses for
`ActionV::Cull` (factor that application into a helper both call). Reusing
the action-apply code, in match order (= row index order, the interpreter's
`entities-where` order), pins semantics; no new effect logic exists in the
compiled path.

*Why not* build a `Val::Array` of actions and feed `exec_tick_value`
unchanged: allocates the interpreted representation we are trying to skip;
the helper split gets identical behavior without it.

Oracle: culls mutate the world, so the render-path dual-run (run compiled,
run interp, compare rows) doesn't transfer. Under `MAKU_LOWER_ORACLE=1` the
compiled scan computes its match set but does NOT apply; the interpreted
form runs instead, and we assert the interpreted evaluation produced exactly
`ActionV::Cull { target }` per predicted row in predicted order. This keeps
single-application semantics while checking both the match set and the
action shape.

### D2. `and` if-chains fold into the existing conjunction list

Post-expansion `and` output is a nested if-chain (verify exact expansion
against the lib macro before coding; recognize the shape, not the name):
`(and a b …)` ⇒ `(if a (if b …) false)`-family. `row_predicate` learns to
flatten that chain into the same `tests: Vec<RowTest>` it builds for `(* …)`.
Short-circuit note: compiled conjuncts are pure and total by construction,
so evaluating all vs. short-circuiting is unobservable — the fold is exact.
`or` chains are recognized structurally only to return `None` (bail whole),
never mis-folded.

### D3. Partial prefiltering is prefix-only, and only over short-circuiting chains

The exactness analysis drives the scope:

- For an `and` if-chain, the interpreter short-circuits: conjuncts after the
  first false one are never evaluated. So splitting at the first
  unrecognized conjunct — compiled prefix filters, interpreted residual
  (the untouched inner if-chain, evaluated as a fn per surviving row) — is
  **exactly** semantics-preserving: rows rejected by the prefix would never
  have had their residual evaluated by the interpreter either. Same errors,
  same effects, same order.
- For a `(* …)` product conjunction, the interpreter evaluates every
  conjunct on every row; skipping the residual on prefix-rejected rows can
  hide an error (or an effect) the interpreter would have surfaced. Not
  exact — **no partial prefiltering for `*` shapes**. They keep today's
  all-or-nothing rule.

So: `PartialPredicate { prefix: RowPredicate, residual: Val /* fn */ }`,
built only from `and`-chain splits where the prefix is non-empty. Execution:
resolve prefix once, scan; per surviving row, evaluate the residual fn via
the existing interpreted per-row query path (handle arg, truthiness rule
identical to `resolve_predicate_query`'s fallback). A fallible prefix read
bails the whole query pre-scan exactly as today. Residual evaluation errors
propagate as the interpreted scan's errors — same row, same message, because
rows before it saw identical evaluation.

This lands in `resolve_predicate_query` (the shared entities-where path), so
every interpreted query benefits, not just compiled tick forms. Compiled
tick forms keep requiring a fully-recognized predicate (a compiled body with
an interpreted residual mid-scan would reorder body/effect interleaving
relative to the interpreter — out of scope).

### D4. Rewrite follow-up (b): rewrite at the expansion seam, not lazily

`sf_deftick` already macro-expands bodies once at registration. Run the
round-7 rewrite (`rewrite_form` with the trivial-def table + builtin-shadow
set derived from the card) over the expanded forms there, before
`lower_tick_form`. The card's trivial-def table must be reachable at
registration — thread it (or the rewritten-card context) into `Ctx` rather
than recomputing per `deftick`. Scope: this change covers the `deftick`
registration seam only; general lazy per-eval macro expansion elsewhere
keeps interpreted cost (unchanged from today, and the proposal only needs
the rule seam).

Shadowing correctness: the rewrite must see names bound in the enclosing
rule env as shadows (same `unshadowed` discipline `lower_tick_form` uses),
not just card-level defs.

### D5. Symbol-id table and beam work are measurement-gated

After D1–D4 land, re-profile the round-21 sample (perf-spec methodology:
wall-only interleaved A/B, macOS `sample` for attribution). Only if
`Rc<str>` clone traffic in sym-column reads still shows: introduce a
per-batch symbol-id table (intern once per pass, compare ids per row).
Only if the beam rule still dominates the remainder: write a follow-up
design for polyline emission. Neither is speculatively built.

## Risks / Trade-offs

- [Cull oracle mode diverges from real mode (compiled scan predicted, interp
  applied)] → The prediction/assertion compares both match set and action
  order; the applied path is the interpreted reference itself, so oracle
  runs can never double-cull or reorder.
- [`and` expansion shape drifts if the lib macro changes] → recognizer keys
  on the post-expansion if-chain structure, with unit tests over the actual
  macro expansion output (not a hand-written imitation), so drift fails
  tests rather than silently bailing everything to interp.
- [Partial prefiltering misjudged as applicable to `*` conjunctions later] →
  the exactness argument (short-circuit only) is recorded here and in the
  lowering spec delta as a requirement, not a comment.
- [Rewriting deftick expansion output changes evaluation of shapes that were
  previously left alone] → the rewrite is the same load-time pass already
  trusted for card forms; oracle suites plus the determinism test gate it.
  Biggest hazard is shadowing (a rule-env binding named `default`/`if`);
  covered by reusing the bound-set discipline and adding shadow tests.
- [DynNode size guard] → none of the new data lives in `DynNode`;
  `CompiledTickAction` lives in `StandingRule`'s compiled slots (per-rule,
  not per-node). No impact, but the pinned size test still gates.
- [Perf regression from partial prefiltering on queries where the residual
  matches ~everything] → the prefix scan is strictly cheaper than the fn
  eval it precedes; worst case approaches today's cost, never exceeds it
  (one extra resolved-test pass per row).

## Open Questions

- None blocking. The beam-rule follow-up and sym-id table are deliberately
  deferred to post-measurement (D5).
