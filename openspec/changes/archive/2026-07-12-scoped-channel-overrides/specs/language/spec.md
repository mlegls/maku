## ADDED Requirements

### Requirement: Scoped channel overrides are dynamic bindings over the action tree
`(with {$chan v ...} body...)` SHALL be an action combinator that dynamically binds each named stream for the extent of the body's execution: reads and writes of an overridden stream inside the extent SHALL resolve to a fresh per-extent cell initialized to the override value, while the base stream, its producer, and the replay tape stay untouched. The extent SHALL follow the in-frame distribution law realized at execution: it pushes through control combinators, reaches pattern and `defn` callees invoked within it, and lands on spawns, which capture the active overrides for their signals' lifetimes — `(live $chan)` reads included, long after the body's evaluation. Any stream SHALL be overridable — injected, derived, and local streams uniformly, with no allowlist. Binding keys are ordinary in-scope stream references: a free `$name` in the override map SHALL be a load error. Override values SHALL evaluate once, at `with`-form evaluation (snap); a stream-handle value SHALL alias, with reads dereferencing to the source stream's current value.

#### Scenario: Spawned signal outlives the body
- **WHEN** a spawn under `(with {$rank 0.5} ...)` captures a dyn reading `(live $rank)`, and the body completes while the bullet flies on
- **THEN** that bullet's signal keeps reading 0.5 for its lifetime, while concurrently spawned bullets outside the extent read the world's `$rank`

#### Scenario: Callee resolves through the override
- **WHEN** a pattern invoked inside `(with {$rank 0.5} ...)` reads `$rank`, resolved in its own definition scope
- **THEN** it reads 0.5 — the extent is dynamic, reaching code the body causes, not just text it contains

#### Scenario: set! inside the extent is scoped
- **WHEN** the body of `(with {$x 1} ...)` executes `(set! $x 2)`
- **THEN** reads inside the extent see 2, and the base stream outside the extent never sees either value

#### Scenario: Nested overrides shadow innermost-first
- **WHEN** `(with {$x 1} ... (with {$x 2} body) ...)` executes
- **THEN** `body`'s extent reads 2, the outer extent reads 1 elsewhere, and exiting an extent is only a scope pop — no restore write occurs

#### Scenario: Overriding an injected channel leaves the tape honest
- **WHEN** a subtree runs under `(with {$player p} ...)` while the host keeps injecting `$player`
- **THEN** the subtree reads the pinned pose, the world's `$player` keeps refreshing from the host, and the recorded tape is identical to a run without the override reads
