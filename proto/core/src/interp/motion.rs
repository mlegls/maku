//! The hot layer: poses, dyn nodes, signal evaluation, scanned motion.

use super::*;
use crate::edn::Form;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Per-bullet scanned state keyed by stable lowered motion ids.
#[derive(Debug, Clone)]
pub enum Cell {
    N([f64; 2]),
    D(DynPose),
    V(EvolveCell),
}
pub type MotionState = HashMap<MotionStateKey, Cell>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionNodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionStateKey {
    /// Stable lowered node id for dense entity state.
    Node(MotionNodeId),
    /// Expression-local stateful sites under a scanned node. These are
    /// discovered from scan builtin specs during expression lowering.
    ScanSite { base: MotionNodeId, index: u32 },
    /// A stage segment's exit parameter cell (pos/vel), written at the stage
    /// boundary. Keyed by the slot token's stable lowered id — slot ptrs are
    /// seeded into node_ids alongside node ptrs.
    StageExit { base: MotionNodeId, field: StageExitField },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageExitField {
    Pos,
    Vel,
}

#[derive(Debug)]
pub struct StageExitSlot;

pub(crate) fn stage_exit_key(
    slot: &Rc<StageExitSlot>,
    readers: &MotionReaders,
    field: StageExitField,
) -> MotionStateKey {
    let ptr = Rc::as_ptr(slot) as usize;
    if let Some(base) = readers.node_ids.borrow().get(&ptr).copied() {
        return MotionStateKey::StageExit { base, field };
    }
    panic!("stage exit slot has no stable lowered id for pointer {ptr:#x}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateN2SlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateDynSlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateValSlotId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct MotionStateSchema {
    pub n2_slots: HashMap<MotionStateKey, StateN2SlotId>,
    pub n2_keys: Vec<MotionStateKey>,
    pub dyn_slots: HashMap<MotionStateKey, StateDynSlotId>,
    pub dyn_keys: Vec<MotionStateKey>,
    pub val_slots: HashMap<MotionStateKey, StateValSlotId>,
    pub val_keys: Vec<MotionStateKey>,
    pub node_ids: HashMap<usize, MotionNodeId>,
    /// node_ids in the shape MotionReaders wants, built once per schema.
    /// Entity schemas are complete at load, so per-row readers can share
    /// this instead of cloning the map per entity per phase; only ad-hoc
    /// direct evaluation seeds ids lazily, through its own fresh maps.
    shared_node_ids: std::cell::OnceCell<Rc<RefCell<HashMap<usize, MotionNodeId>>>>,
}

impl MotionStateSchema {
    pub fn shared_node_ids(&self) -> Rc<RefCell<HashMap<usize, MotionNodeId>>> {
        self.shared_node_ids
            .get_or_init(|| Rc::new(RefCell::new(self.node_ids.clone())))
            .clone()
    }

    pub fn intern_node(&mut self, ptr: usize) -> MotionNodeId {
        if let Some(id) = self.node_ids.get(&ptr).copied() {
            return id;
        }
        let id = MotionNodeId(self.node_ids.len() as u32);
        self.node_ids.insert(ptr, id);
        id
    }

    pub fn intern_n2(&mut self, key: MotionStateKey) -> StateN2SlotId {
        if let Some(slot) = self.n2_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateN2SlotId(self.n2_keys.len() as u32);
        self.n2_keys.push(key);
        self.n2_slots.insert(key, slot);
        slot
    }

    pub fn intern_dyn(&mut self, key: MotionStateKey) -> StateDynSlotId {
        if let Some(slot) = self.dyn_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateDynSlotId(self.dyn_keys.len() as u32);
        self.dyn_keys.push(key);
        self.dyn_slots.insert(key, slot);
        slot
    }

    pub fn intern_val(&mut self, key: MotionStateKey) -> StateValSlotId {
        if let Some(slot) = self.val_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateValSlotId(self.val_keys.len() as u32);
        self.val_keys.push(key);
        self.val_slots.insert(key, slot);
        slot
    }
}

/// Scan-context IO for stateful signal evaluation: carries the bullet's state
/// cells plus a per-evaluation site counter (stable for a fixed expr tree).
pub struct ScanIo {
    pub state: MotionState,
    pub base: usize,
    pub dense_base: Option<MotionNodeId>,
    pub counter: usize,
    pub advance: bool,
    pub dt: f64,
    pub readers: Option<MotionReaders>,
    pub mirror_legacy: bool,
    pub n2_writes: Vec<(MotionStateKey, [f64; 2])>,
}
pub type ScanShared = Rc<std::cell::RefCell<ScanIo>>;
pub type N2Reader = Rc<dyn Fn(MotionStateKey) -> Option<[f64; 2]>>;
pub type DynReader = Rc<dyn Fn(MotionStateKey) -> Option<DynPose>>;
pub type ValReader = Rc<dyn Fn(MotionStateKey) -> Option<EvolveCell>>;

#[derive(Clone)]
pub struct MotionReaders {
    pub n2: N2Reader,
    pub dyns: DynReader,
    pub vals: ValReader,
    pub node_ids: Rc<RefCell<HashMap<usize, MotionNodeId>>>,
}

impl MotionReaders {
    pub fn legacy() -> MotionReaders {
        MotionReaders {
            n2: Rc::new(|_| None),
            dyns: Rc::new(|_| None),
            vals: Rc::new(|_| None),
            node_ids: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn for_node(d: &DynNode) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_dyn_node_ids(d, &readers);
        readers
    }

    pub fn for_pose(d: &DynPose) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_pose_nodes(d, &readers);
        readers
    }

    pub fn for_figure(d: &DynFigure) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_figure_nodes(d, &readers);
        readers
    }
}

#[derive(Clone, Copy)]
pub struct MotionEvalCtx<'a> {
    pub state: &'a MotionState,
    pub sig: &'a SigEnv,
    pub readers: &'a MotionReaders,
    pub tick_rate: f64,
    /// When false the caller provably discards theta, so nodes whose
    /// heading costs extra evaluation (ClosedPt's second sample, Vel's
    /// integrand, RotExpr) may skip it. Frame re-enables it for the
    /// parent: compose rotates the child offset by the parent's theta.
    pub need_theta: bool,
}

impl<'a> MotionEvalCtx<'a> {
    pub fn new(state: &'a MotionState, sig: &'a SigEnv, readers: &'a MotionReaders) -> MotionEvalCtx<'a> {
        MotionEvalCtx::with_tick_rate(state, sig, readers, TickTiming::default().rate())
    }

    pub fn with_tick_rate(
        state: &'a MotionState,
        sig: &'a SigEnv,
        readers: &'a MotionReaders,
        tick_rate: f64,
    ) -> MotionEvalCtx<'a> {
        MotionEvalCtx { state, sig, readers, tick_rate, need_theta: true }
    }

