# render-rows Specification

## Purpose
The tick's render output as an ordered frame of rows and column
batches: ordering, row-expansion equivalence, schema accretion,
absence. Rationale: `docs/notes/render-output-design.md`.

## Requirements
### Requirement: The tick's render output is an ordered frame
The render output of a tick MUST be an ordered frame whose draw order is emission order: standing-rule registration order, and within one rule's pass, entity row order. Batching MUST NOT change the observable order — a batch occupies one position in the stream and expanding it in place reproduces the row sequence exactly.
*Why:* settled before JIT work so optimization has fixed semantics. Rationale: `docs/notes/render-output-design.md`.

#### Scenario: Batch-vs-row order
- **WHEN** a rule's pass is emitted as a column batch instead of rows
- **THEN** expanding the frame yields exactly the row sequence the row-at-a-time path would have produced, in the same positions

### Requirement: Row expansion is the semantic reference
`RenderRow` MUST remain the canonical row form; a batch is a layout, not a new value universe. Expansion from batch to rows MUST be total and exact, and the compat `render()` MUST be defined as frame expansion.

#### Scenario: Oracle expansion equality
- **WHEN** the oracle is enabled and a compiled pass emits a batch
- **THEN** the expanded batch compares row-equal (`==`) against the interpreted re-run's rows

### Requirement: Render schemas accrete one kind per key
The per-world render field schema MUST enforce one kind per key, accreting as new keys appear. Batch fills MUST validate keys against the world schema plus a local pending set and commit registrations only when the whole pass succeeds; any error or kind surprise aborts the batch (world untouched) and re-runs the rule row-at-a-time, reproducing the interpreted error, error site, and partial-row state exactly.
*Why:* per-kind registered schemas and manifest negotiation are future work (`render-schema-per-kind` change); this is the current contract.

#### Scenario: Staged registration abort
- **WHEN** a batch pass encounters a field whose kind contradicts an earlier row's kind
- **THEN** no staged registration is committed and the interpreted re-run raises the identical schema error

### Requirement: Absent fields stay absent
A field whose value is `nothing` for a row MUST simply not be present on that row. Columns therefore carry optional presence, and a field that is `nothing` for every matched row contributes no column at all that pass.

#### Scenario: All-nothing field
- **WHEN** a rule emits a field that evaluates to `nothing` for every matched row in a pass
- **THEN** the resulting batch has no column for that field and expanded rows lack the key

### Requirement: Frames are tick-cadence snapshots
Rule-emitted render rows MUST be snapshots at tick cadence; the engine provides no frame-time re-evaluation or interpolation. Hosts that render between ticks own any interpolation policy.
*Why:* decided trade (round 21); keeps the frame API pure transport.

#### Scenario: Host rendering between ticks
- **WHEN** a host draws at a higher frame rate than the tick rate
- **THEN** consecutive draws between ticks observe the same frame; any smoothing is host-side


## Design

Moved verbatim from docs/notes/render-output-design.md (round-21 design
+ implementation record; host API shapes and parallelism rationale).

---

# SoA render output — milestone C design (host boundary)

Status: DESIGNED AND LANDED 2026-07, round 21 (e1459aa transport,
d43a480 batch fill + hosts, 66df739 small-pass threshold). Companion to
compiled-dyn-design.md (milestone C) and types.md "Schema Checking"
(render kinds/schemas). Goal: settle the render-output semantics and host
API before JIT work — the JIT optimizes over these settled semantics, it
does not get to change them.

Implementation deltas vs the design below:
- **Direct numeric gather**: a `Field`/`FieldOr` column whose name has no
  sym-field slot provably cannot yield a keyword (`entity_field_at_slots`
  checks sym first), so it fills by direct num-column reads —
  `col_at_slot` per row plus a Num/pose-read default — skipping the
  per-row Val round-trip. Pose-component columns read the pose pass
  directly. This, not the batch transport itself, was most of the wall
  win (scaled fruit rig 3111 → ~2490 ms, −20%; fruit 900t 151.5 → 119.3).
- **Small-pass threshold**: passes under 16 matched rows keep the pooled
  row path — per-batch column allocs cost more than a few row boxes
  there (measured on lasers/reimu). Both paths are exact; the frame is a
  mixed stream by design.
- The pos_only fast pose applies whenever no lowered value reads `:th`
  (the stock touhou rule reads `:th` via the `:theta` default, so it
  takes the full pose path).
