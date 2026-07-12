# Tasks

## 1. Core mechanism

- [ ] 1.1 `(with {$chan v ...} body...)` surface: parse to `ActionV::With { binds, inner }` with override values evaluated at form evaluation (snap); free `$name` in the map stays a load error via the schema pass (interp/schema.rs learns the form's shape; `with` references, it does not declare).
- [ ] 1.2 exec.rs extent: `TF::Overrides` pushed at execution with fresh override cells allocated from the world counter, `ambient_overrides()` fold (base-id-keyed, innermost wins), ctx carries the composed map during body/callee evaluation. Forks inherit the stack like frames.
- [ ] 1.3 Control-layer resolution through the map: bare `$x` snap reads, `(set! $x v)` (writes the override cell), `bind!`/`export!` (act on the override cell), stream-handle override values deref at read.

## 2. Spawn capture and signal reads

- [ ] 2.1 Spawns under a non-empty ambient map store `Option<Rc<FxHashMap<u64, u64>>>` on their rows (one word, None default; DynNode untouched — 96-byte guard holds); `MotionEvalCtx` gains `overrides` and `DynNode::LiveStream`/`DynNode::Live` reads resolve through it.
- [ ] 2.2 Lowered-tier reads (`ChanRef::Stream`) resolve through the row map identically — interp and lowered agree bit-exactly under the oracle.

## 3. Tests and gates

- [ ] 3.1 Unit tests: extent scoping (callee resolution, set! isolation, nesting/shadowing, extent exit without restore), spawn capture outliving the body, def'd-dyn-spawned-inside-extent (read-time resolution, not construction-time), stream-handle aliasing, bind!/export! inside an extent, override of an injected channel with the base still refreshing.
- [ ] 3.2 Card-level test exercising `with` + lowered live reads under MAKU_LOWER_ORACLE=1; full gate: `cargo test --release --manifest-path proto/core/Cargo.toml` plus the 4 ignored oracle card suites. Commit each coherent change-set.

## 4. Spec sync (archive time)

- [ ] 4.1 Update `openspec/specs/language/spec.md`: §3 scoped-overrides paragraph loses its residual [decide], §13.8 marked resolved (nesting = innermost wins; all channels overridable; values snap with stream-handle aliasing), §13.12 note stays open only for `:sealed`. Also rewrite the stale let-vs-with clause in §3 ("`let` binds bare symbols only" predates channel-unification and contradicts the local-sigiled-binding form): post-merge, `(let [$x ...])` shadowing an existing name is legal and means a fresh private stream reaching only text it contains; `with` alone rebinds the existing stream's resolution for code it causes (callees, pre-built dyns).
