# language

## ADDED Requirements

### Requirement: One numeric type and interned symbols
The runtime SHALL have exactly one language-level `Number` type — predicate masks, counts, indices, and enum-like words are schema uses of numbers/symbols, not separate scalar types — and interned `Symbol` atoms for keyword-like data. Predicate values SHALL be numeric masks (nonzero true, zero false) with no runtime Bool and no truthiness for non-numeric values.

#### Scenario: Predicate result
- **WHEN** `(entities-where pred)` evaluates a predicate over a row
- **THEN** the predicate returns a number, nonzero meaning matched, and `not` maps zero to 1 and nonzero to 0

### Requirement: Entities are figure, meta, and collider projectors
The semantic entity model SHALL be `Dyn<Figure> * Dyn<Meta> * [ColliderProjector<F>]`. Retained entity fields SHALL be flat primitive typed fields (numbers, symbols, handles) interned at load/reschema time; maps and lists stay source-level data, never retained meta. Poses are `(x, y, theta?)` with `theta = none` meaning unspecified facing.

#### Scenario: Unknown field
- **WHEN** an entity field is first written outside the interned field table at a typed boundary
- **THEN** it is a load/reschema error, not a per-tick allocation

### Requirement: Signals are Closed or Scanned, and contagion classifies
Every time-varying value SHALL be a signal with exactly two constructors: `Closed` (pure function of t — scrub/rewind-safe) and `Scanned` (state + pure step, advanced tick by tick). Composition touching a `Scanned` signal SHALL be `Scanned`; there is no `Scanned → Closed` conversion — the sanctioned exit is rematerialization (snap into fresh spawn-captured constants, swap the signal). Classification SHALL be inferred, never annotated.

#### Scenario: Contagion
- **WHEN** a closed base is composed with a scanned correction term
- **THEN** the result is Scanned, and the compiler MAY hoist only the closed part

### Requirement: Reserved axis names make dyns
`t` and `u` SHALL be reserved free axis names: an expression with free `t`/`u` denotes a dyn-valued expression, type-checking where `Dyn<T>` is expected. They SHALL NOT be bindable by `let`/`loop`; the one sanctioned binder is an explicit `fn` parameter, inside whose body the bound name is an ordinary number.

#### Scenario: Named signal referenced in a slot
- **WHEN** a def'd expression with free `t` is referenced in a motion slot
- **THEN** it resolves hygienically except the axis name, which binds to the receiving slot's axis

### Requirement: Spawn arguments snap by default
Injected/derived channel reads and cell reads appearing in spawn arguments SHALL be snapped (spawn-time capture); continuous tracking SHALL require explicit `(live ...)`. Channels have their own `$name` namespace, single-writer, recorded on the replay tape (derived channels exactly like injected ones); a card's channel manifest is derivable from its `$` reads.

#### Scenario: Aimed ring
- **WHEN** a spawn argument reads `$player` without `live`
- **THEN** the value is captured once at spawn and the bullets do not track the player afterward

### Requirement: Effects live only in the action tree
Signals SHALL be pure; `Action` values are inert first-class effect descriptions executed only by the control-layer scheduler. No signal slot SHALL accept an `Action` and no primitive SHALL evaluate one inside a signal. The hot layer (signals) is loop-free with statically bounded frame cost; the control layer is Turing-complete under a per-frame fuel budget.

#### Scenario: Action in a signal slot
- **WHEN** card code places an action where a signal is expected
- **THEN** it is a type error at load, not a runtime effect

### Requirement: Frames compose as a monoid and distribute over actions
`in-frame` SHALL be pointwise SE(2) composition with `(still)` as unit; frames are applicable and ambient for their bodies at every level. Action-level `in-frame` SHALL distribute over control combinators and land on spawn dyn-roots; distribution is lexical, stopping at pattern-embedding adapters and at `fn` bodies. `(in-frame :world body)` resets the ambient composition. Translation is `+` placement — no offset constructor exists.

#### Scenario: Aim under a frame
- **WHEN** an ambient-reading form like `aim` appears under lexically enclosing frames
- **THEN** it measures against the full lexical frame composition, not the world origin

### Requirement: Arrays broadcast with cyclic zips
Functions SHALL broadcast elementwise over arrays; shorter arrays cycle within an axis (never flat across axes); `nth` is cyclic with `nth-strict` as the marked case. Spawn multiplicity SHALL be the product of array sizes along the root-to-leaf frame path; meta arrays bind to the leading axis, with nested arrays resolving structurally by depth.

#### Scenario: Palette shorter than the ring
- **WHEN** a 3-element color array decorates an 8-element ring spawn
- **THEN** colors cycle (indices mod 3) rather than erroring

### Requirement: Field writes queue to the tick boundary
`(change-col h :field f)` SHALL queue a functional update applied at the next tick boundary; all reads within a tick see pre-tick state, and a slot's queued updates compose in action-execution order over the pre-tick value. `remat` follows the same boundary rule, is per-slot, and restarts only the target slot's epoch. Update functions SHALL be pure (defs only — no channels, cells, or world reads).

#### Scenario: Concurrent increments
- **WHEN** two rules queue `(change-col h :hp (fn [x] (- x 1)))` in the same tick
- **THEN** both compose and hp drops by 2, with no lost write

### Requirement: Handles are generation-safe and row sets are ephemeral
`spawn` SHALL return generation-checked `EntityRef` handles safe across row reuse; dead handles are no-ops for cull/manip. `entities-where` SHALL return ephemeral row-index sets stable only for the producing view; cross-time identity requires handles.

#### Scenario: Stale handle
- **WHEN** a manip callback targets a handle whose row was culled and reused
- **THEN** the manip is a no-op rather than affecting the new occupant

### Requirement: The engine ships no genre defaults
Genre vocabulary (bullet/enemy/player templates, hit/graze/shot rules, hp-death rules, phase templates) SHALL be library card code (`crates/core/lib/`, compile-time embedded, imported by bare name), not engine primitives. The core surface is a semantic kernel; surface vocabulary is lib macros over it, and optimization SHALL recognize macro expansion shapes, never names.

#### Scenario: Hand-written expansion
- **WHEN** card code hand-writes the exact form a lib macro would expand to
- **THEN** it evaluates and optimizes identically to the macro call
