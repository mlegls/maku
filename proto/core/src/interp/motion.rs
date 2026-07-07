//! The hot layer: poses, dyn nodes, signal evaluation, scanned motion.

use super::*;
use crate::edn::Form;
use std::collections::HashMap;
use std::rc::Rc;

pub const TICK_RATE: f64 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Degrees, canonical (language.md §11). `None` means the pose only
    /// specifies a point; consumers that need facing derive it from context.
    pub theta: Option<f64>,
}

impl Pose {
    pub const IDENTITY: Pose = Pose { x: 0.0, y: 0.0, theta: Some(0.0) };

    pub const fn point(x: f64, y: f64) -> Pose {
        Pose { x, y, theta: None }
    }

    pub const fn oriented(x: f64, y: f64, theta: f64) -> Pose {
        Pose { x, y, theta: Some(theta) }
    }

    pub fn angle_or(self, default: f64) -> f64 {
        self.theta.unwrap_or(default)
    }

    /// SE(2) composition: self ∘ child (child expressed in self's frame).
    pub fn compose(&self, child: &Pose) -> Pose {
        let parent_th = self.angle_or(0.0);
        let (s, c) = parent_th.to_radians().sin_cos();
        Pose {
            x: self.x + c * child.x - s * child.y,
            y: self.y + s * child.x + c * child.y,
            theta: match (self.theta, child.theta) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }
}

/// Per-bullet scanned state: keyed by dyn-node identity (Rc pointer) or, for
/// stateful expression sites, a hash of (base node, site index).
#[derive(Debug, Clone)]
pub enum Cell {
    N([f64; 2]),
    D(DynPose),
}
pub type MotionState = HashMap<usize, Cell>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionStateKey {
    /// Bridge key: current interpreter state is keyed by DynNode pointer
    /// identity. Lowering should replace this with stable node ids.
    NodePtr(usize),
    /// Expression-local stateful sites under a scanned node. These are
    /// discovered from scan builtin specs during expression lowering.
    ScanSite { base: usize, index: u32 },
    /// Compatibility storage for lazy stage constructors while they remain
    /// interpreted.
    LazyStage { base: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateN2SlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateDynSlotId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct MotionStateSchema {
    pub n2_slots: HashMap<MotionStateKey, StateN2SlotId>,
    pub n2_keys: Vec<MotionStateKey>,
    pub dyn_slots: HashMap<MotionStateKey, StateDynSlotId>,
    pub dyn_keys: Vec<MotionStateKey>,
}

impl MotionStateSchema {
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
}

/// Scan-context IO for stateful signal evaluation: carries the bullet's state
/// cells plus a per-evaluation site counter (stable for a fixed expr tree).
pub struct ScanIo {
    pub state: MotionState,
    pub base: usize,
    pub counter: usize,
    pub advance: bool,
    pub dt: f64,
    pub read_n2: Option<N2Reader>,
    pub n2_writes: Vec<(MotionStateKey, [f64; 2])>,
}
pub type ScanShared = Rc<std::cell::RefCell<ScanIo>>;
pub type N2Reader = Rc<dyn Fn(MotionStateKey) -> Option<[f64; 2]>>;

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

pub(crate) fn site_key(base: usize, counter: usize) -> usize {
    base ^ (0x9e37_79b9_usize.wrapping_mul(counter + 1))
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
    ClosedPt { a: Form, b: Form, polar: bool, env: Env },
    /// Integrated velocity (Scanned): components over slot-bound t.
    Vel { a: Form, b: Form, polar: bool, env: Env },
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
    RotExpr { form: Form, env: Env },
    /// SCANNED.md's `stages`: segment list with per-entity (idx, epoch) state
    /// and explicit exit handoff into Lazy segments.
    Stages { segs: Vec<StageSeg> },
}

#[derive(Debug)]
pub struct StageSeg {
    pub term: StageTerm,
    pub make: StageMake,
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
    Lazy(Val), // an (fn [exit] ...) closure, instantiated at the boundary
}

#[derive(Debug, Clone)]
pub enum CurveDomain {
    Range { min: f64, max: f64 },
    Values(Rc<[f64]>),
}

#[derive(Debug, Clone)]
pub enum SampleSet {
    /// Concrete parameter values supplied by the constructor/caller.
    Values(Rc<[f64]>),
    /// Compatibility sampling for ranged curves. Higher-level constructors
    /// should prefer Values when they need an exact concrete curve.
    Step { resolution: f64 },
}

#[derive(Debug, Clone)]
pub enum CurveEval {
    /// Compatibility straight curve along the local +x axis.
    Straight,
    /// Interpreter representation of eval: (t, u) -> Pose. This is a
    /// prototype lowering detail; the semantic type is still u -> Pose.
    Expr(DynPose),
}

#[derive(Debug, Clone)]
pub struct ParametricCurve {
    pub eval: CurveEval,
    pub domain: CurveDomain,
}

#[derive(Debug, Clone)]
pub struct Curve {
    pub frame: Pose,
    pub spec: ParametricCurve,
}

#[derive(Debug, Clone)]
pub enum Figure {
    Pose(Pose),
    Curve(Curve),
}

pub trait DynKind {
    type Repr: Clone + std::fmt::Debug;
}

#[derive(Debug, Clone)]
pub struct Dyn<T: DynKind> {
    pub(crate) repr: T::Repr,
}

#[derive(Debug, Clone)]
pub enum NumDynRepr {
    Const(f64),
    Expr { form: Form, env: Env },
}

#[derive(Debug, Clone)]
pub enum PoseDynRepr {
    Node(Rc<DynNode>),
}

#[derive(Debug, Clone)]
pub enum FigureDynRepr {
    Pose(DynPose),
    Curve { frame: DynPose, curve: ParametricCurve },
}

impl DynKind for f64 {
    type Repr = NumDynRepr;
}

impl DynKind for Pose {
    type Repr = PoseDynRepr;
}

impl DynKind for Figure {
    type Repr = FigureDynRepr;
}

pub trait DynEval: DynKind + Sized {
    fn eval_dyn(
        d: &Dyn<Self>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
    ) -> Result<Self, String>;
}

pub fn eval_dyn<T: DynEval>(
    d: &Dyn<T>,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<T, String> {
    T::eval_dyn(d, tau, state, sig)
}

pub type DynNum = Dyn<f64>;
pub type DynPose = Dyn<Pose>;
pub type DynFigure = Dyn<Figure>;

impl Dyn<f64> {
    pub fn num(n: f64) -> DynNum {
        Dyn { repr: NumDynRepr::Const(n) }
    }

    pub fn num_expr(form: Form, env: Env) -> DynNum {
        Dyn { repr: NumDynRepr::Expr { form, env } }
    }

    pub fn repr(&self) -> &NumDynRepr {
        &self.repr
    }
}

impl Dyn<Pose> {
    pub fn pose_node(node: Rc<DynNode>) -> DynPose {
        Dyn { repr: PoseDynRepr::Node(node) }
    }

    pub fn node(&self) -> &Rc<DynNode> {
        match &self.repr {
            PoseDynRepr::Node(node) => node,
        }
    }

    pub fn into_node(self) -> Rc<DynNode> {
        match self.repr {
            PoseDynRepr::Node(node) => node,
        }
    }

    pub fn framed(&self, frame: Rc<DynNode>) -> DynPose {
        DynPose::pose_node(Rc::new(DynNode::Frame(frame, self.node().clone())))
    }
}

impl Dyn<Figure> {
    pub fn figure_const(f: Figure) -> DynFigure {
        match f {
            Figure::Pose(p) => DynFigure::pose_node(Rc::new(DynNode::Const(p))),
            Figure::Curve(c) => {
                let frame = DynPose::pose_node(Rc::new(DynNode::Const(c.frame)));
                DynFigure::figure_curve(frame, c.spec)
            }
        }
    }

    pub fn pose(d: DynPose) -> DynFigure {
        Dyn { repr: FigureDynRepr::Pose(d) }
    }

    pub fn pose_node(d: Rc<DynNode>) -> DynFigure {
        DynFigure::pose(DynPose::pose_node(d))
    }

    pub fn figure_curve(frame: DynPose, curve: ParametricCurve) -> DynFigure {
        Dyn { repr: FigureDynRepr::Curve { frame, curve } }
    }

    pub fn repr(&self) -> &FigureDynRepr {
        &self.repr
    }

    pub fn pose_dyn(&self) -> &Rc<DynNode> {
        match &self.repr {
            FigureDynRepr::Pose(d) => d.node(),
            FigureDynRepr::Curve { frame, .. } => frame.node(),
        }
    }

    pub fn curve(&self) -> Option<&ParametricCurve> {
        match &self.repr {
            FigureDynRepr::Curve { curve, .. } => Some(curve),
            FigureDynRepr::Pose(_) => None,
        }
    }

    pub fn framed(&self, frame: Pose) -> DynFigure {
        if frame == Pose::IDENTITY {
            return self.clone();
        }
        let parent = Rc::new(DynNode::Const(frame));
        match &self.repr {
            FigureDynRepr::Pose(d) => DynFigure::pose(d.framed(parent)),
            FigureDynRepr::Curve { frame: child, curve } => {
                DynFigure::figure_curve(child.framed(parent), curve.clone())
            }
        }
    }
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

pub fn collect_node_state(node: &Rc<DynNode>, schema: &mut MotionStateSchema) {
    let base = Rc::as_ptr(node) as usize;
    match &**node {
        DynNode::Vel { a, b, .. } => {
            schema.intern_n2(MotionStateKey::NodePtr(base));
            let index = collect_scan_sites(a, base, 0, schema);
            collect_scan_sites(b, base, index, schema);
        }
        DynNode::ClosedPt { a, b, .. } => {
            let index = collect_scan_sites(a, base, 0, schema);
            collect_scan_sites(b, base, index, schema);
        }
        DynNode::RotExpr { form, .. } => {
            collect_scan_sites(form, base, 0, schema);
        }
        DynNode::Path { curve, progress, .. } => {
            collect_scan_sites(progress, base, 0, schema);
            collect_node_state(curve, schema);
        }
        DynNode::Stages { segs } => {
            schema.intern_n2(MotionStateKey::NodePtr(base));
            if segs.iter().any(|seg| matches!(seg.make, StageMake::Lazy(_))) {
                schema.intern_dyn(MotionStateKey::LazyStage { base });
            }
            for seg in segs {
                if let StageTerm::Until(pred, _) = &seg.term {
                    collect_scan_sites(pred, base, 0, schema);
                }
                if let StageMake::Ready(d) = &seg.make {
                    collect_pose_state(d, schema);
                }
            }
        }
        DynNode::Translate { child, .. } | DynNode::Clamp { child, .. } => {
            collect_node_state(child, schema);
        }
        DynNode::Frame(a, b) => {
            collect_node_state(a, schema);
            collect_node_state(b, schema);
        }
        DynNode::Const(_) | DynNode::Linear { .. } | DynNode::Live { .. } => {}
    }
}

pub fn collect_scan_sites(
    form: &Form,
    base: usize,
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
    fn eval_dyn(
        d: &Dyn<f64>,
        tau: f64,
        _state: &MotionState,
        sig: &SigEnv,
    ) -> Result<f64, String> {
        match d.repr() {
            NumDynRepr::Const(n) => Ok(*n),
            NumDynRepr::Expr { form, env } => eval_sig(form, env, sig, tau, 0.0, None, None)?.num(),
        }
    }
}

impl DynEval for Figure {
    fn eval_dyn(
        d: &Dyn<Figure>,
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
}

impl DynEval for Pose {
    fn eval_dyn(
        d: &Dyn<Pose>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
    ) -> Result<Pose, String> {
        dyn_node_pose(d.node(), tau, state, sig)
    }
}

pub fn eval_curve_pose(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    match eval {
        CurveEval::Straight => Ok(Pose::oriented(u, 0.0, 0.0)),
        CurveEval::Expr(d) => dyn_pose_u(d, tau, u, state, sig),
    }
}

/// Compatibility extended values before spawn lowering.
#[derive(Debug, Clone)]
pub enum CurveBacking {
    /// Surface `laser` syntax currently lowers to this representation.
    Parametric {
        curve: ParametricCurve,
        sample_set: SampleSet,
        u_max_sig: Option<DynNum>, // signal-valued :u-max (varLength)
        warn: f64,
        active: f64,
        width: f64,
        /// Swept hot fraction as a function of curve age t.
        fill_sig: Option<DynNum>,
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
    };
    let mut w = World::default(); // signals never touch the world (§2)
    evaluate(form, &e, &mut ctx, &mut w)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_pt(
    a: &Form,
    b: &Form,
    polar: bool,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
) -> Result<(f64, f64), String> {
    let av = eval_sig(a, env, sig, tau, u, scan.clone(), pos)?.num()?;
    let bv = eval_sig(b, env, sig, tau, u, scan, pos)?.num()?;
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        Ok((av * c, av * s))
    } else {
        Ok((av, bv))
    }
}

/// Read-only scan context over a clone of the bullet's state.
pub(crate) fn read_scan(state: &MotionState, base: usize) -> ScanShared {
    read_scan_with_dense(state, base, None)
}

pub(crate) fn read_scan_with_dense(
    state: &MotionState,
    base: usize,
    read_n2: Option<N2Reader>,
) -> ScanShared {
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base,
        counter: 0,
        advance: false,
        dt: 0.0,
        read_n2,
        n2_writes: Vec::new(),
    }))
}

pub fn dyn_node_pose(d: &DynNode, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    dyn_node_pose_u(d, tau, 0.0, state, sig)
}

pub fn dyn_pose(d: &DynPose, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    eval_dyn(d, tau, state, sig)
}

pub fn dyn_figure_pose(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    dyn_node_pose(d.pose_dyn(), tau, state, sig)
}

pub fn dyn_figure_pose_with_dense(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    read_n2: &N2Reader,
) -> Result<Pose, String> {
    dyn_node_pose_u_with_dense(d.pose_dyn(), tau, 0.0, state, sig, read_n2)
}

pub fn eval_dyn_figure(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Figure, String> {
    eval_dyn(d, tau, state, sig)
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

pub fn dyn_node_pose_u(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    let read_n2: N2Reader = Rc::new(|_| None);
    dyn_node_pose_u_with_dense(d, tau, u, state, sig, &read_n2)
}

pub fn dyn_node_pose_u_with_dense(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
    read_n2: &N2Reader,
) -> Result<Pose, String> {
    match d {
        DynNode::Const(p) => Ok(*p),
        DynNode::Linear { vx, vy } => Ok(Pose {
            x: vx * tau,
            y: vy * tau,
            theta: Some(vy.atan2(*vx).to_degrees()),
        }),
        DynNode::ClosedPt { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let (x, y) = eval_pt(
                a,
                b,
                *polar,
                env,
                sig,
                tau,
                u,
                Some(read_scan_with_dense(state, key, Some(read_n2.clone()))),
                None,
            )?;
            let eps = 1.0 / TICK_RATE;
            let (x2, y2) = eval_pt(
                a,
                b,
                *polar,
                env,
                sig,
                tau + eps,
                u,
                Some(read_scan_with_dense(state, key, Some(read_n2.clone()))),
                None,
            )?;
            Ok(Pose::oriented(x, y, (y2 - y).atan2(x2 - x).to_degrees()))
        }
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let [x, y] = read_n2(MotionStateKey::NodePtr(key))
                .or_else(|| match state.get(&key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            let (vx, vy) = eval_pt(
                a,
                b,
                *polar,
                env,
                sig,
                tau,
                u,
                Some(read_scan_with_dense(state, key, Some(read_n2.clone()))),
                Some((x, y)),
            )?;
            Ok(Pose::oriented(x, y, vy.atan2(vx).to_degrees()))
        }
        DynNode::Live { channel } => {
            let (x, y) = sig.channel_pos(channel);
            Ok(Pose::point(x, y))
        }
        DynNode::Clamp { lo, hi, child } => {
            let p = dyn_node_pose_u_with_dense(child, tau, 0.0, state, sig, read_n2)?;
            Ok(Pose { x: p.x.clamp(lo.0, hi.0), y: p.y.clamp(lo.1, hi.1), theta: p.theta })
        }
        DynNode::RotExpr { form, env } => {
            let key = d as *const DynNode as usize;
            let th = eval_sig(
                form,
                env,
                sig,
                tau,
                u,
                Some(read_scan_with_dense(state, key, Some(read_n2.clone()))),
                Some((0.0, 0.0)),
            )?
            .num()?;
            Ok(Pose::oriented(0.0, 0.0, th))
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let [idx, epoch] = match state.get(&key) {
                Some(Cell::N(v)) => *v,
                _ => [0.0, 0.0],
            };
            let cur = stage_dyn(segs, idx as usize, state, key)?;
            dyn_node_pose_u_with_dense(cur.node(), tau - epoch, u, state, sig, read_n2)
        }
        DynNode::Translate { dx, dy, child } => {
            let p = dyn_node_pose_u_with_dense(child, tau, u, state, sig, read_n2)?;
            Ok(Pose { x: p.x + dx, y: p.y + dy, theta: p.theta })
        }
        DynNode::Path { curve, progress, env } => {
            let key = d as *const DynNode as usize;
            let u = eval_sig(
                progress,
                env,
                sig,
                tau,
                0.0,
                Some(read_scan_with_dense(state, key, Some(read_n2.clone()))),
                None,
            )?
            .num()?;
            dyn_node_pose_u_with_dense(curve, tau, u, state, sig, read_n2)
        }
        DynNode::Frame(parent, child) => {
            let pp = dyn_node_pose_u_with_dense(parent, tau, u, state, sig, read_n2)?;
            let cp = dyn_node_pose_u_with_dense(child, tau, u, state, sig, read_n2)?;
            Ok(pp.compose(&cp))
        }
    }
}

/// The dyn for the current segment of a Stages node.
pub(crate) fn stage_dyn(
    segs: &[StageSeg],
    idx: usize,
    state: &MotionState,
    key: usize,
) -> Result<DynPose, String> {
    let seg = segs.get(idx).ok_or("stages: segment index out of range")?;
    match &seg.make {
        StageMake::Ready(d) => Ok(d.clone()),
        StageMake::Lazy(_) => match state.get(&(key + 1)) {
            Some(Cell::D(d)) => Ok(d.clone()),
            _ => Err("stages: lazy segment not instantiated".into()),
        },
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
    let read_n2: N2Reader = Rc::new(|_| None);
    step_motion_with_dense(d, tau, dt, state, sig, &read_n2, &mut |_, _| {})
}

fn step_motion_with_dense(
    d: &DynNode,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
    read_n2: &N2Reader,
    write_n2: &mut dyn FnMut(MotionStateKey, [f64; 2]),
) -> Result<(), String> {
    match d {
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let [x, y] = read_n2(MotionStateKey::NodePtr(key))
                .or_else(|| match state.get(&key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            let ((vx, vy), writes) = advance_sites_with_writes(state, key, dt, Some(read_n2.clone()), |scan| {
                eval_pt(a, b, *polar, env, sig, tau, 0.0, Some(scan), Some((x, y)))
            })?;
            for (key, value) in writes {
                write_n2(key, value);
            }
            let next = [x + vx * dt, y + vy * dt];
            state.insert(key, Cell::N(next));
            write_n2(MotionStateKey::NodePtr(key), next);
            Ok(())
        }
        DynNode::RotExpr { form, env } => {
            let key = d as *const DynNode as usize;
            let (_, writes) = advance_sites_with_writes(state, key, dt, Some(read_n2.clone()), |scan| {
                eval_sig(form, env, sig, tau, 0.0, Some(scan), Some((0.0, 0.0)))?.num()
            })?;
            for (key, value) in writes {
                write_n2(key, value);
            }
            Ok(())
        }
        DynNode::Path { curve, progress, env } => {
            let key = d as *const DynNode as usize;
            let (_, writes) = advance_sites_with_writes(state, key, dt, Some(read_n2.clone()), |scan| {
                eval_sig(progress, env, sig, tau, 0.0, Some(scan), None)?.num()
            })?;
            for (key, value) in writes {
                write_n2(key, value);
            }
            step_motion_with_dense(curve, tau, dt, state, sig, read_n2, write_n2)
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let [mut idx, mut epoch] = match state.get(&key) {
                Some(Cell::N(v)) => *v,
                _ => [0.0, 0.0],
            };
            // terminate current segment?
            let seg = segs.get(idx as usize).ok_or("stages: bad segment")?;
            let local = tau - epoch;
            let done = match &seg.term {
                StageTerm::Dur(dsec) => local >= *dsec,
                StageTerm::Until(pred, penv) => {
                    let scan = read_scan(state, key);
                    truthy(&eval_sig(pred, penv, sig, local, 0.0, Some(scan), None)?)
                }
                StageTerm::Forever => false,
            };
            if done && (idx as usize) + 1 < segs.len() {
                // exit snapshot from the finishing segment
                let cur = stage_dyn(segs, idx as usize, state, key)?;
                let p1 = dyn_pose_u(&cur, local, 0.0, state, sig)?;
                let p0 = dyn_pose_u(&cur, (local - dt).max(0.0), 0.0, state, sig)?;
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
                if let StageMake::Lazy(f) = &segs[idx as usize].make {
                    let mut ctx = Ctx {
                        sig: sig.clone(),
                        ambient: Pose::IDENTITY,
                        scan: None,
                        patterns: Rc::new(HashMap::new()),
                        macros: Rc::new(HashMap::new()),
                        deferred: Vec::new(),
                    };
                    let mut w = World::default();
                    let dv = apply_fn(f.clone(), &[exit], &mut ctx, &mut w, false)?;
                    state.insert(key + 1, Cell::D(as_dyn(dv)?));
                }
            }
            state.insert(key, Cell::N([idx, epoch]));
            let cur = stage_dyn(segs, idx as usize, state, key)?;
            // step the inner dyn on the segment-local clock
            step_motion_with_dense(cur.node(), tau - epoch, dt, state, sig, read_n2, write_n2)
        }
        DynNode::Translate { child, .. } => {
            step_motion_with_dense(child, tau, dt, state, sig, read_n2, write_n2)
        }
        DynNode::Frame(a, b) => {
            step_motion_with_dense(a, tau, dt, state, sig, read_n2, write_n2)?;
            step_motion_with_dense(b, tau, dt, state, sig, read_n2, write_n2)
        }
        DynNode::Clamp { lo, hi, child } => {
            step_motion_with_dense(child, tau, dt, state, sig, read_n2, write_n2)?;
            clamp_integrator(child, *lo, *hi, state, write_n2);
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
    let read_n2: N2Reader = Rc::new(|_| None);
    step_dyn_figure_with_dense(d, tau, dt, state, sig, read_n2, &mut |_, _| {})
}

pub fn step_dyn_figure_with_dense(
    d: &DynFigure,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
    read_n2: N2Reader,
    write_n2: &mut dyn FnMut(MotionStateKey, [f64; 2]),
) -> Result<(), String> {
    step_motion_with_dense(d.pose_dyn(), tau, dt, state, sig, &read_n2, write_n2)?;
    if let Some(curve) = d.curve() {
        if let CurveEval::Expr(shape) = &curve.eval {
            step_motion_with_dense(shape.node(), tau, dt, state, sig, &read_n2, write_n2)?;
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
    write_n2: &mut dyn FnMut(MotionStateKey, [f64; 2]),
) {
    match &**d {
        DynNode::Vel { .. } => {
            let key = Rc::as_ptr(d) as *const DynNode as usize;
            if let Some(Cell::N([x, y])) = state.get(&key).cloned() {
                let next = [x.clamp(lo.0, hi.0), y.clamp(lo.1, hi.1)];
                state.insert(key, Cell::N(next));
                write_n2(MotionStateKey::NodePtr(key), next);
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
                        write_n2,
                    );
                }
            }
        }
        DynNode::Translate { dx, dy, child } => {
            clamp_integrator(child, (lo.0 - dx, lo.1 - dy), (hi.0 - dx, hi.1 - dy), state, write_n2);
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
    read_n2: Option<N2Reader>,
    f: impl FnOnce(ScanShared) -> Result<T, String>,
) -> Result<(T, Vec<(MotionStateKey, [f64; 2])>), String> {
    let io = Rc::new(std::cell::RefCell::new(ScanIo {
        state: std::mem::take(state),
        base,
        counter: 0,
        advance: true,
        dt,
        read_n2,
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
        DynNode::Vel { .. } | DynNode::RotExpr { .. } | DynNode::Stages { .. } | DynNode::Path { .. } => true,
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
