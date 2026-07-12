## ADDED Requirements

### Requirement: Streams are sigiled bindings
`$name` SHALL always name a stream. Binding position constructs a stream (`(def $x)` / `(def $x init)` at top level, `(let [$x init] ...)` locally); reference position reads by channel conventions: bare `$x` in control-layer or spawn-arg position snaps the current value, `(live $x)` yields the first-class tracking value, `(set! $x v)` writes, and signal-body reads are per-tick. Passing the stream itself (not its value) SHALL require a sigiled parameter or closure capture. The cell/channel split SHALL NOT exist as separate surface: `defcell` maps to a local sigiled binding, `defchannel` to `def` + `bind!` + `export!`.

#### Scenario: Local stream shared by handle passing
- **WHEN** a pattern binds `(let [$shared 30] (par (turret $shared) (turret $shared)))` and `turret` declares a sigiled parameter `$ammo`
- **THEN** both turret instances receive the same stream handle, and `(set! $ammo ...)` in one is visible to the other

#### Scenario: Unsigiled parameter snaps
- **WHEN** a stream `$x` is passed to an unsigiled pattern parameter
- **THEN** the parameter receives the snapped current value, not the stream

#### Scenario: Any stream anchors a live frame
- **WHEN** `(live $x)` is taken on a locally `let`-bound stream that was never exported
- **THEN** it yields a tracking value framing identically to a global stream (the live node holds a stream handle, not a name)

### Requirement: Free stream references are load errors
A free `$name` — neither bound in scope nor `def`'d — SHALL be a load error. Host-channel checking falls out of scoping: the channel manifest is the set of `(from-host :name)` sites in the loaded card, checked at load time against what the host provides.

#### Scenario: Unbound stream reference
- **WHEN** a card references `$wind` with no `def` or local binding in scope
- **THEN** loading the card fails with an error naming `$wind`, before any tick runs

### Requirement: Host input is a producer expression
`(from-host :name)` SHALL be a stream-valued expression naming a host input explicitly. It is usable standalone — snapped at an eval site, wrapped in `(live ...)`, or passed to a sigiled parameter — not only as a `bind!` producer; `(bind! $x (from-host :name))` is the case where the injected stream gets a local name, mirroring the host stream's per-tick value. `(from-host :name default)` supplies the stream's value until the host first provides one. Host injection SHALL NOT be special syntax beyond this form.

#### Scenario: Anonymous injected stream
- **WHEN** `(from-host :player)` is passed directly to a sigiled parameter without any `bind!`
- **THEN** the parameter receives the injected stream, and the site still counts toward the load-time host manifest

### Requirement: Producers refresh and set! falls back
`(bind! $x expr)` SHALL attach a per-tick refresh producer to the stream `$x` names in scope; it is the single producer-attachment form (no separate global-registration form). Refresh order is pinned: defs in order, then bound producers. At refresh the producer overwrites the stream unless it yields `nothing`, in which case the last `set!` stands — keyed purely on bind!ed-ness, with no host special case, so an always-writing producer (`from-host` included) effectively seals the stream. `set!` on a stream with an always-writing producer SHALL be a lint, not an error.

#### Scenario: Producer yields nothing
- **WHEN** a bound producer yields `nothing` on a tick after a `set!` wrote the stream
- **THEN** the set! value stands for that tick

#### Scenario: Always-writing producer overwrites
- **WHEN** `set!` writes a stream bound to `(from-host :player)` mid-tick
- **THEN** the written value is visible only until the next refresh, and the load reports a lint

### Requirement: Exports are explicit and collisions are load errors
`(export! $x)` SHALL publish a stream to the host/registry; `(export! $x :as $name)` publishes under a different public name. Two exports registering the same public name SHALL be a load error, not latest-wins.

#### Scenario: Two instances export the same name
- **WHEN** two instances of a pattern each run `(export! $vol)` without `:as`
- **THEN** loading fails with a collision error naming `$vol`

#### Scenario: Rename avoids the collision
- **WHEN** the instances export as `(export! $vol :as $p1-vol)` and `(export! $vol :as $p2-vol)`
- **THEN** both publish successfully under distinct names

## MODIFIED Requirements

### Requirement: Spawn arguments snap by default
Stream reads (host-injected or derived) appearing in spawn arguments SHALL be snapped (spawn-time capture); continuous tracking SHALL require explicit `(live ...)`. Streams have their own `$name` namespace, single-writer, recorded on the replay tape (derived streams exactly like injected ones); a card's host-channel manifest is the set of its `(from-host ...)` sites.

#### Scenario: Aimed ring
- **WHEN** a spawn argument reads `$player` without `live`
- **THEN** the value is captured once at spawn and the bullets do not track the player afterward

### Requirement: Field writes queue to the tick boundary
`(change-col h :field f)` SHALL queue a functional update applied at the next tick boundary; all reads within a tick see pre-tick state, and a slot's queued updates compose in action-execution order over the pre-tick value. `remat` follows the same boundary rule, is per-slot, and restarts only the target slot's epoch. Update functions SHALL be pure (defs only — no streams or world reads).

#### Scenario: Concurrent increments
- **WHEN** two rules queue `(change-col h :hp (fn [x] (- x 1)))` in the same tick
- **THEN** both compose and hp drops by 2, with no lost write
