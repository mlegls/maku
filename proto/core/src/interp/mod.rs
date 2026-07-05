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

mod builtins;
mod card;
mod motion;
mod spawn;
mod world;

pub(crate) use builtins::*;
pub use card::*;
pub use motion::*;
pub(crate) use spawn::*;
pub use world::*;


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
        expose: Rc<[(Rc<str>, Rc<str>)]>,
    },
    Manipulate { targets: Vec<u64>, query: Option<Val>, callback: Val },
    Remat { target: u64, f: Val },
    SetStyle { target: u64, style: Val },
    Cull { target: u64 },
    /// (export cell): publish a pattern cell as a read-only channel of the
    /// same name — the pattern-level export surface (host renders it; the
    /// pattern stays the single writer).
    Export { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str> },
    /// Pattern invocation: args pre-evaluated in the CALLER's scope (ir
    /// values); params fill from defaults. The §10 embedding adapter:
    /// fresh_cells=true (the default — isolated defvar state per instance),
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
    Wait { ticks: u64 },
    WaitFor { pred: Form, env: Env },
    DefVar { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str>, init: Val },
    SetVar { scope: Rc<std::cell::RefCell<HashMap<String, u64>>>, name: Rc<str>, val: Val },
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
    /// (in-frame :world body): RESET the ambient composition — patterns
    /// don't self-anchor, so the caller's anchor (e.g. the boss) is the
    /// default; player-side patterns opt out explicitly.
    World,
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
}

