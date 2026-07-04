//! Control-layer interpreter + prototype signal representation.
//!
//! Per language.md §2: Actions are inert data; the scheduler (sim.rs) walks
//! them with an explicit stack. Expressions evaluate instantly and purely;
//! only Action leaves interact with time or the world. Seq bodies are LAZY.
//!
//! Signals evaluate against a SigEnv (defs + injected snapshot) and never
//! touch the world — the spec's purity rule is also what breaks the borrow
//! cycle here. Scanned nodes (Vel) keep per-bullet state keyed by node
//! identity.
//!
//! Two rules this prototype surfaced for the spec:
//!  - `let` in action position defers action-valued bindings to scheduler
//!    reach-time (a spawn executed at evaluation time would miss the ambient
//!    frame the distribution law owes it).
//!  - Ambient frames do not cross `fn` boundaries (manipulate callbacks spawn
//!    in world coordinates; lexical distribution stops at lambdas, the same
//!    way it stops at embedded patterns).

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

fn site_key(base: usize, counter: usize) -> usize {
    base ^ (0x9e37_79b9_usize.wrapping_mul(counter + 1))
}

fn shortest_arc(from: f64, to: f64) -> f64 {
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
    let mut ctx = Ctx { sig: sig.clone(), ambient: Pose::IDENTITY, scan };
    let mut w = World::default(); // signals never touch the world (§2)
    evaluate(form, &e, &mut ctx, &mut w)
}

#[allow(clippy::too_many_arguments)]
fn eval_pt(
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
fn read_scan(state: &MotionState, base: usize) -> ScanShared {
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
fn stage_dyn(
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
                    let mut ctx = Ctx { sig: sig.clone(), ambient: Pose::IDENTITY, scan: None };
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
        _ => Ok(()),
    }
}

/// Run an evaluation with an advancing scan context over the bullet's state,
/// then merge the (possibly grown) state back.
fn advance_sites<T>(
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
        _ => false,
    }
}

/// Does a form reference the slot-bound parameters t/u? (F12)
fn contains_t(form: &Form) -> bool {
    match form {
        Form::Sym(s) => &**s == "t" || &**s == "u",
        Form::List(items) | Form::Vector(items) => items.iter().any(contains_t),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| contains_t(k) || contains_t(v)),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// World: bullets + events. The control layer's mutable half.

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub family: String,
    pub color: String,
    pub variant: String,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Point,
    Laser {
        shape: Option<Rc<DynNode>>,
        warn: f64,
        active: f64,
        u_max: f64,
        u_max_sig: Option<(Form, Env)>,
        resolution: f64,
    },
}

/// A signal-valued meta tag sampled at render time (e.g. :hue).
#[derive(Debug, Clone)]
pub struct MetaSig {
    pub form: Form,
    pub env: Env,
    pub idx: usize, // element index for array-valued tag signals
}

#[derive(Clone)]
pub struct Bullet {
    pub id: u64,
    /// Gameplay team tag (F20: derived channels like $nearest-enemy are
    /// queries over tagged entities). None = hostile fire; :player = player
    /// fire (hits :enemy entities); :enemy = a target with hp.
    pub team: Option<Rc<str>>,
    pub kind: Kind,
    pub motion: Rc<DynNode>,
    pub birth: u64,
    pub style: Style,
    pub alive: bool,
    pub state: MotionState,
    pub scanned: bool,
    pub hue: Option<MetaSig>,
    /// Collider set — archetype data, Rc-shared across a spawn's elements.
    pub colliders: Rc<[Collider]>,
    /// User-defined numeric columns (§9's sidecar, inline for the
    /// prototype). hp is not special — it's just the first custom column.
    pub cols: Vec<(Rc<str>, f64)>,
    /// Standing edge-triggers over own columns — archetype data. Death is
    /// not special: :hp n synthesizes (col hp ≤ 0 → cull + event :died).
    pub triggers: Rc<[TriggerRule]>,
    /// Damage on contact (:damage meta): a number, a DMK player() map whose
    /// :hit is taken, or a PURE FUNCTION (fn [self other] num) evaluated at
    /// contact — contacts are rare, so interpreting there is free.
    pub damage: Val,
    /// Grazes count once per bullet.
    pub grazed: bool,
    /// Last tick's position (collision pass) — contact velocity is the
    /// finite difference, uniform across Closed and Scanned motion.
    pub prev_pos: Option<(f64, f64)>,
}

impl Bullet {
    pub fn col_get(&self, name: &str) -> Option<f64> {
        self.cols.iter().find(|(k, _)| &**k == name).map(|(_, v)| *v)
    }

    pub fn col_set(&mut self, name: &Rc<str>, v: f64) {
        match self.cols.iter_mut().find(|(k, _)| k == name) {
            Some((_, slot)) => *slot = v,
            None => self.cols.push((name.clone(), v)),
        }
    }
}

/// A standing rule over an entity's own columns: when `col ≤ leq` first
/// becomes true (edge-triggered; the latch is itself a column, so it
/// snapshots and scrubs), emit the event and optionally cull. The same
/// mechanism covers death, HP-gated boss phases, enrage thresholds, lives.
#[derive(Clone, Debug)]
pub struct TriggerRule {
    /// Event name; also keys the latch column.
    pub name: Rc<str>,
    /// Precomputed latch column key.
    pub latch: Rc<str>,
    pub col: Rc<str>,
    pub leq: f64,
    pub cull: bool,
}

impl TriggerRule {
    pub fn new(name: &str, col: &str, leq: f64, cull: bool) -> TriggerRule {
        TriggerRule {
            name: name.into(),
            latch: format!("{}#fired", name).into(),
            col: col.into(),
            leq,
            cull,
        }
    }
}

/// Collision layers. The engine's interaction matrix pairs them:
/// damage × player-hurtbox → hit, graze × player-hurtbox → graze,
/// shot × hurt → damage resolution. The player hurtbox is implicit
/// (an engine-side entity at $player).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    Damage, // hostile fire
    Graze,  // graze ring, conventionally on the bullet
    Shot,   // player fire
    Hurt,   // enemy hurtbox
}

/// One collider: a circle in the owner's frame. Lasers derive capsule
/// chains from their sampled curve at collision time instead.
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub layer: Layer,
    pub r: f64,
}

/// Default collider sets by team — archetype data (built once per spawn).
pub fn default_colliders(team: Option<&str>, family: &str, hitbox: Option<f64>) -> Vec<Collider> {
    let style_r = match family {
        "lstar" | "gglcircle" => 0.20,
        "gem" | "star" => 0.09,
        _ => 0.12,
    };
    match team {
        None => vec![
            Collider { layer: Layer::Damage, r: hitbox.unwrap_or(style_r) },
            Collider { layer: Layer::Graze, r: 0.35 },
        ],
        Some("player") => vec![Collider { layer: Layer::Shot, r: hitbox.unwrap_or(style_r) }],
        Some("enemy") => vec![Collider { layer: Layer::Hurt, r: hitbox.unwrap_or(0.3) }],
        Some(_) => vec![],
    }
}

#[derive(Clone)]
pub struct World {
    pub tick: u64,
    pub next_id: u64,
    pub bullets: Vec<Bullet>,
    pub events: Vec<Event>,
    pub rng: u64,
    pub boss: Pose,
    pub boss_anim: Option<BossAnim>,
    /// Gameplay counters — part of World so they snapshot/scrub with it.
    pub graze: u64,
    pub player_hits: u64,
    /// Ticks are ignored for player hits until this tick (post-hit iframes).
    pub iframe_until: u64,
}

