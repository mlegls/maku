//! Control-layer interpreter + prototype signal representation.
//!
//! Per language.md §2: Actions are inert data; the scheduler (sim.rs) walks
//! them with an explicit stack. Expressions evaluate instantly and purely;
//! only Action leaves interact with time or the world. Seq bodies are LAZY.
//!
//! Signals evaluate against a SigEnv (defs + injected snapshot) and never
//! touch the world — the spec's purity rule is also what breaks the borrow
//! cycle here. Scanned nodes (Vel) keep per-entity state keyed by node
//! identity.
//!
//! Two rules this prototype surfaced for the spec:
//!  - `let` in action position defers action-valued bindings to scheduler
//!    reach-time (a spawn executed at evaluation time would miss the ambient
//!    frame the distribution law owes it).
//!  - Ambient frames do not cross `fn` boundaries (manip callbacks spawn
//!    in world coordinates; lexical distribution stops at lambdas, the same
//!    way it stops at embedded patterns).

use crate::edn::Form;
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;

mod builtins;
mod card;
mod colliders;
mod coerce;
mod engine;
mod lower;
mod r#dyn;
pub mod model;
mod motion;
pub mod profile;
mod projectors;
mod rewrite;
mod rulelower;
mod sem;
mod specs;
mod spawn;
pub mod types;
mod world;

pub(crate) use builtins::*;
pub use card::*;
pub use colliders::*;
pub use coerce::*;
pub(crate) use engine::{RenderKey, RenderRowFields};
pub(crate) use lower::*;
pub use r#dyn::*;
pub use crate::model::{
    ColName, ColliderData, CurveDomain, EntityRef, FieldName, Pose, RenderData, RenderFieldKind,
    RenderRow, SampleSet, Symbol,
};
pub use motion::*;
pub(crate) use projectors::*;
pub(crate) use rewrite::*;
pub(crate) use rulelower::*;
pub use sem::*;
pub(crate) use specs::*;
pub(crate) use spawn::*;
pub use world::*;

pub type DataAtom = crate::model::DataAtom<DynPose>;

/// A seq value: shared immutable backing + a window. rest/drop/take are
/// O(1) pointer bumps (fat-pointer semantics — the compiled rep, used now);
/// a view pins its whole backing, which is fine at card scales.
#[derive(Clone, Debug)]
pub struct Seq {
    backing: Rc<[Val]>,
    start: usize,
    len: usize,
}

impl Seq {
    pub fn from_vec(v: Vec<Val>) -> Seq {
        let len = v.len();
        Seq { backing: v.into(), start: 0, len }
    }
    pub fn view(&self, start: usize, len: usize) -> Seq {
        assert!(start <= self.len);
        assert!(len <= self.len - start);
        Seq { backing: self.backing.clone(), start: self.start + start, len }
    }
    #[cfg(test)]
    pub(crate) fn backing_ptr(&self) -> *const Val {
        self.backing.as_ptr()
    }
}

impl Deref for Seq {
    type Target = [Val];

    fn deref(&self) -> &Self::Target {
        &self.backing[self.start..self.start + self.len]
    }
}


#[derive(Clone, Debug)]
pub enum FieldSeed {
    Num(f64),
    Dyn(DynNum),
    Sym(Rc<str>),
}

#[derive(Clone, Debug)]
pub struct ElemFields {
    pub figure: Val,
    pub fields: Rc<[(Rc<str>, FieldSeed)]>,
}

#[derive(Clone, Debug)]
pub enum Val {
    Num(f64),
    Kw(Rc<str>),
    Pose(Pose),
    Figure(Figure),
    ColliderProjector(Rc<[ColliderProjectorValue]>),
    CurveSamples(Rc<CurveSamples>),
    EntitySet(Rc<[usize]>),
    EntityView(EntityRef),
    CollisionSet(Rc<[(usize, usize)]>),
    Arr(Seq),
    Map(Rc<Vec<(Val, Val)>>),
    DynLike(Rc<DynLike>),
    DynPose(DynPose),
    DynFigure(DynFigure),
    CurveV(Rc<ExtCurve>),
    ElemV(Rc<ElemFields>),
    /// A form as a value — what macro code inspects and quasiquote builds.
    FormV(Rc<Form>),
    Action(Rc<ActionV>),
    Fn { params: Rc<[Form]>, body: Rc<[Form]>, env: Env },
    /// `(evolve init step)`: the kernel's stateful signal constructor.
    /// Callable like any dyn — `(d t)` replays the fold — and coercible to
    /// a pose/figure dyn when the carried state is a pose.
    Evolve(Rc<EvolveDyn>),
    Builtin(Rc<str>),
    Handle(EntityRef),
    StageExit(Rc<StageExitSlot>),
    StageExitPose { slot: Rc<StageExitSlot>, field: StageExitField },
    /// The pattern instance's cell scope (name → cell id), bound in the Env
    /// under "#cells" — it rides every captured (Form, Env) pair, so signal
    /// reads resolve the right instance's cells at tick time. Shared-map
    /// mutation across snapshots is replay-safe because ids allocate from
    /// the deterministic world counter (re-stepping converges).
    Cells(Rc<std::cell::RefCell<HashMap<String, u64>>>),
    Nothing,
}

/// The hidden Env key carrying the pattern instance's cell scope. Passed
/// through defn application and def resolution like the slot-bound t/u —
/// cells are DYNAMIC pattern-scoped ambient (§3), not lexical.
pub const CELLS_KEY: &str = "#cells";

pub(crate) fn cell_scope(env: &Env) -> Option<Rc<std::cell::RefCell<HashMap<String, u64>>>> {
    match env.lookup(CELLS_KEY) {
        Some(Val::Cells(m)) => Some(m),
        _ => None,
    }
}

pub(crate) fn fresh_cell_scope() -> Val {
    Val::Cells(Rc::new(std::cell::RefCell::new(HashMap::new())))
}

impl Val {
    pub fn arr(v: Vec<Val>) -> Val {
        Val::Arr(Seq::from_vec(v))
    }

    pub fn num(&self) -> Result<f64, String> {
        match self {
            Val::Num(n) => Ok(*n),
            v => Err(format!("expected number, got {:?}", v)),
        }
    }
}

pub(crate) fn collider_projector_value(projector: ColliderProjectorValue) -> Val {
    Val::ColliderProjector(vec![projector].into())
}

pub(crate) fn flatten_collider_projectors(
    surface: &str,
    value: Val,
    expected_figure: Option<FigureProjectorKind>,
) -> Result<Rc<[ColliderProjectorValue]>, String> {
    let mut out = Vec::new();
    match value {
        Val::Nothing => {}
        Val::ColliderProjector(projectors) => {
            out.extend(projectors.iter().cloned());
        }
        Val::Arr(items) => {
            for item in items.iter() {
                match item {
                    Val::ColliderProjector(projectors) => {
                        out.extend(projectors.iter().cloned());
                    }
                    other => {
                        return Err(format!(
                            "{}: expected collider projector or list of them, got {:?}",
                            surface,
                            other
                        ));
                    }
                }
            }
        }
        other => {
            return Err(format!(
                "{}: expected collider projector or list of them, got {:?}",
                surface,
                other
            ));
        }
    }
    let figure = expected_figure.or_else(|| out.first().map(|p| p.figure));
    if let Some(figure) = figure {
        for projector in &out {
            if projector.figure != figure {
                return Err(format!(
                    "collider projector kind mismatch: :{} vs :{}",
                    figure.name(),
                    projector.figure.name()
                ));
            }
        }
    }
    Ok(out.into())
}

/// One spawn element: a plain dyn or an extended entity, plus its §5 shape
/// path — (axis_len, index) per array level, root to leaf — for the F15
/// leading-axis/by-length meta rule.
pub struct SpawnElem {
    pub dyn_figure: DynFigure,
    pub collider_projector_spec: ColliderProjectorValue,
    pub cache_policy: EntityCachePolicy,
    pub path: Vec<(usize, usize)>,
    pub fields: Rc<[(Rc<str>, FieldSeed)]>,
}

#[derive(Clone, Debug)]
pub struct CurveSamples {
    pub entity: EntityRef,
    pub u_max: f64,
    pub resolution: f64,
}

/// One state of a `states` machine (§8): `(label body…)`.
/// The machine is a bare FSM — labeled states, default successor = next in
/// order, `goto` for everything else. End conditions are ordinary body
/// code (`(until pred …)` as the body, `(fork (seq (wait d) (goto)))` for
/// timeouts); `phases` — the boss-shaped sugar over it — is a stdlib
/// macro (lib/touhou.maku), not engine code.
#[derive(Debug, Clone)]
pub struct StateClause {
    pub label: Rc<str>,
    pub body: Rc<[Form]>,
}

/// Inert action descriptions. Bodies are unevaluated forms + env (lazy seq).
#[derive(Debug)]
pub enum ActionV {
    Seq { items: Rc<[Form]>, env: Env },
    Loop { names: Vec<Rc<str>>, inits: Vec<Val>, body: Rc<[Form]>, env: Env },
    Recur(Vec<Val>),
    InFrame { frame: FrameSpec, inner: Rc<ActionV> },
    /// Bindings whose values are actions execute at scheduler reach-time
    /// (inside the ambient frame); their results (e.g. spawn handles) bind.
    Let { binds: Vec<(Rc<str>, Val)>, body: Rc<[Form]>, env: Env },
    Spawn { entities: Vec<EntitySpec> },
    Render { row: Rc<RenderRow> },
    Manipulate { targets: Vec<EntityRef>, query: Option<Val>, callback: Val },
    Remat { target: EntityRef, spec: RematSpec },
    /// Queue a functional column update on a live entity (dead handles are no-ops at drain).
    ChangeCol { target: EntityRef, col: ColName, f: Val },
    Cull { target: EntityRef },
    /// (export cell): publish a pattern cell as a read-only channel of the
    /// same name — the pattern-level export surface (host renders it; the
    /// pattern stays the single writer).
    Export { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str> },
    /// (bind-channel! $name expr): publish an instance-scoped derived
    /// channel. Unlike top-level defchannel, expr closes over this env.
    BindChannel { name: Rc<str>, expr: Form, env: Env },
    /// Pattern invocation: args pre-evaluated in the CALLER's scope (ir
    /// values); params fill from defaults. The §10 embedding adapter:
    /// fresh_cells=true (the default — isolated defcell state per instance),
    /// false for (inline …) — the embedded pattern shares the caller's
    /// cells ("binds into the embedding pattern's scope").
    CallPattern {
        params: Vec<(Rc<str>, Form)>,
        body: Rc<[Form]>,
        args: Vec<Val>,
        caller_cells: Option<Val>,
        fresh_cells: bool,
    },
    /// Clear all hostile (team-less) fire — bomb semantics.
    CullHostile,
    /// (until pred body...): structured cancellation — run body; the tick
    /// the predicate holds, the body's whole task subtree dies. The §8
    /// phase-end scope-cancellation primitive ((race (wait-for p) body)
    /// degenerate case).
    Until { pred: Form, body: Rc<[Form]>, env: Env },
    /// (finally body cleanup...): unwind-protect for the scheduler's
    /// structured cancellation paths.
    Finally { body: Form, cleanup: Rc<[Form]>, env: Env },
    /// (race arm...): fork all arms; first completion cancels the rest.
    Race { arms: Rc<[Form]>, env: Env },
    /// The §8 state machine: ordered labeled states run as a trampoline —
    /// a state ends by goto or body completion; next = goto target,
    /// defaulting to state order; falling off the end completes the machine.
    States { clauses: Rc<[StateClause]>, env: Env },
    /// (goto label?): scoped non-local exit — cancel the enclosing state
    /// body (finalizers run), re-enter at the label; bare (goto) takes the
    /// default successor (state order). Labels are VALUES (evaluated), so
    /// routing may be computed from ordinary values, so random Markov
    /// routing is card code rather than a special case.
    /// The cell identifies the innermost lexical machine
    /// (bound as #state-cell in state bodies, so outer machines' labels
    /// are simply not in scope).
    Goto { cell: u64, label: Option<Rc<str>> },
    Wait { ticks: u64 },
    WaitFor { pred: Form, env: Env },
    DefVar { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str>, init: Val },
    SetVar { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str>, val: Val },
    Fork(Rc<ActionV>),
    Par(Vec<Rc<ActionV>>),
    Event { name: Symbol, pos: Option<(f64, f64)> },
    Nothing,
}

#[derive(Debug, Clone)]
pub enum FrameSpec {
    Const(Pose),
    /// A signal-valued frame (e.g. an unexpressed guide). Its scan state
    /// lives in whichever bullet shares the node (§5 shared instances); the
    /// scheduler resolves the pose at action time.
    Node(Rc<DynNode>),
    /// (in-frame :world body): RESET the ambient composition — patterns
    /// don't self-anchor, so the caller's anchor (e.g. the boss) is the
    /// default; player-side patterns opt out explicitly.
    World,
}

#[derive(Debug, Clone)]
pub struct EntitySpec {
    pub dyn_figure: DynFigure,
    pub cache_policy: EntityCachePolicy,
    pub sym_fields: Vec<(FieldName, Symbol)>,
    pub cols: Vec<(ColName, f64)>,
    pub dyn_cols: Rc<[(ColName, DynNum)]>,
    pub collider_projector: ColliderProjector,
}

// ---------------------------------------------------------------------------
// Environments: immutable chain, cheap to clone.

#[derive(Clone, Debug)]
pub struct Env(Option<Rc<EnvNode>>);

#[derive(Debug)]
struct EnvNode {
    name: Rc<str>,
    val: Val,
    next: Env,
}

impl Env {
    pub fn empty() -> Env {
        Env(None)
    }
    pub fn bind(&self, name: Rc<str>, val: Val) -> Env {
        Env(Some(Rc::new(EnvNode { name, val, next: self.clone() })))
    }
    pub fn lookup(&self, name: &str) -> Option<Val> {
        let mut cur = &self.0;
        while let Some(n) = cur {
            if &*n.name == name {
                return Some(n.val.clone());
            }
            cur = &n.next.0;
        }
        None
    }
}
#[derive(Clone)]
pub struct SigEnv {
    pub defs: Rc<HashMap<String, Form>>,
    /// Injected + derived channels, by bare name (read as `$name`). The host
    /// passes by name; a card's channel manifest derives from its tree.
    pub channels: Rc<HashMap<String, Val>>,
    /// Pattern-scoped control cells (F16): written by set! (control layer),
    /// read live by signals; shared between world and signal contexts.
    pub cells: Rc<std::cell::RefCell<HashMap<u64, (String, Val)>>>,
    /// Cells published as channels via (export cell): (public name, id).
    pub exports: Rc<std::cell::RefCell<Vec<(String, u64)>>>,
    /// Instance-scoped derived channels registered by (bind-channel! ...).
    pub bound_channels: Rc<std::cell::RefCell<Vec<(Rc<str>, Form, Env)>>>,
}

