# Determinism — delta for rng-spawn-order-independence

## REMOVED Requirements

### Requirement: RNG draws consume one sequential stream in defined order
**Reason**: Replaced by keyed counter-based draws — the sequential stream is exactly what
made spawn order part of the replay contract.
**Migration**: The draw-order contract is superseded by "RNG draws are keyed by local
causal context" and "Capture vectors agree on site numbering" below; the capture
mechanism survives with a weaker obligation.

## ADDED Requirements

### Requirement: RNG draws are keyed by local causal context
A random draw MUST be a stateless mix of the run seed, the drawing context's key, and a
local counter — never a read of shared sequential state. Context keys MUST derive
hierarchically: root tasks from the seed and their creation ordinal, forked tasks from
the parent task's key and the parent's local fork count, tick rules from the rule index,
and every per-tick scope from its context key mixed with the tick. A draw's value MUST be
independent of how many draws other contexts performed.

#### Scenario: Unrelated draws do not shift a task's stream
- **WHEN** two runs differ only in extra `(rand)` draws performed by a different task (or
  in the relative order of two spawns issued by different tasks)
- **THEN** the unmodified task's entities render identically in both runs

#### Scenario: Same seed still replays exactly
- **WHEN** two simulations boot the same card with the same seed and identical inputs
- **THEN** all draws, and therefore all render frames, are identical at every tick

### Requirement: Entities carry rng keys and capture vectors agree on site numbering
Each spawned element MUST receive a persistent rng key derived from the spawning scope
and its element ordinal, stored in the entity store and cloned with snapshots. Capture
vectors MUST draw site k from the element key, the capture domain, and k; the compiled
extraction (`draw_caps`) and the interpreted substitution walk (`subst_rand`) MUST assign
identical site numbers to identical sites. Agreement on temporal draw order is no longer
required.

#### Scenario: Bail path matches compiled captures
- **WHEN** extraction bails and spawn falls back to form substitution
- **THEN** the substituted constants equal the values the capture vector would have
  carried for the same sites, because both derive from the same element key and site
  numbers

### Requirement: Rowful scratch evaluation contexts draw keyed randomness
Evaluation contexts that run against a scratch world while holding an entity row (evolve
step and init application, pending-field functions, masked-update values,
collider-projector bodies) MUST base their draws on the entity's rng key, a domain tag,
and the tick — making uncaptured rand in these contexts per-entity, per-tick, and
scrub-safe. Rowless scratch contexts (direct signal evaluation, `FnPose`) keep a fixed
base: uncaptured rand there is a deterministic constant by design.

#### Scenario: Random-walk evolve actually walks
- **WHEN** an evolve step body draws `(rand -1 1)` each tick
- **THEN** the drawn value differs across ticks and across entities, and rewinding then
  re-stepping to the same tick reproduces the identical value