/// A gameplay event: emitted by collision or by the (event :name) action.
/// Events live in World, so scrubbing rewinds and replays them too.
#[derive(Clone, Debug)]
pub struct Event {
    pub tick: u64,
    pub name: String,
    pub pos: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct BossAnim {
    pub from: Pose,
    pub to: (f64, f64),
    pub start: u64,
    pub dur: u64,
}

impl Default for World {
    fn default() -> Self {
        World {
            tick: 0,
            next_id: 0,
            bullets: Vec::new(),
            events: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            boss: Pose::IDENTITY,
            boss_anim: None,
            graze: 0,
            player_hits: 0,
            iframe_until: 0,
        }
    }
}

impl World {
    /// Deterministic splitmix64-ish stream (counter-based enough for the
    /// prototype: same run order → same stream → replays agree).
    pub fn next_rand(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn find(&self, id: u64) -> Option<usize> {
        self.bullets.iter().position(|b| b.id == id && b.alive)
    }
}

// ---------------------------------------------------------------------------
// Values.

#[derive(Clone, Debug)]
pub enum Val {
    Num(f64),
    Bool(bool),
    Kw(Rc<str>),
    Str(Rc<str>),
    Vec2 { x: f64, y: f64 },
    Pose(Pose),
    Arr(Rc<Vec<Val>>),
    Map(Rc<Vec<(Val, Val)>>),
    Dyn(Rc<DynNode>),
    Ext(Rc<ExtLaser>),
    Action(Rc<ActionV>),
    Fn { params: Rc<[Rc<str>]>, body: Rc<[Form]>, env: Env },
    Builtin(Rc<str>),
    Handle(u64),
    /// A deferred signal expression (shared stateful instance, §5): forced
    /// when referenced inside a scan context.
    Thunk(Rc<(Form, Env)>),
    Nothing,
}

impl Val {
    pub fn num(&self) -> Result<f64, String> {
        match self {
            Val::Num(n) => Ok(*n),
            v => Err(format!("expected number, got {:?}", v)),
        }
    }
}

/// One spawn element: a plain dyn or an extended entity, plus its §5 shape
/// path — (axis_len, index) per array level, root to leaf — for the F15
/// leading-axis/by-length meta rule.
pub struct SpawnElem {
    pub motion: Rc<DynNode>,
    pub kind: Kind,
    pub path: Vec<(usize, usize)>,
}

/// Inert action descriptions. Bodies are unevaluated forms + env (lazy seq).
#[derive(Debug)]
pub enum ActionV {
    Seq { items: Rc<[Form]>, env: Env },
    Dotimes {
        var: Rc<str>,
        n: f64,
        seq_binds: Vec<(Rc<str>, Val)>,
        every_ticks: u64,
        body: Rc<[Form]>,
        env: Env,
    },
    Loop { names: Vec<Rc<str>>, inits: Vec<Val>, body: Rc<[Form]>, env: Env },
    Recur(Vec<Val>),
    InFrame { frame: FrameSpec, inner: Rc<ActionV> },
    /// Bindings whose values are actions execute at scheduler reach-time
    /// (inside the ambient frame); their results (e.g. spawn handles) bind.
    Let { binds: Vec<(Rc<str>, Val)>, body: Rc<[Form]>, env: Env },
    Spawn {
        dyns: Vec<SpawnMade>,
        styles: Vec<Style>,
        hues: Vec<Option<MetaSig>>,
        team: Option<Rc<str>>,
        cols: Vec<(Rc<str>, f64)>,
        triggers: Rc<[TriggerRule]>,
        damage: Val,
        colliders: Rc<[Collider]>,
    },
    Manipulate { targets: Vec<u64>, callback: Val },
    Cull { target: u64 },
    Wait { ticks: u64 },
    WaitFor { pred: Form, env: Env },
    DefVar { name: Rc<str>, init: Val },
    SetVar { name: Rc<str>, val: Val },
    /// Boss/self-entity eased move (derived from remat per the spec; the
    /// prototype animates the world's boss anchor and blocks for `dur`).
    Move { dur_ticks: u64, dest: (f64, f64) },
    Fork(Rc<ActionV>),
    Par(Vec<Rc<ActionV>>),
    Event { channel: Rc<str> },
    Nothing,
}

#[derive(Debug, Clone)]
pub enum FrameSpec {
    Const(Pose),
    /// A signal-valued frame (e.g. an unexpressed guide). Its scan state
    /// lives in whichever bullet shares the node (§5 shared instances); the
    /// scheduler resolves the pose at action time.
    Node(Rc<DynNode>),
}

#[derive(Debug)]
pub struct SpawnMade {
    pub motion: Rc<DynNode>,
    pub kind: Kind,
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

// ---------------------------------------------------------------------------
// Card: top-level definitions.

pub struct Pattern {
    pub name: String,
    pub params: Vec<(Rc<str>, Form)>,
    pub body: Rc<[Form]>,
}

pub struct Card {
    pub patterns: HashMap<String, Pattern>,
    pub order: Vec<String>,
    pub defs: HashMap<String, Form>,
}

pub fn load_card(forms: &[Form]) -> Result<Card, String> {
    let mut patterns = HashMap::new();
    let mut order = Vec::new();
    let mut defs = HashMap::new();
    for f in forms {
        if let Form::List(items) = f {
            match items.first() {
                Some(Form::Sym(s)) if &**s == "defpattern" => {
                    let name = match items.get(1) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defpattern: expected name".into()),
                    };
                    let params = match items.get(2) {
                        Some(Form::Vector(ps)) => {
                            if ps.len() % 2 != 0 {
                                return Err(format!(
                                    "{}: param vector must be name/default pairs",
                                    name
                                ));
                            }
                            ps.chunks(2)
                                .map(|c| match &c[0] {
                                    Form::Sym(n) => Ok((n.clone(), c[1].clone())),
                                    _ => Err(format!("{}: bad param name", name)),
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        }
                        _ => return Err(format!("{}: expected param vector", name)),
                    };
                    let body: Rc<[Form]> = items[3..].to_vec().into();
                    // patterns are callable: synthesize (fn [] (let [p d ...]
                    // (seq body...))) so (par (bowap) (other)) composes.
                    // (Prototype: defaults only; §10 scope adapters later.)
                    if !defs.contains_key(&name) {
                        let mut binds = Vec::new();
                        for (pn, dflt) in &params {
                            binds.push(Form::Sym(pn.clone()));
                            binds.push(dflt.clone());
                        }
                        let mut letf = vec![Form::sym("let"), Form::Vector(binds.into())];
                        let mut seqf = vec![Form::sym("seq")];
                        seqf.extend(items[3..].iter().cloned());
                        letf.push(Form::list(seqf));
                        defs.insert(
                            name.clone(),
                            Form::list(vec![
                                Form::sym("fn"),
                                Form::Vector(Vec::new().into()),
                                Form::list(letf),
                            ]),
                        );
                    }
                    order.push(name.clone());
                    patterns.insert(name.clone(), Pattern { name, params, body });
                }
                Some(Form::Sym(s)) if &**s == "def" => {
                    if let Some(Form::Sym(n)) = items.get(1) {
                        defs.insert(n.to_string(), items[2].clone());
                    }
                }
                Some(Form::Sym(s)) if &**s == "defn" => {
                    // (defn name [params] body...) → def name (fn [params] body...)
                    if let Some(Form::Sym(n)) = items.get(1) {
                        let mut fnform = vec![Form::sym("fn")];
                        fnform.extend(items[2..].iter().cloned());
                        defs.insert(n.to_string(), Form::list(fnform));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(Card { patterns, order, defs })
}

// ---------------------------------------------------------------------------
// Contexts.

/// What signals may see: top-level defs + the injected snapshot. Never the
/// world (the purity rule, load-bearing for the borrow structure too).
#[derive(Clone)]
pub struct SigEnv {
    pub defs: Rc<HashMap<String, Form>>,
    /// Injected + derived channels, by bare name (read as `$name`). The host
    /// passes by name; a card's channel manifest derives from its tree.
    pub channels: Rc<HashMap<String, Val>>,
    /// Pattern-scoped control cells (F16): written by set! (control layer),
    /// read live by signals; shared between world and signal contexts.
    pub cells: Rc<std::cell::RefCell<HashMap<String, Val>>>,
}

impl Default for SigEnv {
    fn default() -> Self {
        let mut ch = HashMap::new();
        ch.insert("player".into(), Val::Vec2 { x: 0.0, y: -4.0 });
        ch.insert("nearest-enemy".into(), Val::Vec2 { x: 0.0, y: 3.0 });
        ch.insert("rank".into(), Val::Num(1.0));
        ch.insert("focus-firing".into(), Val::Bool(true));
        SigEnv {
            defs: Rc::new(HashMap::new()),
            channels: Rc::new(ch),
            cells: Rc::new(std::cell::RefCell::new(HashMap::new())),
        }
    }
}

impl SigEnv {
    pub fn channel(&self, name: &str) -> Option<Val> {
        self.channels.get(name).cloned()
    }
    pub fn channel_pos(&self, name: &str) -> (f64, f64) {
        match self.channels.get(name) {
            Some(Val::Vec2 { x, y }) => (*x, *y),
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
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx { sig: SigEnv::default(), ambient: Pose::IDENTITY, scan: None }
    }
}

// ---------------------------------------------------------------------------
// Expression evaluation.

pub fn evaluate(form: &Form, env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    match form {
        Form::Num(n) => Ok(Val::Num(*n)),
        Form::Bool(b) => Ok(Val::Bool(*b)),
        Form::Str(s) => Ok(Val::Str(s.clone())),
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
                    // a deferred signal (shared scan) forces inside scan contexts
                    if ctx.scan.is_some() {
                        if let Val::Thunk(t) = &v {
                            let (f, e) = &**t;
                            return evaluate(f, e, ctx, world);
                        }
                    }
                    return Ok(v);
                }
                if let Some(v) = ctx.sig.cells.borrow().get(name) {
                    return Ok(v.clone());
                }
                if let Some(f) = ctx.sig.defs.clone().get(name) {
                    // hygienic except the slot-bound parameters: a def'd
                    // signal's t IS the referencing slot's t (F12)
                    let mut e = Env::empty();
                    for slot in ["t", "u"] {
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
            let vals = items
                .iter()
                .map(|i| evaluate(i, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(vals)))
        }
        Form::Map(kvs) => {
            let pairs = kvs
                .iter()
                .map(|(k, v)| Ok((evaluate(k, env, ctx, world)?, evaluate(v, env, ctx, world)?)))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Val::Map(Rc::new(pairs)))
        }
        Form::List(items) => evaluate_list(items, env, ctx, world),
    }
}

fn evaluate_list(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let head = items.first().ok_or("cannot evaluate empty list")?;

    if let Form::Sym(s) = head {
        match &**s {
            "dotimes" => return sf_dotimes(items, env, ctx, world),
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
            "when" => {
                let c = evaluate(&items[1], env, ctx, world)?;
                return if truthy(&c) {
                    Ok(Val::Action(Rc::new(ActionV::Seq {
                        items: items[2..].to_vec().into(),
                        env: env.clone(),
                    })))
                } else {
                    Ok(Val::Action(Rc::new(ActionV::Nothing)))
                };
            }
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
            "let" => return sf_let(items, env, ctx, world),
            "fn" => {
                let Some(Form::Vector(ps)) = items.get(1) else {
                    return Err("fn: expected param vector".into());
                };
                let params: Vec<Rc<str>> = ps
                    .iter()
                    .map(|p| match p {
                        Form::Sym(n) => Ok(n.clone()),
                        _ => Err("fn: bad param (destructuring unimplemented)".to_string()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Val::Fn {
                    params: params.into(),
                    body: items[2..].to_vec().into(),
                    env: env.clone(),
                });
            }
            "wait" => {
                let secs = evaluate(&items[1], env, ctx, world)?.num()?;
                return Ok(Val::Action(Rc::new(ActionV::Wait {
                    ticks: (secs * TICK_RATE).round().max(0.0) as u64,
                })));
            }
            "event" => {
                let ch = match evaluate(&items[1], env, ctx, world)? {
                    Val::Kw(k) => k,
                    v => return Err(format!("event: expected channel keyword, got {:?}", v)),
                };
                return Ok(Val::Action(Rc::new(ActionV::Event { channel: ch })));
            }
            "spawn" => return sf_spawn(items, env, ctx, world),
            "manipulate" => {
                let target = evaluate(&items[1], env, ctx, world)?;
                let callback = evaluate(&items[2], env, ctx, world)?;
                let mut targets = Vec::new();
                collect_handles(&target, &mut targets)?;
                return Ok(Val::Action(Rc::new(ActionV::Manipulate { targets, callback })));
            }
            "cull" => {
                let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                    return Err("cull: expected bullet handle".into());
                };
                return Ok(Val::Action(Rc::new(ActionV::Cull { target: id })));
            }
            "pos" => {
                // (pos b): the bullet's current world position — world read.
                let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                    return Err("pos: expected bullet handle".into());
                };
                let Some(i) = world.find(id) else {
                    return Err("pos: dead handle".into());
                };
                let b = &world.bullets[i];
                let tau = (world.tick - b.birth) as f64 / TICK_RATE;
                let p = dyn_pose(&b.motion, tau, &b.state, &ctx.sig)?;
                return Ok(Val::Vec2 { x: p.x, y: p.y });
            }
            "in-frame" => {
                // frames form a monoid: (in-frame f1 f2 body) folds as
                // (f1 (f2 body)), outer to inner. Last argument is the body.
                if items.len() < 3 {
                    return Err("in-frame: expected (in-frame frame... body)".into());
                }
                let mut val = evaluate(&items[items.len() - 1], env, ctx, world)?;
                for f in items[1..items.len() - 1].iter().rev() {
                    let fv = evaluate(f, env, ctx, world)?;
                    val = match fv {
                        Val::Dyn(d) => apply_dyn_frame(d, val)?,
                        other => apply_frame_val(as_pose(other)?, val)?,
                    };
                }
                return Ok(val);
            }
            "circle" => return sf_circle(items, env, ctx, world),
            "arrow" => return sf_arrow(items, env, ctx, world),
            "fan" => return sf_fan(items, env, ctx, world),
            "cart" | "polar" if items[1..].iter().any(contains_t) => {
                if items.len() != 3 {
                    return Err(format!("{}: expected two components", s));
                }
                return Ok(Val::Dyn(Rc::new(DynNode::ClosedPt {
                    a: items[1].clone(),
                    b: items[2].clone(),
                    polar: &**s == "polar",
                    env: env.clone(),
                })));
            }
            "vel" => return sf_vel(items, env, ctx, world),
            "laser" => return sf_laser(items, env, ctx, world),
            "pather" => {
                // prototype: pathers render as points (trail later); the dyn
                // is the second argument
                return evaluate(&items[2], env, ctx, world);
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
                                Val::Vec2 { .. } | Val::Pose(_) => Ok(Val::Dyn(Rc::new(
                                    DynNode::Live { channel: Rc::from(name) },
                                ))),
                                v => Ok(v),
                            }
                        };
                    }
                    // cells read live by name
                    if let Some(v) = ctx.sig.cells.borrow().get(ch.as_ref()) {
                        return Ok(v.clone());
                    }
                }
                return evaluate(&items[1], env, ctx, world);
            }
            "slew" | "smooth" => {
                if ctx.scan.is_none() {
                    // deferred shared instance (§5): forced in scan contexts
                    return Ok(Val::Thunk(Rc::new((
                        Form::List(items.to_vec().into()),
                        env.clone(),
                    ))));
                }
                return sf_stateful(&**s, items, env, ctx, world);
            }
            "stages" => return sf_stages(items, env, ctx, world),
            "rot" if items.len() == 2 && contains_t(&items[1]) => {
                return Ok(Val::Dyn(Rc::new(DynNode::RotExpr {
                    form: items[1].clone(),
                    env: env.clone(),
                })));
            }
            "aim" => {
                let target = evaluate(&items[1], env, ctx, world)?;
                let Val::Vec2 { x, y } = target else {
                    return Err("aim: expected a point target".into());
                };
                let world_ang = (y - ctx.ambient.y).atan2(x - ctx.ambient.x).to_degrees();
                return Ok(Val::Pose(Pose { x: 0.0, y: 0.0, th: world_ang - ctx.ambient.th }));
            }
            "map" => {
                let f = evaluate(&items[1], env, ctx, world)?;
                let Val::Arr(xs) = evaluate(&items[2], env, ctx, world)? else {
                    return Err("map: expected array".into());
                };
                let out = xs
                    .iter()
                    .map(|x| apply_fn(f.clone(), &[x.clone()], ctx, world, false))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Val::Arr(Rc::new(out)));
            }
            "defvar" => {
                let Some(Form::Sym(name)) = items.get(1) else {
                    return Err("defvar: expected name".into());
                };
                let init = evaluate(&items[2], env, ctx, world)?;
                return Ok(Val::Action(Rc::new(ActionV::DefVar { name: name.clone(), init })));
            }
            "set!" => {
                let Some(Form::Sym(name)) = items.get(1) else {
                    return Err("set!: expected name".into());
                };
                let val = evaluate(&items[2], env, ctx, world)?;
                return Ok(Val::Action(Rc::new(ActionV::SetVar { name: name.clone(), val })));
            }
            "wait-for" => {
                return Ok(Val::Action(Rc::new(ActionV::WaitFor {
                    pred: items[1].clone(),
                    env: env.clone(),
                })));
            }
            "move" => {
                // (move dur ease dest)
                let dur = evaluate(&items[1], env, ctx, world)?.num()?;
                let dest = match evaluate(&items[3], env, ctx, world)? {
                    Val::Vec2 { x, y } => (x, y),
                    v => return Err(format!("move: expected point dest, got {:?}", v)),
                };
                return Ok(Val::Action(Rc::new(ActionV::Move {
                    dur_ticks: (dur * TICK_RATE).round().max(0.0) as u64,
                    dest,
                })));
            }
            "rand" => {
                let (a, b) = (
                    evaluate(&items[1], env, ctx, world)?.num()?,
                    evaluate(&items[2], env, ctx, world)?.num()?,
                );
                return Ok(Val::Num(a + world.next_rand() * (b - a)));
            }
            "rand-int" => {
                let (a, b) = (
                    evaluate(&items[1], env, ctx, world)?.num()?,
                    evaluate(&items[2], env, ctx, world)?.num()?,
                );
                return Ok(Val::Num((a + world.next_rand() * (b - a)).floor()));
            }
            "randpm1" => {
                return Ok(Val::Num(if world.next_rand() < 0.5 { -1.0 } else { 1.0 }));
            }
            "phases" | "stages-action" | "scan" => {
                return Err(format!("'{}' not implemented in this milestone", s));
            }
            _ => {}
        }
    }

    // Ordinary application.
    if let Form::Sym(name) = head {
        if env.lookup(name).is_none()
            && !ctx.sig.defs.contains_key(&**name)
            && !name.starts_with('$')
        {
            let args = items[1..]
                .iter()
                .map(|f| evaluate(f, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            return builtin(name, &args);
        }
    }
    let hv = evaluate(head, env, ctx, world)?;
    match hv {
        Val::Pose(p) => {
            if items.len() != 2 {
                return Err("frame application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx, world)?;
            apply_frame_val(p, child)
        }
        // signal-valued frame (live channel, rot-expr): compose dyns
        Val::Dyn(fd) => {
            if items.len() != 2 {
                return Err("frame application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx, world)?;
            apply_dyn_frame(fd, child)
        }
        Val::Arr(_) => {
            if items.len() != 2 {
                return Err("frame-array application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx, world)?;
            apply_frame_arr(&hv, child)
        }
        Val::Kw(k) => {
            // keyword application: map access, e.g. (:vel exit)
            let arg = evaluate(&items[1], env, ctx, world)?;
            Ok(map_get(&arg, &k).unwrap_or(Val::Nothing))
        }
        f @ (Val::Fn { .. } | Val::Builtin(_)) => {
            let args = items[1..]
                .iter()
                .map(|x| evaluate(x, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            apply_fn(f, &args, ctx, world, false)
        }
        _ => Err(format!("cannot apply {:?}", hv)),
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
            Ok(Val::Arr(Rc::new(out)))
        }
        Val::Ext(l) => Ok(Val::Ext(Rc::new(ExtLaser {
            anchor: Rc::new(DynNode::Frame(frame, l.anchor.clone())),
            shape: l.shape.clone(),
            warn: l.warn,
            active: l.active,
            u_max: l.u_max,
            u_max_sig: l.u_max_sig.clone(),
            resolution: l.resolution,
        }))),
        other => Ok(Val::Dyn(Rc::new(DynNode::Frame(frame, as_dyn(other)?)))),
    }
}

/// Apply a user fn or builtin. Ambient frames do not cross fn boundaries
/// (F18). `exec_actions` is set only for manipulate callbacks, whose bodies
/// run instantaneously; ordinary fns RETURN action values for composition.
pub fn apply_fn(
    f: Val,
    args: &[Val],
    ctx: &mut Ctx,
    world: &mut World,
    exec_actions: bool,
) -> Result<Val, String> {
    match f {
        Val::Builtin(name) => builtin(&name, args),
        Val::Fn { params, body, env } => {
            let mut e = env.clone();
            for (p, a) in params.iter().zip(args.iter()) {
                e = e.bind(p.clone(), a.clone());
            }
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
        ActionV::Event { channel } => {
            world.events.push(Event { tick: world.tick, name: channel.to_string(), pos: None });
            Ok(Val::Nothing)
        }
        ActionV::DefVar { name, init } | ActionV::SetVar { name, val: init } => {
            ctx.sig.cells.borrow_mut().insert(name.to_string(), init.clone());
            Ok(Val::Nothing)
        }
        ActionV::Cull { target } => {
            if let Some(i) = world.find(*target) {
                world.bullets[i].alive = false;
            }
            Ok(Val::Nothing)
        }
        ActionV::Spawn { dyns, styles, hues, team, cols, triggers, damage, colliders } => {
            let mut handles = Vec::new();
            for ((d, s), h) in dyns.iter().zip(styles.iter()).zip(hues.iter()) {
                let motion = if ctx.ambient == Pose::IDENTITY {
                    d.motion.clone()
                } else {
                    Rc::new(DynNode::Frame(
                        Rc::new(DynNode::Const(ctx.ambient)),
                        d.motion.clone(),
                    ))
                };
                let scanned = is_scanned(&motion);
                let id = world.next_id;
                world.next_id += 1;
                world.bullets.push(Bullet {
                    id,
                    team: team.clone(),
                    kind: d.kind.clone(),
                    motion,
                    birth: world.tick,
                    style: s.clone(),
                    alive: true,
                    state: MotionState::new(),
                    scanned,
                    hue: h.clone(),
                    colliders: colliders.clone(),
                    cols: cols.clone(),
                    triggers: triggers.clone(),
                    damage: damage.clone(),
                    grazed: false,
                    prev_pos: None,
                });
                handles.push(Val::Handle(id));
            }
            Ok(Val::Arr(Rc::new(handles)))
        }
        ActionV::Manipulate { targets, callback } => {
            for id in targets {
                if world.find(*id).is_some() {
                    apply_fn(callback.clone(), &[Val::Handle(*id)], ctx, world, true)?;
                }
            }
            Ok(Val::Nothing)
        }
        ActionV::Seq { items, env } => {
            // instantaneous only: run each item now
            let mut e = Ctx { sig: ctx.sig.clone(), ambient: ctx.ambient, scan: None };
            for f in items.iter() {
                let v = evaluate(f, env, &mut e, world)?;
                if let Val::Action(a) = &v {
                    exec_instant(a, &mut e, world)?;
                }
            }
            Ok(Val::Nothing)
        }
        ActionV::Wait { .. } => Err("cannot wait in instantaneous context (fn body)".into()),
        other => Err(format!("action not instantaneous: {:?}", other)),
    }
}

fn collect_handles(v: &Val, out: &mut Vec<u64>) -> Result<(), String> {
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

fn truthy(v: &Val) -> bool {
    !matches!(v, Val::Bool(false) | Val::Nothing)
}

fn as_action(v: Val) -> Result<Rc<ActionV>, String> {
    match v {
        Val::Action(a) => Ok(a),
        v => Err(format!("expected action, got {:?}", v)),
    }
}

fn as_pose(v: Val) -> Result<Pose, String> {
    match v {
        Val::Pose(p) => Ok(p),
        Val::Vec2 { x, y } => Ok(Pose { x, y, th: 0.0 }),
        v => Err(format!("expected pose, got {:?}", v)),
    }
}

fn as_dyn(v: Val) -> Result<Rc<DynNode>, String> {
    match v {
        Val::Dyn(d) => Ok(d),
        Val::Pose(p) => Ok(Rc::new(DynNode::Const(p))),
        Val::Vec2 { x, y } => Ok(Rc::new(DynNode::Const(Pose { x, y, th: 0.0 }))),
        v => Err(format!("expected dyn, got {:?}", v)),
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
            Ok(Val::Arr(Rc::new(out)))
        }
        Val::Ext(l) => Ok(Val::Ext(Rc::new(ExtLaser {
            anchor: Rc::new(DynNode::Frame(Rc::new(DynNode::Const(frame)), l.anchor.clone())),
            shape: l.shape.clone(),
            warn: l.warn,
            active: l.active,
            u_max: l.u_max,
            u_max_sig: l.u_max_sig.clone(),
            resolution: l.resolution,
        }))),
        other => {
            let d = as_dyn(other)?;
            Ok(Val::Dyn(Rc::new(DynNode::Frame(
                Rc::new(DynNode::Const(frame)),
                d,
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
    Ok(Val::Arr(Rc::new(out)))
}

// ---------------------------------------------------------------------------
// Special forms.

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

fn sf_dotimes(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let Some(Form::Vector(spec)) = items.get(1) else {
        return Err("dotimes: expected binding vector".into());
    };
    let mut every_ticks: u64 = 0;
    let mut pairs: Vec<(&Form, &Form)> = Vec::new();
    let mut k = 0;
    while k < spec.len() {
        if let Form::Kw(kw) = &spec[k] {
            if &**kw == "every" {
                let secs = evaluate(&spec[k + 1], env, ctx, world)?.num()?;
                every_ticks = (secs * TICK_RATE).round().max(0.0) as u64;
                k += 2;
                continue;
            }
        }
        if k + 1 >= spec.len() {
            return Err("dotimes: dangling binding".into());
        }
        pairs.push((&spec[k], &spec[k + 1]));
        k += 2;
    }
    let (counter, rest) = pairs.split_first().ok_or("dotimes: missing counter")?;
    let Form::Sym(var) = counter.0 else {
        return Err("dotimes: bad counter name".into());
    };
    let n = evaluate(counter.1, env, ctx, world)?.num()?;
    let seq_binds = rest
        .iter()
        .map(|(name, src)| {
            let Form::Sym(nm) = name else {
                return Err("dotimes: bad seq binding name".to_string());
            };
            Ok((nm.clone(), evaluate(src, env, ctx, world)?))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Val::Action(Rc::new(ActionV::Dotimes {
        var: var.clone(),
        n,
        seq_binds,
        every_ticks,
        body: items[2..].to_vec().into(),
        env: env.clone(),
    })))
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
        a: comps[0].clone(),
        b: comps[1].clone(),
        polar,
        env: env.clone(),
    });
    match items.get(2) {
        None => Ok(Val::Dyn(node)),
        Some(cf) => {
            // trailing-child sugar on dyn constructors
            let child = evaluate(cf, env, ctx, world)?;
            match child {
                Val::Arr(_) => {
                    // one vel frame carrying an array of children: product
                    let Val::Arr(kids) = child else { unreachable!() };
                    let out = kids
                        .iter()
                        .map(|k| {
                            Ok(Val::Dyn(Rc::new(DynNode::Frame(node.clone(), as_dyn(k.clone())?))))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok(Val::Arr(Rc::new(out)))
                }
                other => Ok(Val::Dyn(Rc::new(DynNode::Frame(node, as_dyn(other)?)))),
            }
        }
    }
}

fn sf_laser(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    // (laser shape? opts): shape is a dyn over (t, u); opts is a map.
    let (shape, opts_idx) = match items.get(1) {
        Some(Form::Map(_)) => (None, 1),
        Some(_) => {
            let sv = evaluate(&items[1], env, ctx, world)?;
            (Some(as_dyn(sv)?), 2)
        }
        None => return Err("laser: expected options".into()),
    };
    // evaluate options, keeping signal-valued entries (contain t) as forms
    let mut u_max_sig = None;
    let opts = match items.get(opts_idx) {
        Some(Form::Map(kvs)) => {
            let mut pairs = Vec::new();
            for (k, v) in kvs.iter() {
                let kv = evaluate(k, env, ctx, world)?;
                if contains_t(v) {
                    if matches!(&kv, Val::Kw(kw) if &**kw == "u-max") {
                        u_max_sig = Some((v.clone(), env.clone()));
                    }
                    pairs.push((kv, Val::Nothing));
                } else {
                    let vv = evaluate(v, env, ctx, world)?;
                    pairs.push((kv, vv));
                }
            }
            Val::Map(Rc::new(pairs))
        }
        Some(m) => evaluate(m, env, ctx, world)?,
        None => Val::Map(Rc::new(vec![])),
    };
    let getf = |key: &str, dflt: f64| -> f64 {
        map_get(&opts, key).and_then(|v| v.num().ok()).unwrap_or(dflt)
    };
    Ok(Val::Ext(Rc::new(ExtLaser {
        anchor: Rc::new(DynNode::Const(Pose::IDENTITY)),
        shape,
        warn: getf("warn", 0.0),
        active: getf("active", f64::INFINITY),
        u_max: getf("u-max", 10.0),
        u_max_sig,
        resolution: getf("resolution", 0.1),
    })))
}

/// slew/smooth: stateful expression sites. State keyed by (base, site index);
/// the site counter is stable for a fixed expression tree.
fn sf_stateful(
    which: &str,
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let scan = ctx.scan.clone().unwrap();
    let (key, advance, dt) = {
        let mut io = scan.borrow_mut();
        let k = site_key(io.base, io.counter);
        io.counter += 1;
        (k, io.advance, io.dt)
    };
    match which {
        "slew" => {
            // (slew rate init? target)
            let rate = evaluate(&items[1], env, ctx, world)?.num()?;
            let (init, target_form) = if items.len() > 3 {
                (Some(evaluate(&items[2], env, ctx, world)?.num()?), &items[3])
            } else {
                (None, &items[2])
            };
            let target = evaluate(target_form, env, ctx, world)?.num()?;
            let stored = {
                let io = scan.borrow();
                match io.state.get(&key) {
                    Some(Cell::N(v)) => Some(v[0]),
                    _ => None,
                }
            };
            let mut cur = stored.unwrap_or(init.unwrap_or(target));
            if advance {
                let d = shortest_arc(cur, target);
                cur += d.clamp(-rate * dt, rate * dt);
                scan.borrow_mut().state.insert(key, Cell::N([cur, 0.0]));
            }
            Ok(Val::Num(cur))
        }
        "smooth" => {
            // (smooth k target): one-pole follower, per tick
            let k = evaluate(&items[1], env, ctx, world)?.num()?;
            let target = evaluate(&items[2], env, ctx, world)?;
            let (tx, ty) = match target {
                Val::Vec2 { x, y } => (x, y),
                Val::Num(x) => (x, 0.0),
                v => return Err(format!("smooth: bad target {:?}", v)),
            };
            let stored = {
                let io = scan.borrow();
                match io.state.get(&key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                }
            };
            let [mut x, mut y] = stored.unwrap_or([tx, ty]);
            if advance {
                x += k * (tx - x);
                y += k * (ty - y);
                scan.borrow_mut().state.insert(key, Cell::N([x, y]));
            }
            Ok(Val::Vec2 { x, y })
        }
        _ => unreachable!(),
    }
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
            "until" => (StageTerm::Until(parts[1].clone(), env.clone()), &parts[2]),
            "forever" => (StageTerm::Forever, &parts[1]),
            h => return Err(format!("stages: unknown clause '{}'", h)),
        };
        let v = evaluate(sig_form, env, ctx, world)?;
        let make = match v {
            Val::Fn { .. } => StageMake::Lazy(v),
            other => StageMake::Ready(as_dyn(other)?),
        };
        segs.push(StageSeg { term, make });
    }
    if segs.is_empty() {
        return Err("stages: no segments".into());
    }
    if matches!(segs[0].make, StageMake::Lazy(_)) {
        return Err("stages: first segment cannot be lazy (no exit yet)".into());
    }
    Ok(Val::Dyn(Rc::new(DynNode::Stages { segs })))
}

/// Meta keys whose values are signals sampled later (§7): never evaluated at
/// spawn time (they reference slot-bound t).
const SIGNAL_TAGS: &[&str] = &["hue", "facing"];

fn sf_spawn(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let dv = evaluate(&items[1], env, ctx, world)?;
    let meta = match items.get(2) {
        Some(Form::Map(kvs)) => {
            let mut pairs = Vec::new();
            for (k, v) in kvs.iter() {
                let kv = evaluate(k, env, ctx, world)?;
                let skip = matches!(&kv, Val::Kw(kw) if SIGNAL_TAGS.contains(&kw.as_ref()));
                let vv = if skip { Val::Nothing } else { evaluate(v, env, ctx, world)? };
                pairs.push((kv, vv));
            }
            Val::Map(Rc::new(pairs))
        }
        Some(m) => evaluate(m, env, ctx, world)?,
        None => Val::Map(Rc::new(vec![])),
    };
    let mut elems = Vec::new();
    flatten_elems(dv, &mut Vec::new(), &mut elems)?;
    // rand in signal expressions is an ir constant per element (§5): clone the
    // motion tree per element, substituting rand calls with drawn constants
    for e in elems.iter_mut() {
        if dyn_has_rand(&e.motion) {
            e.motion = instantiate_rand(&e.motion, world);
        }
    }
    let styles = resolve_styles(&meta, &elems)?;
    let hues = resolve_hue(items.get(2), &meta, env, elems.len());
    let team: Option<Rc<str>> = match map_get(&meta, "team") {
        Some(Val::Kw(k)) => Some(Rc::from(&*k)),
        _ => None,
    };
    // columns: :hp n is sugar for a col; :cols {:armor 2 ...} adds more.
    // :team :enemy defaults hp to 1 so untyped enemies still die to a shot.
    let mut cols: Vec<(Rc<str>, f64)> = Vec::new();
    match map_get(&meta, "hp") {
        Some(Val::Num(n)) => cols.push(("hp".into(), n)),
        _ => {
            if team.as_deref() == Some("enemy") {
                cols.push(("hp".into(), 1.0));
            }
        }
    }
    if let Some(Val::Map(kvs)) = map_get(&meta, "cols") {
        for (k, v) in kvs.iter() {
            if let (Val::Kw(k), Val::Num(n)) = (k, v) {
                cols.push((k.as_ref().into(), *n));
            }
        }
    }
    // triggers: explicit :triggers replaces the synthesized default
    // (hp col present → death rule: hp ≤ 0 → cull + event :died)
    let triggers: Rc<[TriggerRule]> = match map_get(&meta, "triggers") {
        Some(Val::Arr(items)) => {
            let mut rules = Vec::new();
            for it in items.iter() {
                let Val::Map(kvs) = it else {
                    return Err("triggers: expected maps".into());
                };
                let get = |name: &str| {
                    kvs.iter().find_map(|(k, v)| match k {
                        Val::Kw(kw) if &**kw == name => Some(v.clone()),
                        _ => None,
                    })
                };
                let col = match get("col") {
                    Some(Val::Kw(k)) => k.to_string(),
                    _ => return Err("triggers: missing :col".into()),
                };
                let leq = match get("leq") {
                    Some(Val::Num(n)) => n,
                    _ => return Err("triggers: missing :leq".into()),
                };
                let event = match get("event") {
                    Some(Val::Kw(k)) => k.to_string(),
                    _ => return Err("triggers: missing :event".into()),
                };
                let cull = matches!(get("cull"), Some(Val::Bool(true)));
                rules.push(TriggerRule::new(&event, &col, leq, cull));
            }
            rules.into()
        }
        _ => {
            if cols.iter().any(|(k, _)| &**k == "hp") {
                vec![TriggerRule::new("died", "hp", 0.0, true)].into()
            } else {
                Vec::new().into()
            }
        }
    };
    // :damage n | {:hit n ...} (DMK player() map) | (fn [self other] n)
    let damage = match map_get(&meta, "damage") {
        Some(Val::Num(n)) => Val::Num(n),
        Some(f @ (Val::Fn { .. } | Val::Builtin(_))) => f,
        Some(Val::Map(kvs)) => Val::Num(
            kvs.iter()
                .find_map(|(k, v)| match (k, v) {
                    (Val::Kw(kw), Val::Num(n)) if &**kw == "hit" => Some(*n),
                    _ => None,
                })
                .unwrap_or(1.0),
        ),
        _ => Val::Num(1.0),
    };
    let hitbox = match map_get(&meta, "hitbox") {
        Some(Val::Num(n)) => Some(n),
        _ => None,
    };
    // collider set: archetype data, one Rc shared by every spawned element.
    // :colliders [{:layer :damage :r 0.1} ...] replaces the team default;
    // :hitbox r resizes the default primary collider.
    let colliders: Rc<[Collider]> = match map_get(&meta, "colliders") {
        Some(Val::Arr(items)) => {
            let mut cs = Vec::new();
            for it in items.iter() {
                let Val::Map(kvs) = it else {
                    return Err("colliders: expected maps".into());
                };
                let get = |name: &str| {
                    kvs.iter().find_map(|(k, v)| match k {
                        Val::Kw(kw) if &**kw == name => Some(v.clone()),
                        _ => None,
                    })
                };
                let layer = match get("layer") {
                    Some(Val::Kw(k)) => match &*k {
                        "damage" => Layer::Damage,
                        "graze" => Layer::Graze,
                        "shot" => Layer::Shot,
                        "hurt" => Layer::Hurt,
                        other => return Err(format!("colliders: unknown layer :{}", other)),
                    },
                    _ => return Err("colliders: missing :layer".into()),
                };
                let r = match get("r") {
                    Some(Val::Num(n)) => n,
                    _ => return Err("colliders: missing :r".into()),
                };
                cs.push(Collider { layer, r });
            }
            cs.into()
        }
        _ => {
            let fam = styles.first().map(|s| s.family.as_str()).unwrap_or("");
            default_colliders(team.as_deref(), fam, hitbox).into()
        }
    };
    let dyns = elems
        .into_iter()
        .map(|e| SpawnMade { motion: e.motion, kind: e.kind })
        .collect();
    Ok(Val::Action(Rc::new(ActionV::Spawn { dyns, styles, hues, team, cols, triggers, damage, colliders })))
}

fn flatten_elems(
    v: Val,
    path: &mut Vec<(usize, usize)>,
    out: &mut Vec<SpawnElem>,
) -> Result<(), String> {
    match v {
        Val::Arr(items) => {
            let len = items.len();
            for (i, item) in items.iter().enumerate() {
                path.push((len, i));
                flatten_elems(item.clone(), path, out)?;
                path.pop();
            }
            Ok(())
        }
        Val::Ext(l) => {
            out.push(SpawnElem {
                motion: l.anchor.clone(),
                kind: Kind::Laser {
                    shape: l.shape.clone(),
                    warn: l.warn,
                    active: l.active,
                    u_max: l.u_max,
                    u_max_sig: l.u_max_sig.clone(),
                    resolution: l.resolution,
                },
                path: path.clone(),
            });
            Ok(())
        }
        other => {
            out.push(SpawnElem { motion: as_dyn(other)?, kind: Kind::Point, path: path.clone() });
            Ok(())
        }
    }
}

fn form_has_rand(f: &Form) -> bool {
    match f {
        Form::List(items) => {
            matches!(items.first(), Some(Form::Sym(s)) if matches!(s.as_ref(), "rand" | "rand-int" | "randpm1"))
                || items.iter().any(form_has_rand)
        }
        Form::Vector(items) => items.iter().any(form_has_rand),
        _ => false,
    }
}

fn dyn_has_rand(d: &DynNode) -> bool {
    match d {
        DynNode::ClosedPt { a, b, .. } | DynNode::Vel { a, b, .. } => {
            form_has_rand(a) || form_has_rand(b)
        }
        DynNode::RotExpr { form, .. } => form_has_rand(form),
        DynNode::Translate { child, .. } => dyn_has_rand(child),
        DynNode::Frame(a, b) => dyn_has_rand(a) || dyn_has_rand(b),
        _ => false,
    }
}

fn subst_rand(f: &Form, world: &mut World) -> Form {
    match f {
        Form::List(items) => {
            if let Some(Form::Sym(s)) = items.first() {
                match s.as_ref() {
                    "rand" | "rand-int" => {
                        let a = matches!(&items[1], Form::Num(_))
                            .then(|| if let Form::Num(n) = items[1] { n } else { 0.0 })
                            .unwrap_or(0.0);
                        let b = matches!(&items[2], Form::Num(_))
                            .then(|| if let Form::Num(n) = items[2] { n } else { 1.0 })
                            .unwrap_or(1.0);
                        let v = a + world.next_rand() * (b - a);
                        return Form::Num(if s.as_ref() == "rand-int" { v.floor() } else { v });
                    }
                    "randpm1" => {
                        return Form::Num(if world.next_rand() < 0.5 { -1.0 } else { 1.0 });
                    }
                    _ => {}
                }
            }
            Form::List(items.iter().map(|i| subst_rand(i, world)).collect::<Vec<_>>().into())
        }
        Form::Vector(items) => {
            Form::Vector(items.iter().map(|i| subst_rand(i, world)).collect::<Vec<_>>().into())
        }
        other => other.clone(),
    }
}

fn instantiate_rand(d: &Rc<DynNode>, world: &mut World) -> Rc<DynNode> {
    match &**d {
        DynNode::ClosedPt { a, b, polar, env } => Rc::new(DynNode::ClosedPt {
            a: subst_rand(a, world),
            b: subst_rand(b, world),
            polar: *polar,
            env: env.clone(),
        }),
        DynNode::Vel { a, b, polar, env } => Rc::new(DynNode::Vel {
            a: subst_rand(a, world),
            b: subst_rand(b, world),
            polar: *polar,
            env: env.clone(),
        }),
        DynNode::RotExpr { form, env } => Rc::new(DynNode::RotExpr {
            form: subst_rand(form, world),
            env: env.clone(),
        }),
        DynNode::Translate { dx, dy, child } => Rc::new(DynNode::Translate {
            dx: *dx,
            dy: *dy,
            child: instantiate_rand(child, world),
        }),
        DynNode::Frame(a, b) => Rc::new(DynNode::Frame(
            instantiate_rand(a, world),
            instantiate_rand(b, world),
        )),
        _ => d.clone(),
    }
}

fn map_get(m: &Val, key: &str) -> Option<Val> {
    if let Val::Map(kvs) = m {
        for (k, v) in kvs.iter() {
            if let Val::Kw(kw) = k {
                if &**kw == key {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}

fn kw_str(v: &Val) -> String {
    match v {
        Val::Kw(k) => k.to_string(),
        Val::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// §5/F15: a meta axis array binds to the first array level (root to leaf)
/// whose length matches; otherwise it cycles on the flat index.
fn axis_value(v: &Val, elem: &SpawnElem, flat: usize) -> String {
    match v {
        Val::Arr(items) if !items.is_empty() => {
            let len = items.len();
            for (axis_len, idx) in &elem.path {
                if *axis_len == len {
                    return kw_str(&items[idx % len]);
                }
            }
            kw_str(&items[flat % len])
        }
        v => kw_str(v),
    }
}

fn resolve_styles(meta: &Val, elems: &[SpawnElem]) -> Result<Vec<Style>, String> {
    let style = map_get(meta, "style").unwrap_or(Val::Map(Rc::new(vec![])));
    Ok(elems
        .iter()
        .enumerate()
        .map(|(k, e)| Style {
            family: map_get(&style, "family").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
            color: map_get(&style, "color").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
            variant: map_get(&style, "variant").map(|v| axis_value(&v, e, k)).unwrap_or_default(),
        })
        .collect())
}

/// :hue is signal-valued meta (§7): keep the FORM and sample at render time.
fn resolve_hue(meta_form: Option<&Form>, meta: &Val, env: &Env, n: usize) -> Vec<Option<MetaSig>> {
    let has_hue = map_get(meta, "hue").is_some();
    if !has_hue {
        return vec![None; n];
    }
    // find the hue form in the meta map form
    if let Some(Form::Map(kvs)) = meta_form {
        for (k, v) in kvs.iter() {
            if let Form::Kw(kw) = k {
                if &**kw == "hue" {
                    return (0..n)
                        .map(|idx| Some(MetaSig { form: v.clone(), env: env.clone(), idx }))
                        .collect();
                }
            }
        }
    }
    vec![None; n]
}

fn formation(
    poses: Vec<Pose>,
    child: Option<&Form>,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Val, String> {
    let arr = Val::Arr(Rc::new(poses.into_iter().map(Val::Pose).collect()));
    match child {
        None => Ok(arr),
        Some(cf) => {
            let child = evaluate(cf, env, ctx, world)?;
            apply_frame_arr(&arr, child)
        }
    }
}

fn sf_circle(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as usize;
    if n == 0 {
        return Err("circle: zero elements".into());
    }
    let poses = (0..n)
        .map(|k| Pose { x: 0.0, y: 0.0, th: k as f64 * 360.0 / n as f64 })
        .collect();
    formation(poses, items.get(2), env, ctx, world)
}

fn sf_arrow(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let back = evaluate(&items[2], env, ctx, world)?.num()?;
    let side = evaluate(&items[3], env, ctx, world)?.num()?;
    let half = (n - 1) / 2;
    let poses = (-half..=(n - 1 - half))
        .map(|j| Pose { x: -back * (j.abs() as f64), y: side * j as f64, th: 0.0 })
        .collect();
    formation(poses, items.get(4), env, ctx, world)
}

fn sf_fan(items: &[Form], env: &Env, ctx: &mut Ctx, world: &mut World) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx, world)?.num()? as i64;
    let step = evaluate(&items[2], env, ctx, world)?.num()?;
    let mid = (n - 1) as f64 / 2.0;
    let poses = (0..n)
        .map(|k| Pose { x: 0.0, y: 0.0, th: (k as f64 - mid) * step })
        .collect();
    formation(poses, items.get(3), env, ctx, world)
}

// ---------------------------------------------------------------------------
// Builtins.

const BUILTINS: &[&str] = &[
    "+", "-", "*", "/", "mod", "pow", "inc", "dec", "=", "<", ">", "<=", ">=", "min", "max",
    "abs", "quot", "ticks", "sin", "cos", "sine", "lssht", "cart", "polar", "pose", "rot", "still",
    "linear", "iota", "range", "nth", "without", "stutter", "lerp", "lerp3", "lerpsmooth",
    "angle-of", "mag", "einsine", "eoutsine", "eiosine",
];

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// Broadcast-aware numeric binop (§5: zips cycle; scalars lift).
fn num_bin(a: Val, b: Val, f: fn(f64, f64) -> f64) -> Result<Val, String> {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => Ok(Val::Num(f(x, y))),
        (Val::Arr(xs), Val::Arr(ys)) => {
            let len = xs.len().max(ys.len());
            let out = (0..len)
                .map(|k| num_bin(xs[k % xs.len()].clone(), ys[k % ys.len()].clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(out)))
        }
        (Val::Arr(xs), y) => {
            let out = xs
                .iter()
                .map(|x| num_bin(x.clone(), y.clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(out)))
        }
        (x, Val::Arr(ys)) => {
            let out = ys
                .iter()
                .map(|y| num_bin(x.clone(), y.clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(out)))
        }
        (a, b) => Err(format!("numeric op on {:?} and {:?}", a, b)),
    }
}

fn add2(a: Val, b: Val) -> Result<Val, String> {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => Ok(Val::Num(x + y)),
        (Val::Vec2 { x: ax, y: ay }, Val::Vec2 { x: bx, y: by }) => {
            Ok(Val::Vec2 { x: ax + bx, y: ay + by })
        }
        (Val::Vec2 { x, y }, Val::Pose(p)) | (Val::Pose(p), Val::Vec2 { x, y }) => {
            Ok(Val::Pose(Pose { x: p.x + x, y: p.y + y, th: p.th }))
        }
        (v @ Val::Vec2 { .. }, Val::Arr(items)) | (Val::Arr(items), v @ Val::Vec2 { .. }) => {
            let out = items
                .iter()
                .map(|i| add2(v.clone(), i.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(out)))
        }
        (Val::Vec2 { x, y }, Val::Dyn(d)) | (Val::Dyn(d), Val::Vec2 { x, y }) => Ok(Val::Dyn(
            Rc::new(DynNode::Translate { dx: x, dy: y, child: d }),
        )),
        (a @ (Val::Num(_) | Val::Arr(_)), b @ (Val::Num(_) | Val::Arr(_))) => {
            num_bin(a, b, |x, y| x + y)
        }
        (a, b) => Err(format!("+: cannot add {:?} and {:?}", a, b)),
    }
}

fn ease(name: &str, r: f64) -> f64 {
    use std::f64::consts::FRAC_PI_2;
    let r = r.clamp(0.0, 1.0);
    match name {
        "einsine" => 1.0 - (r * FRAC_PI_2).cos(),
        "eoutsine" => (r * FRAC_PI_2).sin(),
        "eiosine" => 0.5 - 0.5 * (r * std::f64::consts::PI).cos(),
        _ => r,
    }
}

fn builtin(name: &str, args: &[Val]) -> Result<Val, String> {
    let n = |i: usize| -> Result<f64, String> {
        args.get(i)
            .ok_or_else(|| format!("{}: missing argument {}", name, i))?
            .num()
    };
    let fold_num = |init: f64, f: fn(f64, f64) -> f64| -> Result<Val, String> {
        let mut acc = if args.is_empty() { Val::Num(init) } else { args[0].clone() };
        if args.len() == 1 {
            acc = num_bin(Val::Num(init), acc, f)?;
        }
        for a in &args[1..] {
            acc = num_bin(acc, a.clone(), f)?;
        }
        Ok(acc)
    };
    match name {
        "+" => {
            let mut acc = args.first().cloned().unwrap_or(Val::Num(0.0));
            for a in &args[1..] {
                acc = add2(acc, a.clone())?;
            }
            Ok(acc)
        }
        "-" => match (&args.first(), args.len()) {
            (Some(Val::Vec2 { x: ax, y: ay }), 2) => {
                if let Val::Vec2 { x: bx, y: by } = &args[1] {
                    Ok(Val::Vec2 { x: ax - bx, y: ay - by })
                } else {
                    Err("-: mixed vector/number".into())
                }
            }
            _ => fold_num(0.0, |a, b| a - b),
        },
        "*" => fold_num(1.0, |a, b| a * b),
        "/" => fold_num(1.0, |a, b| a / b),
        "mod" => Ok(Val::Num(n(0)?.rem_euclid(n(1)?))),
        "pow" => Ok(Val::Num(n(0)?.powf(n(1)?))),
        "inc" => Ok(Val::Num(n(0)? + 1.0)),
        "dec" => Ok(Val::Num(n(0)? - 1.0)),
        "=" => match (&args[0], &args[1]) {
            (Val::Kw(a), Val::Kw(b)) => Ok(Val::Bool(a == b)),
            (Val::Str(a), Val::Str(b)) => Ok(Val::Bool(a == b)),
            (Val::Map(a), Val::Map(b)) => Ok(Val::Bool(format!("{:?}", a) == format!("{:?}", b))),
            _ => Ok(Val::Bool((n(0)? - n(1)?).abs() < 1e-9)),
        },
        "<" => Ok(Val::Bool(n(0)? < n(1)?)),
        ">" => Ok(Val::Bool(n(0)? > n(1)?)),
        "<=" => Ok(Val::Bool(n(0)? <= n(1)?)),
        ">=" => Ok(Val::Bool(n(0)? >= n(1)?)),
        "min" => Ok(Val::Num(n(0)?.min(n(1)?))),
        "max" => Ok(Val::Num(n(0)?.max(n(1)?))),
        "abs" => Ok(Val::Num(n(0)?.abs())),
        "quot" => Ok(Val::Num((n(0)? / n(1)?).trunc())),
        "ticks" => Ok(Val::Num(n(0)? / TICK_RATE)),
        "sin" => Ok(Val::Num(n(0)?.to_radians().sin())),
        "cos" => Ok(Val::Num(n(0)?.to_radians().cos())),
        "sine" => {
            let (period, amp, x) = (n(0)?, n(1)?, n(2)?);
            Ok(Val::Num(amp * (std::f64::consts::TAU * x / period).sin()))
        }
        "cart" => Ok(Val::Vec2 { x: n(0)?, y: n(1)? }),
        "polar" => {
            let (r, th) = (n(0)?, n(1)?);
            let (s, c) = th.to_radians().sin_cos();
            Ok(Val::Vec2 { x: r * c, y: r * s })
        }
        "pose" => as_pose(args[0].clone()).map(Val::Pose),
        "rot" => Ok(Val::Pose(Pose { x: 0.0, y: 0.0, th: n(0)? })),
        "still" => Ok(Val::Pose(Pose::IDENTITY)),
        "linear" => match &args[0] {
            Val::Vec2 { x, y } => Ok(Val::Dyn(Rc::new(DynNode::Linear { vx: *x, vy: *y }))),
            v => Err(format!("linear: expected point, got {:?}", v)),
        },
        "angle-of" => match &args[0] {
            Val::Vec2 { x, y } => Ok(Val::Num(y.atan2(*x).to_degrees())),
            v => Err(format!("angle-of: expected point, got {:?}", v)),
        },
        "mag" => match &args[0] {
            Val::Vec2 { x, y } => Ok(Val::Num((x * x + y * y).sqrt())),
            v => Err(format!("mag: expected point, got {:?}", v)),
        },
        "iota" => {
            let count = n(0)? as usize;
            Ok(Val::Arr(Rc::new(
                (0..count).map(|k| Val::Num(k as f64)).collect(),
            )))
        }
        "range" => {
            let (a, b) = (n(0)?, n(1)?);
            let step = if args.len() > 2 { n(2)? } else { 1.0 };
            let mut out = Vec::new();
            let mut x = a;
            while (step > 0.0 && x < b) || (step < 0.0 && x > b) {
                out.push(Val::Num(x));
                x += step;
            }
            Ok(Val::Arr(Rc::new(out)))
        }
        "nth" => match (&args[0], &args[1]) {
            (Val::Arr(items), Val::Arr(idxs)) if !items.is_empty() => {
                // broadcast: (nth xs (iota n))
                let out = idxs
                    .iter()
                    .map(|i| {
                        let k = i.num()? as i64;
                        Ok(items[(k.rem_euclid(items.len() as i64)) as usize].clone())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Val::Arr(Rc::new(out)))
            }
            (Val::Arr(items), i) if !items.is_empty() => {
                let k = i.num()? as i64;
                Ok(items[(k.rem_euclid(items.len() as i64)) as usize].clone())
            }
            (v, _) => Err(format!("nth: expected non-empty array, got {:?}", v)),
        },
        "without" => {
            let Val::Arr(items) = &args[1] else {
                return Err("without: expected array".into());
            };
            let x = n(0)?;
            let out = items
                .iter()
                .filter(|v| !matches!(v, Val::Num(y) if (*y - x).abs() < 1e-9))
                .cloned()
                .collect();
            Ok(Val::Arr(Rc::new(out)))
        }
        "stutter" => {
            let reps = n(0)? as usize;
            let Val::Arr(items) = &args[1] else {
                return Err("stutter: expected array".into());
            };
            let mut out = Vec::with_capacity(items.len() * reps);
            for it in items.iter() {
                for _ in 0..reps {
                    out.push(it.clone());
                }
            }
            Ok(Val::Arr(Rc::new(out)))
        }
        "lerp" => {
            let (a, b, ctrl, v1, v2) = (n(0)?, n(1)?, n(2)?, n(3)?, n(4)?);
            let r = ((ctrl - a) / (b - a)).clamp(0.0, 1.0);
            Ok(Val::Num(v1 + r * (v2 - v1)))
        }
        "lerp3" => {
            // (lerp3 a1 b1 a2 b2 ctrl v1 v2 v3): v1→v2 over [a1,b1], v2→v3 over [a2,b2]
            let (a1, b1, a2, b2, ctrl) = (n(0)?, n(1)?, n(2)?, n(3)?, n(4)?);
            let (v1, v2, v3) = (n(5)?, n(6)?, n(7)?);
            let out = if ctrl < a2 {
                let r = ((ctrl - a1) / (b1 - a1)).clamp(0.0, 1.0);
                v1 + r * (v2 - v1)
            } else {
                let r = ((ctrl - a2) / (b2 - a2)).clamp(0.0, 1.0);
                v2 + r * (v3 - v2)
            };
            Ok(Val::Num(out))
        }
        "lerpsmooth" => {
            // (lerpsmooth ease a b ctrl v1 v2)
            let ename = match &args[0] {
                Val::Builtin(nm) => nm.to_string(),
                v => return Err(format!("lerpsmooth: expected easing fn, got {:?}", v)),
            };
            let (a, b, ctrl, v1, v2) = (n(1)?, n(2)?, n(3)?, n(4)?, n(5)?);
            let r = ((ctrl - a) / (b - a)).clamp(0.0, 1.0);
            Ok(Val::Num(v1 + ease(&ename, r) * (v2 - v1)))
        }
        "einsine" | "eoutsine" | "eiosine" => Ok(Val::Num(ease(name, n(0)?))),
        "lssht" => {
            // logsumexp soft-switch between curves at a t-pivot (BPYRepo);
            // prototype: sigmoid blend, sharpness |c|
            let (c, pv, f1, f2) = (n(0)?, n(1)?, n(2)?, n(3)?);
            // lssht is used with t pre-substituted into f1/f2; ctrl is implicit
            // t which we do not have here — blend on the pivot vs f-magnitudes
            // is not recoverable, so approximate with the pivot on f1's scale:
            let w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
            let _ = w;
            // pragmatic: soft-min/soft-max of the two curves by sharpness sign
            let k = c;
            let m = (k * f1).exp() + (k * f2).exp();
            Ok(Val::Num(m.ln() / k))
        }
        _ => Err(format!("unknown function '{}'", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::read_one;

    fn ev(src: &str) -> Val {
        let f = read_one(src).unwrap();
        evaluate(&f, &Env::empty(), &mut Ctx::default(), &mut World::default()).unwrap()
    }

    #[test]
    fn arithmetic_and_math_macro() {
        let f = read_one("m\"0.2*(i+1)*(i+2)\"").unwrap();
        let env = Env::empty().bind("i".into(), Val::Num(3.0));
        let v = evaluate(&f, &env, &mut Ctx::default(), &mut World::default()).unwrap();
        assert!((v.num().unwrap() - 0.2 * 4.0 * 5.0).abs() < 1e-9);
    }

    #[test]
    fn variadic_arithmetic() {
        assert_eq!(ev("(+ 1 2 3)").num().unwrap(), 6.0);
        assert_eq!(ev("(- 10 1 2)").num().unwrap(), 7.0);
        assert_eq!(ev("(- 4)").num().unwrap(), -4.0);
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
        let Val::Arr(items) = ev("(map (fn [x] (inc x)) [1 2 3])") else { panic!() };
        assert_eq!(items[2].num().unwrap(), 4.0);
        assert!((ev("(eoutsine 1)").num().unwrap() - 1.0).abs() < 1e-9);
        let v = ev("(lerpsmooth eoutsine 0 4 2 0 480)").num().unwrap();
        assert!((v - 480.0 * (0.5f64 * std::f64::consts::FRAC_PI_2).sin()).abs() < 1e-9);
    }

    #[test]
    fn circle_returns_poses() {
        let Val::Arr(items) = ev("(circle 4)") else { panic!() };
        assert_eq!(items.len(), 4);
        let Val::Pose(p) = &items[1] else { panic!() };
        assert!((p.th - 90.0).abs() < 1e-9);
    }

    #[test]
    fn frame_application_builds_dyn() {
        let Val::Dyn(d) = ev("((rot 90) (linear c[4 0]))") else {
            panic!("expected dyn")
        };
        let st = MotionState::new();
        let p = dyn_pose(&d, 1.0, &st, &SigEnv::default()).unwrap();
        assert!(p.x.abs() < 1e-9 && (p.y - 4.0).abs() < 1e-9, "rotated 90°: {:?}", p);
    }

    #[test]
    fn closed_polar_dyn() {
        let Val::Dyn(d) = ev("(polar m\"2*t\" m\"20*t\")") else { panic!() };
        let st = MotionState::new();
        let p = dyn_pose(&d, 1.0, &st, &SigEnv::default()).unwrap();
        let (ex, ey) = (2.0 * (20f64).to_radians().cos(), 2.0 * (20f64).to_radians().sin());
        assert!((p.x - ex).abs() < 1e-9 && (p.y - ey).abs() < 1e-9, "{:?}", p);
        assert!(matches!(ev("p[2 90]"), Val::Vec2 { .. }));
    }

    #[test]
    fn vel_integrates() {
        let Val::Dyn(d) = ev("(vel c[4 0])") else { panic!() };
        let mut st = MotionState::new();
        let dt = 1.0 / TICK_RATE;
        let sig = SigEnv::default();
        for k in 0..120 {
            step_motion(&d, k as f64 * dt, dt, &mut st, &sig).unwrap();
        }
        let p = dyn_pose(&d, 1.0, &st, &sig).unwrap();
        assert!((p.x - 4.0).abs() < 1e-6, "integrated x: {}", p.x);
        assert!(is_scanned(&d));
    }

    #[test]
    fn vel_with_trailing_child() {
        // 200's guide: (vel c[..] (circle 7 (polar ...)))
        let Val::Arr(items) = ev("(vel c[1 0] (circle 7 (linear c[1 0])))") else { panic!() };
        assert_eq!(items.len(), 7);
        assert!(matches!(&items[0], Val::Dyn(d) if is_scanned(d)));
    }

    #[test]
    fn laser_value_and_framing() {
        let Val::Arr(items) =
            ev("(circle 6 (laser p[m\"2*t\" m\"-14*u\"] {:warn 1.5 :active inf :u-max 3.5 :resolution 0.4}))")
        else {
            panic!()
        };
        assert_eq!(items.len(), 6);
        let Val::Ext(l) = &items[0] else { panic!("expected laser") };
        assert_eq!(l.u_max, 3.5);
        // shape at t=1, u=1: r=2, θ=-14°
        let p = dyn_pose_u(l.shape.as_ref().unwrap(), 1.0, 1.0, &MotionState::new(), &SigEnv::default()).unwrap();
        let ex = 2.0 * (-14f64).to_radians().cos();
        assert!((p.x - ex).abs() < 1e-9);
    }

    #[test]
    fn aim_is_ambient_relative() {
        let ctx = &mut Ctx::default();
        let f = read_one("(aim $player)").unwrap();
        let Val::Pose(p) = evaluate(&f, &Env::empty(), ctx, &mut World::default()).unwrap()
        else {
            panic!()
        };
        assert!((p.th - -90.0).abs() < 1e-9, "aim down: {}", p.th);
    }

    #[test]
    fn plus_translates_formations() {
        let Val::Arr(items) = ev("(+ c[-7 0] (arrow 3 1.0 0.5))") else { panic!() };
        assert_eq!(items.len(), 3);
        let Val::Pose(center) = &items[1] else { panic!() };
        assert!((center.x - -7.0).abs() < 1e-9 && center.y.abs() < 1e-9);
    }
}