impl Default for SigEnv {
    fn default() -> Self {
        SigEnv {
            defs: Rc::new(HashMap::new()),
            channels: Rc::new(HashMap::new()),
            cells: Rc::new(std::cell::RefCell::new(HashMap::new())),
            exports: Rc::new(std::cell::RefCell::new(Vec::new())),
            bound_channels: Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }
}

impl SigEnv {
    pub fn channel(&self, name: &str) -> Option<Val> {
        self.channels.get(name).cloned()
    }
    pub fn channel_pos(&self, name: &str) -> (f64, f64) {
        match self.channels.get(name) {
            Some(Val::Pose(p)) => (p.x, p.y),
            _ => (0.0, 0.0),
        }
    }
}

#[derive(Clone)]
pub struct Ctx {
    pub sig: SigEnv,
    pub ambient: Pose,
    /// Some(...) while evaluating inside a scan (stateful sites active).
    pub scan: Option<ScanShared>,
    /// Card patterns, callable by name: (bowap 6.0) resolves here when the
    /// head isn't lexically bound.
    pub patterns: Rc<HashMap<String, Pattern>>,
    /// Card macros: expanded at application, before pattern resolution.
    pub macros: Rc<HashMap<String, Macro>>,
    /// Forks issued inside instantaneous contexts (manip callbacks —
    /// DMK's temporal-control-at-a-bullet case): collected here, adopted as
    /// child tasks by the executing task's scope after the instant returns.
    pub deferred: Vec<Rc<ActionV>>,
    /// Bound while elaborating/evaluating a collider projector body. Primitive
    /// projector constructors may reference only these names for entity-local
    /// and per-tick context data.
    pub projector_scope: Option<ProjectorScope>,
    /// True while evaluating a signal slot (eval_sig_at_rate). Signals read
    /// cells only via `(live name)` (language.md §control-cells: plain reads
    /// belong to the control layer; snap-by-default applies to cells exactly
    /// as to channels), so bare symbols skip the cell scope here — which also
    /// makes def resolution static and inlinable for the signal lowerer.
    pub signal_scope: bool,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx {
            sig: SigEnv::default(),
            ambient: Pose::IDENTITY,
            scan: None,
            patterns: Rc::new(HashMap::new()),
            macros: Rc::new(HashMap::new()),
            deferred: Vec::new(),
            projector_scope: None,
            signal_scope: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Expression evaluation.

pub fn evaluate(form: &Form, env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    match form {
        Form::Num(n) => Ok(Val::Num(*n)),
        Form::Bool(b) => Ok(Val::Num(if *b { 1.0 } else { 0.0 })),
        Form::Str(s) => Ok(Val::Kw(s.clone())),
        Form::Kw(k) => Ok(Val::Kw(k.clone())),
        Form::Sym(s) => match &**s {
            "inf" => Ok(Val::Num(f64::INFINITY)),
            "phi" => Ok(Val::Num(1.618_033_988_749_895)),
            name if name.starts_with('$') => ctx
                .sig
                .channel(&name[1..])
                .ok_or_else(|| format!("host does not provide channel {}", name)),
            name => {
                if let Some(v) = env.lookup(name) {
                    return Ok(v);
                }
                if !ctx.signal_scope {
                    if let Some(scope) = cell_scope(env) {
                        let id = scope.borrow().get(name).copied();
                        if let Some(id) = id {
                            if let Some((_, v)) = ctx.sig.cells.borrow().get(&id) {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                if let Some(f) = ctx.sig.defs.clone().get(name) {
                    // hygienic except the slot-bound parameters (and the
                    // cell scope, which is dynamic ambient): a def'd
                    // signal's t IS the referencing slot's t (F12)
                    let mut e = Env::empty();
                    for slot in ["t", "u", CELLS_KEY] {
                        if let Some(v) = env.lookup(slot) {
                            e = e.bind(slot.into(), v);
                        }
                    }
                    return evaluate(f, &e, ctx, world);
                }
                if is_builtin(name) {
                    return Ok(Val::Builtin(s.clone()));
                }
                Err(format!("unresolved symbol '{}'", name))
            }
        },
        Form::Vector(items) => {
            let lifted = match items
                .iter()
                .map(|i| eval_dynlike_form(i, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(lifted) => lifted,
                Err(err) => {
                    let vals = items
                        .iter()
                        .map(|i| evaluate(i, env, ctx, world))
                        .collect::<Result<Vec<_>, _>>()?;
                    if vals.iter().any(val_contains_structural_dyn) {
                        return Err(err);
                    }
                    return Ok(Val::arr(vals));
                }
            };
            if lifted.iter().any(DynLike::is_dynamic) {
                Ok(Val::DynLike(Rc::new(DynLike::List(lifted.into()))))
            } else {
                lifted
                    .iter()
                    .map(|v| v.eval_with_tick_rate(0.0, &MotionState::default(), &ctx.sig, world.tick_rate()))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Val::arr)
            }
        }
        Form::Map(kvs) => {
            let pairs = match kvs
                .iter()
                .map(|(k, v)| {
                    Ok((
                        data_atom_from_key(evaluate(k, env, ctx, world)?)?,
                        eval_dynlike_form(v, env, ctx, world)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()
            {
                Ok(pairs) => pairs,
                Err(err) => {
                    let pairs = kvs
                        .iter()
                        .map(|(k, v)| Ok((evaluate(k, env, ctx, world)?, evaluate(v, env, ctx, world)?)))
                        .collect::<Result<Vec<_>, String>>()?;
                    if pairs.iter().any(|(k, v)| {
                        val_contains_structural_dyn(k) || val_contains_structural_dyn(v)
                    }) {
                        return Err(err);
                    }
                    return Ok(Val::Map(Rc::new(pairs)));
                }
            };
            if pairs.iter().any(|(_, v)| v.is_dynamic()) {
                Ok(Val::DynLike(Rc::new(DynLike::Map(Rc::new(pairs)))))
            } else {
                pairs
                    .iter()
                    .map(|(k, v)| Ok((k.to_val(), v.eval_with_tick_rate(0.0, &MotionState::default(), &ctx.sig, world.tick_rate())?)))
                    .collect::<Result<Vec<_>, String>>()
                    .map(|pairs| Val::Map(Rc::new(pairs)))
            }
        }
        Form::List(items) => evaluate_list(items, env, ctx, world),
    }
}

fn val_contains_structural_dyn(v: &Val) -> bool {
    match v {
        Val::DynLike(_) | Val::DynPose(_) | Val::DynFigure(_) => true,
        Val::Arr(items) => items.iter().any(val_contains_structural_dyn),
        Val::Map(kvs) => kvs
            .iter()
            .any(|(k, v)| val_contains_structural_dyn(k) || val_contains_structural_dyn(v)),
        Val::ElemV(e) => val_contains_structural_dyn(&e.figure)
            || e.fields.iter().any(|(_, seed)| matches!(seed, FieldSeed::Dyn(_))),
        _ => false,
    }
}

fn eval_dynlike_form(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<DynLike, String> {
    match form {
        Form::Vector(items) => items
            .iter()
            .map(|i| eval_dynlike_form(i, env, ctx, world))
            .collect::<Result<Vec<_>, _>>()
            .map(|items| DynLike::List(items.into())),
        Form::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| {
                Ok((
                    data_atom_from_key(evaluate(k, env, ctx, world)?)?,
                    eval_dynlike_form(v, env, ctx, world)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|pairs| DynLike::Map(Rc::new(pairs))),
        form if contains_t(form) => Ok(DynLike::Dyn(DynVal::Expr {
            form: form.clone(),
            env: env.clone(),
        })),
        form => match evaluate(form, env, ctx, world) {
            Ok(v) => DynLike::from_val(v),
            Err(e) if e == "unresolved symbol 't'" || e == "unresolved symbol 'u'" => {
                Ok(DynLike::Dyn(DynVal::Expr {
                    form: form.clone(),
                    env: env.clone(),
                }))
            }
            Err(e) => Err(e),
        },
    }
}

pub(crate) fn elem_fields_from_val_map(m: &Val, signal_hint: &str) -> Result<Vec<(Rc<str>, FieldSeed)>, String> {
    let Val::Map(kvs) = m else {
        return Err(format!("fields: expected map, got {:?}", m));
    };
    let mut out = Vec::new();
    for (k, v) in kvs.iter() {
        let Val::Kw(key) = k else {
            return Err(format!("fields: expected keyword field name, got {:?}", k));
        };
        match v {
            Val::Nothing => {}
            Val::Num(n) => out.push((key.clone(), FieldSeed::Num(*n))),
            Val::Kw(s) => out.push((key.clone(), FieldSeed::Sym(s.clone()))),
            Val::DynLike(d) => out.push((key.clone(), FieldSeed::Dyn(as_dyn_num(d)?))),
            other => return Err(format!("fields: field :{} expected number, keyword, or signal; got {:?}. Use (fields ...) for signal seeds in {}", key, other, signal_hint)),
        }
    }
    Ok(out)
}

fn elem_fields_from_form_map(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Vec<(Rc<str>, FieldSeed)>, String> {
    match form {
        Form::Map(kvs) => {
            let mut out = Vec::new();
            for (k, v) in kvs.iter() {
                let kv = evaluate(k, env, ctx, world)?;
                let Val::Kw(key) = kv else {
                    return Err(format!("fields: expected keyword field name, got {:?}", kv));
                };
                if contains_t(v) {
                    out.push((key, FieldSeed::Dyn(DynNum::num_expr(v.clone(), env.clone()))));
                    continue;
                }
                match evaluate(v, env, ctx, world)? {
                    Val::Nothing => {}
                    Val::Num(n) => out.push((key, FieldSeed::Num(n))),
                    Val::Kw(s) => out.push((key, FieldSeed::Sym(s))),
                    other => return Err(format!("fields: field :{} expected number, keyword, or signal; got {:?}", key, other)),
                }
            }
            Ok(out)
        }
        other => {
            let mv = evaluate(other, env, ctx, world)?;
            elem_fields_from_val_map(&mv, "(fields ...)")
        }
    }
}

pub(crate) fn wrap_elem_fields(figure: Val, fields: Vec<(Rc<str>, FieldSeed)>) -> Val {
    if fields.is_empty() {
        return figure;
    }
    match figure {
        Val::ElemV(e) => {
            let mut merged = fields;
            for (k, v) in e.fields.iter() {
                if !merged.iter().any(|(existing, _)| existing.as_ref() == k.as_ref()) {
                    merged.push((k.clone(), v.clone()));
                }
            }
            Val::ElemV(Rc::new(ElemFields {
                figure: e.figure.clone(),
                fields: merged.into(),
            }))
        }
        figure => Val::ElemV(Rc::new(ElemFields {
            figure,
            fields: fields.into(),
        })),
    }
}

fn evaluate_list(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    // profiling attributes by evaluated head symbol — special, builtin, and
    // user defn alike; the profiler doesn't care what a name lowers to
    if profile::enabled() {
        if let Some(Form::Sym(s)) = items.first() {
            let name = s.clone();
            let frame = profile::open();
            let r = evaluate_list_inner(items, env, ctx, world);
            profile::close(&name, frame);
            return r;
        }
    }
    evaluate_list_inner(items, env, ctx, world)
}

fn evaluate_list_inner(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let head = items.first().ok_or("cannot evaluate empty list")?;

    if let Form::Sym(s) = head {
        match &**s {
            "loop" => return sf_loop(items, env, ctx, world),
            "recur" => {
                let vals = items[1..]
                    .iter()
                    .map(|f| evaluate(f, env, ctx, world))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Val::Action(Rc::new(ActionV::Recur(vals))));
            }
            "seq" => {
                return Ok(Val::Action(Rc::new(ActionV::Seq {
                    items: items[1..].to_vec().into(),
                    env: env.clone(),
                })));
            }
            "par" => {
                let kids = items[1..]
                    .iter()
                    .map(|f| as_action(evaluate(f, env, ctx, world)?))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Val::Action(Rc::new(ActionV::Par(kids))));
            }
            "fork" => {
                let inner = as_action(evaluate(&items[1], env, ctx, world)?)?;
                return Ok(Val::Action(Rc::new(ActionV::Fork(inner))));
            }
            // `when` is a prelude macro over `if` — no special. `if` with a
            // false condition and no else yields nothing, and nothing
            // coerces to the no-op action (as_action), so (when p …) works
            // in action position.
            "if" => {
                let c = evaluate(&items[1], env, ctx, world)?;
                return if truthy(&c) {
                    evaluate(&items[2], env, ctx, world)
                } else if items.len() > 3 {
                    evaluate(&items[3], env, ctx, world)
                } else {
                    Ok(Val::Nothing)
                };
            }
            VALUE_OR_INTRINSIC => {
                if items.len() != 3 {
                    return Err(format!("{}: expected two arguments", VALUE_OR_INTRINSIC));
                }
                let value = evaluate(&items[1], env, ctx, world)?;
                return if matches!(value, Val::Nothing) {
                    evaluate(&items[2], env, ctx, world)
                } else {
                    Ok(value)
                };
            }
            "cond" => {
                if items.len() < 2 {
                    return Ok(Val::Nothing);
                }
                if (items.len() - 1) % 2 != 0 {
                    return Err("cond: expected predicate/value pairs".into());
                }
                let mut projector_clauses = Vec::new();
                let mut all_projectors = true;
                let mut saw_projector = false;
                for pair in items[1..].chunks(2) {
                    let pred = match &pair[0] {
                        Form::Kw(k) if &**k == "else" => None,
                        pred => Some(pred.clone()),
                    };
                    match evaluate(&pair[1], env, ctx, world) {
                        Ok(value) => {
                            saw_projector |= match &value {
                                Val::ColliderProjector(_) => true,
                                Val::Arr(items) => items.iter().any(|v| matches!(v, Val::ColliderProjector(_))),
                                _ => false,
                            };
                            match flatten_collider_projectors("cond", value, None) {
                                Ok(projectors) => projector_clauses.push((pred, projectors)),
                                Err(_) => {
                                    all_projectors = false;
                                    break;
                                }
                            }
                        }
                        Err(_) => {
                            all_projectors = false;
                            break;
                        }
                    }
                }
                if all_projectors && (saw_projector || ctx.projector_scope.is_some()) {
                    let figure = projector_clauses
                        .iter()
                        .flat_map(|(_, ps)| ps.iter())
                        .next()
                        .map(|p| p.figure)
                        .unwrap_or(FigureProjectorKind::Pose);
                    if projector_clauses
                        .iter()
                        .flat_map(|(_, ps)| ps.iter())
                        .any(|p| p.figure != figure)
                    {
                        return Err("cond: collider projector branches must use the same figure type".into());
                    }
                    if ctx.projector_scope.is_none()
                        && projector_clauses
                            .iter()
                            .any(|(pred, _)| pred.as_ref().is_some_and(contains_legacy_projector_context))
                    {
                        return Err("cond: entity/context predicates require a projector scope".into());
                    }
                    return Ok(collider_projector_value(
                        ColliderProjectorValue::cond(
                            figure,
                            projector_clauses,
                            env.clone(),
                            ctx.projector_scope.clone(),
                        ),
                    ));
                }
                for pair in items[1..].chunks(2) {
                    let enabled = match &pair[0] {
                        Form::Kw(k) if &**k == "else" => true,
                        pred => truthy(&evaluate(pred, env, ctx, world)?),
                    };
                    if enabled {
                        return evaluate(&pair[1], env, ctx, world);
                    }
                }
                return Ok(Val::Nothing);
            }
            "let" => return sf_let(items, env, ctx, world),
            "fn" => {
                let Some(Form::Vector(ps)) = items.get(1) else {
                    return Err("fn: expected param vector".into());
                };
                validate_fn_params(ps)?;
                return Ok(Val::Fn {
                    params: ps.iter().cloned().collect::<Vec<_>>().into(),
                    body: items[2..].to_vec().into(),
                    env: env.clone(),
                });
            }
            "wait" => {
                let secs = evaluate(&items[1], env, ctx, world)?.num()?;
                return Ok(Val::Action(Rc::new(ActionV::Wait {
                    ticks: (secs * world.tick_rate()).round().max(0.0) as u64,
                })));
            }
            "deftick" => return sf_deftick(items, env, ctx, world),
            "collider" => {
                let (figure, params_idx, body_idx) = match items.get(1) {
                    Some(Form::Kw(k)) => (
                        FigureProjectorKind::from_projector_keyword("collider", k)?,
                        2,
                        3,
                    ),
                    _ => (FigureProjectorKind::Pose, 1, 2),
                };
                let Some(Form::Vector(ps)) = items.get(params_idx) else {
                    return Err("collider: expected parameter vector".into());
                };
                if ps.len() != 2 {
                    return Err("collider: expected two parameters".into());
                }
                let (Some(Form::Sym(e_name)), Some(Form::Sym(ctx_name))) = (ps.get(0), ps.get(1)) else {
                    return Err("collider: params must be symbols".into());
                };
                if items.len() <= body_idx {
                    return Err("collider: expected body".into());
                }
                let previous_scope = ctx.projector_scope.clone();
                ctx.projector_scope = Some(ProjectorScope {
                    entity: e_name.clone(),
                    context: ctx_name.clone(),
                    figure,
                });
                let mut last = Val::Nothing;
                for form in items[body_idx..].iter() {
                    match evaluate(form, env, ctx, world) {
                        Ok(v) => last = v,
                        Err(_) => {
                            ctx.projector_scope = previous_scope;
                            let params = vec![e_name.clone(), ctx_name.clone()];
                            return Ok(collider_projector_value(ColliderProjectorValue::callable(
                                figure,
                                params,
                                items[body_idx..].to_vec().into(),
                                env.clone(),
                            )));
                        }
                    }
                }
                ctx.projector_scope = previous_scope;
                return match flatten_collider_projectors("collider", last, Some(figure)) {
                    Ok(projectors) => Ok(Val::ColliderProjector(projectors)),
                    Err(_) => {
                        let params = vec![e_name.clone(), ctx_name.clone()];
                        Ok(collider_projector_value(ColliderProjectorValue::callable(
                            figure,
                            params,
                            items[body_idx..].to_vec().into(),
                            env.clone(),
                        )))
                    }
                };
            }
            "spawn" => return sf_spawn(items, env, ctx, world),
            "circle-collider" => return sf_circle_collider(items, env, ctx, world),
            "capsule-chain-collider" => return sf_capsule_chain_collider(items, env, ctx, world),
            name if engine::is_special(name) => {
                return engine::special(name, items, env, ctx, world).map(|v| v.unwrap());
            }
            "inline" => {
                // adapter: run the embedded pattern IN the caller's cell
                // scope ("binds into the embedding pattern's scope", §10)
                let inner = evaluate(&items[1], env, ctx, world)?;
                let Val::Action(a) = &inner else {
                    return Err("inline: expected a pattern call".into());
                };
                let ActionV::CallPattern { params, body, args, caller_cells, .. } = &**a else {
                    return Err("inline: expected a pattern call".into());
                };
                return Ok(Val::Action(Rc::new(ActionV::CallPattern {
                    params: params.clone(),
                    body: body.clone(),
                    args: args.clone(),
                    caller_cells: caller_cells.clone(),
                    fresh_cells: false,
                })));
            }
            "until" => {
                if items.len() < 3 {
                    return Err("until: expected (until pred body...)".into());
                }
                return Ok(Val::Action(Rc::new(ActionV::Until {
                    pred: items[1].clone(),
                    body: items[2..].to_vec().into(),
                    env: env.clone(),
                })));
            }
            "finally" => {
                if items.len() < 3 {
                    return Err("finally: expected (finally body cleanup...)".into());
                }
                return Ok(Val::Action(Rc::new(ActionV::Finally {
                    body: items[1].clone(),
                    cleanup: items[2..].to_vec().into(),
                    env: env.clone(),
                })));
            }
            "race" => {
                if items.len() < 3 {
                    return Err("race: expected at least two arms".into());
                }
                return Ok(Val::Action(Rc::new(ActionV::Race {
                    arms: items[1..].to_vec().into(),
                    env: env.clone(),
                })));
            }
            "states" => {
                // (states (label body…) …) — the bare FSM
                // primitive: ordered labeled states, label keyword as head.
                // End conditions are body code — (until pred …) as the
                // body, (fork (seq (wait d) (goto))) for timeouts. General
                // enough for player-control machines too: ground/air zones
                // with per-state movesets forked in the body (they die with
                // the state) and computed goto routing.
                let mut clauses = Vec::new();
                for cf in &items[1..] {
                    clauses.push(parse_state_clause(cf)?);
                }
                if clauses.is_empty() {
                    return Err("states: no states".into());
                }
                return Ok(Val::Action(Rc::new(ActionV::States {
                    clauses: clauses.into(),
                    env: env.clone(),
                })));
            }
            // `phases` — the boss-shaped layer over `states` — is a stdlib
            // macro now (lib/touhou.maku): what a "phase" means is genre
            // policy, and the macro-time form vocabulary expresses the
            // whole desugar as card code.
            "goto" => {
                // (goto label?) — the label is a VALUE (computed routing is
                // a Markov chain); bare (goto) exits to the default
                // successor (state order — what a timeout fork wants, since
                // it can't name what comes next).
                let label = match items.get(1) {
                    None => None,
                    Some(f) => match evaluate(f, env, ctx, world)? {
                        Val::Kw(l) => Some(l),
                        v => return Err(format!("goto: expected a :label, got {:?}", v)),
                    },
                };
                // scoped strictly to the innermost lexical machine: it binds
                // its request cell as #state-cell in state bodies; inner
                // machines shadow, called patterns don't see it
                let cell = match env.lookup("#state-cell") {
                    Some(Val::Num(n)) => n as u64,
                    _ => return Err("goto: no enclosing state machine".into()),
                };
                return Ok(Val::Action(Rc::new(ActionV::Goto { cell, label })));
            }
            "state-end?" => {
                // internal (the machine's guard over each state body, and
                // what forks under it inherit): a goto has been requested,
                // OR the machine has already left the state this guard was
                // armed in (the generation bumped at state exit — so
                // movesets forked in a state die when it ends, however it
                // ends, even though the request cell is long cleared).
                let req = evaluate(&items[1], env, ctx, world)?.num()? as u64;
                let genc = evaluate(&items[2], env, ctx, world)?.num()? as u64;
                let expect = evaluate(&items[3], env, ctx, world)?.num()?;
                let cells = ctx.sig.cells.borrow();
                let req_set = matches!(cells.get(&req), Some((_, Val::Kw(_))))
                    || matches!(cells.get(&req), Some((_, Val::Num(n))) if *n != 0.0);
                let g = match cells.get(&genc) {
                    Some((_, Val::Num(n))) => *n,
                    _ => expect,
                };
                return Ok(Val::Num(if req_set || g != expect { 1.0 } else { 0.0 }));
            }
            "race-done?" => {
                // internal: race children and the waiting parent share this
                // cell; true means one arm has completed.
                let cell = evaluate(&items[1], env, ctx, world)?.num()? as u64;
                let cells = ctx.sig.cells.borrow();
                let done = matches!(cells.get(&cell), Some((_, Val::Num(n))) if *n != 0.0);
                return Ok(Val::Num(if done { 1.0 } else { 0.0 }));
            }
            "race-won!" => {
                // internal: first completion wins; later writes are
                // idempotent because the cell stays true.
                let cell = evaluate(&items[1], env, ctx, world)?.num()? as u64;
                ctx.sig.cells.borrow_mut().insert(cell, ("#race".to_string(), Val::Num(1.0)));
                return Ok(Val::Nothing);
            }
            "in-frame" => {
                // frames form a monoid: (in-frame f1 f2 body) folds as
                // (f1 (f2 body)), outer to inner. Last argument is the body.
                // Frames evaluate left→right EXTENDING THE AMBIENT, so
                // ambient-reading forms in the body (aim) see the lexical
                // frame composition — uniform with the action-level
                // distribution law. Signal-valued frames extend by their
                // spawn-instant pose.
                if items.len() < 3 {
                    return Err("in-frame: expected (in-frame frame... body)".into());
                }
                let saved = ctx.ambient;
                let mut fvals = Vec::new();
                for f in &items[1..items.len() - 1] {
                    let fv = evaluate(f, env, ctx, world)?;
                    match &fv {
                        // :world resets the ambient (escape the caller anchor)
                        Val::Kw(k) if &**k == "world" => ctx.ambient = Pose::IDENTITY,
                        Val::DynPose(d) => {
                            let p = dyn_pose(d, 0.0, &MotionState::default(), &ctx.sig)
                                .unwrap_or(Pose::IDENTITY);
                            ctx.ambient = ctx.ambient.compose(&p);
                        }
                        other => {
                            let p = as_pose(other.clone()).unwrap_or(Pose::IDENTITY);
                            ctx.ambient = ctx.ambient.compose(&p);
                        }
                    }
                    fvals.push(fv);
                }
                let body = evaluate(&items[items.len() - 1], env, ctx, world);
                ctx.ambient = saved;
                let mut val = body?;
                for fv in fvals.into_iter().rev() {
                    val = match fv {
                        Val::Kw(k) if &*k == "world" => match val {
                            Val::Action(a) => Val::Action(Rc::new(ActionV::InFrame {
                                frame: FrameSpec::World,
                                inner: a,
                            })),
                            other => other, // dyns: value composition has no anchor to strip
                        },
                        Val::DynPose(d) => apply_dyn_frame(d.into_node(), val)?,
                        other => apply_frame_val(as_pose(other)?, val)?,
                    };
                }
                return Ok(val);
            }
            "clamp" => {
                // (clamp lo hi dyn): position clamp, e.g. playfield walls
                let lo = as_pose(evaluate(&items[1], env, ctx, world)?)?;
                let hi = as_pose(evaluate(&items[2], env, ctx, world)?)?;
                let child = as_dyn_pose(evaluate(&items[3], env, ctx, world)?)?;
                return Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Clamp {
                    lo: (lo.x, lo.y),
                    hi: (hi.x, hi.y),
                    child: child.into_node(),
                }))));
            }
            "cart" | "polar" if items[1..].iter().any(|f| contains_unbound_axis(f, env)) => {
                if items.len() != 3 {
                    return Err(format!("{}: expected two components", s));
                }
                return Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::ClosedPt {
                    a: expand_macros(&items[1], env, ctx, world)?,
                    b: expand_macros(&items[2], env, ctx, world)?,
                    polar: &**s == "polar",
                    env: env.clone(),
                    programs: std::cell::OnceCell::new(),
                }))));
            }
            "vel" => return sf_vel(items, env, ctx, world),
            "curve" => return sf_curve(items, env, ctx, world),
            "fields" => {
                if items.len() != 3 {
                    return Err("fields: expected (fields figure {field value ...})".into());
                }
                let figure = evaluate(&items[1], env, ctx, world)?;
                let fields = elem_fields_from_form_map(&items[2], env, ctx, world)?;
                return Ok(wrap_elem_fields(figure, fields));
            }
            "quasiquote" => {
                if items.len() != 2 {
                    return Err("quasiquote: expected one argument".into());
                }
                return qq(&items[1], env, ctx, world).map(|f| Val::FormV(Rc::new(f)));
            }
            "quote" => {
                if items.len() != 2 {
                    return Err("quote: expected one argument".into());
                }
                return Ok(Val::FormV(Rc::new(items[1].clone())));
            }
            "match" => return sf_match(items, env, ctx, world),
            // (map f xs) / (filter f xs): eager, value-level. Sequences are
            // arrays or form lists/vectors (macro code maps clause
            // transformers over its rest-args). These need the evaluator —
            // f may be a defn — hence specials, not builtins.
            "map" | "filter" => {
                let f = evaluate(&items[1], env, ctx, world)?;
                let subject = evaluate(&items[2], env, ctx, world)?;
                if let Val::EntitySet(idxs) = &subject {
                    let mut out = Vec::new();
                    for i in idxs.iter().copied() {
                        if !world.entities.is_alive(i) {
                            continue;
                        }
                        let r = apply_fn(f.clone(), &[Val::Handle(world.entity_ref(i))], ctx, world, false)?;
                        if &**s == "map" {
                            out.push(r);
                        } else if truthy(&r) {
                            out.push(Val::Handle(world.entity_ref(i)));
                        }
                    }
                    return Ok(Val::arr(out));
                }
                if let Val::CollisionSet(pairs) = &subject {
                    let mut out = Vec::new();
                    for (i, j) in pairs.iter().copied() {
                        if !world.entities.is_alive(i) || !world.entities.is_alive(j) {
                            continue;
                        }
                        let pair = Val::arr(vec![Val::Handle(world.entity_ref(i)), Val::Handle(world.entity_ref(j))]);
                        let r = apply_fn(f.clone(), &[pair.clone()], ctx, world, false)?;
                        if &**s == "map" {
                            out.push(r);
                        } else if truthy(&r) {
                            out.push(pair);
                        }
                    }
                    return Ok(Val::arr(out));
                }
                let xs = match seq_view(&subject) {
                    Some(xs) => xs,
                    None => return Err(format!("{}: not a sequence: {:?}", s, subject)),
                };
                let mut out = Vec::with_capacity(xs.len());
                for x in xs.iter() {
                    let r = apply_fn(f.clone(), &[x.clone()], ctx, world, false)?;
                    if &**s == "map" {
                        out.push(r);
                    } else if truthy(&r) {
                        out.push(x.clone());
                    }
                }
                return Ok(Val::arr(out));
            }
            "pather" => {
                // (pather window dyn): a trailing time-window of the
                // trajectory, materialized as geometry (§6)
                if !(items.len() == 3 || items.len() == 4) {
                    return Err("pather: expected (pather window dyn) or (pather window dyn {fields...})".into());
                }
                let window = evaluate(&items[1], env, ctx, world)?.num()?;
                let dv = as_dyn_pose(evaluate(&items[2], env, ctx, world)?)?;
                let figure = Val::CurveV(Rc::new(ExtCurve {
                    anchor: dv,
                    backing: CurveBacking::Trace { window },
                }));
                return match items.get(3) {
                    Some(fields) => Ok(wrap_elem_fields(figure, elem_fields_from_form_map(fields, env, ctx, world)?)),
                    None => Ok(figure),
                };
            }
            "live" => {
                // in a scan context: the channel's current value (class b/d);
                // at control level: a live pose signal usable as a frame
                if let Some(Form::Sym(ch)) = items.get(1) {
                    if let Some(name) = ch.strip_prefix('$') {
                        let cur = ctx
                            .sig
                            .channel(name)
                            .ok_or_else(|| format!("host does not provide channel {}", ch))?;
                        return if ctx.scan.is_some() {
                            Ok(cur)
                        } else {
                            match cur {
                                Val::Pose(_) => Ok(Val::DynPose(DynPose::pose_node(
                                    Rc::new(DynNode::Live { channel: Rc::from(name) }),
                                ))),
                                v => Ok(v),
                            }
                        };
                    }
                    // cells read live via the env-carried scope
                    if let Some(scope) = cell_scope(env) {
                        let id = scope.borrow().get(ch.as_ref()).copied();
                        if let Some(id) = id {
                            if let Some((_, v)) = ctx.sig.cells.borrow().get(&id) {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                return evaluate(&items[1], env, ctx, world);
            }
            "channel" => {
                let Some(Form::Sym(ch)) = items.get(1) else {
                    return Err("channel: expected a $channel name".into());
                };
                let Some(name) = ch.strip_prefix('$') else {
                    return Err("channel: name must start with $".into());
                };
                if let Some(v) = ctx.sig.channel(name) {
                    return Ok(v);
                }
                return match items.get(2) {
                    Some(default) => evaluate(default, env, ctx, world),
                    None => Ok(Val::Nothing),
                };
            }
            "stages" => return sf_stages(items, env, ctx, world),
            "rot" if items.len() == 2 && contains_unbound_axis(&items[1], env) => {
                return Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::RotExpr {
                    form: expand_macros(&items[1], env, ctx, world)?,
                    env: env.clone(),
                    program: std::cell::OnceCell::new(),
                }))));
            }
            "aim" => {
                let target = evaluate(&items[1], env, ctx, world)?;
                let Val::Pose(target) = target else {
                    return Err("aim: expected a point target".into());
                };
                let world_ang = (target.y - ctx.ambient.y).atan2(target.x - ctx.ambient.x).to_degrees();
                return Ok(Val::Pose(Pose::oriented(
                    0.0,
                    0.0,
                    world_ang - ctx.ambient.angle_or(0.0),
                )));
            }
            "defcell" => {
                let Some(Form::Sym(name)) = items.get(1) else {
                    return Err("defcell: expected name".into());
                };
                let init = evaluate(&items[2], env, ctx, world)?;
                let scope = cell_scope(env).ok_or("defcell: no cell scope")?;
                return Ok(Val::Action(Rc::new(ActionV::DefVar { scope, name: name.clone(), init })));
            }
            "set!" => {
                let Some(Form::Sym(name)) = items.get(1) else {
                    return Err("set!: expected name".into());
                };
                let val = evaluate(&items[2], env, ctx, world)?;
                let scope = cell_scope(env).ok_or("set!: no cell scope")?;
                return Ok(Val::Action(Rc::new(ActionV::SetVar { scope, name: name.clone(), val })));
            }
            "wait-for" => {
                return Ok(Val::Action(Rc::new(ActionV::WaitFor {
                    pred: items[1].clone(),
                    env: env.clone(),
                })));
            }
            "path" => {
                let curve = as_dyn_pose(evaluate(&items[1], env, ctx, world)?)?;
                return Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Path {
                    curve: curve.into_node(),
                    progress: expand_macros(&items[2], env, ctx, world)?,
                    env: env.clone(),
                }))));
            }
            "rand" => {
                let (a, b) = (
                    evaluate(&items[1], env, ctx, world)?.num()?,
                    evaluate(&items[2], env, ctx, world)?.num()?,
                );
                return Ok(Val::Num(a + world.next_rand() * (b - a)));
            }
            "evolve" => {
                // (evolve init step): the stateful signal constructor
                // (docs/notes/evolve-design.md). The result is a dyn value:
                // apply it to a time to sample, or put it in a pose/figure
                // slot. Under an active scan context the SITE, not the
                // construction, is the evolve's identity, and the form
                // evaluates to the settled state value
                // (docs/notes/evolve-reexpression-design.md).
                if items.len() != 3 {
                    return Err("evolve: expected (evolve init step)".into());
                }
                if ctx.scan.is_some() {
                    return sf_sited_evolve(items, env, ctx, world);
                }
                // expand at capture: the step's body re-evaluates per tick
                // in a macro-less Ctx, and the liveness walk should see
                // expansion shapes, not macro names
                let init_form = expand_macros(&items[1], env, ctx, world)?;
                let step_form = expand_macros(&items[2], env, ctx, world)?;
                let step = evaluate(&step_form, env, ctx, world)?;
                if !matches!(step, Val::Fn { .. } | Val::Builtin(_)) {
                    return Err(format!("evolve: step must be callable, got {:?}", step));
                }
                let live = evolve_is_live(&init_form, &step_form);
                let init = EvolveInit::Thunk { form: init_form, env: env.clone() };
                return Ok(Val::Evolve(Rc::new(EvolveDyn { init, step, live })));
            }
            _ => {}
        }
    }

