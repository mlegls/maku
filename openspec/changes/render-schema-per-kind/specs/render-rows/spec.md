## MODIFIED Requirements

### Requirement: Render schemas accrete one kind per key
The render field schema MUST scope per render kind: `kind → (key → field kind)`, one field kind per key within a render kind. Rows and batches MUST carry their render kind as a distinguished slot (not a keyed field); rows without an explicit kind belong to `:default`. Undeclared kinds accrete at runtime as keys appear; declared kinds (see manifest negotiation) have their table fixed at load, and a new key against a declared kind is a schema error. Batch fills MUST validate keys against the kind's table plus a local pending set and commit registrations only when the whole pass succeeds; any error or kind surprise aborts the batch (world untouched) and re-runs the rule row-at-a-time, reproducing the interpreted error, error site, and partial-row state exactly.

#### Scenario: Staged registration abort
- **WHEN** a batch pass encounters a field whose kind contradicts an earlier row's kind within the same render kind
- **THEN** no staged registration is committed and the interpreted re-run raises the identical schema error

#### Scenario: Same key across kinds
- **WHEN** one card's `:beam` rows carry a numeric `:width` and an imported card's `:banner` rows carry a sym `:width`
- **THEN** both register without conflict — the key is scoped per kind

## ADDED Requirements

### Requirement: Declared render kinds negotiate at load
`(defrender-kind :name {:geometry g :fields {…}})` SHALL declare a render kind — geometry class (point or polyline), field table, and identity — collected by the one load-time pass. The card's render manifest is its declared kinds plus the kinds its standing rules statically emit. A host MAY provide its supported kind set at load; a declared kind the host does not support SHALL fail the load naming the kind and the declaring card, before any tick runs. Kinds emitted without declaration are outside the manifest: a load lint under a host manifest, never an error. A declared kind's schema identity SHALL be stable from load, so hosts key precomputed layouts on it without a settling period.

#### Scenario: Unsupported declared kind
- **WHEN** a card declaring `:sprite` loads on a host whose manifest lacks `:sprite`
- **THEN** the load fails naming `:sprite` and the declaring card, before tick 0

#### Scenario: Undeclared kind under a strict host
- **WHEN** a card emits rows of an undeclared kind on a host that provided a manifest
- **THEN** the card loads with a lint naming the kind, and the rows flow with accretion semantics

### Requirement: Render adaptation rewrites at registration
A builtin rename/pick adapter SHALL wrap rules whose emissions use a foreign kind or field convention: the emitted kind and field keys rewrite at rule registration, the adapted rule registers against the target kind's schema, and fields absent from the adapter's map are dropped. Downstream consumers (schema store, batches, hosts, the oracle) SHALL observe only the post-adapter kind and keys; the remap folds into the rule's schema with no per-row cost on the compiled path.

#### Scenario: Imported card adapted to a local pack
- **WHEN** an imported rule emitting `:their-sprite` rows with `:col` is wrapped to target `:sprite` with `{:col :color}`
- **THEN** its rows register and render as `:sprite` rows carrying `:color`, indistinguishable from natively emitted ones
