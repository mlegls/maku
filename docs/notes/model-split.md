# model/ split — moving dyn (and friends) to the semantic model

Status: DIRECTION SETTLED, sequencing recorded here; execution deferred until
the evolve re-expression lands (see "Sequencing"). Companion to the module
doc in `proto/core/src/model/mod.rs`: model/ is the semantic representation
at a level that doesn't depend on backend — an interpreter vs compiler, CPU
vs GPU — and the backend converts it to runtime-optimized forms, including
choice of data layout.

## The criterion

A type belongs in model/ iff a second backend would need it UNCHANGED.
Everything that encodes a choice of evaluation strategy or data layout stays
on the backend side. `Figure<E>`/`CurveEval<E>` already demonstrate the
pattern: the semantic shape is parametric over the expression representation
`E`; the frontend/backend picks `E` (Form+Env for the interpreter,
NumProgram for the compiled path, a kernel handle for a GPU).

## Where dyn stands semantically (assessed 2026-07)

The target is NOT "everything is `t -> T`". The settled semantics
(evolve-design.md, SCANNED) are a trichotomy, and the model type should
encode it explicitly:

- **closed** — pure `t -> T`, samplable at any tau (Const/Linear/ClosedPt/
  RotExpr/Frame/Translate/Clamp/Path over closed children);
- **evolve** — a fold over the tick clock; pure in tau only relative to an
  epoch; live evolves error on off-clock sampling by design;
- **live** — a function of the host channel stream, not of t at all.

Today the runtime has three specialized encodings instead of one parametric
dyn: `interp::DynNode` is hardwired to poses (the Frame/Translate/Clamp
algebra is SE(2)-specific), dyn COLUMNS are a separate scalar path
(`DynNum`, `refresh_dyn_cols`), and `EvolveDyn` is the only genuinely
Val-valued one. `Vel`, `slew`/`smooth`, and `Stages` are clocked folds
wearing a sampling interface — semantically evolve shapes. `slew`/
`smooth` are re-expressed (prelude macros over sited evolves); `vel`
and `stages` remain kernel nodes pending this split and their own
round respectively (evolve-design step 3).

## The model dyn (target shape)

Cut at the POST-re-expression kernel, parametric over expression repr and
(eventually) value domain:

```rust
// model/dyn.rs — sketch, names illustrative
enum Dyn<E> {
    Const(Pose),
    Closed { a: E, b: E, polar: bool },   // pure (t, u) -> component
    Evolve { init: E, step: E },          // tick-clock fold; epoch-relative
    Live { channel: Symbol },
    Frame(Box<Dyn<E>>, Box<Dyn<E>>),
    Translate { dx: f64, dy: f64, child: Box<Dyn<E>> },
    Clamp { lo: (f64, f64), hi: (f64, f64), child: Box<Dyn<E>> },
    Path { curve: Box<Dyn<E>>, progress: E },
}
```

Notes:
- `Vel` and `Stages` are deliberately ABSENT: after the evolve
  re-expression they are surface shapes (lib macros over evolve + closed
  exprs), not kernel nodes. Moving DynNode before that lands would enshrine
  nodes we intend to delete — this is the main reason to sequence, not rush.
- Each node DECLARES its state kind (integrator `[f64; 2]`, evolve Val
  cell) as a schema the backend realizes; the model never owns storage.
- Whether `Dyn` also abstracts the value domain (pose vs scalar for dyn
  cols) is decided at execution time; at minimum the scalar-column path
  should stop being a parallel machinery and become `Dyn<E>` over a scalar
  domain.

## What must NOT move (backend artifacts currently tangled into DynNode)

- `OnceCell<NumProgram>` caches — the compiled backend's lowering artifact.
- `Env` captures inside Form leaves — the interpreter's closure repr; in
  model terms these become declared input slots (compiled-dyn milestone B's
  capture vector is the same concept from the other end).
- Pointer-identity `node_ids` / `MotionStateKey` — interpreter state
  addressing. The model side is only "this node has a state cell of kind K".
- `FnPose(Val)` — interpreter value repr; model-side it's an `E` adapter.
- `NumProgram` itself — one backend's IR, not semantics.
- `Form`/`Env`/`Val` overall: Form is frontend syntax, Val the interpreter's
  value repr. model/ sits between them — post-elaboration, pre-layout.

## What else cuts along the same seam

Already in model/ (the 2026-07 commits): Pose/Curve/Figure, collider shapes
+ parametric collider projectors, renderer projector definitions, flat
primitive entity meta.

Still to split when dyn moves (fold into the same move — same seam):
- **Entity spec** (`interp/specs.rs`): what an entity IS (fields, motion
  slot, render/collider bindings) vs how the SoA lays it out. The
  EntitySpecStore dedup TODO keys naturally off the model type, and
  compiled-dyn milestone B needs that dedup anyway — these converge.
- **Motion state schema, semantic half**: "this figure carries these state
  cells of these kinds" is model; slot indices, snapshots,
  `shared_node_ids` are backend.
- **Clock/epoch contract**: tick rate, epoch-local tau, `motion_birth`
  semantics — currently implicit across world.rs/motion.rs; any compiler
  backend needs the same contract.
- **Channel contract**: the names/types the host injects are semantic;
  `sim/channels.rs` runtime stays.

## Sequencing

1. Land live-evolve milestones 1–3 (engine-clock advance, live evolves).
2. Evolve re-expression: `vel`/`slew`/`smooth`/`stages`/`pather` as lib
   shapes over evolve + closed exprs (evolve-design step 3). The kernel dyn
   shrinks to the model shape above.
3. Cut `model::dyn` as `Dyn<E>`; `interp::DynNode` becomes the
   `E = (Form, Env)` instantiation plus its caches; `NumProgram` the
   compiled backend's `E`. Fold in the entity-spec and state-schema splits.
4. Optimization work (compiled-dyn B/C, group eval, GPU-shaped layouts)
   then targets the model type, per the standing rule that optimization
   recognizes expansion SHAPES, never names.

Anti-goal: moving code for tidiness while the seams are churning. Steps 1–2
are actively rewriting exactly the state/layout boundaries the split cuts
along; every early move would land twice.
