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
    pub th: f64, // degrees, canonical (language.md §11)
}

impl Pose {
    pub const IDENTITY: Pose = Pose { x: 0.0, y: 0.0, th: 0.0 };
    /// SE(2) composition: self ∘ child (child expressed in self's frame).
    pub fn compose(&self, child: &Pose) -> Pose {
        let (s, c) = self.th.to_radians().sin_cos();
        Pose {
            x: self.x + c * child.x - s * child.y,
            y: self.y + s * child.x + c * child.y,
            th: self.th + child.th,
        }
    }
}

/// Per-bullet scanned state: keyed by dyn-node identity (Rc pointer) or, for
/// stateful expression sites (slew/smooth), a hash of (base node, site index).
#[derive(Debug, Clone)]
pub enum Cell {
    N([f64; 2]),
    D(Rc<DynNode>),
}
pub type MotionState = HashMap<usize, Cell>;

/// Scan-context IO for stateful signal evaluation: carries the bullet's state
/// cells plus a per-evaluation site counter (stable for a fixed expr tree).
pub struct ScanIo {
    pub state: MotionState,
    pub base: usize,
    pub counter: usize,
    pub advance: bool,
    pub dt: f64,
}
pub type ScanShared = Rc<std::cell::RefCell<ScanIo>>;

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
    /// Closed position expression over slot-bound t (and u, for laser shapes).
    ClosedPt { a: Form, b: Form, polar: bool, env: Env },
    /// Integrated velocity (Scanned): components over slot-bound t.
    Vel { a: Form, b: Form, polar: bool, env: Env },
    /// Point-translation (the `+` of the two-op algebra): θ untouched.
    Translate { dx: f64, dy: f64, child: Rc<DynNode> },
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
    /// SCANNED.md's `stages`: segment list with per-bullet (idx, epoch) state
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
    Ready(Rc<DynNode>),
    Lazy(Val), // an (fn [exit] ...) closure, instantiated at the boundary
}

/// Extended entity (§6 axis materialization): a laser = anchor dyn + shape
/// over (t, u) + lifecycle window.
#[derive(Debug)]
pub struct ExtLaser {
    pub anchor: Rc<DynNode>,
    pub shape: Option<Rc<DynNode>>, // None = straight along frame +x
    pub warn: f64,
    pub active: f64,
    pub u_max: f64,
    pub u_max_sig: Option<(Form, Env)>, // signal-valued :u-max (varLength)
    pub resolution: f64,
    pub width: f64,
}

/// A pather value pre-spawn: the guide dyn plus its remembrance window.
#[derive(Debug)]
pub struct ExtPather {
    pub anchor: Rc<DynNode>,
    pub window: f64,
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
        e = e.bind("pos".into(), Val::Vec2 { x: px, y: py });
    }
    let mut ctx = Ctx {
        sig: sig.clone(),
        ambient: Pose::IDENTITY,
        scan,
        patterns: Rc::new(HashMap::new()),
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
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base,
        counter: 0,
        advance: false,
        dt: 0.0,
    }))
}

pub fn dyn_pose(d: &DynNode, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    dyn_pose_u(d, tau, 0.0, state, sig)
}