    // Ordinary application. Unbound symbol heads resolve macro-first
    // (arguments arrive unevaluated; the expansion evaluates in the
    // caller's scope), then pattern (§10 embedding: args evaluated in the
    // CALLER's scope as ir values, defaults filling the rest; default
    // adapter = isolated cells, (inline …) shares the caller's), then
    // fall back to builtins.
    if let Form::Sym(name) = head {
        if env.lookup(name).is_none()
            && !ctx.sig.defs.contains_key(&**name)
            && !name.starts_with('$')
        {
            if let Some(mac) = ctx.macros.clone().get(&**name) {
                // args arrive unevaluated as forms; & rest binds the tail
                let menv = bind_params(Env::empty(), &mac.params, &items[1..], |f| {
                    Val::FormV(Rc::new(f.clone()))
                })?;
                let mut expansion = Val::Nothing;
                for f in mac.body.iter() {
                    expansion = evaluate(f, &menv, ctx, world)?;
                }
                let form = val_to_form(&expansion)?;
                return evaluate(&form, env, ctx, world);
            }
            let args = items[1..]
                .iter()
                .map(|f| evaluate(f, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(pat) = ctx.patterns.clone().get(&**name) {
                return Ok(Val::Action(Rc::new(ActionV::CallPattern {
                    params: pat.params.clone(),
                    body: pat.body.clone(),
                    args,
                    caller_cells: env.lookup(CELLS_KEY),
                    fresh_cells: true,
                })));
            }
            return builtin_with_eval_ctx(name, &args, ctx, world);
        }
    }
    let hv = evaluate(head, env, ctx, world)?;
    match hv {
        Val::Pose(p) => {
            if items.len() != 2 {
                return Err("frame application takes exactly one child".into());
            }
            // the applied frame is ambient for its child (see in-frame)
            let saved = ctx.ambient;
            ctx.ambient = ctx.ambient.compose(&p);
            let child = evaluate(&items[1], env, ctx, world);
            ctx.ambient = saved;
            apply_frame_val(p, child?)
        }
        // signal-valued frame (live channel, rot-expr): compose dyns
        // An evolve applied to a time replays its fold: the value at tick n
        // is the n-fold step application (any state type — pose, num, map).
        Val::Evolve(ev) => {
            if items.len() != 2 {
                return Err("evolve application takes exactly one sample time".into());
            }
            let t = evaluate(&items[1], env, ctx, world)?.num()?;
            evolve_value(&ev, t, &ctx.sig, world.tick_rate())
        }
        Val::DynPose(fd) => {
            // (d t u): two args are unambiguously curve sampling — frame
            // application takes exactly one child.
            if items.len() == 3 {
                let t = evaluate(&items[1], env, ctx, world)?.num()?;
                let u = evaluate(&items[2], env, ctx, world)?.num()?;
                return apply_dyn_pose_at(&fd, t, Some(u), &ctx.sig);
            }
            if items.len() != 2 {
                return Err("dyn application takes a sample time (and optional u) or one frame child".into());
            }
            let saved = ctx.ambient;
            let p0 = dyn_pose(&fd, 0.0, &MotionState::default(), &ctx.sig)
                .unwrap_or(Pose::IDENTITY);
            ctx.ambient = ctx.ambient.compose(&p0);
            let child = evaluate(&items[1], env, ctx, world);
            ctx.ambient = saved;
            // (d t): the child is evaluated ONCE (it may have effects — rand,
            // cells); a numeric result means sampling, anything else frames.
            match child? {
                Val::Num(t) => apply_dyn_pose_at(&fd, t, None, &ctx.sig),
                child => apply_dyn_frame(fd.into_node(), child),
            }
        }
        Val::Arr(_) => {
            if items.len() != 2 {
                return Err("frame-array application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx, world)?;
            apply_frame_arr(&hv, child)
        }
        Val::Kw(k) => {
            // keyword application: map access, e.g. (:vel exit); on
            // Pose, :x/:y/:th read components; on entity handles/sets,
            // the same keyword reads a flat entity field.
            let arg = evaluate(&items[1], env, ctx, world)?;
            match (&*k, &arg) {
                ("x", Val::Pose(p)) => return Ok(Val::Num(p.x)),
                ("y", Val::Pose(p)) => return Ok(Val::Num(p.y)),
                ("th", Val::Pose(p)) => return Ok(Val::Num(p.angle_or(0.0))),
                ("x", Val::StageExitPose { .. })
                | ("y", Val::StageExitPose { .. })
                | ("th", Val::StageExitPose { .. }) => {
                    let Val::Pose(p) = resolve_stage_exit_value(arg, ctx)? else {
                        unreachable!("stage exit pose resolves to a pose")
                    };
                    return match &*k {
                        "x" => Ok(Val::Num(p.x)),
                        "y" => Ok(Val::Num(p.y)),
                        _ => Ok(Val::Num(p.angle_or(0.0))),
                    };
                }
                ("pos", Val::StageExit(slot)) => {
                    return Ok(Val::StageExitPose { slot: slot.clone(), field: StageExitField::Pos });
                }
                ("vel", Val::StageExit(slot)) => {
                    return Ok(Val::StageExitPose { slot: slot.clone(), field: StageExitField::Vel });
                }
                ("pose", Val::StageExit(slot)) => {
                    return Ok(Val::StageExitPose { slot: slot.clone(), field: StageExitField::Pos });
                }
                _ => {}
            }
            if let Val::EntityView(id) = arg {
                return match world.entities.generation(id.row).filter(|generation| *generation == id.generation) {
                    Some(_) => entity_field_at(id.row, &k, world, &ctx.sig),
                    None => Ok(Val::Nothing),
                };
            }
            if matches!(arg, Val::Handle(_) | Val::EntitySet(_)) {
                return entity_field_value(arg, &k, world, &ctx.sig);
            }
            Ok(map_path_get(&arg, &k).unwrap_or(Val::Nothing))
        }
        f @ (Val::Fn { .. } | Val::Builtin(_)) => {
            let args = items[1..]
                .iter()
                .map(|x| evaluate(x, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            // cells are dynamic ambient: the caller's scope flows into the
            // callee (hygiene excepts #cells, like the slot-bound t/u)
            let f = match f {
                Val::Fn { params, body, env: fenv } => {
                    let mut fenv = fenv;
                    if fenv.lookup(CELLS_KEY).is_none() {
                        if let Some(cells) = env.lookup(CELLS_KEY) {
                            fenv = fenv.bind(CELLS_KEY.into(), cells);
                        }
                    }
                    for name in ["t", "u"] {
                        if fenv.lookup(name).is_none() {
                            if let Some(v) = env.lookup(name) {
                                fenv = fenv.bind(name.into(), v);
                            }
                        }
                    }
                    Val::Fn { params, body, env: fenv }
                }
                f => f,
            };
            apply_fn(f, &args, ctx, world, false)
        }
        _ => Err(format!("cannot apply {:?}", hv)),
    }
}

fn apply_dyn_pose_at(d: &DynPose, t: f64, u: Option<f64>, sig: &SigEnv) -> Result<Val, String> {
    // Application sampling currently uses a fresh state, which is only
    // semantically correct for closed/stateless dyns. Stateful dyn replay gets
    // defined when `scan` lands.
    let st = MotionState::default();
    match u {
        Some(u) => {
            let p0 = dyn_pose_u(d, t, u, &st, sig)?;
            let p1 = dyn_pose_u(d, t, u + 0.01, &st, sig)?;
            let th = (p1.y - p0.y).atan2(p1.x - p0.x).to_degrees();
            Ok(Val::Pose(Pose::oriented(p0.x, p0.y, th)))
        }
        None => Ok(Val::Pose(dyn_pose(d, t, &st, sig)?)),
    }
}

/// A dyn in frame (head) position: composes over dyns, exts, and arrays.
fn apply_dyn_frame(frame: Rc<DynNode>, child: Val) -> Result<Val, String> {
    match child {
        Val::Action(a) => Ok(Val::Action(Rc::new(ActionV::InFrame {
            frame: FrameSpec::Node(frame),
            inner: a,
        }))),
        Val::Arr(items) => {
            let out = items
                .iter()
                .map(|c| apply_dyn_frame(frame.clone(), c.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        Val::CurveV(l) => Ok(Val::CurveV(Rc::new(ExtCurve {
            anchor: l.anchor.framed(frame),
            backing: l.backing.clone(),
        }))),
        Val::ElemV(e) => Ok(wrap_elem_fields(
            apply_dyn_frame(frame, e.figure.clone())?,
            e.fields.iter().cloned().collect(),
        )),
        other => Ok(Val::DynPose(DynPose::pose_node(frame_node(
            frame,
            as_dyn_pose(other)?.into_node(),
        )))),
    }
}

/// Apply a user fn or builtin. Ambient frames do not cross fn boundaries
/// (F18). `exec_actions` is set only for manip callbacks, whose bodies
/// run instantaneously; ordinary fns RETURN action values for composition.
pub(crate) fn map_path_get(v: &Val, path: &str) -> Option<Val> {
    let mut cur = v.clone();
    for key in path.split('.') {
        cur = map_get(&cur, key)?;
    }
    Some(cur)
}

fn read_stage_exit_pose(ctx: &Ctx, slot: &Rc<StageExitSlot>, field: StageExitField) -> Result<Pose, String> {
    let Some(scan) = &ctx.scan else {
        return Err("stage exit can only be read inside a staged signal".into());
    };
    let io = scan.borrow();
    let Some(key_readers) = io.readers.as_ref() else {
        return Err("stage exit requires motion readers".into());
    };
    let key = stage_exit_key(slot, key_readers, field);
    let value = match io.state.get(&key) {
        Some(Cell::N(v)) => Some(*v),
        _ => None,
    }
    .or_else(|| io.readers.as_ref().and_then(|readers| readers.n2(key)))
    .ok_or("stage exit has not been written")?;
    Ok(Pose::point(value[0], value[1]))
}

fn resolve_stage_exit_value(v: Val, ctx: &Ctx) -> Result<Val, String> {
    match v {
        Val::StageExitPose { slot, field } => read_stage_exit_pose(ctx, &slot, field).map(Val::Pose),
        other => Ok(other),
    }
}

fn builtin_with_eval_ctx(name: &str, args: &[Val], ctx: &Ctx, world: &World) -> Result<Val, String> {
    if name == "get" {
        if let (Some(Val::EntityView(id)), Some(Val::Kw(field))) = (args.get(0), args.get(1)) {
            return match world.entities.generation(id.row).filter(|generation| *generation == id.generation) {
                Some(_) => {
                    let v = entity_field_at(id.row, field, world, &ctx.sig)?;
                    if matches!(v, Val::Nothing) {
                        Ok(args.get(2).cloned().unwrap_or(Val::Nothing))
                    } else {
                        Ok(v)
                    }
                }
                None => Ok(args.get(2).cloned().unwrap_or(Val::Nothing)),
            };
        }
    }
    let materialized;
    let args = if args.iter().any(|v| matches!(v, Val::EntityView(_))) {
        materialized = args
            .iter()
            .map(|v| match v {
                Val::EntityView(id) => entity_view(id.row, world, &ctx.sig),
                v => Ok(v.clone()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        materialized.as_slice()
    } else {
        args
    };
    // deferred stage-exit reads are rare; keep the common path clone-free
    if args.iter().any(|v| matches!(v, Val::StageExitPose { .. })) {
        let args = args
            .iter()
            .cloned()
            .map(|v| resolve_stage_exit_value(v, ctx))
            .collect::<Result<Vec<_>, _>>()?;
        return builtin_with_tick_rate(name, &args, world.tick_rate());
    }
    builtin_with_tick_rate(name, args, world.tick_rate())
}

/// Entity view passed to predicate queries and manip callbacks.
/// The entity view contains current kinematic fields, sym fields, and columns.
pub(crate) fn entity_motion_readers(i: usize, world: &World) -> MotionReaders {
    let probe = profile::enabled().then(profile::open);
    let r = entity_motion_readers_inner(i, world);
    if let Some(f) = probe {
        profile::close("sim:motion-readers", f);
    }
    r
}

fn entity_motion_readers_inner(i: usize, world: &World) -> MotionReaders {
    match world.entities.motion_schema(i) {
        None => return MotionReaders::stateless(Rc::default()),
        Some(schema)
            if schema.n2_keys.is_empty()
                && schema.dyn_keys.is_empty()
                && schema.val_keys.is_empty() =>
        {
            return MotionReaders::stateless(schema.shared_node_ids());
        }
        _ => {}
    }
    let snapshot = world
        .entities
        .row_state_snapshot(i)
        .expect("schema presence checked above");
    MotionReaders::for_row_snapshot(snapshot)
}

pub(crate) fn entity_view(i: usize, world: &World, sig: &SigEnv) -> Result<Val, String> {
    let dyn_figure = world
        .entities
        .dyn_figure(i)
        .ok_or_else(|| format!("entity view: missing dyn figure for row {i}"))?;
    let tau = world.entity_motion_tau(i, world.tick);
    let readers = entity_motion_readers(i, world);
    let state = MotionState::default();
    let p = dyn_figure_pose_in(
        dyn_figure,
        tau,
        MotionEvalCtx::with_tick_rate(&state, sig, &readers, world.tick_rate()).pos_only(),
    )?;
    let vel = world.entity_velocity_from_samples(i, world.tick);
    let mut view = vec![
        (Val::Kw("pos".into()), Val::Pose(Pose::point(p.x, p.y))),
        (Val::Kw("vel".into()), Val::Pose(Pose::point(vel.0, vel.1))),
        (Val::Kw("t".into()), Val::Num(tau)),
        (Val::Kw("tick".into()), Val::Num(world.tick as f64)),
        (Val::Kw("handle".into()), Val::Handle(world.entity_ref(i))),
        (Val::Kw("kind".into()), Val::Kw(match dyn_figure.repr() {
            FigureDynRepr::Pose(_) if world.entities.is_traced(i) => "pather",
            FigureDynRepr::Pose(_) => "point",
            FigureDynRepr::Curve { .. } => "curve",
        }.into())),
    ];
    for (field, value) in world.sym_fields_for_view(i) {
        view.push((Val::Kw(field), Val::Kw(value)));
    }
    for (k, v) in world.cols_for_view(i) {
        view.push((Val::Kw(k), Val::Num(v)));
    }
    Ok(Val::Map(Rc::new(view)))
}

fn is_query_value(v: &Val) -> bool {
    matches!(v, Val::Map(_) | Val::Fn { .. } | Val::Builtin(_) | Val::EntitySet(_))
}

#[derive(Debug)]
pub(crate) struct RowPredicate {
    tests: Vec<RowTest>,
}

#[derive(Debug)]
pub(crate) enum RowTest {
    KwEq { field: Rc<str>, value: Rc<str> },
    NumCmp { op: CmpOp, lhs: RowNum, rhs: RowNum },
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CmpOp { Lt, Le, Gt, Ge, Eq }

#[derive(Debug)]
pub(crate) enum RowNum {
    Lit(f64),
    Tau,
    Tick,
    ColOr(Rc<str>, Box<RowNum>),
    Add(Box<RowNum>, Box<RowNum>),
    Sub(Box<RowNum>, Box<RowNum>),
    Mul(Box<RowNum>, Box<RowNum>),
}

fn row_head_unshadowed(name: &str, param: &str, env: &Env, ctx: &Ctx) -> bool {
    name != param && env.lookup(name).is_none() && !ctx.sig.defs.contains_key(name)
}

fn row_access(form: &Form, param: &str) -> Option<Rc<str>> {
    let Form::List(items) = form else { return None };
    let [Form::Kw(field), Form::Sym(subject)] = &items[..] else { return None };
    (subject.as_ref() == param).then(|| field.clone())
}

fn row_num(form: &Form, param: &str, env: &Env, ctx: &Ctx) -> Option<RowNum> {
    match form {
        Form::Num(n) => Some(RowNum::Lit(*n)),
        Form::Sym(name) if name.as_ref() == "inf" && row_head_unshadowed("inf", param, env, ctx) => {
            Some(RowNum::Lit(f64::INFINITY))
        }
        _ => {
            if let Some(field) = row_access(form, param) {
                return match field.as_ref() {
                    "t" => Some(RowNum::Tau),
                    "tick" => Some(RowNum::Tick),
                    _ => None,
                };
            }
            let Form::List(items) = form else { return None };
            let Form::Sym(head) = items.first()? else { return None };
            if !row_head_unshadowed(head, param, env, ctx) { return None; }
            match (head.as_ref(), &items[1..]) {
                ("%value-or", [value, default]) => {
                    let field = row_access(value, param)?;
                    if matches!(field.as_ref(), "pos" | "vel" | "t" | "tick" | "handle" | "kind") {
                        return None;
                    }
                    Some(RowNum::ColOr(field, Box::new(row_num(default, param, env, ctx)?)))
                }
                ("+", [a, b]) => Some(RowNum::Add(
                    Box::new(row_num(a, param, env, ctx)?), Box::new(row_num(b, param, env, ctx)?))),
                ("-", [a, b]) => Some(RowNum::Sub(
                    Box::new(row_num(a, param, env, ctx)?), Box::new(row_num(b, param, env, ctx)?))),
                ("*", [a, b]) => Some(RowNum::Mul(
                    Box::new(row_num(a, param, env, ctx)?), Box::new(row_num(b, param, env, ctx)?))),
                _ => None,
            }
        }
    }
}

pub(crate) fn row_predicate(q: &Val, ctx: &Ctx) -> Option<RowPredicate> {
    let Val::Fn { params, body, env } = q else { return None };
    let [Form::Sym(param)] = &params[..] else { return None };
    if &**param == "&" {
        return None;
    }
    let [body] = &body[..] else { return None };
    let forms = match body {
        Form::List(items) if matches!(items.first(), Some(Form::Sym(head)) if &**head == "*") => {
            if items.len() < 2 || !row_head_unshadowed("*", param, env, ctx) {
                return None;
            }
            &items[1..]
        }
        form => std::slice::from_ref(form),
    };
    let mut tests = Vec::with_capacity(forms.len());
    for form in forms {
        let Form::List(call) = form else { return None };
        let [Form::Sym(head), left, right] = &call[..] else { return None };
        if head.as_ref() == "=" && row_head_unshadowed("=", param, env, ctx) {
            let kw = match (left, right) {
                (access, Form::Kw(value)) | (Form::Kw(value), access) => {
                    row_access(access, param).map(|field| (field, value.clone()))
                }
                _ => None,
            };
            if let Some((field, value)) = kw {
                if matches!(field.as_ref(), "pos" | "vel" | "t" | "tick" | "handle") {
                    return None;
                }
                tests.push(RowTest::KwEq { field, value });
                continue;
            }
        }
        let op = match head.as_ref() {
            "<" => CmpOp::Lt, "<=" => CmpOp::Le, ">" => CmpOp::Gt,
            ">=" => CmpOp::Ge, "=" => CmpOp::Eq, _ => return None,
        };
        if !row_head_unshadowed(head, param, env, ctx)
            || matches!(left, Form::Kw(_)) || matches!(right, Form::Kw(_)) {
            return None;
        }
        tests.push(RowTest::NumCmp {
            op, lhs: row_num(left, param, env, ctx)?, rhs: row_num(right, param, env, ctx)?,
        });
    }
    Some(RowPredicate { tests })
}

/// Per-query resolved form of a RowTest: symbol lookups happen once
/// per query, not once per row. A field or value that was never interned
/// anywhere cannot match any row (mirrors sym_field_matches_at).
pub(crate) enum ResolvedRowTest {
    Kind(Rc<str>),
    SymEq { field: FieldName, value: Symbol },
    Never,
    NumCmp { op: CmpOp, lhs: ResolvedRowNum, rhs: ResolvedRowNum },
}

pub(crate) enum ResolvedRowNum {
    Lit(f64), Tau, Tick,
    ColOr(Option<Symbol>, Box<ResolvedRowNum>),
    Add(Box<ResolvedRowNum>, Box<ResolvedRowNum>),
    Sub(Box<ResolvedRowNum>, Box<ResolvedRowNum>),
    Mul(Box<ResolvedRowNum>, Box<ResolvedRowNum>),
}

fn resolve_row_num(value: &RowNum, world: &World) -> ResolvedRowNum {
    match value {
        RowNum::Lit(n) => ResolvedRowNum::Lit(*n), RowNum::Tau => ResolvedRowNum::Tau,
        RowNum::Tick => ResolvedRowNum::Tick,
        RowNum::ColOr(name, d) => ResolvedRowNum::ColOr(world.symbols.lookup(name), Box::new(resolve_row_num(d, world))),
        RowNum::Add(a, b) => ResolvedRowNum::Add(Box::new(resolve_row_num(a, world)), Box::new(resolve_row_num(b, world))),
        RowNum::Sub(a, b) => ResolvedRowNum::Sub(Box::new(resolve_row_num(a, world)), Box::new(resolve_row_num(b, world))),
        RowNum::Mul(a, b) => ResolvedRowNum::Mul(Box::new(resolve_row_num(a, world)), Box::new(resolve_row_num(b, world))),
    }
}

impl RowPredicate {
    pub(crate) fn resolve(&self, world: &World) -> Vec<ResolvedRowTest> {
        self.tests
            .iter()
            .map(|test| match test {
                RowTest::KwEq { field, value } => {
                    if &**field == "kind" { return ResolvedRowTest::Kind(value.clone()); }
                    match (world.symbols.lookup(field), world.symbols.lookup(value)) {
                        (Some(field), Some(value)) => ResolvedRowTest::SymEq { field, value },
                        _ => ResolvedRowTest::Never,
                    }
                }
                RowTest::NumCmp { op, lhs, rhs } => ResolvedRowTest::NumCmp {
                    op: *op,
                    lhs: resolve_row_num(lhs, world), rhs: resolve_row_num(rhs, world),
                },
            })
            .collect()
    }
}

fn resolved_row_num(value: &ResolvedRowNum, row: usize, world: &World) -> Option<f64> {
    Some(match value {
        ResolvedRowNum::Lit(n) => *n, ResolvedRowNum::Tau => world.entity_tau(row, world.tick),
        ResolvedRowNum::Tick => world.tick as f64,
        ResolvedRowNum::ColOr(sym, default) => {
            if sym.is_some_and(|sym| world.sym_field_value_at(row, sym).is_some()) { return None; }
            match sym.and_then(|sym| world.col_get_sym_at(row, sym)) {
                Some(value) => value,
                None => resolved_row_num(default, row, world)?,
            }
        }
        ResolvedRowNum::Add(a, b) | ResolvedRowNum::Sub(a, b) | ResolvedRowNum::Mul(a, b) => {
            let (left, right) = (resolved_row_num(a, row, world), resolved_row_num(b, row, world));
            let (left, right) = (left?, right?);
            match value {
                ResolvedRowNum::Add(_, _) => left + right,
                ResolvedRowNum::Sub(_, _) => left - right,
                ResolvedRowNum::Mul(_, _) => left * right,
                _ => unreachable!(),
            }
        }
    })
}

pub(crate) fn resolved_row_tests_match(tests: &[ResolvedRowTest], row: usize, world: &World) -> Option<bool> {
    let mut all_pass = true;
    for test in tests {
        let passes = match test {
        ResolvedRowTest::Kind(value) => world.entities.dyn_figure(row).is_some_and(|figure| {
            let kind = match figure.repr() {
                FigureDynRepr::Pose(_) if world.entities.is_traced(row) => "pather",
                FigureDynRepr::Pose(_) => "point",
                FigureDynRepr::Curve { .. } => "curve",
            };
            kind == &**value
        }),
        ResolvedRowTest::SymEq { field, value } => {
            world.sym_field_value_at(row, *field) == Some(*value)
        }
            ResolvedRowTest::Never => false,
            ResolvedRowTest::NumCmp { op, lhs, rhs } => {
                let (a, b) = (resolved_row_num(lhs, row, world), resolved_row_num(rhs, row, world));
                let (a, b) = (a?, b?);
                match op { CmpOp::Lt => a < b, CmpOp::Le => a <= b, CmpOp::Gt => a > b,
                    CmpOp::Ge => a >= b, CmpOp::Eq => (a - b).abs() < 1e-9 }
            }
        };
        all_pass &= passes;
    }
    Some(all_pass)
}

fn sf_matches(items: &[Form], env: &Env) -> Result<Val, String> {
    if items.len() < 3 || items.len() % 2 == 0 {
        return Err("matches: expected field/value pairs".into());
    }
    let row = Form::sym("x");
    let mut tests = Vec::new();
    for pair in items[1..].chunks(2) {
        let Form::Kw(field) = &pair[0] else {
            return Err("matches: expected keyword field".into());
        };
        tests.push(Form::list(vec![
            Form::sym("="),
            Form::list(vec![Form::Kw(field.clone()), row.clone()]),
            pair[1].clone(),
        ]));
    }
    let body = if tests.len() == 1 {
        tests
    } else {
        let mut forms = vec![Form::sym("*")];
        forms.extend(tests);
        vec![Form::list(forms)]
    };
    Ok(Val::Fn {
        params: vec![Form::sym("x")].into(),
        body: body.into(),
        env: env.clone(),
    })
}

pub(crate) fn resolve_query(q: &Val, ctx: &mut Ctx, world: &mut World) -> Result<Vec<usize>, String> {
    match q {
        Val::Map(_) => resolve_map_query(q, ctx, world),
        Val::Fn { .. } | Val::Builtin(_) => resolve_predicate_query(q, ctx, world),
        Val::EntitySet(_) => entity_index_value(q.clone(), world)
            .map(|idxs| idxs.into_iter().filter(|i| world.entities.is_alive(*i)).collect()),
        v => Err(format!("query: expected predicate, entity set, or map, got {:?}", v)),
    }
}

fn resolve_predicate_query(q: &Val, ctx: &mut Ctx, world: &mut World) -> Result<Vec<usize>, String> {
    let candidates = world
        .entities
        .iter()
        .enumerate()
        .filter_map(|(i, _)| world.entities.is_alive(i).then_some(i))
        .collect::<Vec<_>>();
    let Some(predicate) = row_predicate(q, ctx) else {
        return resolve_predicate_query_fallback(q, ctx, world, &candidates);
    };
    // Mixed predicates deliberately fall back as a whole; partial prefiltering
    // would change the error and effect ordering of the residual expression.
    let tests = predicate.resolve(world);
    let mut out = Vec::new();
    for &i in &candidates {
        let Some(matches) = resolved_row_tests_match(&tests, i, world) else {
            return resolve_predicate_query_fallback(q, ctx, world, &candidates);
        };
        if matches { out.push(i); }
    }
    if lower::oracle_enabled() {
        let expected = resolve_predicate_query_fallback(q, ctx, world, &candidates)?;
        assert_eq!(out, expected, "row predicate mismatch for {:?}", q);
    }
    Ok(out)
}

fn resolve_predicate_query_fallback(
    q: &Val,
    ctx: &mut Ctx,
    world: &mut World,
    candidates: &[usize],
) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for &i in candidates {
        let view = Val::EntityView(world.entity_ref(i));
        if truthy(&apply_fn(q.clone(), &[view], ctx, world, false)?) {
            out.push(i);
        }
    }
    Ok(out)
}

fn resolve_map_query(q: &Val, _ctx: &mut Ctx, world: &mut World) -> Result<Vec<usize>, String> {
    let Val::Map(kvs) = q else { return Err("query: expected a map".into()) };
    // Selector symbols resolve once per query, not per row. A keyword that
    // was never interned anywhere can match no row's sym field; likewise a
    // non-keyword selector matches nothing. Reserved axis keys keep their
    // first-occurrence-wins behavior; other keys test every occurrence.
    enum SelSyms {
        One(Option<Symbol>),
        Many(Vec<Option<Symbol>>),
        Never,
    }
    let mut seen_reserved: Vec<&str> = Vec::new();
    let mut filters: Vec<(Option<FieldName>, SelSyms)> = Vec::new();
    for (k, v) in kvs.iter() {
        let Val::Kw(field) = k else { continue };
        if matches!(field.as_ref(), "team" | "family" | "color" | "variant") {
            if seen_reserved.contains(&field.as_ref()) {
                continue;
            }
            seen_reserved.push(field.as_ref());
        }
        let sel = match v {
            Val::Kw(kw) => SelSyms::One(world.symbols.lookup(kw)),
            Val::Arr(xs) => SelSyms::Many(
                xs.iter()
                    .map(|x| match x {
                        Val::Kw(kw) => world.symbols.lookup(kw),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => SelSyms::Never,
        };
        filters.push((world.symbols.lookup(field), sel));
    }
    let sel_matches = |sel: &SelSyms, actual: Symbol| match sel {
        SelSyms::One(s) => *s == Some(actual),
        SelSyms::Many(xs) => xs.contains(&Some(actual)),
        SelSyms::Never => false,
    };
    let mut candidates: Vec<usize> = Vec::new();
    for (i, _) in world.entities.iter().enumerate() {
        if !world.entities.is_alive(i) {
            continue;
        }
        let ok = filters.iter().all(|(field, sel)| {
            field
                .and_then(|field| world.sym_field_value_at(i, field))
                .is_some_and(|actual| sel_matches(sel, actual))
        });
        if ok {
            candidates.push(i);
        }
    }
    Ok(candidates)
}

fn entity_index_value(v: Val, world: &World) -> Result<Vec<usize>, String> {
    match v {
        Val::EntitySet(idxs) => Ok(idxs.iter().copied().filter(|i| *i < world.entities.len()).collect()),
        Val::Handle(id) => world
            .find(id)
            .map(|i| vec![i])
            .ok_or_else(|| format!("dead entity handle {:?}", id)),
        v => Err(format!("expected entity set or handle, got {:?}", v)),
    }
}

fn singleton_or_array(mut vals: Vec<Val>) -> Val {
    if vals.len() == 1 {
        vals.remove(0)
    } else {
        Val::arr(vals)
    }
}

pub(crate) fn entity_pose_at(i: usize, world: &World, sig: &SigEnv) -> Result<Pose, String> {
    if let Some((x, y)) = world.entities.sampled_pos(i, world.tick) {
        return Ok(Pose::point(x, y));
    }
    let dyn_figure = world
        .entities
        .dyn_figure(i)
        .ok_or_else(|| format!("field: missing dyn figure for row {i}"))?;
    let tau = world.entity_motion_tau(i, world.tick);
    let readers = entity_motion_readers(i, world);
    let state = MotionState::default();
    let p = dyn_figure_pose_in(
        dyn_figure,
        tau,
        MotionEvalCtx::with_tick_rate(&state, sig, &readers, world.tick_rate()).pos_only(),
    )?;
    Ok(Pose::point(p.x, p.y))
}

pub(crate) fn entity_field_at(i: usize, field: &str, world: &World, sig: &SigEnv) -> Result<Val, String> {
    match field {
        "pos" => Ok(Val::Pose(entity_pose_at(i, world, sig)?)),
        "vel" => {
            let vel = world.entity_velocity_from_samples(i, world.tick);
            Ok(Val::Pose(Pose::point(vel.0, vel.1)))
        }
        "t" => {
            Ok(Val::Num(world.entity_tau(i, world.tick)))
        }
        "tick" => Ok(Val::Num(world.tick as f64)),
        "handle" => Ok(Val::Handle(world.entity_ref(i))),
        "kind" => Ok(Val::Kw(match world
            .entities
            .dyn_figure(i)
            .ok_or_else(|| format!("field: missing dyn figure for row {i}"))?
            .repr() {
            FigureDynRepr::Pose(_) if world.entities.is_traced(i) => "pather",
            FigureDynRepr::Pose(_) => "point",
            FigureDynRepr::Curve { .. } => "curve",
        }
        .into())),
        field => {
            // One symbol lookup covers both stores: a name never interned
            // anywhere can name neither a sym field nor a numeric column.
            Ok(entity_field_sym_at(i, world.symbols.lookup(field), world))
        }
    }
}

/// The non-special-field read of `entity_field_at`, keyed by an
/// already-resolved symbol. `None` (never interned) reads as Nothing.
pub(crate) fn entity_field_sym_at(i: usize, sym: Option<Symbol>, world: &World) -> Val {
    let Some(sym) = sym else {
        return Val::Nothing;
    };
    if let Some(value) = world.sym_field_value_at(i, sym) {
        if let Some(resolved) = world.symbols.resolve(value) {
            return Val::Kw(resolved.into());
        }
    }
    world.col_get_sym_at(i, sym).map(Val::Num).unwrap_or(Val::Nothing)
}

fn entity_col_value(v: Val, col: &str, world: &World) -> Result<Val, String> {
    let idxs = entity_index_value(v, world)?;
    let mut vals = Vec::with_capacity(idxs.len());
    for i in idxs {
        if world.entities.is_alive(i) {
            vals.push(Val::Num(world.col_get_at(i, col).unwrap_or(0.0)));
        }
    }
    Ok(singleton_or_array(vals))
}

fn entity_field_value(v: Val, field: &str, world: &World, sig: &SigEnv) -> Result<Val, String> {
    if let Val::Handle(id) = v {
        return match world.entities.generation(id.row).filter(|generation| *generation == id.generation) {
            Some(_) => entity_field_at(id.row, field, world, sig),
            None => Ok(Val::Nothing),
        };
    }
    let idxs = entity_index_value(v, world)?;
    let mut vals = Vec::with_capacity(idxs.len());
    for i in idxs {
        if world.entities.is_alive(i) {
            vals.push(entity_field_at(i, field, world, sig)?);
        }
    }
    Ok(singleton_or_array(vals))
}

fn sf_deftick(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    if items.len() < 2 {
        return Err("deftick: expected body".into());
    }
    let key: Rc<str> = format!("{:?}", items).into();
    let body = items[1..]
        .iter()
        .map(|form| expand_macros(form, env, ctx, world))
        .collect::<Result<Vec<_>, _>>()?;
    let compiled = body
        .iter()
        .map(|form| rulelower::lower_tick_form(form, env, ctx, world).map(Rc::new))
        .collect::<Vec<_>>();
    let rule = StandingRule {
        key: key.clone(),
        body: body.into(),
        compiled: compiled.into(),
        env: env.clone(),
    };
    match world.standing_rules.iter_mut().find(|r| r.key == key) {
        Some(slot) => *slot = rule,
        None => world.standing_rules.push(rule),
    }
    Ok(Val::Nothing)
}

fn validate_fn_params(params: &[Form]) -> Result<(), String> {
    let mut rest = false;
    for (i, p) in params.iter().enumerate() {
        if matches!(p, Form::Sym(s) if &**s == "&") {
            if rest || i + 1 >= params.len() || i + 2 != params.len() {
                return Err("fn: & must appear once before the final rest parameter".into());
            }
            if !matches!(params.get(i + 1), Some(Form::Sym(_))) {
                return Err("fn: rest parameter must be a symbol".into());
            }
            rest = true;
        }
    }
    Ok(())
}

/// Bind a symbol param vector, honoring a `& rest` tail. Used for macros,
/// whose parameters are still simple symbols and receive unevaluated forms.
fn bind_params<T>(
    mut env: Env,
    params: &[Rc<str>],
    args: &[T],
    to_val: impl Fn(&T) -> Val,
) -> Result<Env, String> {
    for (pi, p) in params.iter().enumerate() {
        if &**p == "&" {
            let Some(rest_name) = params.get(pi + 1) else {
                return Err("params: & must be followed by a rest name".into());
            };
            let rest: Vec<Val> =
                args.get(pi..).unwrap_or(&[]).iter().map(&to_val).collect();
            return Ok(env.bind(rest_name.clone(), Val::arr(rest)));
        }
        if let Some(a) = args.get(pi) {
            env = env.bind(p.clone(), to_val(a));
        }
    }
    Ok(env)
}

fn bind_fn_params(mut env: Env, params: &[Form], args: &[Val]) -> Result<Env, String> {
    let mut pi = 0;
    while pi < params.len() {
        if matches!(&params[pi], Form::Sym(s) if &**s == "&") {
            let Some(Form::Sym(rest_name)) = params.get(pi + 1) else {
                return Err("params: & must be followed by a rest name".into());
            };
            return Ok(env.bind(rest_name.clone(), Val::arr(args.get(pi..).unwrap_or(&[]).to_vec())));
        }
        if let Some(arg) = args.get(pi) {
            match &params[pi] {
                Form::Sym(name) => env = env.bind(name.clone(), arg.clone()),
                pat => {
                    let mut binds = Vec::new();
                    if !match_pattern(pat, arg, &mut binds)? {
                        return Err(format!("params: argument {} did not match pattern", pi));
                    }
                    for (name, value) in binds {
                        env = env.bind(name, value);
                    }
                }
            }
        }
        pi += 1;
    }
    Ok(env)
}

pub fn apply_fn(
    f: Val,
    args: &[Val],
    ctx: &mut Ctx,
    world: &mut World,
    exec_actions: bool,
) -> Result<Val, String> {
    match f {
        Val::Builtin(name) => builtin_with_eval_ctx(&name, args, ctx, world),
        Val::Fn { params, body, env } => {
            let e = bind_fn_params(env.clone(), &params, args)?;
            let saved_ambient = ctx.ambient;
            ctx.ambient = Pose::IDENTITY;
            let mut last = Val::Nothing;
            let mut result = Ok(());
            for form in body.iter() {
                match evaluate(form, &e, ctx, world) {
                    Ok(v) => {
                        if exec_actions {
                            if let Val::Action(a) = &v {
                                if let Err(err) = exec_instant(a, ctx, world) {
                                    result = Err(err);
                                    break;
                                }
                            }
                        }
                        last = v;
                    }
                    Err(err) => {
                        result = Err(err);
                        break;
                    }
                }
            }
            ctx.ambient = saved_ambient;
            let last = result.map(|_| last)?;
            // a loop with no temporal actions is a pure fold (F3): run it now
            if let Val::Action(a) = &last {
                if let ActionV::Loop { names, inits, body, env } = &**a {
                    return run_pure_loop(names, inits.clone(), body, env, ctx, world);
                }
            }
            Ok(last)
        }
        v => Err(format!("cannot apply {:?}", v)),
    }
}

/// Execute a loop synchronously as a pure fold. Temporal actions inside are
/// errors — the scheduler owns time; this path owns only recursion.
fn run_pure_loop(
    names: &[Rc<str>],
    mut cur: Vec<Val>,
    body: &Rc<[Form]>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let mut fuel: u32 = 100_000;
    'outer: loop {
        fuel -= 1;
        if fuel == 0 {
            return Err("pure loop: fuel exhausted".into());
        }
        let mut e = env.clone();
        for (nm, v) in names.iter().zip(cur.iter()) {
            e = e.bind(nm.clone(), v.clone());
        }
        let mut last = Val::Nothing;
        for form in body.iter() {
            let v = evaluate(form, &e, ctx, world)?;
            if let Val::Action(a) = &v {
                match &**a {
                    ActionV::Recur(vals) => {
                        cur = vals.clone();
                        continue 'outer;
                    }
                    ActionV::Nothing => {}
                    other => {
                        return Err(format!("temporal action in pure loop: {:?}", other));
                    }
                }
            }
            last = v;
        }
        return Ok(last);
    }
}

/// Execute an instantaneous action immediately (fn bodies, let bindings).
/// Returns the action's result value (spawn → handles).
pub fn exec_instant(a: &ActionV, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    match a {
        ActionV::Nothing => Ok(Val::Nothing),
        ActionV::Event { name, pos } => {
            world.push_event(StoredEvent { tick: world.tick, name: *name, pos: *pos });
            Ok(Val::Nothing)
        }
        ActionV::Export { scope, name } => {
            let id = scope
                .borrow()
                .get(&**name)
                .copied()
                .ok_or_else(|| format!("export: no cell '{}' in scope", name))?;
            {
                let mut ex = ctx.sig.exports.borrow_mut();
                if !ex.iter().any(|(_, i)| *i == id) {
                    ex.push((name.to_string(), id));
                }
            }
            // same-tick availability
            let v = ctx.sig.cells.borrow().get(&id).map(|(_, v)| v.clone());
            if let Some(v) = v {
                let mut m = (*ctx.sig.channels).clone();
                m.insert(name.to_string(), v);
                ctx.sig.channels = Rc::new(m);
            }
            Ok(Val::Nothing)
        }
        ActionV::BindChannel { name, expr, env } => {
            ctx.sig.bound_channels.borrow_mut().push((name.clone(), expr.clone(), env.clone()));
            let v = evaluate(expr, env, ctx, world)?;
            if !matches!(v, Val::Nothing) {
                let mut m = (*ctx.sig.channels).clone();
                m.insert(name.to_string(), v);
                ctx.sig.channels = Rc::new(m);
            }
            Ok(Val::Nothing)
        }
        ActionV::DefVar { scope, name, init } => {
            let id = world.next_id;
            world.next_id += 1;
            scope.borrow_mut().insert(name.to_string(), id);
            ctx.sig.cells.borrow_mut().insert(id, (name.to_string(), init.clone()));
            Ok(Val::Nothing)
        }
        ActionV::SetVar { scope, name, val } => {
            let id = scope
                .borrow()
                .get(&**name)
                .copied()
                .ok_or_else(|| format!("set!: no cell '{}' in scope", name))?;
            ctx.sig.cells.borrow_mut().insert(id, (name.to_string(), val.clone()));
            Ok(Val::Nothing)
        }
        ActionV::CullHostile => {
            let targets = world
                .entities
                .iter()
                .enumerate()
                .filter_map(|(i, _)| {
                    (world.entities.is_alive(i) && world.sym_field_missing_at(i, "team")).then_some(i)
                })
                .collect::<Vec<_>>();
            for i in targets {
                world.cull_at(i);
            }
            Ok(Val::Nothing)
        }
        ActionV::Cull { target } => {
            if let Some(i) = world.find(*target) {
                world.cull_at(i);
            }
            Ok(Val::Nothing)
        }
        ActionV::Spawn { entities } => {
            let mut handles = Vec::new();
            for spec in entities {
                let dyn_figure = spec.dyn_figure.framed(ctx.ambient);
                let row = world.install_entity(
                    dyn_figure,
                    spec.cache_policy.clone(),
                    spec.dyn_cols.clone(),
                    spec.collider_projector.clone(),
                )?;
                for (field, value) in &spec.sym_fields {
                    world.sym_field_set_at(row, *field, *value);
                }
                for (name, val) in &spec.cols {
                    world.col_set_sym_at(row, *name, *val);
                }
                let handle = world.entity_ref(row);
                handles.push(Val::Handle(handle));
            }
            Ok(Val::arr(handles))
        }
        ActionV::Render { row } => {
            world.render_rows.push(row.clone());
            Ok(Val::Nothing)
        }
        ActionV::Manipulate { targets, query, callback } => {
            let handles: Vec<EntityRef> = match query {
                Some(q) => resolve_query(q, ctx, world)?
                    .into_iter()
                    .map(|i| world.entity_ref(i))
                    .collect(),
                None => targets.clone(),
            };
            for handle in handles {
                if world.find(handle).is_some() {
                    apply_fn(callback.clone(), &[Val::Handle(handle)], ctx, world, true)?;
                }
            }
            Ok(Val::Nothing)
        }
        ActionV::Remat { target, spec } => {
            world.pending_writes.push(PendingWrite::Remat {
                target: *target,
                spec: spec.clone(),
            });
            Ok(Val::Nothing)
        }
        ActionV::ChangeCol { target, col, f } => {
            world.pending_writes.push(PendingWrite::Field {
                target: *target,
                col: *col,
                f: f.clone(),
            });
            Ok(Val::Nothing)
        }
        ActionV::Seq { items, env } => {
            // instantaneous only: run each item now, on the REAL ctx —
            // effects like deferred forks and same-tick channel writes
            // must survive; only scan state and the ambient are scoped
            let saved_scan = ctx.scan.take();
            let saved_ambient = ctx.ambient;
            let mut result = Ok(());
            for f in items.iter() {
                match evaluate(f, env, ctx, world) {
                    Ok(Val::Action(a)) => {
                        if let Err(e) = exec_instant(&a, ctx, world) {
                            result = Err(e);
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            ctx.scan = saved_scan;
            ctx.ambient = saved_ambient;
            result?;
            Ok(Val::Nothing)
        }
        // fork in an instant context defers: the callback's timed work is
        // adopted by the executing task after the instant completes
        ActionV::Fork(inner) => {
            let inner = if ctx.ambient == Pose::IDENTITY {
                inner.clone()
            } else {
                Rc::new(ActionV::InFrame {
                    frame: FrameSpec::Const(ctx.ambient),
                    inner: inner.clone(),
                })
            };
            ctx.deferred.push(inner);
            Ok(Val::Nothing)
        }
        // a const frame is instantaneous: compose the ambient, run inner
        // (callback spawns anchored with ((pose (pos b)) (spawn ...)))
        ActionV::InFrame { frame: FrameSpec::Const(p), inner } => {
            let saved = ctx.ambient;
            ctx.ambient = ctx.ambient.compose(p);
            let r = exec_instant(inner, ctx, world);
            ctx.ambient = saved;
            r?;
            Ok(Val::Nothing)
        }
        // a goto from an instant context (manip callback) just files the
        // request; the machine's guard picks it up on its next step
        ActionV::Goto { cell, label } => {
            let mut cells = ctx.sig.cells.borrow_mut();
            // first request wins until the machine clears it (tree order);
            // bare (goto) files numeric true = "default successor"
            let already_set = matches!(cells.get(cell), Some((_, Val::Kw(_))))
                || matches!(cells.get(cell), Some((_, Val::Num(n))) if *n != 0.0);
            if !already_set {
                let v = match label {
                    Some(l) => Val::Kw(l.clone()),
                    None => Val::Num(1.0),
                };
                cells.insert(*cell, ("#goto".to_string(), v));
            }
            Ok(Val::Nothing)
        }
        ActionV::Wait { .. } => Err("cannot wait in instantaneous context (fn body)".into()),
        other => Err(format!("action not instantaneous: {:?}", other)),
    }
}

fn collect_handles(v: &Val, out: &mut Vec<EntityRef>) -> Result<(), String> {
    match v {
        Val::Handle(id) => {
            out.push(*id);
            Ok(())
        }
        Val::Arr(items) => {
            for i in items.iter() {
                collect_handles(i, out)?;
            }
            Ok(())
        }
        v => Err(format!("expected handle(s), got {:?}", v)),
    }
}

/// Parse one (label body…) state clause of a `states` machine.
fn parse_state_clause(cf: &Form) -> Result<StateClause, String> {
    let Form::List(parts) = cf else {
        return Err("states: expected (:label body…) states".into());
    };
    let Some(Form::Kw(label)) = parts.first() else {
        return Err("states: state head must be a :label keyword".into());
    };
    Ok(StateClause { label: label.clone(), body: parts[1..].to_vec().into() })
}

/// Quasiquote: walk the template, evaluating (unquote e) and splicing
/// (unquote-splicing e) inside lists/vectors.
fn qq(f: &Form, env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Form, String> {
    match f {
        Form::List(items) => {
            if let Some(Form::Sym(s)) = items.first() {
                if &**s == "unquote" {
                    let v = evaluate(&items[1], env, ctx, world)?;
                    return val_to_form(&v);
                }
            }
            Ok(Form::list(qq_seq(items, env, ctx, world)?))
        }
        Form::Vector(items) => Ok(Form::Vector(qq_seq(items, env, ctx, world)?.into())),
        Form::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((qq(k, env, ctx, world)?, qq(v, env, ctx, world)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(|pairs| Form::Map(pairs.into())),
        other => Ok(other.clone()),
    }
}

fn qq_seq(
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Vec<Form>, String> {
    let mut out = Vec::new();
    for it in items {
        if let Form::List(inner) = it {
            if matches!(inner.first(), Some(Form::Sym(s)) if &**s == "unquote-splicing") {
                match evaluate(&inner[1], env, ctx, world)? {
                    Val::Arr(xs) => {
                        for x in xs.iter() {
                            out.push(val_to_form(x)?);
                        }
                    }
                    v => out.push(val_to_form(&v)?),
                }
                continue;
            }
        }
        out.push(qq(it, env, ctx, world)?);
    }
    Ok(out)
}

/// Convert a value back into a form (what unquote splices into templates).
fn val_to_form(v: &Val) -> Result<Form, String> {
    Ok(match v {
        Val::FormV(f) => (**f).clone(),
        Val::Num(n) => Form::Num(*n),
        Val::Kw(k) => Form::Kw(k.clone()),
        Val::Arr(xs) => Form::Vector(
            xs.iter().map(val_to_form).collect::<Result<Vec<_>, _>>()?.into(),
        ),
        Val::Pose(p) if p.theta.is_none() => {
            Form::list(vec![Form::sym("cart"), Form::Num(p.x), Form::Num(p.y)])
        }
        Val::Pose(p) => {
            Form::list(vec![
                Form::sym("+"),
                Form::list(vec![Form::sym("cart"), Form::Num(p.x), Form::Num(p.y)]),
                Form::list(vec![Form::sym("rot"), Form::Num(p.angle_or(0.0))]),
            ])
        }
        other => return Err(format!("cannot embed {:?} in a form template", other)),
    })
}

pub(crate) fn truthy(v: &Val) -> bool {
    match v {
        Val::Num(n) => *n != 0.0,
        Val::Nothing => false,
        _ => false,
    }
}

fn contains_unbound_axis(form: &Form, env: &Env) -> bool {
    match form {
        Form::Sym(s) if &**s == "t" || &**s == "u" => env.lookup(s).is_none(),
        Form::List(items) => {
            if matches!(items.first(), Some(Form::Sym(s)) if &**s == "live") {
                return true;
            }
            items.iter().any(|f| contains_unbound_axis(f, env))
        }
        Form::Vector(items) => items.iter().any(|f| contains_unbound_axis(f, env)),
        Form::Map(kvs) => kvs
            .iter()
            .any(|(k, v)| contains_unbound_axis(k, env) || contains_unbound_axis(v, env)),
        _ => false,
    }
}

fn as_action(v: Val) -> Result<Rc<ActionV>, String> {
    match v {
        Val::Action(a) => Ok(a),
        // nothing is the no-op action: (if p body) with p false, in an
        // action slot, simply does nothing — what the prelude's `when` means
        Val::Nothing => Ok(Rc::new(ActionV::Nothing)),
        v => Err(format!("expected action, got {:?}", v)),
    }
}

fn as_pose(v: Val) -> Result<Pose, String> {
    match v {
        Val::Pose(p) => Ok(p),
        v => Err(format!("expected pose, got {:?}", v)),
    }
}

pub(crate) fn as_dyn_pose(v: Val) -> Result<DynPose, String> {
    match v {
        Val::DynPose(d) => Ok(d),
        Val::Pose(p) => Ok(DynPose::pose_node(Rc::new(DynNode::Const(p)))),
        Val::Fn { .. } => Ok(DynPose::pose_node(Rc::new(DynNode::FnPose(v)))),
        Val::Evolve(ev) => Ok(DynPose::pose_node(Rc::new(DynNode::Evolve(ev)))),
        v => Err(format!("expected dyn pose, got {:?}", v)),
    }
}

fn as_dyn_figure(v: Val) -> Result<DynFigure, String> {
    match v {
        Val::DynFigure(d) => Ok(d),
        Val::DynPose(d) => Ok(DynFigure::pose(d)),
        Val::Figure(f) => Ok(DynFigure::figure_const(f)),
        Val::Pose(p) => Ok(DynFigure::pose_node(Rc::new(DynNode::Const(p)))),
        Val::Fn { .. } => Ok(DynFigure::pose_node(Rc::new(DynNode::FnPose(v)))),
        Val::Evolve(ev) => Ok(DynFigure::pose_node(Rc::new(DynNode::Evolve(ev)))),
        v => Err(format!("expected dyn figure, got {:?}", v)),
    }
}

fn apply_frame_val(frame: Pose, child: Val) -> Result<Val, String> {
    match child {
        Val::Action(a) => Ok(Val::Action(Rc::new(ActionV::InFrame {
            frame: FrameSpec::Const(frame),
            inner: a,
        }))),
        Val::Arr(items) => {
            let out = items
                .iter()
                .map(|c| apply_frame_val(frame, c.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        Val::CurveV(l) => Ok(Val::CurveV(Rc::new(ExtCurve {
            anchor: l.anchor.framed(Rc::new(DynNode::Const(frame))),
            backing: l.backing.clone(),
        }))),
        Val::ElemV(e) => Ok(wrap_elem_fields(
            apply_frame_val(frame, e.figure.clone())?,
            e.fields.iter().cloned().collect(),
        )),
        other => {
            let d = as_dyn_pose(other)?;
            Ok(Val::DynPose(DynPose::pose_node(frame_node(
                Rc::new(DynNode::Const(frame)),
                d.into_node(),
            ))))
        }
    }
}

fn apply_frame_arr(frames: &Val, child: Val) -> Result<Val, String> {
    let Val::Arr(fs) = frames else { unreachable!() };
    let out = fs
        .iter()
        .map(|f| apply_frame_val(as_pose(f.clone())?, child.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Val::arr(out))
}

// ---------------------------------------------------------------------------
// Special forms.

fn sf_match(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    if items.len() < 2 {
        return Err("match: expected subject".into());
    }
    if (items.len() - 2) % 2 != 0 {
        return Err("match: pattern without a result".into());
    }
    let subject = evaluate(&items[1], env, ctx, world)?;
    for pair in items[2..].chunks(2) {
        let mut binds = Vec::new();
        if match_pattern(&pair[0], &subject, &mut binds)? {
            let mut e = env.clone();
            for (name, val) in binds {
                e = e.bind(name, val);
            }
            return evaluate(&pair[1], &e, ctx, world);
        }
    }
    Err("match: no clause matched".into())
}

fn match_pattern(
    pat: &Form,
    subject: &Val,
    binds: &mut Vec<(Rc<str>, Val)>,
) -> Result<bool, String> {
    match pat {
        Form::Sym(s) if &**s == "_" => Ok(true),
        Form::Sym(s) => {
            binds.push((s.clone(), subject.clone()));
            Ok(true)
        }
        Form::Num(_) | Form::Kw(_) | Form::Str(_) | Form::Bool(_) => {
            Ok(literal_pattern_matches(pat, subject))
        }
        Form::List(items) => match items.first() {
            Some(Form::Sym(s)) if &**s == "quote" => {
                if items.len() != 2 {
                    return Err("match: malformed quote pattern".into());
                }
                Ok(matches!(subject, Val::FormV(f) if **f == items[1]))
            }
            Some(Form::Sym(s)) if &**s == "as" => {
                if items.len() != 3 {
                    return Err("match: malformed as pattern".into());
                }
                let Form::Sym(name) = &items[1] else {
                    return Err("match: as name must be a symbol".into());
                };
                if match_pattern(&items[2], subject, binds)? {
                    binds.push((name.clone(), subject.clone()));
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => Err(format!("match: unsupported pattern {}", pat)),
        },
        Form::Vector(parts) => match_seq_pattern(parts, subject, binds),
        Form::Map(kvs) => match_map_pattern(kvs, subject, binds),
    }
}

fn literal_pattern_matches(pat: &Form, subject: &Val) -> bool {
    match (pat, subject) {
        (Form::Num(a), Val::Num(b)) => (a - b).abs() < 1e-9,
        (Form::Kw(a), Val::Kw(b)) | (Form::Str(a), Val::Kw(b)) => a == b,
        (Form::Bool(a), Val::Num(b)) => (*a && *b != 0.0) || (!*a && *b == 0.0),
        (_, Val::FormV(f)) => form_literal_matches(pat, f),
        _ => false,
    }
}

fn form_literal_matches(pat: &Form, subject: &Form) -> bool {
    match (pat, subject) {
        (Form::Num(a), Form::Num(b)) => (a - b).abs() < 1e-9,
        (Form::Kw(a), Form::Kw(b)) | (Form::Str(a), Form::Str(b)) => a == b,
        (Form::Bool(a), Form::Bool(b)) => a == b,
        _ => false,
    }
}

fn match_seq_pattern(
    parts: &[Form],
    subject: &Val,
    binds: &mut Vec<(Rc<str>, Val)>,
) -> Result<bool, String> {
    let xs = match seq_view(subject) {
        Some(xs) => xs,
        None => return Ok(false),
    };
    let rest_i = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| matches!(p, Form::Sym(s) if &**s == "&").then_some(i))
        .collect::<Vec<_>>();
    if rest_i.len() > 1 {
        return Err("match: multiple & in vector pattern".into());
    }
    let Some(rest_i) = rest_i.first().copied() else {
        if xs.len() != parts.len() {
            return Ok(false);
        }
        return match_pairs(parts, &xs, binds);
    };
    let Some(Form::Sym(rest_name)) = parts.get(rest_i + 1) else {
        return Err("match: & must be followed by a rest symbol".into());
    };
    let before = &parts[..rest_i];
    let after = &parts[rest_i + 2..];
    if xs.len() < before.len() + after.len() {
        return Ok(false);
    }
    if !match_pairs(before, &xs[..before.len()], binds)? {
        return Ok(false);
    }
    if !match_pairs(after, &xs[xs.len() - after.len()..], binds)? {
        return Ok(false);
    }
    if &**rest_name != "_" {
        binds.push((rest_name.clone(), Val::Arr(xs.view(before.len(), xs.len() - before.len() - after.len()))));
    }
    Ok(true)
}

fn match_pairs(
    pats: &[Form],
    vals: &[Val],
    binds: &mut Vec<(Rc<str>, Val)>,
) -> Result<bool, String> {
    for (p, v) in pats.iter().zip(vals.iter()) {
        if !match_pattern(p, v, binds)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn match_map_pattern(
    kvs: &[(Form, Form)],
    subject: &Val,
    binds: &mut Vec<(Rc<str>, Val)>,
) -> Result<bool, String> {
    if !matches!(subject, Val::Map(_) | Val::FormV(_)) {
        return Ok(false);
    }
    for (k, p) in kvs {
        let key = map_pattern_key(k)?;
        let Some(v) = get_in(subject, &key) else {
            return Ok(false);
        };
        if !match_pattern(p, &v, binds)? {
            return Ok(false);
        }
    }
    match subject {
        Val::Map(_) => Ok(true),
        Val::FormV(f) => Ok(matches!(&**f, Form::Map(_))),
        _ => Ok(false),
    }
}

fn map_pattern_key(k: &Form) -> Result<Val, String> {
    match k {
        Form::Kw(k) => Ok(Val::Kw(k.clone())),
        Form::Str(s) => Ok(Val::Kw(s.clone())),
        Form::Num(n) => Ok(Val::Num(*n)),
        _ => Err(format!("match: unsupported map pattern key {}", k)),
    }
}

fn sf_let(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let Some(Form::Vector(binds)) = items.get(1) else {
        return Err("let: expected binding vector".into());
    };
    if binds.len() % 2 != 0 {
        return Err("let: odd binding vector".into());
    }
    // Evaluate bindings. If any binding value is an ACTION, defer the whole
    // let to scheduler reach-time (Action::Let) so e.g. spawns execute inside
    // the ambient frame and their handles bind.
    let mut e = env.clone();
    let mut deferred: Vec<(Rc<str>, Val)> = Vec::new();
    let mut any_action = false;
    for c in binds.chunks(2) {
        let v = evaluate(&c[1], &e, ctx, world)?;
        match &c[0] {
            Form::Sym(name) => {
                if matches!(v, Val::Action(_)) {
                    any_action = true;
                }
                e = e.bind(name.clone(), v.clone());
                deferred.push((name.clone(), v));
            }
            // {:keys [x y]} destructuring over a map value
            Form::Map(kvs) => {
                for (k, kv) in kvs.iter() {
                    if matches!(k, Form::Kw(kw) if &**kw == "keys") {
                        let Form::Vector(names) = kv else {
                            return Err("let: :keys expects a vector".into());
                        };
                        for nm in names.iter() {
                            let Form::Sym(nm) = nm else {
                                return Err("let: bad :keys name".into());
                            };
                            let field = map_get(&v, nm).unwrap_or(Val::Nothing);
                            e = e.bind(nm.clone(), field.clone());
                            deferred.push((nm.clone(), field));
                        }
                    }
                }
            }
            _ => return Err("let: bad binding form".into()),
        }
    }
    if any_action {
        return Ok(Val::Action(Rc::new(ActionV::Let {
            binds: deferred,
            body: items[2..].to_vec().into(),
            env: env.clone(),
        })));
    }
    match items.len() - 2 {
        1 => evaluate(&items[2], &e, ctx, world),
        _ => Ok(Val::Action(Rc::new(ActionV::Seq {
            items: items[2..].to_vec().into(),
            env: e,
        }))),
    }
}

fn sf_loop(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let Some(Form::Vector(binds)) = items.get(1) else {
        return Err("loop: expected binding vector".into());
    };
    if binds.len() % 2 != 0 {
        return Err("loop: odd binding vector".into());
    }
    let mut names = Vec::new();
    let mut inits = Vec::new();
    for c in binds.chunks(2) {
        let Form::Sym(name) = &c[0] else {
            return Err("loop: bad binding name".into());
        };
        names.push(name.clone());
        inits.push(evaluate(&c[1], env, ctx, world)?);
    }
    Ok(Val::Action(Rc::new(ActionV::Loop {
        names,
        inits,
        body: items[2..].to_vec().into(),
        env: env.clone(),
    })))
}

fn sf_vel(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    if !(items.len() == 2 || items.len() == 3) {
        return Err("vel: expected (vel c[..]), (vel c[..] child), or (vel c[..] {fields...})".into());
    }
    let Some(Form::List(arg)) = items.get(1) else {
        return Err("vel: expected a coordinate argument".into());
    };
    let (polar, comps) = match arg.first() {
        Some(Form::Sym(s)) if &**s == "cart" => (false, &arg[1..]),
        Some(Form::Sym(s)) if &**s == "polar" => (true, &arg[1..]),
        _ => return Err("vel: expected c[..] or p[..]".into()),
    };
    if comps.len() != 2 {
        return Err("vel: expected two components".into());
    }
    let node = Rc::new(DynNode::Vel {
        a: expand_macros(&comps[0], env, ctx, world)?,
        b: expand_macros(&comps[1], env, ctx, world)?,
        polar,
        env: env.clone(),
        programs: std::cell::OnceCell::new(),
    });
    match items.get(2) {
        None => Ok(Val::DynPose(DynPose::pose_node(node))),
        Some(Form::Map(_)) => Ok(wrap_elem_fields(
            Val::DynPose(DynPose::pose_node(node)),
            elem_fields_from_form_map(&items[2], env, ctx, world)?,
        )),
        Some(cf) => {
            // trailing-child sugar on dyn constructors
            let child = evaluate(cf, env, ctx, world)?;
            match child {
                Val::Map(_) => Ok(wrap_elem_fields(
                    Val::DynPose(DynPose::pose_node(node)),
                    elem_fields_from_val_map(&child, "vel")?,
                )),
                Val::Arr(_) => {
                    // one vel frame carrying an array of children: product
                    let Val::Arr(kids) = child else { unreachable!() };
                    let out = kids
                        .iter()
                        .map(|k| {
                            Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Frame(
                                node.clone(),
                                as_dyn_pose(k.clone())?.into_node(),
                            )))))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok(Val::arr(out))
                }
                Val::ElemV(e) => Ok(wrap_elem_fields(
                    Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Frame(
                        node,
                        as_dyn_pose(e.figure.clone())?.into_node(),
                    )))),
                    e.fields.iter().cloned().collect(),
                )),
                other => Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Frame(
                    node,
                    as_dyn_pose(other)?.into_node(),
                ))))),
            }
        }
    }
}

fn sf_curve(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    // (curve shape? opts): shape is a dyn over (t, u); opts is a map.
    let (shape, opts_idx) = match items.get(1) {
        Some(Form::Map(_)) => (None, 1),
        Some(_) => {
            let sv = evaluate(&items[1], env, ctx, world)?;
            (Some(as_dyn_pose(sv)?), 2)
        }
        None => return Err("curve: expected options".into()),
    };
    // evaluate options, keeping signal-valued entries (contain t) as fields
    let mut seeds = Vec::new();
    let opts = match items.get(opts_idx) {
        Some(Form::Map(kvs)) => {
            let mut pairs = Vec::new();
            for (k, v) in kvs.iter() {
                let kv = evaluate(k, env, ctx, world)?;
                let key = match &kv {
                    Val::Kw(kw) => Some(kw.clone()),
                    _ => None,
                };
                if matches!(&kv, Val::Kw(kw) if &**kw == "fill") && !matches!(v, Form::Num(_)) {
                    let dyn_num = DynNum::num_expr(v.clone(), env.clone());
                    if let Some(key) = key {
                        seeds.push((key, FieldSeed::Dyn(dyn_num)));
                    }
                    pairs.push((kv, Val::Nothing));
                } else if contains_t(v) {
                    let dyn_num = DynNum::num_expr(v.clone(), env.clone());
                    if let Some(key) = key {
                        seeds.push((key, FieldSeed::Dyn(dyn_num)));
                    }
                    pairs.push((kv, Val::Nothing));
                } else {
                    let vv = evaluate(v, env, ctx, world)?;
                    if let Some(key) = key {
                        match &vv {
                            Val::Nothing => {}
                            Val::Num(n) => seeds.push((key, FieldSeed::Num(*n))),
                            Val::Kw(s) => seeds.push((key, FieldSeed::Sym(s.clone()))),
                            Val::DynLike(d) => seeds.push((key, FieldSeed::Dyn(as_dyn_num(d)?))),
                            _ => {}
                        }
                    }
                    pairs.push((kv, vv));
                }
            }
            Val::Map(Rc::new(pairs))
        }
        Some(m) => {
            let opts = evaluate(m, env, ctx, world)?;
            if let Val::Map(kvs) = &opts {
                for (k, v) in kvs.iter() {
                    let Val::Kw(key) = k else { continue };
                    match v {
                        Val::Nothing => {}
                        Val::Num(n) => seeds.push((key.clone(), FieldSeed::Num(*n))),
                        Val::Kw(s) => seeds.push((key.clone(), FieldSeed::Sym(s.clone()))),
                        Val::DynLike(d) => seeds.push((key.clone(), FieldSeed::Dyn(as_dyn_num(d)?))),
                        _ => {}
                    }
                }
            }
            opts
        }
        None => Val::Map(Rc::new(vec![])),
    };
    let getf = |key: &str, dflt: f64| -> f64 {
        map_get(&opts, key).and_then(|v| v.num().ok()).unwrap_or(dflt)
    };
    let curve = Val::CurveV(Rc::new(ExtCurve {
        anchor: DynPose::pose_node(Rc::new(DynNode::Const(Pose::IDENTITY))),
        backing: CurveBacking::Parametric {
            curve: ParametricCurve {
                eval: shape.map(CurveEval::Expr).unwrap_or(CurveEval::Straight),
                domain: CurveDomain::Range { min: 0.0, max: getf("u-max", 10.0) },
            },
        },
    }));
    Ok(wrap_elem_fields(curve, seeds))
}

/// Stateful scan expression sites. State keyed by (base, site index); the
/// site counter is stable for a fixed expression tree.
/// Recursively expand card-macro calls in a form before it is captured
/// into a dyn expression. Captured forms are re-evaluated per tick, so
/// expanding at capture keeps macro expansion out of the hot loop and —
/// the real point — makes expansion shapes (sited evolves) visible to
/// the spawn-time scan-site walk and the lowerer
/// (evolve-reexpression-design.md).
///
/// Head resolution mirrors evaluate_list_inner's macro dispatch: a head
/// is a macro call only when unbound in the env / defs, not a '$'
/// channel, and not shadowed by an enclosing let/loop/fn binder in the
/// form itself. quote/quasiquote bodies are left alone (they are data);
/// an expansion is re-expanded until no macro heads remain.
pub(crate) fn expand_macros(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Form, String> {
    if ctx.macros.is_empty() {
        return Ok(form.clone());
    }
    let mut bound = std::collections::HashSet::new();
    expand_macros_in(form, env, ctx, world, &mut bound)
}

fn expand_macros_in(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
    bound: &mut std::collections::HashSet<Rc<str>>,
) -> Result<Form, String> {
    match form {
        Form::List(items) => expand_macros_list(items, env, ctx, world, bound),
        Form::Vector(items) => Ok(Form::Vector(
            items
                .iter()
                .map(|f| expand_macros_in(f, env, ctx, world, bound))
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        )),
        Form::Map(kvs) => Ok(Form::Map(
            kvs.iter()
                .map(|(k, v)| {
                    Ok((
                        expand_macros_in(k, env, ctx, world, bound)?,
                        expand_macros_in(v, env, ctx, world, bound)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?
                .into(),
        )),
        atom => Ok(atom.clone()),
    }
}

fn expand_macros_list(
    items: &Rc<[Form]>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
    bound: &mut std::collections::HashSet<Rc<str>>,
) -> Result<Form, String> {
    let recurse_all = |ctx: &mut Ctx,
                       world: &mut World,
                       bound: &mut std::collections::HashSet<Rc<str>>|
     -> Result<Form, String> {
        Ok(Form::List(
            items
                .iter()
                .map(|f| expand_macros_in(f, env, ctx, world, bound))
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        ))
    };
    let Some(Form::Sym(head)) = items.first() else {
        return recurse_all(ctx, world, bound);
    };
    match head.as_ref() {
        // data, not code: never expand inside
        "quote" | "quasiquote" => Ok(Form::List(items.to_vec().into())),
        "fn" => {
            let mut out = Vec::with_capacity(items.len());
            out.push(items[0].clone());
            if let Some(params) = items.get(1) {
                out.push(params.clone());
                let names = macro_binding_names(params);
                let inserted: Vec<_> =
                    names.into_iter().filter(|n| bound.insert(n.clone())).collect();
                let body = items[2..]
                    .iter()
                    .map(|f| expand_macros_in(f, env, ctx, world, bound))
                    .collect::<Result<Vec<_>, _>>();
                for name in inserted {
                    bound.remove(&name);
                }
                out.extend(body?);
            }
            Ok(Form::List(out.into()))
        }
        "let" | "loop" if matches!(items.get(1), Some(Form::Vector(_))) => {
            let Some(Form::Vector(binds)) = items.get(1) else { unreachable!() };
            // let binds sequentially; loop's names scope over its own
            // init exprs conservatively too (a shadowing macro name in a
            // loop init is vanishingly unlikely, and conservative here
            // only means a call is left unexpanded for the evaluator)
            let mut local = bound.clone();
            let mut new_binds = Vec::with_capacity(binds.len());
            for pair in binds.chunks(2) {
                if pair.len() == 2 {
                    new_binds.push(pair[0].clone());
                    new_binds.push(expand_macros_in(&pair[1], env, ctx, world, &mut local)?);
                    for name in macro_binding_names(&pair[0]) {
                        local.insert(name);
                    }
                } else {
                    new_binds.push(pair[0].clone());
                }
            }
            let mut out = vec![items[0].clone(), Form::Vector(new_binds.into())];
            for f in &items[2..] {
                out.push(expand_macros_in(f, env, ctx, world, &mut local)?);
            }
            Ok(Form::List(out.into()))
        }
        name => {
            if !bound.contains(name)
                && env.lookup(name).is_none()
                && !ctx.sig.defs.contains_key(name)
                && !name.starts_with('$')
            {
                if let Some(mac) = ctx.macros.clone().get(name) {
                    let menv = bind_params(Env::empty(), &mac.params, &items[1..], |f| {
                        Val::FormV(Rc::new(f.clone()))
                    })?;
                    let mut expansion = Val::Nothing;
                    for f in mac.body.iter() {
                        expansion = evaluate(f, &menv, ctx, world)?;
                    }
                    let form = val_to_form(&expansion)?;
                    return expand_macros_in(&form, env, ctx, world, bound);
                }
            }
            recurse_all(ctx, world, bound)
        }
    }
}

fn macro_binding_names(form: &Form) -> Vec<Rc<str>> {
    match form {
        Form::Sym(s) if &**s != "&" => vec![s.clone()],
        Form::Vector(items) => items.iter().flat_map(macro_binding_names).collect(),
        Form::Map(kvs) => kvs
            .iter()
            .flat_map(|(k, v)| {
                if matches!(k, Form::Kw(kw) if &**kw == "keys") {
                    macro_binding_names(v)
                } else {
                    Vec::new()
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A sited evolve: (evolve init step) inside a per-tick re-evaluated dyn
/// expression. State lives at the ScanSite key; the form evaluates to the
/// settled state value — the ambient clock is the enclosing slot's clock,
/// so "the dyn sampled at the ambient tick" IS the settled value.
///
/// Counter discipline: every evaluation consumes exactly
/// 1 + sites(init) + sites(step) indices to match collect_scan_sites'
/// static walk, fast-forwarding over skipped regions. Sites reachable
/// only through non-literal step fns (a def'd closure) are invisible to
/// the static walk — same limitation as the other scan builtins.
fn sf_sited_evolve(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let scan = ctx.scan.clone().expect("sited evolve requires a scan context");
    let (site, advance, dt) = {
        let mut io = scan.borrow_mut();
        let base = io
            .dense_base
            .ok_or("evolve: scan context missing stable lowered base id")?;
        let site = MotionStateKey::ScanSite { base, index: io.counter as u32 };
        io.counter += 1;
        (site, io.advance, io.dt)
    };
    let stored = {
        let io = scan.borrow();
        io.readers
            .as_ref()
            .and_then(|readers| readers.vals(site))
            .or_else(|| match io.state.get(&site) {
                Some(Cell::V(cell)) => Some(cell.clone()),
                _ => None,
            })
    };
    let cell = match stored {
        Some(cell) => {
            scan.borrow_mut().counter += form_site_count(&items[1]) as usize;
            cell
        }
        None => EvolveCell { state: evaluate(&items[1], env, ctx, world)?, tick: 0 },
    };
    if !advance {
        scan.borrow_mut().counter += form_site_count(&items[2]) as usize;
        return Ok(cell.state);
    }
    let step = evaluate(&items[2], env, ctx, world)?;
    if !matches!(step, Val::Fn { .. } | Val::Builtin(_)) {
        return Err(format!("evolve: step must be callable, got {:?}", step));
    }
    let step_ctx = evolve_step_ctx(cell.tick, dt);
    let state = apply_fn(step, &[cell.state, step_ctx], ctx, world, false)?;
    let cell = EvolveCell { state, tick: cell.tick + 1 };
    let mut io = scan.borrow_mut();
    if io.mirror_legacy {
        io.state.insert(site, Cell::V(cell.clone()));
    }
    io.val_writes.push((site, cell.clone()));
    Ok(cell.state)
}

/// (stages (stage dur sig) (until pred sig) (forever sig-or-fn) ...)
fn sf_stages(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let mut segs = Vec::new();
    for seg in &items[1..] {
        let Form::List(parts) = seg else {
            return Err("stages: expected (stage ...) clauses".into());
        };
        let head = match parts.first() {
            Some(Form::Sym(h)) => h.to_string(),
            _ => return Err("stages: bad clause head".into()),
        };
        let (term, sig_form) = match head.as_str() {
            "stage" => {
                let dur = evaluate(&parts[1], env, ctx, world)?.num()?;
                (StageTerm::Dur(dur), &parts[2])
            }
            "until" => (
                StageTerm::Until(expand_macros(&parts[1], env, ctx, world)?, env.clone()),
                &parts[2],
            ),
            "forever" => (StageTerm::Forever, &parts[1]),
            h => return Err(format!("stages: unknown clause '{}'", h)),
        };
        let v = evaluate(sig_form, env, ctx, world)?;
        let (make, exit_slot) = match v {
            Val::Fn { .. } => {
                if segs.is_empty() {
                    return Err("stages: first segment cannot be lazy (no exit yet)".into());
                }
                let slot = Rc::new(StageExitSlot);
                let lowered = apply_fn(
                    v,
                    &[Val::StageExit(slot.clone())],
                    ctx,
                    world,
                    false,
                )?;
                (StageMake::Ready(as_dyn_pose(lowered)?), Some(slot))
            }
            other => (StageMake::Ready(as_dyn_pose(other)?), None),
        };
        segs.push(StageSeg { term, make, exit_slot });
    }
    if segs.is_empty() {
        return Err("stages: no segments".into());
    }
    Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Stages { segs }))))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::{read_all, read_one};

    fn ev(src: &str) -> Val {
        let f = read_one(src).unwrap();
        evaluate(&f, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap()
    }

    fn ev_prelude(src: &str) -> Val {
        let expanded = crate::edn::expand_src("").unwrap();
        let forms = read_all(&expanded).unwrap();
        let card = load_card(&forms).unwrap();
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let f = read_one(src).unwrap();
        evaluate(&f, &Env::empty(), &mut ctx, &mut World::default()).unwrap()
    }

    fn ev_err(src: &str) -> String {
        let f = read_one(src).unwrap();
        evaluate(&f, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap_err()
    }

    fn predicate(src: &str) -> Val {
        let form = read_one(src).unwrap();
        evaluate(&form, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap()
    }

    #[test]
    fn row_predicate_recognizes_keyword_field_conjunctions() {
        let ctx = Ctx::default();
        for src in [
            "(fn [e] (* (= e.render :beam) (= e.kind :curve)))",
            "(fn [e] (* (= :touhou-sprite e.render) (= :point e.kind)))",
            "(fn [e] (= e.team :enemy))",
        ] {
            assert!(row_predicate(&predicate(src), &ctx).is_some(), "{src}");
        }
    }

    #[test]
    fn row_predicate_recognizes_numeric_comparisons() {
        fn intrinsic(form: &Form) -> Form {
            match form {
                Form::Sym(name) if name.as_ref() == "value-or" => Form::sym("%value-or"),
                Form::List(items) => Form::List(items.iter().map(intrinsic).collect()),
                Form::Vector(items) => Form::Vector(items.iter().map(intrinsic).collect()),
                other => other.clone(),
            }
        }
        let ctx = Ctx::default();
        for src in [
            "(fn [e] (* (= e.team :enemy) (<= (value-or (:hp e) 1) 0)))",
            "(fn [e] (* (= e.team :player-body) (<= (value-or (:lives e) 1) 0) (< (value-or (:game-over-fired e) 0) 1)))",
            "(fn [e] (* (= e.kind :curve) (> (:t e) (+ (value-or (:warn e) 0) (value-or (:active e) inf)))))",
            "(fn [e] (= (value-or (:hp e) 0) 1))",
        ] {
            let form = intrinsic(&read_one(src).unwrap());
            let q = evaluate(&form, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap();
            assert!(row_predicate(&q, &ctx).is_some(), "{src}");
        }
    }

    #[test]
    fn row_predicate_rejects_unsafe_numeric_shapes_and_shadowing() {
        let ctx = Ctx::default();
        for src in [
            "(fn [e] (<= (:hp e) 0))",
            "(fn [e] (< (+ 1 2 3) 4))",
        ] {
            assert!(row_predicate(&predicate(src), &ctx).is_none(), "{src}");
        }
        for (name, src) in [
            ("+", "(fn [e] (< (+ (:t e) 1) 4))"),
            ("inf", "(fn [e] (< (:t e) inf))"),
            ("<=", "(fn [e] (<= (:t e) 1))"),
        ] {
            let form = read_one(src).unwrap();
            let env = Env::empty().bind(name.into(), Val::Num(0.0));
            let q = evaluate(&form, &env, &mut Ctx::default(), &mut World::default()).unwrap();
            assert!(row_predicate(&q, &ctx).is_none(), "{src}");
        }
    }

    #[test]
    fn row_predicate_rejects_unsafe_shapes_and_shadowing() {
        let ctx = Ctx::default();
        for src in [
            "(fn [e & more] (= e.team :enemy))",
            "(fn [e] (= e.team 1))",
            "(fn [e] (= e.pos :origin))",
            "(fn [e] (* (= e.team :enemy) (< e.hp 1)))",
        ] {
            assert!(row_predicate(&predicate(src), &ctx).is_none(), "{src}");
        }

        let shadowed = Val::Fn {
            params: vec![Form::sym("e")].into(),
            body: vec![read_one("(= e.team :enemy)").unwrap()].into(),
            env: Env::empty().bind("=".into(), Val::Num(1.0)),
        };
        assert!(row_predicate(&shadowed, &ctx).is_none());

        let mut defs_ctx = Ctx::default();
        Rc::make_mut(&mut defs_ctx.sig.defs).insert("=".into(), Form::Num(1.0));
        assert!(row_predicate(&predicate("(fn [e] (= e.team :enemy))"), &defs_ctx).is_none());
    }

    #[test]
    fn row_predicate_matches_spawned_world_fallback() {
        const CARD: &str = r#"
(defpattern p []
  (par
    (spawn (pose c[0 0]) {:team :enemy :render :sprite})
    (spawn (pose c[1 0]) {:team :friend :render :sprite})
    (spawn ((pose c[0 0]) (curve {:u-max 1})) {:team :enemy :render :beam})))
"#;
        let mut sim = crate::sim::Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let mut ctx = Ctx::default();
        let q = predicate("(fn [e] (* (= e.team :enemy) (= e.kind :point)))");
        let candidates = sim.world.entities.iter().enumerate()
            .filter_map(|(i, _)| sim.world.entities.is_alive(i).then_some(i))
            .collect::<Vec<_>>();
        let expected = resolve_predicate_query_fallback(&q, &mut ctx, &mut sim.world, &candidates).unwrap();
        let actual = resolve_predicate_query(&q, &mut ctx, &mut sim.world).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn defcollider_elaborates_simple_body_to_projector_algebra() {
        let forms = read_all(
            "(defcollider hitbox-collider [entity context]\n  \
               (circle-collider {:layer :damage :r entity.hitbox}))"
        ).unwrap();
        let card = load_card(&forms).unwrap();
        let form = card.defs.get("hitbox-collider").unwrap();
        let val = evaluate(form, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap();
        let Val::ColliderProjector(projector) = val else {
            panic!("defcollider did not evaluate to a collider projector");
        };
        assert_eq!(projector.len(), 1);
        assert!(
            matches!(projector[0].expr, ColliderProjectorExpr::Circle { .. }),
            "simple defcollider should elaborate to the primitive projector algebra"
        );
    }

    #[test]
    fn collider_body_vector_elaborates_to_plural_projector_algebra() {
        let val = ev(
            "(collider :pose [entity context]\n  \
               [(circle-collider {:layer :damage :r entity.hitbox})\n   \
                (circle-collider {:layer :graze :r 0.35})])"
        );
        let Val::ColliderProjector(projectors) = val else {
            panic!("collider did not evaluate to collider projectors");
        };
        assert_eq!(projectors.len(), 2);
        assert!(matches!(projectors[0].expr, ColliderProjectorExpr::Circle { .. }));
        assert!(matches!(projectors[1].expr, ColliderProjectorExpr::Stable(_)));
    }

    #[test]
    fn arithmetic_and_math_macro() {
        let f = read_one("m\"0.2*(i+1)*(i+2)\"").unwrap();
        let env = Env::empty().bind("i".into(), Val::Num(3.0));
        let v = evaluate(&f, &env, &mut Ctx::default(), &mut World::default()).unwrap();
        assert!((v.num().unwrap() - 0.2 * 4.0 * 5.0).abs() < 1e-9);

        let f = read_one("-x").unwrap();
        let env = Env::empty().bind("x".into(), Val::Num(3.0));
        let v = evaluate(&f, &env, &mut Ctx::default(), &mut World::default()).unwrap();
        assert_eq!(v.num().unwrap(), -3.0);
    }

    #[test]
    fn variadic_arithmetic() {
        assert_eq!(ev("(+ 1 2 3)").num().unwrap(), 6.0);
        assert_eq!(ev("(- 10 1 2)").num().unwrap(), 7.0);
        assert_eq!(ev("(- 4)").num().unwrap(), -4.0);
        assert_eq!(ev("(= :point :curve)").num().unwrap(), 0.0);
        assert_eq!(ev("(= :point :point)").num().unwrap(), 1.0);
    }

    #[test]
    fn cyclic_nth_iota_stutter() {
        assert_eq!(ev("(nth [10 20 30] 7)").num().unwrap(), 20.0);
        assert_eq!(ev("(nth [10 20 30] -1)").num().unwrap(), 30.0);
        let Val::Arr(items) = ev("(stutter 2 [1 2])") else { panic!() };
        let got: Vec<f64> = items.iter().map(|v| v.num().unwrap()).collect();
        assert_eq!(got, vec![1.0, 1.0, 2.0, 2.0]);
        // nth broadcast (200's :color axis targeting)
        let Val::Arr(items) = ev("(nth [10 20 30] (iota 4))") else { panic!() };
        assert_eq!(items.len(), 4);
        assert_eq!(items[3].num().unwrap(), 10.0);
    }

    #[test]
    fn fn_map_and_easings() {
        assert_eq!(ev("((fn [x] (* x x)) 5)").num().unwrap(), 25.0);
        assert_eq!(ev("((fn [[x y]] (+ x y)) [2 3])").num().unwrap(), 5.0);
        let Val::Arr(items) = ev_prelude("(map (fn [x] (inc x)) [1 2 3])") else { panic!() };
        assert_eq!(items[2].num().unwrap(), 4.0);
        assert!((ev("(eoutsine 1)").num().unwrap() - 1.0).abs() < 1e-9);
        let v = ev("(lerpsmooth eoutsine 0 4 2 0 480)").num().unwrap();
        assert!((v - 480.0 * (0.5f64 * std::f64::consts::FRAC_PI_2).sin()).abs() < 1e-9);
    }

    #[test]
    fn form_vocabulary() {
        // seq ops see a form list as a sequence of subforms
        assert_eq!(ev("(count `(:a {:x 1} b))").num().unwrap(), 3.0);
        assert!(matches!(ev("(form-type (first `(:a b)))"), Val::Kw(k) if &*k == "kw"));
        assert!(matches!(ev("(form-name (first `(:a b)))"), Val::Kw(s) if &*s == "a"));
        assert_eq!(ev("(count (rest `(:a b c)))").num().unwrap(), 2.0);
        assert_eq!(ev("(count (drop 2 `(a b c)))").num().unwrap(), 1.0);
        assert_eq!(ev("(count (take 2 [1 2 3]))").num().unwrap(), 2.0);
        assert_eq!(ev("(count (concat [1] `(a b)))").num().unwrap(), 3.0);
        // get is total: map forms give the value SUBFORM; misses give nothing
        let Val::FormV(f) = ev("(get `{:until (<= x 2)} :until)") else { panic!() };
        assert!(matches!(&*f, Form::List(_)));
        assert!(matches!(ev("(get `{:a 1} :b)"), Val::Nothing));
        assert!(matches!(ev("(get `(no map) :b)"), Val::Nothing));
        assert_eq!(ev("(nothing? (get `{:a 1} :b))").num().unwrap(), 1.0);
        // nth indexes form lists too (cyclic, like arrays)
        assert!(matches!(ev("(form-name (nth `(a b c) 1))"), Val::Kw(s) if &*s == "b"));
        // filter over a form list keeps subform values
        assert_eq!(
            ev("(count (filter (fn [f] (= (form-type f) :map)) `(a {:x 1} b)))")
                .num()
                .unwrap(),
            1.0
        );
    }

    #[test]
    fn seq_views_share_backing() {
        let subject = ev("[1 2 3 4]");
        let Val::Arr(orig) = &subject else { panic!() };
        let orig_ptr = orig.backing_ptr();
        let tick_rate = TickTiming::default().rate();

        let rest = builtin_with_tick_rate("rest", &[subject.clone()], tick_rate).unwrap();
        let Val::Arr(rest_seq) = &rest else { panic!() };
        assert_eq!(rest_seq.len(), 3);
        assert!(matches!(&rest_seq[0], Val::Num(n) if (*n - 2.0).abs() < 1e-9));
        assert_eq!(rest_seq.backing_ptr(), orig_ptr);

        let rest_rest = builtin_with_tick_rate("rest", &[rest.clone()], tick_rate).unwrap();
        let Val::Arr(rest_rest_seq) = &rest_rest else { panic!() };
        assert_eq!(rest_rest_seq.backing_ptr(), orig_ptr);
        assert_eq!(rest_rest_seq.len(), 2);
        assert!(matches!(&rest_rest_seq[0], Val::Num(n) if (*n - 3.0).abs() < 1e-9));

        let taken = builtin_with_tick_rate("take", &[Val::Num(2.0), rest.clone()], tick_rate).unwrap();
        let Val::Arr(taken_seq) = &taken else { panic!() };
        assert_eq!(taken_seq.backing_ptr(), orig_ptr);
        assert_eq!(taken_seq.len(), 2);

        let dropped = builtin_with_tick_rate("drop", &[Val::Num(2.0), subject.clone()], tick_rate).unwrap();
        let Val::Arr(dropped_seq) = &dropped else { panic!() };
        assert_eq!(dropped_seq.backing_ptr(), orig_ptr);
        assert_eq!(dropped_seq.len(), 2);

        let Form::Vector(pat) = read_one("[a & r]").unwrap() else { panic!() };
        let mut binds = Vec::new();
        assert!(match_seq_pattern(&pat, &subject, &mut binds).unwrap());
        let r = binds
            .iter()
            .find_map(|(name, val)| (&**name == "r").then_some(val))
            .unwrap();
        let Val::Arr(r_seq) = r else { panic!() };
        assert_eq!(r_seq.backing_ptr(), orig_ptr);
        assert_eq!(r_seq.len(), 3);
    }

    #[test]
    fn seq_view_language_regressions() {
        let Val::Arr(items) = ev("(take 2 (drop 1 (rest [0 10 20 30 40])))") else {
            panic!()
        };
        let got: Vec<f64> = items.iter().map(|v| v.num().unwrap()).collect();
        assert_eq!(got, vec![20.0, 30.0]);
        assert_eq!(ev("(nth (rest [10 20 30]) 5)").num().unwrap(), 30.0);
        assert_eq!(ev("(count (drop 2 [1 2 3]))").num().unwrap(), 1.0);
        assert_eq!(
            ev("(match (rest [0 1 2 3 4]) [a & mid 4] (nth mid 1))")
                .num()
                .unwrap(),
            3.0
        );
    }

    #[test]
    fn match_special() {
        assert_eq!(ev("(match 2 1 :one n (+ n 3) _ 0)").num().unwrap(), 5.0);
        assert_eq!(ev("(match :miss :hit 1 _ 2)").num().unwrap(), 2.0);
        assert_eq!(ev("(match [1 2] [1 x] x)").num().unwrap(), 2.0);
        assert_eq!(ev("(match [1 2 3] [a & r] (count r))").num().unwrap(), 2.0);
        assert_eq!(ev("(match [1 2 3 4] [a & mid 4] (count mid))").num().unwrap(), 2.0);
        assert_eq!(ev("(match {:x 1} {:hp n} n {} 7)").num().unwrap(), 7.0);
        assert_eq!(ev("(match {:hp 9} {:hp n} n {} 7)").num().unwrap(), 9.0);
        assert_eq!(ev("(match [1 2] (as whole [a b]) (count whole))").num().unwrap(), 2.0);
        assert_eq!(ev("(match 'finally 'finally 1 _ 0)").num().unwrap(), 1.0);
        let Val::FormV(f) = ev("(quote abc)") else { panic!() };
        assert!(matches!(&*f, Form::Sym(s) if &**s == "abc"));
        assert_eq!(ev("(match 2 _ 1 n 2)").num().unwrap(), 1.0);
        assert_eq!(ev_err("(match :x :y 1)"), "match: no clause matched");

        let Val::FormV(f) = ev("(match `(:a {:hp 10} (fire)) [label (as opts {}) & rest] (get opts :hp))") else { panic!() };
        assert!(matches!(&*f, Form::Num(n) if (*n - 10.0).abs() < 1e-9));
    }

    #[test]
    fn circle_returns_poses() {
        let Val::Arr(items) = ev_prelude("(circle 4)") else { panic!() };
        assert_eq!(items.len(), 4);
        let Val::Pose(p) = &items[1] else { panic!() };
        assert!((p.angle_or(0.0) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn frame_application_builds_dyn() {
        let Val::DynPose(d) = ev("((rot 90) (linear c[4 0]))") else {
            panic!("expected dyn")
        };
        let st = MotionState::default();
        let p = dyn_pose(&d, 1.0, &st, &SigEnv::default()).unwrap();
        assert!(p.x.abs() < 1e-9 && (p.y - 4.0).abs() < 1e-9, "rotated 90°: {:?}", p);
    }

    #[test]
    fn closed_polar_dyn() {
        let Val::DynPose(d) = ev("(polar m\"2*t\" m\"20*t\")") else { panic!() };
        let st = MotionState::default();
        let p = dyn_pose(&d, 1.0, &st, &SigEnv::default()).unwrap();
        let (ex, ey) = (2.0 * (20f64).to_radians().cos(), 2.0 * (20f64).to_radians().sin());
        assert!((p.x - ex).abs() < 1e-9 && (p.y - ey).abs() < 1e-9, "{:?}", p);
        assert!(matches!(ev("p[2 90]"), Val::Pose(p) if p.theta.is_none()));
    }

    #[test]
    fn vel_integrates() {
        let Val::DynPose(d) = ev("(vel c[4 0])") else { panic!() };
        let mut st = MotionState::default();
        let dt = 1.0 / DEFAULT_TICK_RATE;
        let sig = SigEnv::default();
        for k in 0..120 {
            step_motion(d.node(), k as f64 * dt, dt, &mut st, &sig).unwrap();
        }
        let p = dyn_pose(&d, 1.0, &st, &sig).unwrap();
        assert!((p.x - 4.0).abs() < 1e-6, "integrated x: {}", p.x);
        assert!(is_scanned(d.node()));
    }

    #[test]
    fn motion_state_schema_collects_node_slots() {
        let Val::DynPose(d) = ev("(vel c[4 0])") else { panic!() };
        let schema = collect_motion_state_schema(&DynFigure::pose(d));
        assert_eq!(schema.n2_keys.len(), 1, "vel has one numeric state slot");
        assert_eq!(schema.dyn_keys.len(), 0);
    }

    #[test]
    fn motion_state_schema_does_not_recognize_scan_sites_by_name() {
        let Val::DynPose(d) = ev("(vel (cart m\"smooth(0.5, 4)\" m\"slew(10, 0, 90)\"))") else { panic!() };
        let schema = collect_motion_state_schema(&DynFigure::pose(d));
        assert_eq!(schema.n2_keys.len(), 1, "vel numeric state slot");
        assert!(schema.val_keys.is_empty(), "unexpanded names are not scan sites");
    }

    #[test]
    fn motion_state_schema_collects_lazy_stage_slots() {
        let ready = DynPose::pose_node(std::rc::Rc::new(DynNode::Linear { vx: 1.0, vy: 0.0 }));
        let exit_slot = Rc::new(StageExitSlot);
        let staged = DynPose::pose_node(std::rc::Rc::new(DynNode::Stages {
            segs: vec![
                StageSeg { term: StageTerm::Dur(1.0), make: StageMake::Ready(ready), exit_slot: None },
                StageSeg {
                    term: StageTerm::Forever,
                    make: StageMake::Ready(DynPose::pose_node(std::rc::Rc::new(DynNode::Linear {
                        vx: 0.0,
                        vy: 1.0,
                    }))),
                    exit_slot: Some(exit_slot),
                },
            ],
        }));
        let schema = collect_motion_state_schema(&DynFigure::pose(staged));
        assert_eq!(schema.n2_keys.len(), 3, "stages has idx/epoch plus exit pos/vel state");
        assert_eq!(schema.dyn_keys.len(), 0, "lowered stage segments do not keep dyn state");
    }

    #[test]
    fn vel_with_trailing_child() {
        // 200's guide: (vel c[..] (circle 7 (polar ...)))
        let Val::Arr(items) = ev_prelude("(vel c[1 0] (circle 7 (linear c[1 0])))") else { panic!() };
        assert_eq!(items.len(), 7);
        assert!(matches!(&items[0], Val::DynPose(d) if is_scanned(d.node())));
    }

    #[test]
    fn laser_value_and_framing() {
        let Val::Arr(items) =
            ev_prelude("(circle 6 (curve p[m\"2*t\" m\"-14*u\"] {:warn 1.5 :active inf :u-max 3.5 :resolution 0.4}))")
        else {
            panic!()
        };
        assert_eq!(items.len(), 6);
        let Val::ElemV(e) = &items[0] else { panic!("expected seeded laser") };
        assert!(e.fields.iter().any(|(k, v)| k.as_ref() == "warn" && matches!(v, FieldSeed::Num(n) if *n == 1.5)));
        assert!(e.fields.iter().any(|(k, v)| k.as_ref() == "u-max" && matches!(v, FieldSeed::Num(n) if *n == 3.5)));
        let Val::CurveV(l) = &e.figure else { panic!("expected laser") };
        let CurveBacking::Parametric { curve } = &l.backing else {
            panic!("expected parametric curve")
        };
        assert!(matches!(&curve.domain, CurveDomain::Range { min, max } if *min == 0.0 && *max == 3.5));
        // shape at t=1, u=1: r=2, θ=-14°
        let p = eval_curve_pose(&curve.eval, 1.0, 1.0, &MotionState::default(), &SigEnv::default()).unwrap();
        let ex = 2.0 * (-14f64).to_radians().cos();
        assert!((p.x - ex).abs() < 1e-9);
    }

    #[test]
    fn aim_is_ambient_relative() {
        let ctx = &mut Ctx::default();
        let mut ch = HashMap::new();
        ch.insert("player".to_string(), Val::Pose(Pose::point(0.0, -4.0)));
        ctx.sig.channels = Rc::new(ch);
        let f = read_one("(aim $player)").unwrap();
        let Val::Pose(p) = evaluate(&f, &Env::empty(), ctx, &mut World::default()).unwrap()
        else {
            panic!()
        };
        assert!(
            (p.angle_or(0.0) - -90.0).abs() < 1e-9,
            "aim down: {}",
            p.angle_or(0.0)
        );
    }

    #[test]
    fn plus_translates_formations() {
        let Val::Arr(items) = ev_prelude("(+ c[-7 0] (arrow 3 1.0 0.5))") else { panic!() };
        assert_eq!(items.len(), 3);
        let Val::Pose(center) = &items[1] else { panic!() };
        assert!((center.x - -7.0).abs() < 1e-9 && center.y.abs() < 1e-9);
    }
}
