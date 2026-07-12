# rule-lowering-remainder — Tasks

Gate for every task: core suite green; lowering-touching tasks also pass the
4 ignored oracle card suites (`cargo test --release -- --ignored`,
`MAKU_LOWER_ORACLE=1`). Commit each task as its own change-set.

## 1. Recognizer: short-circuit conjunction chains

- [x] 1.1 Capture the actual post-expansion shape of the lib's short-circuit
      conjunction (`and`) and disjunction (`or`) macros in unit tests
      (expand the real macros; don't hand-write the expected forms).
- [x] 1.2 Extend `row_predicate` to fold the `and` if-chain shape into the
      existing conjunct list; recognize the `or` chain shape only to return
      `None`. Shadow-check every structural head. Unit tests: folded chain
      equals the equivalent `*` form's tests; shadowed heads bail; `or`
      bails.

## 2. Registration-time rewrite of deftick expansion output (rewrite b)

- [ ] 2.1 Make the card's trivial-def table (and builtin-shadow set)
      reachable at rule registration (thread through `Ctx` or the card
      context) instead of existing only inside `rewrite_card`.
- [ ] 2.2 In `sf_deftick`, run `rewrite_form` over the macro-expanded body
      with the enclosing env's bindings treated as shadows, before
      `lower_tick_form`. Tests: a macro-generated body with a
      default-column read compiles; a body-local shadow of a rewrite head
      suppresses the rewrite; oracle suites green.

## 3. Compiled cull rules

- [ ] 3.1 Factor the `ActionV::Cull` application out of `exec_tick_value`
      into a helper callable per row.
- [ ] 3.2 Generalize `CompiledTickForm` to
      `{ predicate, action: Render{…} | Cull }`; `lower_tick_form`
      recognizes the `(map (fn [e] (cull e)) (entities-where …))` shape
      (exact body, unshadowed `cull`).
- [ ] 3.3 Execute compiled cull: shared predicate scan (resolve-once,
      fallible-read bail → whole-form interp rerun), then the cull helper
      per matched row in row order.
- [ ] 3.4 Oracle mode for effectful compiled forms: compiled scan predicts
      matches without applying; interpreted form runs as the sole applier;
      assert predicted rows == produced `Cull` actions (set and order).
      Tests: hp-cull and beam-eol shapes from the touhou lib compile and
      dual-run clean; deviant bodies bail; ignored card suites green.

## 4. Partial prefiltering for mixed short-circuit predicates

- [ ] 4.1 In `resolve_predicate_query`, split a recognized-prefix /
      unrecognized-tail `and`-chain into compiled prefix + interpreted
      residual fn; scan with the prefix, evaluate the residual only on
      survivors (same truthiness and error propagation as the interpreted
      fallback). `*` shapes keep all-or-nothing.
- [ ] 4.2 Tests: match parity vs fully-interpreted on mixed predicates;
      residual error surfaces on the same row with the same message;
      effectful/erroring residual on a prefix-rejected row is provably
      never evaluated interpreted either (short-circuit test); compiled
      tick forms still require fully-recognized predicates.

## 5. Measure and close

- [ ] 5.1 Re-profile the round-21 sample per the perf spec (interleaved
      wall A/B vs pre-change baseline; macOS `sample` attribution). Record
      the delta and the new `evaluate_list_inner` share in this change's
      design.md.
- [ ] 5.2 Decide from the profile: per-batch symbol-id table (only if
      `Rc<str>` clone traffic in sym-column reads shows) — implement or
      record as not-warranted; beam polyline lowering — leave to a
      follow-up proposal if it still dominates, noting the measured share.
- [ ] 5.3 Update the lowering capability spec's milestone-C prose to match
      what landed (delta spec syncs the requirements at archive).