    pub fn pos_only(mut self) -> Self {
        self.need_theta = false;
        self
    }

    fn with_theta(mut self) -> Self {
        self.need_theta = true;
        self
    }

    pub fn node_key(&self, ptr: usize) -> MotionStateKey {
        state_key_for_node(ptr, self.readers)
    }
}

pub struct MotionStepCtx<'a> {
    pub state: &'a mut MotionState,
    pub sig: &'a SigEnv,
    pub readers: &'a MotionReaders,
    pub tick_rate: f64,
    pub mirror_legacy: bool,
    pub write_n2: &'a mut dyn FnMut(MotionStateKey, [f64; 2]),
    pub write_dyn: &'a mut dyn FnMut(MotionStateKey, DynPose),
    pub write_val: &'a mut dyn FnMut(MotionStateKey, EvolveCell),
}

impl<'a> MotionStepCtx<'a> {
    pub fn node_key(&self, ptr: usize) -> MotionStateKey {
        state_key_for_node(ptr, self.readers)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanStateShape {
    N2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanBuiltinSpec {
    pub state: ScanStateShape,
}

pub fn scan_builtin_spec(name: &str) -> Option<ScanBuiltinSpec> {
    match name {
        "slew" | "smooth" => Some(ScanBuiltinSpec { state: ScanStateShape::N2 }),
        _ => None,
    }
}

pub(crate) fn state_key_for_node(ptr: usize, readers: &MotionReaders) -> MotionStateKey {
    if let Some(id) = readers.node_ids.borrow().get(&ptr).copied() {
        return MotionStateKey::Node(id);
    }
    panic!("motion node has no stable lowered id for pointer {ptr:#x}")
}

pub(crate) fn shortest_arc(from: f64, to: f64) -> f64 {
    let mut d = (to - from).rem_euclid(360.0);
    if d > 180.0 {
        d -= 360.0;
    }
    d
}

#[derive(Debug)]
pub enum DynNode {
    Const(Pose),
    /// pos = v·τ in the local frame; θ = heading.
    Linear { vx: f64, vy: f64 },
    /// Closed pose expression over slot-bound t (and u, for curve shapes).
    ClosedPt {
        a: Form,
        b: Form,
        polar: bool,
        env: Env,
        programs: OnceCell<Option<(Rc<NumProgram>, Rc<NumProgram>)>>,
    },
    /// Integrated velocity (Scanned): components over slot-bound t.
    Vel {
        a: Form,
        b: Form,
        polar: bool,
        env: Env,
        programs: OnceCell<Option<(Rc<NumProgram>, Rc<NumProgram>)>>,
    },
    /// Point-translation (the `+` of the two-op algebra): θ untouched.
    Translate { dx: f64, dy: f64, child: Rc<DynNode> },
    /// Sample a curve dyn at u = progress(t). This is the point-motion
    /// analogue of curve materialization, without expressing a curve entity.
    Path { curve: Rc<DynNode>, progress: Form, env: Env },
    Frame(Rc<DynNode>, Rc<DynNode>),
    /// A live injected channel as a pose (class (b): pointwise, no state).
    Live { channel: Rc<str> },
    /// Position clamp (playfield walls). Output-clamps the child pose; for
    /// integrated children (vel under const frames) the integrator STATE is
    /// clamped after each step — pushing a wall doesn't bank phantom
    /// distance, you slide and turn back instantly.
    Clamp { lo: (f64, f64), hi: (f64, f64), child: Rc<DynNode> },
    /// Time-varying rotation frame: θ(t), stateful sites allowed inside.
    RotExpr { form: Form, env: Env, program: OnceCell<Option<Rc<NumProgram>>> },
    /// A user function adapted to a stateless pose dyn by calling it as (f t).
    FnPose(Val),
    /// A closed evolve used in a pose slot: the fold is replayed from epoch
    /// start at each evaluation (pure in tau), so the node carries no
    /// per-entity motion state. Memoized monotone advance is a later
    /// optimization, not a semantic need.
    Evolve(Rc<EvolveDyn>),
    /// SCANNED.md's `stages`: segment list with per-entity (idx, epoch) state.
    /// Closure segments are lowered at construction with fixed exit-pose cells.
    Stages { segs: Vec<StageSeg> },
}

/// `(evolve init step)` — the kernel's stateful signal constructor
/// (docs/notes/evolve-design.md). `init` is the state at epoch start
/// (already evaluated: construction is epoch start until remat lands);
/// `step` is a callable `(fn [s ctx] ...)` applied once per tick.
#[derive(Debug)]
pub struct EvolveDyn {
    pub init: Val,
    pub step: Val,
}

#[derive(Clone, Debug)]
pub struct EvolveCell {
    pub state: Val,
    pub tick: u64,
}

pub(crate) fn evolve_tick(tau: f64, tick_rate: f64) -> u64 {
    (tau * tick_rate + 1e-9).floor().max(0.0) as u64
}

fn evolve_step_ctx(k: u64, dt: f64) -> Val {
    Val::Map(Rc::new(vec![
        (Val::Kw("t".into()), Val::Num(k as f64 * dt)),
        (Val::Kw("dt".into()), Val::Num(dt)),
        (Val::Kw("tick".into()), Val::Num(k as f64)),
    ]))
}

fn apply_evolve_step(ev: &EvolveDyn, state: Val, k: u64, sig: &SigEnv, tick_rate: f64) -> Result<Val, String> {
    let step_ctx = evolve_step_ctx(k, 1.0 / tick_rate);
    let mut call_ctx = Ctx {
        sig: sig.clone(),
        ambient: Pose::IDENTITY,
        scan: None,
        patterns: Rc::new(HashMap::new()),
        macros: Rc::new(HashMap::new()),
        deferred: Vec::new(),
        projector_scope: None,
    };
    let mut w = World::for_eval(tick_rate);
    apply_fn(ev.step.clone(), &[state, step_ctx], &mut call_ctx, &mut w, false)
}

/// The value of a closed evolve at time tau: the fold of `step` over ticks
/// 0..floor(tau·rate), starting from `init`. Steps evaluate against a
/// CLOSED SigEnv — defs carry over (pure), but channels/cells are empty so
/// live channel reads error, enforcing the closed-evolve rule (live
/// evolves get engine-clock advance in a later milestone).
pub fn evolve_value(ev: &EvolveDyn, tau: f64, sig: &SigEnv, tick_rate: f64) -> Result<Val, String> {
    let closed_sig = SigEnv { defs: sig.defs.clone(), ..SigEnv::default() };
    let n = evolve_tick(tau, tick_rate);
    let mut s = ev.init.clone();
    for k in 0..n {
        s = apply_evolve_step(ev, s, k, &closed_sig, tick_rate)?;
    }
    Ok(s)
}

#[derive(Debug)]
pub struct StageSeg {
    pub term: StageTerm,
    pub make: StageMake,
    pub exit_slot: Option<Rc<StageExitSlot>>,
}

#[derive(Debug)]
pub enum StageTerm {
    Dur(f64),
    Until(Form, Env),
    Forever,
}

#[derive(Debug)]
pub enum StageMake {
    Ready(DynPose),
}

pub fn collect_motion_state_schema(d: &DynFigure) -> MotionStateSchema {
    let mut schema = MotionStateSchema::default();
    collect_figure_state(d, &mut schema);
    schema
}

pub fn collect_figure_state(d: &DynFigure, schema: &mut MotionStateSchema) {
    match d.repr() {
        FigureDynRepr::Pose(pose) => collect_pose_state(pose, schema),
        FigureDynRepr::Curve { frame, curve } => {
            collect_pose_state(frame, schema);
            if let CurveEval::Expr(shape) = &curve.eval {
                collect_pose_state(shape, schema);
            }
        }
    }
}

pub fn collect_pose_state(d: &DynPose, schema: &mut MotionStateSchema) {
    collect_node_state(d.node(), schema);
}

fn seed_reader_figure_nodes(d: &DynFigure, readers: &MotionReaders) {
    match d.repr() {
        FigureDynRepr::Pose(pose) => seed_reader_pose_nodes(pose, readers),
        FigureDynRepr::Curve { frame, curve } => {
            seed_reader_pose_nodes(frame, readers);
            if let CurveEval::Expr(shape) = &curve.eval {
                seed_reader_pose_nodes(shape, readers);
            }
        }
    }
}

fn seed_reader_pose_nodes(d: &DynPose, readers: &MotionReaders) {
    let mut node_ids = readers.node_ids.borrow_mut();
    let mut next = node_ids.values().map(|id| id.0).max().map(|id| id + 1).unwrap_or(0);
    seed_reader_node_ids(d.node(), &mut node_ids, &mut next);
}

fn seed_reader_dyn_node_ids(d: &DynNode, readers: &MotionReaders) {
    let mut node_ids = readers.node_ids.borrow_mut();
    let mut next = node_ids.values().map(|id| id.0).max().map(|id| id + 1).unwrap_or(0);
    seed_dyn_node_ids(d, &mut node_ids, &mut next);
}

fn seed_reader_node_ids(
    node: &Rc<DynNode>,
    node_ids: &mut HashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    let ptr = Rc::as_ptr(node) as usize;
    seed_dyn_node_ids_with_ptr(&**node, ptr, node_ids, next);
}

fn seed_dyn_node_ids(
    node: &DynNode,
    node_ids: &mut HashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    let ptr = node as *const DynNode as usize;
    seed_dyn_node_ids_with_ptr(node, ptr, node_ids, next);
}

fn seed_dyn_node_ids_with_ptr(
    node: &DynNode,
    ptr: usize,
    node_ids: &mut HashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    if let std::collections::hash_map::Entry::Vacant(entry) = node_ids.entry(ptr) {
        entry.insert(MotionNodeId(*next));
        *next += 1;
    }
    match node {
        DynNode::Path { curve, .. } => seed_reader_node_ids(curve, node_ids, next),
        DynNode::Stages { segs } => {
            for seg in segs {
                // slot before child: must mirror collect_node_state's order
                // so lowered ids line up between schema and readers
                if let Some(slot) = &seg.exit_slot {
                    let slot_ptr = Rc::as_ptr(slot) as usize;
                    if let std::collections::hash_map::Entry::Vacant(entry) = node_ids.entry(slot_ptr) {
                        entry.insert(MotionNodeId(*next));
                        *next += 1;
                    }
                }
                let StageMake::Ready(d) = &seg.make;
                seed_reader_node_ids(d.node(), node_ids, next);
            }
        }
        DynNode::Translate { child, .. } | DynNode::Clamp { child, .. } => {
            seed_reader_node_ids(child, node_ids, next);
        }
        DynNode::Frame(a, b) => {
            seed_reader_node_ids(a, node_ids, next);
            seed_reader_node_ids(b, node_ids, next);
        }
        DynNode::Const(_)
        | DynNode::Linear { .. }
        | DynNode::Live { .. }
        | DynNode::Vel { .. }
        | DynNode::ClosedPt { .. }
        | DynNode::FnPose(_)
        | DynNode::Evolve(_)
        | DynNode::RotExpr { .. } => {}
    }
}

pub fn collect_node_state(node: &Rc<DynNode>, schema: &mut MotionStateSchema) {
    let base = Rc::as_ptr(node) as usize;
    let node_id = schema.intern_node(base);
    match &**node {
        DynNode::Vel { a, b, .. } => {
            schema.intern_n2(MotionStateKey::Node(node_id));
            let index = collect_scan_sites(a, node_id, 0, schema);
            collect_scan_sites(b, node_id, index, schema);
        }
        DynNode::ClosedPt { a, b, .. } => {
            let index = collect_scan_sites(a, node_id, 0, schema);
            collect_scan_sites(b, node_id, index, schema);
        }
        DynNode::RotExpr { form, .. } => {
            collect_scan_sites(form, node_id, 0, schema);
        }
        DynNode::Path { curve, progress, .. } => {
            collect_scan_sites(progress, node_id, 0, schema);
            collect_node_state(curve, schema);
        }
        DynNode::Stages { segs } => {
            schema.intern_n2(MotionStateKey::Node(node_id));
            for seg in segs {
                if let StageTerm::Until(pred, _) = &seg.term {
                    collect_scan_sites(pred, node_id, 0, schema);
                }
                if let Some(slot) = &seg.exit_slot {
                    let base = schema.intern_node(Rc::as_ptr(slot) as usize);
                    schema.intern_n2(MotionStateKey::StageExit { base, field: StageExitField::Pos });
                    schema.intern_n2(MotionStateKey::StageExit { base, field: StageExitField::Vel });
                }
                let StageMake::Ready(d) = &seg.make;
                collect_pose_state(d, schema);
            }
        }
        DynNode::Translate { child, .. } | DynNode::Clamp { child, .. } => {
            collect_node_state(child, schema);
        }
        DynNode::Frame(a, b) => {
            collect_node_state(a, schema);
            collect_node_state(b, schema);
        }
        DynNode::Evolve(_) => {
            schema.intern_val(MotionStateKey::Node(node_id));
        }
        DynNode::Const(_)
        | DynNode::Linear { .. }
        | DynNode::Live { .. }
        | DynNode::FnPose(_) => {}
    }
}

pub fn collect_scan_sites(
    form: &Form,
    base: MotionNodeId,
    start_index: u32,
    schema: &mut MotionStateSchema,
) -> u32 {
    match form {
        Form::List(items) => {
            let mut index = start_index;
            if let Some(Form::Sym(s)) = items.first() {
                if let Some(spec) = scan_builtin_spec(s) {
                    match spec.state {
                        ScanStateShape::N2 => {
                            schema.intern_n2(MotionStateKey::ScanSite { base, index });
                        }
                    }
                    index += 1;
                }
            }
            for item in items.iter() {
                index = collect_scan_sites(item, base, index, schema);
            }
            index
        }
        Form::Vector(items) => items
            .iter()
            .fold(start_index, |index, item| collect_scan_sites(item, base, index, schema)),
        Form::Map(kvs) => kvs.iter().fold(start_index, |index, (k, v)| {
            let index = collect_scan_sites(k, base, index, schema);
            collect_scan_sites(v, base, index, schema)
        }),
        _ => start_index,
    }
}

impl DynEval for f64 {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<f64>,
        tau: f64,
        _state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<f64, String> {
        match d.repr() {
            NumDynRepr::Const(n) => Ok(*n),
            NumDynRepr::Expr { form, env } => eval_sig_at_rate(form, env, sig, tau, 0.0, None, None, tick_rate)?.num(),
            NumDynRepr::AxisSel { form, env, path, flat } => {
                let v = eval_sig_at_rate(form, env, sig, tau, 0.0, None, None, tick_rate)?;
                super::spawn::axis_select_val(&v, path, *flat).num()
            }
        }
    }
}

impl DynEval for Figure {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<Figure>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<Figure, String> {
        match d.repr() {
            FigureDynRepr::Pose(p) => Ok(Figure::Pose(dyn_pose_with_tick_rate(p, tau, state, sig, tick_rate)?)),
            FigureDynRepr::Curve { frame, curve } => Ok(Figure::Curve(Curve {
                frame: dyn_pose_with_tick_rate(frame, tau, state, sig, tick_rate)?,
                spec: curve.clone(),
            })),
        }
    }
}

impl DynEval for Pose {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<Pose>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<Pose, String> {
        dyn_node_pose_with_tick_rate(d.node(), tau, state, sig, tick_rate)
    }
}

pub fn eval_curve_pose(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    eval_curve_pose_with_tick_rate(eval, tau, u, state, sig, TickTiming::default().rate())
}

pub fn eval_curve_pose_with_tick_rate(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    match eval {
        CurveEval::Straight => Ok(Pose::oriented(u, 0.0, 0.0)),
        CurveEval::Expr(d) => dyn_node_pose_u_with_tick_rate(d.node(), tau, u, state, sig, tick_rate),
    }
}

/// Compatibility extended values before spawn lowering.
#[derive(Debug, Clone)]
pub enum CurveBacking {
    /// Surface `curve` syntax lowers to this representation.
    Parametric {
        curve: ParametricCurve,
    },
    /// Surface `pather` syntax currently lowers to a pose entity with a
    /// legacy trace cache enabled.
    Trace { window: f64 },
}

#[derive(Debug)]
pub struct ExtCurve {
    pub anchor: DynPose,
    pub backing: CurveBacking,
}

pub fn eval_sig(
    form: &Form,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
) -> Result<Val, String> {
    eval_sig_at_rate(form, env, sig, tau, u, scan, pos, TickTiming::default().rate())
}

pub fn eval_sig_at_rate(
    form: &Form,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
    tick_rate: f64,
) -> Result<Val, String> {
    let mut e = env.bind("t".into(), Val::Num(tau)).bind("u".into(), Val::Num(u));
    if let Some((px, py)) = pos {
        e = e.bind("pos".into(), Val::Pose(Pose::point(px, py)));
    }
    let mut ctx = Ctx {
        sig: sig.clone(),
        ambient: Pose::IDENTITY,
        scan,
        patterns: Rc::new(HashMap::new()),
        macros: Rc::new(HashMap::new()),
        deferred: Vec::new(),
        projector_scope: None,
    };
    let mut w = World::for_eval(tick_rate); // signals never touch the world (§2)
    evaluate(form, &e, &mut ctx, &mut w)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_pt_at_rate(
    a: &Form,
    b: &Form,
    polar: bool,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
    tick_rate: f64,
) -> Result<(f64, f64), String> {
    let av = eval_sig_at_rate(a, env, sig, tau, u, scan.clone(), pos, tick_rate)?.num()?;
    let bv = eval_sig_at_rate(b, env, sig, tau, u, scan, pos, tick_rate)?.num()?;
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        Ok((av * c, av * s))
    } else {
        Ok((av, bv))
    }
}

fn eval_num_program_pair(
    a: &NumProgram,
    b: &NumProgram,
    polar: bool,
    tau: f64,
    u: f64,
    pos: Option<(f64, f64)>,
) -> (f64, f64) {
    let av = run_num_program(a, tau, u, pos);
    let bv = run_num_program(b, tau, u, pos);
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        (av * c, av * s)
    } else {
        (av, bv)
    }
}

fn lower_program_pair(
    a: &Form,
    b: &Form,
    env: &Env,
    sig: &SigEnv,
    allow_pos: bool,
) -> Option<(Rc<NumProgram>, Rc<NumProgram>)> {
    let ap = lower_num_form(a, env, &sig.defs)?;
    let bp = lower_num_form(b, env, &sig.defs)?;
    if !allow_pos && (program_uses_pos(&ap) || program_uses_pos(&bp)) {
        return None;
    }
    Some((Rc::new(ap), Rc::new(bp)))
}

fn lower_single_program(form: &Form, env: &Env, sig: &SigEnv, allow_pos: bool) -> Option<Rc<NumProgram>> {
    let prog = lower_num_form(form, env, &sig.defs)?;
    if !allow_pos && program_uses_pos(&prog) {
        return None;
    }
    Some(Rc::new(prog))
}

fn assert_num_close(label: &str, form: &Form, got: f64, expected: f64) {
    assert!(
        (got - expected).abs() <= 1e-9,
        "{} compiled/interpreted mismatch for {:?}: compiled={}, interpreted={}",
        label,
        form,
        got,
        expected
    );
}

/// Read-only scan context over a clone of the bullet's state.
pub(crate) fn read_scan_in(
    state: &MotionState,
    base: usize,
    readers: MotionReaders,
) -> ScanShared {
    let MotionStateKey::Node(dense_base) = state_key_for_node(base, &readers) else {
        unreachable!("node keys are always stable")
    };
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base,
        dense_base: Some(dense_base),
        counter: 0,
        advance: false,
        dt: 0.0,
        readers: Some(readers),
        mirror_legacy: false,
        n2_writes: Vec::new(),
    }))
}