pub fn dyn_pose_u(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    match d {
        DynNode::Const(p) => Ok(*p),
        DynNode::Linear { vx, vy } => Ok(Pose {
            x: vx * tau,
            y: vy * tau,
            th: vy.atan2(*vx).to_degrees(),
        }),
        DynNode::ClosedPt { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let (x, y) = eval_pt(a, b, *polar, env, sig, tau, u, Some(read_scan(state, key)), None)?;
            let eps = 1.0 / TICK_RATE;
            let (x2, y2) =
                eval_pt(a, b, *polar, env, sig, tau + eps, u, Some(read_scan(state, key)), None)?;
            Ok(Pose { x, y, th: (y2 - y).atan2(x2 - x).to_degrees() })
        }
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let [x, y] = match state.get(&key) {
                Some(Cell::N(v)) => *v,
                _ => [0.0, 0.0],
            };
            let (vx, vy) =
                eval_pt(a, b, *polar, env, sig, tau, u, Some(read_scan(state, key)), Some((x, y)))?;
            Ok(Pose { x, y, th: vy.atan2(vx).to_degrees() })
        }
        DynNode::Live { channel } => {
            let (x, y) = sig.channel_pos(channel);
            Ok(Pose { x, y, th: 0.0 })
        }
        DynNode::Clamp { lo, hi, child } => {
            let p = dyn_pose(child, tau, state, sig)?;
            Ok(Pose { x: p.x.clamp(lo.0, hi.0), y: p.y.clamp(lo.1, hi.1), th: p.th })
        }
        DynNode::RotExpr { form, env } => {
            let key = d as *const DynNode as usize;
            let th = eval_sig(form, env, sig, tau, u, Some(read_scan(state, key)), Some((0.0, 0.0)))?
                .num()?;
            Ok(Pose { x: 0.0, y: 0.0, th })
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let [idx, epoch] = match state.get(&key) {
                Some(Cell::N(v)) => *v,
                _ => [0.0, 0.0],
            };
            let cur = stage_dyn(segs, idx as usize, state, key)?;
            dyn_pose_u(&cur, tau - epoch, u, state, sig)
        }
        DynNode::Translate { dx, dy, child } => {
            let p = dyn_pose_u(child, tau, u, state, sig)?;
            Ok(Pose { x: p.x + dx, y: p.y + dy, th: p.th })
        }
        DynNode::Frame(parent, child) => {
            let pp = dyn_pose_u(parent, tau, u, state, sig)?;
            let cp = dyn_pose_u(child, tau, u, state, sig)?;
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
) -> Result<Rc<DynNode>, String> {
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
    match d {
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let [x, y] = match state.get(&key) {
                Some(Cell::N(v)) => *v,
                _ => [0.0, 0.0],
            };
            let (vx, vy) = advance_sites(state, key, dt, |scan| {
                eval_pt(a, b, *polar, env, sig, tau, 0.0, Some(scan), Some((x, y)))
            })?;
            state.insert(key, Cell::N([x + vx * dt, y + vy * dt]));
            Ok(())
        }
        DynNode::RotExpr { form, env } => {
            let key = d as *const DynNode as usize;
            advance_sites(state, key, dt, |scan| {
                eval_sig(form, env, sig, tau, 0.0, Some(scan), Some((0.0, 0.0)))?.num()
            })?;
            Ok(())
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
                    (Val::Kw("pos".into()), Val::Vec2 { x: p1.x, y: p1.y }),
                    (
                        Val::Kw("vel".into()),
                        Val::Vec2 { x: (p1.x - p0.x) / dt, y: (p1.y - p0.y) / dt },
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
            step_motion(&cur, tau - epoch, dt, state, sig)
        }
        DynNode::Translate { child, .. } => step_motion(child, tau, dt, state, sig),
        DynNode::Frame(a, b) => {
            step_motion(a, tau, dt, state, sig)?;
            step_motion(b, tau, dt, state, sig)
        }
        DynNode::Clamp { lo, hi, child } => {
            step_motion(child, tau, dt, state, sig)?;
            clamp_integrator(child, *lo, *hi, state);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Walk through unrotated const offsets to an integrating Vel node and
/// clamp its state (bounds shifted into the integrator's local frame).
/// Anything else: the output clamp in dyn_pose is the only effect.
pub(crate) fn clamp_integrator(d: &Rc<DynNode>, lo: (f64, f64), hi: (f64, f64), state: &mut MotionState) {
    match &**d {
        DynNode::Vel { .. } => {
            let key = Rc::as_ptr(d) as *const DynNode as usize;
            if let Some(Cell::N([x, y])) = state.get(&key).cloned() {
                state.insert(
                    key,
                    Cell::N([x.clamp(lo.0, hi.0), y.clamp(lo.1, hi.1)]),
                );
            }
        }
        DynNode::Frame(a, b) => {
            if let DynNode::Const(p) = &**a {
                if p.th.abs() < 1e-12 {
                    clamp_integrator(
                        b,
                        (lo.0 - p.x, lo.1 - p.y),
                        (hi.0 - p.x, hi.1 - p.y),
                        state,
                    );
                }
            }
        }
        DynNode::Translate { dx, dy, child } => {
            clamp_integrator(child, (lo.0 - dx, lo.1 - dy), (hi.0 - dx, hi.1 - dy), state);
        }
        _ => {}
    }
}

/// Run an evaluation with an advancing scan context over the bullet's state,
/// then merge the (possibly grown) state back.
pub(crate) fn advance_sites<T>(
    state: &mut MotionState,
    base: usize,
    dt: f64,
    f: impl FnOnce(ScanShared) -> Result<T, String>,
) -> Result<T, String> {
    let io = Rc::new(std::cell::RefCell::new(ScanIo {
        state: std::mem::take(state),
        base,
        counter: 0,
        advance: true,
        dt,
    }));
    let r = f(io.clone());
    *state = Rc::try_unwrap(io)
        .map_err(|_| "scan context escaped".to_string())?
        .into_inner()
        .state;
    r
}

pub fn is_scanned(d: &DynNode) -> bool {
    match d {
        DynNode::Vel { .. } | DynNode::RotExpr { .. } | DynNode::Stages { .. } => true,
        DynNode::Translate { child, .. } => is_scanned(child),
        DynNode::Frame(a, b) => is_scanned(a) || is_scanned(b),
        DynNode::Clamp { child, .. } => is_scanned(child),
        _ => false,
    }
}

/// Does a form reference the slot-bound parameters t/u? (F12)
pub(crate) fn contains_t(form: &Form) -> bool {
    match form {
        Form::Sym(s) => &**s == "t" || &**s == "u",
        Form::List(items) | Form::Vector(items) => items.iter().any(contains_t),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| contains_t(k) || contains_t(v)),
        _ => false,
    }
}