- Batch bodies are dropped, not pooled — a few column Vecs per rule per
  tick is nothing next to the per-row box churn they replace.
- Columns are f64 today; under the 2026-07 scale target (TODO.md: 10k
  normal, 100k–1M ceiling) render columns are in the f32 hot-column
  class — the natural moment to narrow is when they become GPU instance
  buffers, since hosts consume f32 anyway (web already casts per value).

## What this replaces

Today every render row is an `Rc<RenderRow>` built row-at-a-time: compiled
deftick rules evaluate each field to a `Val` per row (`eval_compiled_row_val`),
push through per-row schema checks, and the host's `render()` clones every
row's boxes out per tick. Post-round-20 profile: ~6% row eval + ~2% row
pool churn, and the shape is wrong for any column-oriented consumer (GPU
attribute buffers, wasm typed arrays).

## Semantics (settled here)


1. **The render output of a tick is an ordered frame.** Draw order is
   emission order: standing-rule registration order, and within one rule's
   pass, entity row order. This is exactly today's observable order;
   batching must not change it. Explicit layering (z/layer fields) remains
   future renderer-projector policy (types.md), not transport semantics.
2. **A frame is a sequence of items**: interpreted/legacy rows one at a
   time, and per-rule **batches** — one batch per compiled point-rule pass,
   holding that pass's rows as typed columns. A batch occupies one position
   in the stream; expanding it in place reproduces the row sequence
   exactly.
3. **Row expansion is the semantic reference.** `RenderRow` (geometry +
   keyed nums/syms) stays the canonical row form; a batch is a layout, not
   a new value universe. `expand` from batch to rows is total and exact —
   the oracle compares expanded rows against the interpreted rows with
   `==`, and the compat `render()` is defined as frame expansion.
4. **Schema checks keep interpreted semantics.** The per-world render
   field schema (one kind per key, accreting) is enforced for batch fills
   by STAGING: the batch pass validates keys against the world schema plus
   a local pending set, and commits registrations only when the whole pass
   succeeds. Any error or kind surprise aborts the batch, discards it, and
   reruns the rule through the row-at-a-time path — which reproduces the
   interpreted error, error site, and partial-row state exactly (same
   driver-level abort-and-rerun stance as compiled predicate bails and the
   JIT totality contract).
5. **Absent fields stay absent.** A field whose value is `nothing` for a
   row is simply not present on that row (today: no push). Columns
   therefore carry optional presence; a field that is `nothing` for every
   matched row contributes no column at all that pass.

## Host API

```rust
// model/renderers.rs — transport types (model-side: any backend needs
// them unchanged; layout choice is exactly what they encode, which is
// the host boundary's job)
pub struct RenderSchema {
    /// Extra (non-geometry) fields in emission order: key + kind.
    pub cols: Vec<(Rc<str>, RenderFieldKind)>,
}

pub enum NumColumn { Const(f64), Rows(Vec<f64>) }

pub enum Column {
    Num(NumColumn),
    /// Per-row nums with presence (mask[i] == value present).
    NumOpt(Vec<f64>, Vec<bool>),
    SymConst(Rc<str>),
    /// Per-row syms; None == absent on that row.
    Syms(Vec<Option<Rc<str>>>),
}

pub struct RenderBatch {
    pub schema: Rc<RenderSchema>,
    pub len: usize,
    // Point geometry as columns; Const covers literal/default slots.
    pub x: NumColumn, pub y: NumColumn, pub theta: NumColumn,
    pub scale: NumColumn, pub alpha: NumColumn, pub hue: NumColumn,
    /// Parallel to schema.cols.
    pub cols: Vec<Column>,
}

pub enum RenderItem {
    Row(Rc<RenderRow>),
    Batch(Rc<RenderBatch>),
}
```

```rust
impl Sim {
    /// The tick's render output in draw order. Batches are Rc-shared with
    /// the sim's pools; hosts read columns in place.
    pub fn render_frame(&mut self) -> Vec<RenderItem>;
    /// Compat: the frame expanded to rows (exactly today's output).
    pub fn render(&mut self) -> Vec<RenderRow>;
}
```

Schema negotiation, this milestone: a host inspects `batch.schema` and may
key precomputed layouts on `Rc::ptr_eq` — the sim memoizes the schema per
rule and rebuilds only if observed kinds change, so schema identity is
stable at steady state (it can evolve during the first ticks while
dynamic-kind fields settle). Full load-time negotiation — render-kind
manifests, hosts declaring supported kinds, load failure on unsupported
kinds — is the renderer-projector milestone (types.md); it will hand its
extraction output to THIS transport. `RenderSchema` is deliberately the
record-schema shape that manifest will use.