pub(crate) fn read_scan(state: &MotionState, base: MotionNodeId) -> ScanShared {
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base: 0,
        dense_base: Some(base),
        counter: 0,
        advance: false,
        dt: 0.0,
        readers: None,
        mirror_legacy: false,
        n2_writes: Vec::new(),
    }))
}

pub fn dyn_node_pose(d: &DynNode, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_node_pose_u_in(d, tau, 0.0, ctx)
}

pub fn dyn_node_pose_with_tick_rate(
    d: &DynNode,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    dyn_node_pose_u_with_tick_rate(d, tau, 0.0, state, sig, tick_rate)
}

pub fn dyn_pose(d: &DynPose, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    let readers = MotionReaders::for_pose(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_pose_in(d, tau, ctx)
}

pub fn dyn_pose_with_tick_rate(
    d: &DynPose,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_pose(d);
    let ctx = MotionEvalCtx::with_tick_rate(state, sig, &readers, tick_rate);
    dyn_pose_in(d, tau, ctx)
}

pub fn dyn_figure_pose(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_figure(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_figure_pose_in(d, tau, ctx)
}

pub fn dyn_figure_pose_in(d: &DynFigure, tau: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    dyn_node_pose_u_in(d.pose_dyn(), tau, 0.0, ctx)
}

pub fn eval_dyn_figure(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Figure, String> {
    match d.repr() {
        FigureDynRepr::Pose(p) => Ok(Figure::Pose(dyn_pose(p, tau, state, sig)?)),
        FigureDynRepr::Curve { frame, curve } => Ok(Figure::Curve(Curve {
            frame: dyn_pose(frame, tau, state, sig)?,
            spec: curve.clone(),
        })),
    }
}

pub fn dyn_pose_u(
    d: &DynPose,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    dyn_node_pose_u(d.node(), tau, u, state, sig)
}

pub fn dyn_pose_in(d: &DynPose, tau: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    dyn_node_pose_u_in(d.node(), tau, 0.0, ctx)
}

pub fn dyn_node_pose_u(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_node_pose_u_in(d, tau, u, ctx)
}

pub fn dyn_node_pose_u_with_tick_rate(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::with_tick_rate(state, sig, &readers, tick_rate);
    dyn_node_pose_u_in(d, tau, u, ctx)
}

pub fn dyn_node_pose_u_in(d: &DynNode, tau: f64, u: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    if crate::interp::profile::enabled() {
        let frame = crate::interp::profile::open();
        let r = dyn_node_pose_u_in_inner(d, tau, u, ctx);
        crate::interp::profile::close(dyn_node_name(d), frame);
        return r;
    }
    dyn_node_pose_u_in_inner(d, tau, u, ctx)
}

fn dyn_node_name(d: &DynNode) -> &'static str {
    match d {
        DynNode::Const(_) => "dyn:const",
        DynNode::Linear { .. } => "dyn:linear",
        DynNode::ClosedPt { programs, .. } => match programs.get() {
            Some(Some(_)) => "dyn:closed-pt-c",
            _ => "dyn:closed-pt",
        },
        DynNode::Vel { programs, .. } => match programs.get() {
            Some(Some(_)) => "dyn:vel-c",
            _ => "dyn:vel",
        },
        DynNode::Translate { .. } => "dyn:translate",
        DynNode::Path { .. } => "dyn:path",
        DynNode::Frame(..) => "dyn:frame",
        DynNode::Live { .. } => "dyn:live",
        DynNode::Clamp { .. } => "dyn:clamp",
        DynNode::RotExpr { .. } => "dyn:rot-expr",
        DynNode::FnPose(_) => "dyn:fn-pose",
        DynNode::Evolve(_) => "dyn:evolve",
        DynNode::Stages { .. } => "dyn:stages",
    }
}

fn dyn_node_pose_u_in_inner(d: &DynNode, tau: f64, u: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    let state = ctx.state;
    let sig = ctx.sig;
    let readers = ctx.readers;
    let tick_rate = ctx.tick_rate;
    match d {
        DynNode::Const(p) => Ok(*p),
        DynNode::Linear { vx, vy } => Ok(Pose {
            x: vx * tau,
            y: vy * tau,
            theta: Some(vy.atan2(*vx).to_degrees()),
        }),
        DynNode::ClosedPt { a, b, polar, env, programs } => {
            let key = d as *const DynNode as usize;
            if let Some((ap, bp)) = programs
                .get_or_init(|| lower_program_pair(a, b, env, sig, false))
                .as_ref()
            {
                let (x, y) = eval_num_program_pair(ap, bp, *polar, tau, u, None);
                if !ctx.need_theta {
                    if oracle_enabled() {
                        let (ix, iy) = eval_pt_at_rate(
                            a,
                            b,
                            *polar,
                            env,
                            sig,
                            tau,
                            u,
                            Some(read_scan_in(state, key, readers.clone())),
                            None,
                            tick_rate,
                        )?;
                        assert_num_close("closed-pt/a", a, x, ix);
                        assert_num_close("closed-pt/b", b, y, iy);
                    }
                    return Ok(Pose::point(x, y));
                }
                let eps = 1.0 / tick_rate;
                let (x2, y2) = eval_num_program_pair(ap, bp, *polar, tau + eps, u, None);
                if oracle_enabled() {
                    let (ix, iy) = eval_pt_at_rate(
                        a,
                        b,
                        *polar,
                        env,
                        sig,
                        tau,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        None,
                        tick_rate,
                    )?;
                    let (ix2, iy2) = eval_pt_at_rate(
                        a,
                        b,
                        *polar,
                        env,
                        sig,
                        tau + eps,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        None,
                        tick_rate,
                    )?;
                    assert_num_close("closed-pt/a", a, x, ix);
                    assert_num_close("closed-pt/b", b, y, iy);
                    assert_num_close("closed-pt/a+eps", a, x2, ix2);
                    assert_num_close("closed-pt/b+eps", b, y2, iy2);
                }
                return Ok(Pose::oriented(x, y, (y2 - y).atan2(x2 - x).to_degrees()));
            }
            let (x, y) = eval_pt_at_rate(
                a,
                b,
                *polar,
                env,
                sig,
                tau,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?;
            if !ctx.need_theta {
                return Ok(Pose::point(x, y));
            }
            let eps = 1.0 / tick_rate;
            let (x2, y2) = eval_pt_at_rate(
                a,
                b,
                *polar,
                env,
                sig,
                tau + eps,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?;
            Ok(Pose::oriented(x, y, (y2 - y).atan2(x2 - x).to_degrees()))
        }
        DynNode::Vel { a, b, polar, env, programs } => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let [x, y] = (readers.n2)(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            if !ctx.need_theta {
                // (x, y) come from the integrator state; the integrand
                // eval below only feeds the heading.
                return Ok(Pose::point(x, y));
            }
            if let Some((ap, bp)) = programs
                .get_or_init(|| lower_program_pair(a, b, env, sig, true))
                .as_ref()
            {
                let (vx, vy) = eval_num_program_pair(ap, bp, *polar, tau, u, Some((x, y)));
                if oracle_enabled() {
                    let (ivx, ivy) = eval_pt_at_rate(
                        a,
                        b,
                        *polar,
                        env,
                        sig,
                        tau,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        Some((x, y)),
                        tick_rate,
                    )?;
                    assert_num_close("vel/a", a, vx, ivx);
                    assert_num_close("vel/b", b, vy, ivy);
                }
                return Ok(Pose::oriented(x, y, vy.atan2(vx).to_degrees()));
            }
            let (vx, vy) = eval_pt_at_rate(
                a,
                b,
                *polar,
                env,
                sig,
                tau,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                Some((x, y)),
                tick_rate,
            )?;
            Ok(Pose::oriented(x, y, vy.atan2(vx).to_degrees()))
        }
        DynNode::Live { channel } => {
            let (x, y) = sig.channel_pos(channel);
            Ok(Pose::point(x, y))
        }
        DynNode::Clamp { lo, hi, child } => {
            let p = dyn_node_pose_u_in(child, tau, 0.0, ctx)?;
            Ok(Pose { x: p.x.clamp(lo.0, hi.0), y: p.y.clamp(lo.1, hi.1), theta: p.theta })
        }
        DynNode::RotExpr { form, env, program } => {
            if !ctx.need_theta {
                // a rot-expr's pose IS its theta; nothing else to compute
                return Ok(Pose::point(0.0, 0.0));
            }
            let key = d as *const DynNode as usize;
            if let Some(prog) = program
                .get_or_init(|| lower_single_program(form, env, sig, true))
                .as_ref()
            {
                let th = run_num_program(prog, tau, u, Some((0.0, 0.0)));
                if oracle_enabled() {
                    let ith = eval_sig_at_rate(
                        form,
                        env,
                        sig,
                        tau,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        Some((0.0, 0.0)),
                        tick_rate,
                    )?
                    .num()?;
                    assert_num_close("rot-expr", form, th, ith);
                }
                return Ok(Pose::oriented(0.0, 0.0, th));
            }
            let th = eval_sig_at_rate(
                form,
                env,
                sig,
                tau,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                Some((0.0, 0.0)),
                tick_rate,
            )?
            .num()?;
            Ok(Pose::oriented(0.0, 0.0, th))
        }
        DynNode::FnPose(f) => {
            let mut call_ctx = Ctx {
                sig: sig.clone(),
                ambient: Pose::IDENTITY,
                scan: None,
                patterns: Rc::new(HashMap::new()),
                macros: Rc::new(HashMap::new()),
                deferred: Vec::new(),
                projector_scope: None,
            };
            let mut w = World::for_eval(tick_rate);
            match apply_fn(f.clone(), &[Val::Num(tau)], &mut call_ctx, &mut w, false)? {
                Val::Pose(p) => Ok(p),
                Val::DynPose(d) => dyn_pose_with_tick_rate(&d, tau, state, sig, tick_rate),
                other => Err(format!("fn-backed dyn expected fn to return pose, got {:?}", other)),
            }
        }
        DynNode::Evolve(ev) => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let tick = evolve_tick(tau, tick_rate);
            let cell = (readers.vals)(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::V(v)) => Some(v.clone()),
                    _ => None,
                });
            let value = match cell {
                Some(cell) if cell.tick == tick => cell.state,
                _ => evolve_value(ev, tau, sig, tick_rate)?,
            };
            match value {
                Val::Pose(p) => Ok(p),
                other => Err(format!(
                    "evolve in a pose slot expected pose state, got {:?}",
                    other
                )),
            }
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let [idx, epoch] = (readers.n2)(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
            dyn_node_pose_u_in(cur.node(), tau - epoch, u, ctx)
        }
        DynNode::Translate { dx, dy, child } => {
            let p = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(Pose { x: p.x + dx, y: p.y + dy, theta: p.theta })
        }
        DynNode::Path { curve, progress, env } => {
            let key = d as *const DynNode as usize;
            let u = eval_sig_at_rate(
                progress,
                env,
                sig,
                tau,
                0.0,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?
            .num()?;
            dyn_node_pose_u_in(curve, tau, u, ctx)
        }
        DynNode::Frame(parent, child) => {
            // compose rotates the child offset by the parent theta, so the
            // parent needs its heading even when the caller discards ours
            let pp = dyn_node_pose_u_in(parent, tau, u, ctx.with_theta())?;
            let cp = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(pp.compose(&cp))
        }
    }
}

/// The dyn for the current segment of a Stages node.
pub(crate) fn stage_dyn_in(
    segs: &[StageSeg],
    idx: usize,
    _state: &MotionState,
    _key: usize,
    _readers: &MotionReaders,
) -> Result<DynPose, String> {
    let seg = segs.get(idx).ok_or("stages: segment index out of range")?;
    match &seg.make {
        StageMake::Ready(d) => Ok(d.clone()),
    }
}

/// Advance the Scanned leaves of a motion tree by one tick.
pub fn step_motion(
    d: &DynNode,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
) -> Result<(), String> {
    let readers = MotionReaders::for_node(d);
    let mut ignore_n2 = |_, _| {};
    let mut ignore_dyn = |_, _| {};
    let mut ignore_val = |_, _| {};
    let mut ctx = MotionStepCtx {
        state,
        sig,
        readers: &readers,
        tick_rate: TickTiming::default().rate(),
        mirror_legacy: true,
        write_n2: &mut ignore_n2,
        write_dyn: &mut ignore_dyn,
        write_val: &mut ignore_val,
    };
    step_motion_in(d, tau, dt, &mut ctx)
}

pub fn step_motion_in(
    d: &DynNode,
    tau: f64,
    dt: f64,
    ctx: &mut MotionStepCtx<'_>,
) -> Result<(), String> {
    match d {
        DynNode::Vel { a, b, polar, env, programs } => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let state = &mut *ctx.state;
            let sig = ctx.sig;
            let readers = ctx.readers;
            let tick_rate = ctx.tick_rate;
            let mirror_legacy = ctx.mirror_legacy;
            let write_n2 = &mut *ctx.write_n2;
            let [x, y] = (readers.n2)(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            // scan-free integrands (lowered programs) have no sites to
            // advance, so the step is just the compiled velocity sample
            let (vx, vy) = if let Some((ap, bp)) = programs
                .get_or_init(|| lower_program_pair(a, b, env, sig, true))
                .as_ref()
            {
                let (vx, vy) = eval_num_program_pair(ap, bp, *polar, tau, 0.0, Some((x, y)));
                if oracle_enabled() {
                    let ((ivx, ivy), _) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                        eval_pt_at_rate(a, b, *polar, env, sig, tau, 0.0, Some(scan), Some((x, y)), tick_rate)
                    })?;
                    assert_num_close("vel-step/a", a, vx, ivx);
                    assert_num_close("vel-step/b", b, vy, ivy);
                }
                (vx, vy)
            } else {
                let ((vx, vy), writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                    eval_pt_at_rate(a, b, *polar, env, sig, tau, 0.0, Some(scan), Some((x, y)), tick_rate)
                })?;
                for (key, value) in writes {
                    write_n2(key, value);
                }
                (vx, vy)
            };
            let next = [x + vx * dt, y + vy * dt];
            state.insert(dense_key, Cell::N(next));
            write_n2(dense_key, next);
            Ok(())
        }
        DynNode::RotExpr { form, env, program } => {
            // a lowered program is scan-free: nothing to advance
            if program
                .get_or_init(|| lower_single_program(form, env, ctx.sig, true))
                .is_some()
            {
                return Ok(());
            }
            let state = &mut *ctx.state;
            let sig = ctx.sig;
            let readers = ctx.readers;
            let tick_rate = ctx.tick_rate;
            let mirror_legacy = ctx.mirror_legacy;
            let write_n2 = &mut *ctx.write_n2;
            let key = d as *const DynNode as usize;
            let (_, writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                eval_sig_at_rate(form, env, sig, tau, 0.0, Some(scan), Some((0.0, 0.0)), tick_rate)?.num()
            })?;
            for (key, value) in writes {
                write_n2(key, value);
            }
            Ok(())
        }
        DynNode::Path { curve, progress, env } => {
            {
                let state = &mut *ctx.state;
                let sig = ctx.sig;
                let readers = ctx.readers;
                let tick_rate = ctx.tick_rate;
                let mirror_legacy = ctx.mirror_legacy;
                let write_n2 = &mut *ctx.write_n2;
                let key = d as *const DynNode as usize;
                let (_, writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                    eval_sig_at_rate(progress, env, sig, tau, 0.0, Some(scan), None, tick_rate)?.num()
                })?;
                for (key, value) in writes {
                    write_n2(key, value);
                }
            }
            step_motion_in(curve, tau, dt, ctx)
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let (cur, epoch, _mirror_legacy) = {
                let dense_key = ctx.node_key(key);
                let state = &mut *ctx.state;
                let sig = ctx.sig;
                let readers = ctx.readers;
                let tick_rate = ctx.tick_rate;
                let mirror_legacy = ctx.mirror_legacy;
                let write_n2 = &mut *ctx.write_n2;
                let [mut idx, mut epoch] = (readers.n2)(dense_key)
                    .or_else(|| match state.get(&dense_key) {
                        Some(Cell::N(v)) => Some(*v),
                        _ => None,
                    })
                    .unwrap_or([0.0, 0.0]);
                let seg = segs.get(idx as usize).ok_or("stages: bad segment")?;
                let local = tau - epoch;
                let done = match &seg.term {
                    StageTerm::Dur(dsec) => local >= *dsec,
                    StageTerm::Until(pred, penv) => {
                        let scan = read_scan_in(state, key, readers.clone());
                        truthy(&eval_sig_at_rate(pred, penv, sig, local, 0.0, Some(scan), None, tick_rate)?)
                    }
                    StageTerm::Forever => false,
                };
                if done && (idx as usize) + 1 < segs.len() {
                    let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
                    let eval_ctx = MotionEvalCtx::with_tick_rate(state, sig, readers, tick_rate);
                    let p1 = dyn_node_pose_u_in(cur.node(), local, 0.0, eval_ctx)?;
                    let p0 = dyn_node_pose_u_in(cur.node(), (local - dt).max(0.0), 0.0, eval_ctx)?;
                    let exit = Val::Map(Rc::new(vec![
                        (Val::Kw("pos".into()), Val::Pose(Pose::point(p1.x, p1.y))),
                        (
                            Val::Kw("vel".into()),
                            Val::Pose(Pose::point((p1.x - p0.x) / dt, (p1.y - p0.y) / dt)),
                        ),
                        (Val::Kw("pose".into()), Val::Pose(p1)),
                    ]));
                    idx += 1.0;
                    epoch = tau;
                    if let Some(slot) = &segs[idx as usize].exit_slot {
                        let pos_key = stage_exit_key(slot, readers, StageExitField::Pos);
                        let vel_key = stage_exit_key(slot, readers, StageExitField::Vel);
                        let pos = match map_path_get(&exit, "pos") {
                            Some(Val::Pose(p)) => [p.x, p.y],
                            _ => return Err("stages: internal exit pos missing".into()),
                        };
                        let vel = match map_path_get(&exit, "vel") {
                            Some(Val::Pose(p)) => [p.x, p.y],
                            _ => return Err("stages: internal exit vel missing".into()),
                        };
                        state.insert(pos_key, Cell::N(pos));
                        state.insert(vel_key, Cell::N(vel));
                        write_n2(pos_key, pos);
                        write_n2(vel_key, vel);
                    }
                }
                let next = [idx, epoch];
                state.insert(dense_key, Cell::N(next));
                write_n2(dense_key, next);
                let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
                (cur, epoch, mirror_legacy)
            };
            // step the inner dyn on the segment-local clock
            step_motion_in(cur.node(), tau - epoch, dt, ctx)
        }
        DynNode::Translate { child, .. } => {
            step_motion_in(child, tau, dt, ctx)
        }
        DynNode::Frame(a, b) => {
            step_motion_in(a, tau, dt, ctx)?;
            step_motion_in(b, tau, dt, ctx)
        }
        DynNode::Clamp { lo, hi, child } => {
            step_motion_in(child, tau, dt, ctx)?;
            clamp_integrator(
                child,
                *lo,
                *hi,
                ctx.state,
                ctx.readers,
                ctx.mirror_legacy,
                ctx.write_n2,
            );
            Ok(())
        }
        DynNode::Evolve(ev) => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let target_tick = evolve_tick(tau, ctx.tick_rate);
            let closed_sig = SigEnv { defs: ctx.sig.defs.clone(), ..SigEnv::default() };
            let mut cell = (ctx.readers.vals)(dense_key)
                .or_else(|| match ctx.state.get(&dense_key) {
                    Some(Cell::V(v)) => Some(v.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| EvolveCell { state: ev.init.clone(), tick: 0 });
            if cell.tick < target_tick {
                let next = apply_evolve_step(ev, cell.state, cell.tick, &closed_sig, ctx.tick_rate)?;
                cell = EvolveCell { state: next, tick: cell.tick + 1 };
            }
            if ctx.mirror_legacy {
                ctx.state.insert(dense_key, Cell::V(cell.clone()));
            }
            (ctx.write_val)(dense_key, cell);
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn step_dyn_figure(
    d: &DynFigure,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
) -> Result<(), String> {
    let readers = MotionReaders::for_figure(d);
    let mut ignore_n2 = |_, _| {};
    let mut ignore_dyn = |_, _| {};
    let mut ignore_val = |_, _| {};
    let mut ctx = MotionStepCtx {
        state,
        sig,
        readers: &readers,
        tick_rate: TickTiming::default().rate(),
        mirror_legacy: true,
        write_n2: &mut ignore_n2,
        write_dyn: &mut ignore_dyn,
        write_val: &mut ignore_val,
    };
    step_dyn_figure_in(d, tau, dt, &mut ctx)
}

pub fn step_dyn_figure_in(
    d: &DynFigure,
    tau: f64,
    dt: f64,
    ctx: &mut MotionStepCtx<'_>,
) -> Result<(), String> {
    step_motion_in(d.pose_dyn(), tau, dt, ctx)?;
    if let Some(curve) = d.curve() {
        if let CurveEval::Expr(shape) = &curve.eval {
            step_motion_in(shape.node(), tau, dt, ctx)?;
        }
    }
    Ok(())
}

/// Walk through unrotated const offsets to an integrating Vel node and
/// clamp its state (bounds shifted into the integrator's local frame).
/// Anything else: the output clamp in dyn_pose is the only effect.
pub(crate) fn clamp_integrator(
    d: &Rc<DynNode>,
    lo: (f64, f64),
    hi: (f64, f64),
    state: &mut MotionState,
    readers: &MotionReaders,
    mirror_legacy: bool,
    write_n2: &mut dyn FnMut(MotionStateKey, [f64; 2]),
) {
    match &**d {
        DynNode::Vel { .. } => {
            let key = Rc::as_ptr(d) as *const DynNode as usize;
            let dense_key = state_key_for_node(key, readers);
            if let Some([x, y]) = match state.get(&dense_key) {
                Some(Cell::N(v)) => Some(*v),
                _ => None,
            }
            .or_else(|| (readers.n2)(dense_key))
            {
                let next = [x.clamp(lo.0, hi.0), y.clamp(lo.1, hi.1)];
                if mirror_legacy {
                    state.insert(dense_key, Cell::N(next));
                }
                write_n2(dense_key, next);
            }
        }
        DynNode::Frame(a, b) => {
            if let DynNode::Const(p) = &**a {
                if p.angle_or(0.0).abs() < 1e-12 {
                    clamp_integrator(
                        b,
                        (lo.0 - p.x, lo.1 - p.y),
                        (hi.0 - p.x, hi.1 - p.y),
                        state,
                        readers,
                        mirror_legacy,
                        write_n2,
                    );
                }
            }
        }
        DynNode::Translate { dx, dy, child } => {
            clamp_integrator(child, (lo.0 - dx, lo.1 - dy), (hi.0 - dx, hi.1 - dy), state, readers, mirror_legacy, write_n2);
        }
        _ => {}
    }
}

/// Run an evaluation with an advancing scan context over the bullet's state,
/// then merge the (possibly grown) state back.
pub(crate) fn advance_sites_with_writes<T>(
    state: &mut MotionState,
    base: usize,
    dt: f64,
    readers: MotionReaders,
    mirror_legacy: bool,
    f: impl FnOnce(ScanShared) -> Result<T, String>,
) -> Result<(T, Vec<(MotionStateKey, [f64; 2])>), String> {
    let MotionStateKey::Node(dense_base) = state_key_for_node(base, &readers) else {
        unreachable!("node keys are always stable")
    };
    let io = Rc::new(std::cell::RefCell::new(ScanIo {
        state: std::mem::take(state),
        base,
        dense_base: Some(dense_base),
        counter: 0,
        advance: true,
        dt,
        readers: Some(readers),
        mirror_legacy,
        n2_writes: Vec::new(),
    }));
    let r = f(io.clone());
    let io = Rc::try_unwrap(io)
        .map_err(|_| "scan context escaped".to_string())?
        .into_inner();
    *state = io.state;
    r.map(|value| (value, io.n2_writes))
}

pub fn is_scanned(d: &DynNode) -> bool {
    match d {
        DynNode::Vel { .. }
        | DynNode::RotExpr { .. }
        | DynNode::Stages { .. }
        | DynNode::Path { .. }
        | DynNode::Evolve(_) => true,
        DynNode::Translate { child, .. } => is_scanned(child),
        DynNode::Frame(a, b) => is_scanned(a) || is_scanned(b),
        DynNode::Clamp { child, .. } => is_scanned(child),
        _ => false,
    }
}

pub fn is_scanned_figure(d: &DynFigure) -> bool {
    is_scanned(d.pose_dyn())
        || d.curve()
            .and_then(|curve| match &curve.eval {
                CurveEval::Expr(shape) => Some(is_scanned(shape.node())),
                CurveEval::Straight => None,
            })
            .unwrap_or(false)
}

/// Is this form time-dependent — does it reference the slot-bound
/// parameters t/u (F12), or contain a (live …) read? live means
/// "re-read at eval time" (§3's snap boundary), so a wall-clock signal
/// like (cart m"(live($tick) - t0)/120" 0) must defer exactly like a
/// t-dependent one instead of constant-folding at spawn.
pub(crate) fn contains_t(form: &Form) -> bool {
    match form {
        Form::Sym(s) => &**s == "t" || &**s == "u",
        Form::List(items) => {
            if matches!(items.first(), Some(Form::Sym(s)) if &**s == "live") {
                return true;
            }
            items.iter().any(contains_t)
        }
        Form::Vector(items) => items.iter().any(contains_t),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| contains_t(k) || contains_t(v)),
        _ => false,
    }
}
