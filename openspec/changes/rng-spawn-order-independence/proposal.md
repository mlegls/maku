# RNG spawn-order independence

Backlog stub — design/specs/tasks are generated when this is picked up.

## Why

RNG is sequential splitmix, so replay determinism holds but spawn-order independence does not: reordering spawns changes every subsequent draw.

## What Changes

Settled at decision level 2026-08-10 (implementation round still to be picked up). Governing invariant: **a draw may depend on the drawing entity's own local causal history, never on global order.** Local program order is deterministic and parallel-safe; global order is what this change removes.

- **Hierarchical entity keys** — flat keying can't discriminate two instances of the same pattern without the parent's identity, which is recursively the same question, so hierarchy is forced: `entity_key = mix(parent_key, site_ordinal, invocation, element_index)`, rooted at the world seed. The invocation discriminator for repeat-firing sites is spawn tick plus a per-(parent, site, tick) counter — order-dependent only on the parent's own program order, which the invariant allows. Storage: one u64 column on the entity store; DynNode layout untouched.
- **Site keys reuse the extraction walk ordinal** — the ordinal `subst_rand`/`draw_caps` already agree on changes role from temporal draw position to the counter `k`; capture vectors become `caps[k] = rand(entity_key, CAPS, k)`, computable in any order. The round-22 contract shrinks from "same walk order at the same time" to "same site numbering", which shared extraction already guarantees and the oracle already checks. Explicit non-goal: draw stability across card edits (edits may renumber sites; tapes are tied to a card version).
- **Live `(rand)` becomes pure per site** — rules/actions key off (entity_key, tick, local draw ordinal). Rand inside a signal keys off (entity_key, site, τ) — a semantic change: re-evaluation yields the same value, so evaluation count (a caching/lowering implementation detail) stops leaking into semantics, and rand-in-signals becomes scrub-safe and tier-stable. This eliminates a class of oracle divergence rather than adding contract to protect it.
- **Algorithm is behind the contract, not a decision** — spec says "stateless mix of (seed, derived key, k)"; implement with a splitmix64-style finalizer over the chained key. 64-bit birthday collisions are irrelevant at danmaku scale; Philox buys nothing here, and a later swap is the same one-time break class as this change.
- **Host-facing seed-reset API**: the world exposes an explicit "reset RNG seed" call, and construction seeds by implicitly calling it (replacing the hardcoded constant in `World::new`). The seed is the root all path-derived keys hash from, so a reset re-roots future draws without touching already-captured vectors; the active seed is part of run identity and must land on the replay tape. Mid-run reset is callable anytime but replay fidelity is only promised for tape-recorded calls — making init-only vs mid-run a tape question, not an RNG question.
- **Smooth noise**: rider requirement only — noise gradients/hashes derive from the same `rand(seed, path, k)` primitive with cell coordinates as the path. No noise functions land in this round.

## Capabilities

To be finalized at pick-up.

## Impact

- RNG stream contract; the round-22 capture-vector draw order (`draw_caps` mirrors `subst_rand`'s walk) is part of the same contract and would need re-deriving.
- Also a prerequisite for parallelizing compiled entity hot loops (parallel entity order changes RNG unless entities are independently seeded) — relevant to the JIT tier's data-parallelism (`jit-native-codegen`).
- Related decision: smooth noise should be a pure deterministic function of coords+seed, not sequential RNG state (`openspec/specs/language/spec.md`).
- One-time break: every rand-using card's output changes. Old-vs-new A/B parity (abdump worktree method) applies only to rand-free cards; the oracle (lowered-vs-interpreted within the new engine) carries verification for the rest.
- Sequencing: land before `entity-representation-flip`, so the flip doesn't bake more capture vectors into the sequential contract.