Both hosts keep working unchanged through `render()`; the wasm host's
`dots()` is the first consumer to read columns directly.

## Batch fill (the compiled-rule pass)

For a `CompiledRowPlan` (point-shape) rule, replace the per-row
`Rc<RenderRow>` loop with column fills over the matched-row set:

1. Pose pass: if the rule needs a pose, fill a scratch `Vec<Pose>`
   (fast_pos_pose, falling back to `entity_pose_at`) — one pose read per
   row per pass, shared by all pose-reading columns.
2. Geometry columns in coercion order (x, y, theta, scale, alpha, hue):
   literal → `Const`, missing → `Const(default)`, else fill per row.
3. Extra columns in field order: evaluate per row; the first non-nothing
   value fixes the column kind (staged against the schema); later rows
   that disagree in kind abort the batch.
4. Commit staged schema registrations, push one `RenderItem::Batch`.
5. Any error anywhere → discard the batch (world untouched: staged checks,
   no partial pushes) and rerun the rule row-at-a-time for exact
   interpreted error behavior.

Batch bodies are pooled like row boxes (`Rc::get_mut` + clear on recycle);
column `Vec`s amortize to zero allocation at steady state.

Oracle (`MAKU_LOWER_ORACLE=1`): each compiled pass expands its batch and
asserts row-exact equality against the interpreted rerun, as today.

## Parallelism — where it lives (decided, round 21)

Per-entity per-tick loops (motion lanes, collider materialization, these
column fills) are data-parallel, but parallelism is a BACKEND/DRIVER
property, not an IR marking:

- The batch call convention already makes kernels parallel-safe by
  construction — total, callback-free, per-lane reads, disjoint per-lane
  writes, rand/captures as pre-filled inputs. A per-program "parallel ok"
  bit would be constant true; there is nothing to mark.
- What the semantics must guarantee (and the design docs record) are the
  invariants that make any schedule legal AND bit-deterministic: no
  cross-lane reads, no `&mut World` exposure during a kernel run, and all
  cross-lane combining (frame item order, collision index build, channel
  accumulation) in a fixed merge order independent of thread schedule.
  Same ops per lane in the same order ⇒ bit-identical output at any
  thread count, preserving the oracle/replay story.
- Scheduling (rayon chunk size, SIMD width, single-threaded wasm) differs
  per host, so it lives in the driver loops. Wasm is the forcing case: the
  IR must stay meaningful on a host with no threads.

The frame is parallel-ready under these rules: each batch's columns can be
filled by independent workers; item order is fixed by rule registration
before any fill starts.

## Mesh renderers are hosts (decided 2026-07)

A render-to-mesh package (frame → instance buffers / tessellated
strips) is architecturally A HOST: a separate, optional consumer of the
public frame API with no privileged relationship to the engine — it
just implements the drawing more efficiently than a naive per-row host
loop would. This follows from schemas being user-defined: a mesh
renderer must be schema-AWARE (its texturing/styling rules come from
its own external API, or it defines them for the schemas it knows), so
baking one into the engine would privilege one schema. Different render
schemas get separately packaged mesh renderers (a touhou pack, a
vampire-survivors pack, …), all consuming the same `render_frame()`.
Consequences:
- the engine's obligation ends at the frame: typed columns, stable
  schema identity, row fallback — nothing texture- or genre-shaped;
- style vocabulary stays host/library policy (types.md) — a mesh pack
  ships its own style table or takes one as configuration;
- packing to f32 GPU buffers happens inside the pack, which is the
  natural narrowing point for render columns under the scale target.

Motivation snapshot: at ~2k bullets the naive per-row host draw
(~1µs/call immediate mode) already costs 5-10x the engine tick
(~200µs); at the 10k-normal scale target per-row drawing is
disqualifying while the tick is still ~1ms.

## Sequencing

1. Transport types + `render_rows: Vec<RenderItem>` migration (rows only —
   behavior-neutral).
2. Batch fill for compiled point rules + staged schema checks + oracle
   expansion + pooling.
3. wasm `dots()` reads columns.
4. Later, separate work: renderer-projector extraction targets the same
   transport; JIT render kernels drop in behind the column-fill seam
   (compiled-dyn-design.md gap 4).