impl Default for SigEnv {
    fn default() -> Self {
        let mut ch = HashMap::new();
        ch.insert("player".into(), Val::Vec2 { x: 0.0, y: -4.0 });
        ch.insert("nearest-enemy".into(), Val::Vec2 { x: 0.0, y: 3.0 });
        ch.insert("rank".into(), Val::Num(1.0));
        // truthy default so :while (live $focus-firing) lasers run headless;
        // numeric so rigs can do arithmetic on it (hosts override per tick)
        ch.insert("focus-firing".into(), Val::Num(1.0));
        SigEnv {
            defs: Rc::new(HashMap::new()),
            channels: Rc::new(ch),
            cells: Rc::new(std::cell::RefCell::new(HashMap::new())),
            exports: Rc::new(std::cell::RefCell::new(Vec::new())),
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
    /// Card patterns, callable by name: (bowap 6.0) resolves here when the
    /// head isn't lexically bound.
    pub patterns: Rc<HashMap<String, Pattern>>,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx {
            sig: SigEnv::default(),
            ambient: Pose::IDENTITY,
            scan: None,
            patterns: Rc::new(HashMap::new()),
        }
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
                if let Some(scope) = cell_scope(env) {
                    let id = scope.borrow().get(name).copied();
                    if let Some(id) = id {
                        if let Some((_, v)) = ctx.sig.cells.borrow().get(&id) {
                            return Ok(v.clone());
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
                // a query map selects by style axes (Kw exact / Arr any-of)
                // and :where (pure fn over the bullet view) — queries cut
                // across birth structures (§9); handles are the degenerate
                // case
                if matches!(target, Val::Map(_)) {
                    return Ok(Val::Action(Rc::new(ActionV::Manipulate {
                        targets: Vec::new(),
                        query: Some(target),
                        callback,
                    })));
                }
                let mut targets = Vec::new();
                collect_handles(&target, &mut targets)?;
                return Ok(Val::Action(Rc::new(ActionV::Manipulate {
                    targets,
                    query: None,
                    callback,
                })));
            }
            "remat" => {
                // (remat b (fn [exit] dyn)): snap {:pos :vel :t}, swap the
                // motion signal, rebase the epoch — the §9 event mechanism.
                // C⁰ holds by construction (the new dyn anchors at the
                // snapped pose).
                let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                    return Err("remat: expected bullet handle".into());
                };
                let f = evaluate(&items[2], env, ctx, world)?;
                return Ok(Val::Action(Rc::new(ActionV::Remat { target: id, f })));
            }
            "set-style" => {
                // restyle = pool migration (§7): style is ir, changing it
                // is an event-level operation, never a signal
                let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                    return Err("set-style: expected bullet handle".into());
                };
                let style = evaluate(&items[2], env, ctx, world)?;
                return Ok(Val::Action(Rc::new(ActionV::SetStyle { target: id, style })));
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
            "cull" => {
                // (cull): clear all hostile fire (bomb semantics);
                // (cull handle): cull one bullet
                if items.len() == 1 {
                    return Ok(Val::Action(Rc::new(ActionV::CullHostile)));
                }
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
                        Val::Dyn(d) => {
                            let p = dyn_pose(d, 0.0, &MotionState::new(), &ctx.sig)
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
                        Val::Dyn(d) => apply_dyn_frame(d, val)?,
                        other => apply_frame_val(as_pose(other)?, val)?,
                    };
                }
                return Ok(val);
            }
            "clamp" => {
                // (clamp lo hi dyn): position clamp, e.g. playfield walls
                let lo = as_pose(evaluate(&items[1], env, ctx, world)?)?;
                let hi = as_pose(evaluate(&items[2], env, ctx, world)?)?;
                let child = as_dyn(evaluate(&items[3], env, ctx, world)?)?;
                return Ok(Val::Dyn(Rc::new(DynNode::Clamp {
                    lo: (lo.x, lo.y),
                    hi: (hi.x, hi.y),
                    child,
                })));
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
            "export" => {
                let Form::Sym(name) = &items[1] else {
                    return Err("export: expected a cell name".into());
                };
                let scope = cell_scope(env).ok_or("export: no cell scope")?;
                return Ok(Val::Action(Rc::new(ActionV::Export { scope, name: name.clone() })));
            }
            "defvar" => {
                let Some(Form::Sym(name)) = items.get(1) else {
                    return Err("defvar: expected name".into());
                };
                let init = evaluate(&items[2], env, ctx, world)?;
                let scope = cell_scope(env).ok_or("defvar: no cell scope")?;
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

    // Ordinary application. Unbound symbol heads resolve pattern-first
    // (§10 embedding: args evaluated in the CALLER's scope as ir values,
    // defaults filling the rest; default adapter = isolated cells, (inline
    // …) shares the caller's), then fall back to builtins.
    if let Form::Sym(name) = head {
        if env.lookup(name).is_none()
            && !ctx.sig.defs.contains_key(&**name)
            && !name.starts_with('$')
        {
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
            return builtin(name, &args);
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
        Val::Dyn(fd) => {
            if items.len() != 2 {
                return Err("frame application takes exactly one child".into());
            }
            let saved = ctx.ambient;
            let p0 = dyn_pose(&fd, 0.0, &MotionState::new(), &ctx.sig)
                .unwrap_or(Pose::IDENTITY);
            ctx.ambient = ctx.ambient.compose(&p0);
            let child = evaluate(&items[1], env, ctx, world);
            ctx.ambient = saved;
            let child = child?;
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
            // keyword application: map access, e.g. (:vel exit); on
            // Vec2/Pose, :x/:y/:th read components
            let arg = evaluate(&items[1], env, ctx, world)?;
            match (&*k, &arg) {
                ("x", Val::Vec2 { x, .. }) => return Ok(Val::Num(*x)),
                ("y", Val::Vec2 { y, .. }) => return Ok(Val::Num(*y)),
                ("x", Val::Pose(p)) => return Ok(Val::Num(p.x)),
                ("y", Val::Pose(p)) => return Ok(Val::Num(p.y)),
                ("th", Val::Pose(p)) => return Ok(Val::Num(p.th)),
                _ => {}
            }
            Ok(map_get(&arg, &k).unwrap_or(Val::Nothing))
        }
        f @ (Val::Fn { .. } | Val::Builtin(_)) => {
            let args = items[1..]
                .iter()
                .map(|x| evaluate(x, env, ctx, world))
                .collect::<Result<Vec<_>, _>>()?;
            // cells are dynamic ambient: the caller's scope flows into the
            // callee (hygiene excepts #cells, like the slot-bound t/u)
            let f = match (f, env.lookup(CELLS_KEY)) {
                (Val::Fn { params, body, env: fenv }, Some(cells))
                    if fenv.lookup(CELLS_KEY).is_none() =>
                {
                    Val::Fn { params, body, env: fenv.bind(CELLS_KEY.into(), cells) }
                }
                (f, _) => f,
            };
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
/// Evaluate a manipulate query map against the world in canonical order:
/// style axes / team match exactly (Kw) or any-of (Arr); :where is a pure
/// fn over the bullet view {:pos :vel :t :family :color :variant + cols}.
fn resolve_query(q: &Val, ctx: &mut Ctx, world: &mut World) -> Result<Vec<u64>, String> {
    let Val::Map(kvs) = q else { return Err("query: expected a map".into()) };
    let get = |name: &str| {
        kvs.iter().find_map(|(k, v)| match k {
            Val::Kw(kw) if &**kw == name => Some(v.clone()),
            _ => None,
        })
    };
    let axis_ok = |sel: &Option<Val>, actual: &str| match sel {
        None => true,
        Some(Val::Kw(k)) => &**k == actual,
        Some(Val::Arr(xs)) => xs.iter().any(|v| matches!(v, Val::Kw(k) if &**k == actual)),
        _ => false,
    };
    let (family, color, variant, team) =
        (get("family"), get("color"), get("variant"), get("team"));
    let where_f = get("where");
    let tick = world.tick;
    let sig = ctx.sig.clone();
    let mut candidates: Vec<(u64, Val)> = Vec::new();
    for b in &world.bullets {
        if !b.alive
            || !axis_ok(&family, &b.style.family)
            || !axis_ok(&color, &b.style.color)
            || !axis_ok(&variant, &b.style.variant)
            || !axis_ok(&team, b.team.as_deref().unwrap_or(""))
        {
            continue;
        }
        let view = if where_f.is_some() {
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_pose(&b.motion, tau, &b.state, &sig)?;
            let vel = match b.prev_pos {
                Some((ox, oy)) => ((p.x - ox) * TICK_RATE, (p.y - oy) * TICK_RATE),
                None => (0.0, 0.0),
            };
            let mut view = vec![
                (Val::Kw("pos".into()), Val::Vec2 { x: p.x, y: p.y }),
                (Val::Kw("vel".into()), Val::Vec2 { x: vel.0, y: vel.1 }),
                (Val::Kw("t".into()), Val::Num(tau)),
                (Val::Kw("family".into()), Val::Kw(b.style.family.as_str().into())),
            ];
            for (k, v) in &b.cols {
                view.push((Val::Kw(k.as_ref().into()), Val::Num(*v)));
            }
            Val::Map(Rc::new(view))
        } else {
            Val::Nothing
        };
        candidates.push((b.id, view));
    }
    let mut out = Vec::new();
    for (id, view) in candidates {
        let keep = match &where_f {
            Some(f) => truthy(&apply_fn(f.clone(), &[view], ctx, world, false)?),
            None => true,
        };
        if keep {
            out.push(id);
        }
    }
    Ok(out)
}

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
            world.push_event(Event { tick: world.tick, name: channel.as_ref().into(), pos: None });
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
            for b in world.bullets.iter_mut() {
                if b.team.is_none() {
                    b.alive = false;
                }
            }
            Ok(Val::Nothing)
        }
        ActionV::Cull { target } => {
            if let Some(i) = world.find(*target) {
                world.bullets[i].alive = false;
            }
            Ok(Val::Nothing)
        }
        ActionV::Spawn { dyns, styles, hues, team, cols, triggers, damage, colliders, expose } => {
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
                for (col, chan) in expose.iter() {
                    world.exposes.push((chan.clone(), id, col.clone()));
                    // same-tick availability: the channel exists the moment
                    // the entity does (gates may read it this very tick)
                    let v = world.bullets.last().and_then(|b| b.col_get(col)).unwrap_or(0.0);
                    let mut m = (*ctx.sig.channels).clone();
                    m.insert(chan.to_string(), Val::Num(v));
                    ctx.sig.channels = Rc::new(m);
                }
                handles.push(Val::Handle(id));
            }
            Ok(Val::Arr(Rc::new(handles)))
        }
        ActionV::Manipulate { targets, query, callback } => {
            let ids: Vec<u64> = match query {
                Some(q) => resolve_query(q, ctx, world)?,
                None => targets.clone(),
            };
            for id in ids {
                if world.find(id).is_some() {
                    apply_fn(callback.clone(), &[Val::Handle(id)], ctx, world, true)?;
                }
            }
            Ok(Val::Nothing)
        }
        ActionV::Remat { target, f } => {
            let Some(i) = world.find(*target) else { return Ok(Val::Nothing) };
            let (exit, anchor) = {
                let b = &world.bullets[i];
                let tau = (world.tick - b.birth) as f64 / TICK_RATE;
                let p = dyn_pose(&b.motion, tau, &b.state, &ctx.sig)?;
                let vel = match b.prev_pos {
                    Some((ox, oy)) => ((p.x - ox) * TICK_RATE, (p.y - oy) * TICK_RATE),
                    None => (0.0, 0.0),
                };
                let heading = if vel.0 == 0.0 && vel.1 == 0.0 {
                    p.th
                } else {
                    vel.1.atan2(vel.0).to_degrees()
                };
                let exit = Val::Map(Rc::new(vec![
                    (Val::Kw("pos".into()), Val::Vec2 { x: p.x, y: p.y }),
                    (Val::Kw("vel".into()), Val::Vec2 { x: vel.0, y: vel.1 }),
                    (Val::Kw("t".into()), Val::Num(tau)),
                ]));
                (exit, Pose { x: p.x, y: p.y, th: heading })
            };
            let new_dyn = as_dyn(apply_fn(f.clone(), &[exit], ctx, world, false)?)?;
            let b = &mut world.bullets[i];
            // the new signal anchors at the snapped world pose (position +
            // exit heading) and runs on a fresh epoch: τ restarts at 0
            b.motion = Rc::new(DynNode::Frame(Rc::new(DynNode::Const(anchor)), new_dyn));
            b.scanned = is_scanned(&b.motion);
            b.state = MotionState::new();
            b.birth = world.tick;
            b.prev_pos = Some((anchor.x, anchor.y));
            Ok(Val::Nothing)
        }
        ActionV::SetStyle { target, style } => {
            if let Some(i) = world.find(*target) {
                let mut st = world.bullets[i].style.clone();
                if let Val::Map(kvs) = style {
                    for (k, v) in kvs.iter() {
                        if let Val::Kw(k) = k {
                            let val = kw_str(v);
                            match &**k {
                                "family" => st.family = val,
                                "color" => st.color = val,
                                "variant" => st.variant = val,
                                _ => {}
                            }
                        }
                    }
                }
                world.bullets[i].style = st;
            }
            Ok(Val::Nothing)
        }
        ActionV::Seq { items, env } => {
            // instantaneous only: run each item now
            let mut e = Ctx {
                sig: ctx.sig.clone(),
                ambient: ctx.ambient,
                scan: None,
                patterns: ctx.patterns.clone(),
            };
            for f in items.iter() {
                let v = evaluate(f, env, &mut e, world)?;
                if let Val::Action(a) = &v {
                    exec_instant(a, &mut e, world)?;
                }
            }
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

pub(crate) fn truthy(v: &Val) -> bool {
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
