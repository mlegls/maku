//! Control-layer interpreter + prototype signal representation.
//!
//! Per language.md §2: Actions are inert data; the scheduler (sim.rs) walks
//! them with an explicit stack. Expressions evaluate instantly and purely;
//! only Action leaves (wait/spawn/event) interact with time. Seq bodies are
//! LAZY (each item form evaluates when reached), which gives loop-var and
//! cell timing for free.
//!
//! Dyns: Closed nodes evaluate at arbitrary τ (slot-bound t, F12). Vel nodes
//! are the first Scanned constructor: per-bullet state integrated per tick
//! (state keyed by node identity; each bullet owns its state cells).

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

/// Per-bullet scanned state: keyed by dyn-node identity (Rc pointer).
pub type MotionState = HashMap<usize, [f64; 2]>;

#[derive(Debug)]
pub enum DynNode {
    Const(Pose),
    /// pos = v·τ in the local frame; θ = heading.
    Linear { vx: f64, vy: f64 },
    /// Closed position expression over slot-bound t (F12); polar or cart.
    ClosedPt { a: Form, b: Form, polar: bool, env: Env },
    /// Integrated velocity (Scanned): components over slot-bound t.
    Vel { a: Form, b: Form, polar: bool, env: Env },
    /// Point-translation (the `+` of the two-op algebra): θ untouched.
    Translate { dx: f64, dy: f64, child: Rc<DynNode> },
    Frame(Rc<DynNode>, Rc<DynNode>),
}

fn eval_pt(a: &Form, b: &Form, polar: bool, env: &Env, tau: f64) -> Result<(f64, f64), String> {
    let e = env.bind("t".into(), Val::Num(tau));
    let mut ctx = Ctx::default();
    let av = evaluate(a, &e, &mut ctx)?.num()?;
    let bv = evaluate(b, &e, &mut ctx)?.num()?;
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        Ok((av * c, av * s))
    } else {
        Ok((av, bv))
    }
}

pub fn dyn_pose(d: &DynNode, tau: f64, state: &MotionState) -> Result<Pose, String> {
    match d {
        DynNode::Const(p) => Ok(*p),
        DynNode::Linear { vx, vy } => Ok(Pose {
            x: vx * tau,
            y: vy * tau,
            th: vy.atan2(*vx).to_degrees(),
        }),
        DynNode::ClosedPt { a, b, polar, env } => {
            let (x, y) = eval_pt(a, b, *polar, env, tau)?;
            // heading by finite difference (orientation policy: derive by default)
            let eps = 1.0 / TICK_RATE;
            let (x2, y2) = eval_pt(a, b, *polar, env, tau + eps)?;
            Ok(Pose { x, y, th: (y2 - y).atan2(x2 - x).to_degrees() })
        }
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let [x, y] = state.get(&key).copied().unwrap_or([0.0, 0.0]);
            let (vx, vy) = eval_pt(a, b, *polar, env, tau)?;
            Ok(Pose { x, y, th: vy.atan2(vx).to_degrees() })
        }
        DynNode::Translate { dx, dy, child } => {
            let p = dyn_pose(child, tau, state)?;
            Ok(Pose { x: p.x + dx, y: p.y + dy, th: p.th })
        }
        DynNode::Frame(parent, child) => {
            let pp = dyn_pose(parent, tau, state)?;
            let cp = dyn_pose(child, tau, state)?;
            Ok(pp.compose(&cp))
        }
    }
}

/// Advance the Scanned leaves of a motion tree by one tick.
pub fn step_motion(d: &DynNode, tau: f64, dt: f64, state: &mut MotionState) -> Result<(), String> {
    match d {
        DynNode::Vel { a, b, polar, env } => {
            let key = d as *const DynNode as usize;
            let (vx, vy) = eval_pt(a, b, *polar, env, tau)?;
            let cell = state.entry(key).or_insert([0.0, 0.0]);
            cell[0] += vx * dt;
            cell[1] += vy * dt;
            Ok(())
        }
        DynNode::Translate { child, .. } => step_motion(child, tau, dt, state),
        DynNode::Frame(a, b) => {
            step_motion(a, tau, dt, state)?;
            step_motion(b, tau, dt, state)
        }
        _ => Ok(()),
    }
}

/// True if any Scanned node exists (bullets without them never need stepping).
pub fn is_scanned(d: &DynNode) -> bool {
    match d {
        DynNode::Vel { .. } => true,
        DynNode::Translate { child, .. } => is_scanned(child),
        DynNode::Frame(a, b) => is_scanned(a) || is_scanned(b),
        _ => false,
    }
}

/// Does a form reference the slot-bound parameters t/u? (F12: such expressions
/// denote signals; without them, the same constructor is a plain value.)
fn contains_t(form: &Form) -> bool {
    match form {
        Form::Sym(s) => &**s == "t" || &**s == "u",
        Form::List(items) | Form::Vector(items) => items.iter().any(contains_t),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| contains_t(k) || contains_t(v)),
        _ => false,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub family: String,
    pub color: String,
    pub variant: String,
}

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
    Action(Rc<ActionV>),
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

/// Inert action descriptions. Bodies are unevaluated forms + env (lazy seq).
#[derive(Debug)]
pub enum ActionV {
    Seq { items: Rc<[Form]>, env: Env },
    Dotimes {
        var: Rc<str>,
        n: f64, // f64::INFINITY for inf
        seq_binds: Vec<(Rc<str>, Val)>,
        every_ticks: u64,
        body: Rc<[Form]>,
        env: Env,
    },
    Loop { names: Vec<Rc<str>>, inits: Vec<Val>, body: Rc<[Form]>, env: Env },
    Recur(Vec<Val>),
    InFrame { frame: Pose, inner: Rc<ActionV> },
    Spawn { dyns: Vec<Rc<DynNode>>, styles: Vec<Style> },
    Wait { ticks: u64 },
    Fork(Rc<ActionV>),
    Par(Vec<Rc<ActionV>>),
    Event { channel: Rc<str> },
    Nothing,
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
    pub params: Vec<(Rc<str>, Form)>, // name, default expr
    pub body: Rc<[Form]>,
}

pub struct Card {
    pub patterns: HashMap<String, Pattern>,
    pub order: Vec<String>,
}

pub fn load_card(forms: &[Form]) -> Result<Card, String> {
    let mut patterns = HashMap::new();
    let mut order = Vec::new();
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
                    order.push(name.clone());
                    patterns.insert(name.clone(), Pattern { name, params, body });
                }
                // defn/def/defvar: next milestone
                _ => {}
            }
        }
    }
    Ok(Card { patterns, order })
}

// ---------------------------------------------------------------------------
// Expression evaluation.

/// Evaluation context: injected snapshot + the lexically distributed ambient
/// frame (needed by `aim`, which is relative to the emitter).
#[derive(Clone, Debug)]
pub struct Ctx {
    pub player: (f64, f64),
    pub ambient: Pose,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx { player: (0.0, -4.0), ambient: Pose::IDENTITY }
    }
}

pub fn evaluate(form: &Form, env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    match form {
        Form::Num(n) => Ok(Val::Num(*n)),
        Form::Bool(b) => Ok(Val::Bool(*b)),
        Form::Str(s) => Ok(Val::Str(s.clone())),
        Form::Kw(k) => Ok(Val::Kw(k.clone())),
        Form::Sym(s) => match &**s {
            "inf" => Ok(Val::Num(f64::INFINITY)),
            // injected signal, snapped by default (§3)
            "player" => Ok(Val::Vec2 { x: ctx.player.0, y: ctx.player.1 }),
            name => env
                .lookup(name)
                .ok_or_else(|| format!("unresolved symbol '{}'", name)),
        },
        Form::Vector(items) => {
            let vals = items
                .iter()
                .map(|i| evaluate(i, env, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(vals)))
        }
        Form::Map(kvs) => {
            let pairs = kvs
                .iter()
                .map(|(k, v)| Ok((evaluate(k, env, ctx)?, evaluate(v, env, ctx)?)))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Val::Map(Rc::new(pairs)))
        }
        Form::List(items) => evaluate_list(items, env, ctx),
    }
}

fn evaluate_list(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let head = items.first().ok_or("cannot evaluate empty list")?;

    // Special forms first.
    if let Form::Sym(s) = head {
        match &**s {
            "dotimes" => return sf_dotimes(items, env, ctx),
            "loop" => return sf_loop(items, env, ctx),
            "recur" => {
                let vals = items[1..]
                    .iter()
                    .map(|f| evaluate(f, env, ctx))
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
                    .map(|f| as_action(evaluate(f, env, ctx)?))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Val::Action(Rc::new(ActionV::Par(kids))));
            }
            "fork" => {
                let inner = as_action(evaluate(&items[1], env, ctx)?)?;
                return Ok(Val::Action(Rc::new(ActionV::Fork(inner))));
            }
            "when" => {
                let c = evaluate(&items[1], env, ctx)?;
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
                let c = evaluate(&items[1], env, ctx)?;
                return if truthy(&c) {
                    evaluate(&items[2], env, ctx)
                } else if items.len() > 3 {
                    evaluate(&items[3], env, ctx)
                } else {
                    Ok(Val::Nothing)
                };
            }
            "let" => return sf_let(items, env, ctx),
            "wait" => {
                let secs = evaluate(&items[1], env, ctx)?.num()?;
                return Ok(Val::Action(Rc::new(ActionV::Wait {
                    ticks: (secs * TICK_RATE).round().max(0.0) as u64,
                })));
            }
            "event" => {
                let ch = match evaluate(&items[1], env, ctx)? {
                    Val::Kw(k) => k,
                    v => return Err(format!("event: expected channel keyword, got {:?}", v)),
                };
                return Ok(Val::Action(Rc::new(ActionV::Event { channel: ch })));
            }
            "spawn" => return sf_spawn(items, env, ctx),
            "in-frame" => {
                let frame = as_pose(evaluate(&items[1], env, ctx)?)?;
                let child = evaluate(&items[2], env, ctx)?;
                return apply_frame_val(frame, child);
            }
            "circle" => return sf_circle(items, env, ctx),
            "arrow" => return sf_arrow(items, env, ctx),
            "fan" => return sf_fan(items, env, ctx),
            // F12: cart/polar with slot-bound t denote Closed position signals;
            // without t they are plain point values (handled by builtin).
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
            "vel" => return sf_vel(items, env, ctx),
            "aim" => {
                // (aim player): rot toward the (snapped) target, relative to
                // the ambient emitter frame.
                let target = evaluate(&items[1], env, ctx)?;
                let Val::Vec2 { x, y } = target else {
                    return Err("aim: expected a point target".into());
                };
                let world =
                    (y - ctx.ambient.y).atan2(x - ctx.ambient.x).to_degrees();
                return Ok(Val::Pose(Pose { x: 0.0, y: 0.0, th: world - ctx.ambient.th }));
            }
            "fn" | "defn" | "defvar" | "set!" | "phases" | "stages" | "scan" => {
                return Err(format!("'{}' not implemented in this milestone", s));
            }
            _ => {}
        }
    }

    // Ordinary application. Symbols with no lexical binding resolve as builtins;
    // anything else evaluates the head and dispatches on the value (applicable
    // frames, language.md §4).
    if let Form::Sym(name) = head {
        if env.lookup(name).is_none() && &**name != "player" {
            let args = items[1..]
                .iter()
                .map(|f| evaluate(f, env, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            return builtin(name, &args);
        }
    }
    let hv = evaluate(head, env, ctx)?;
    match hv {
        Val::Pose(p) => {
            if items.len() != 2 {
                return Err("frame application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx)?;
            apply_frame_val(p, child)
        }
        Val::Arr(_) => {
            if items.len() != 2 {
                return Err("frame-array application takes exactly one child".into());
            }
            let child = evaluate(&items[1], env, ctx)?;
            apply_frame_arr(&hv, child)
        }
        _ => Err(format!("cannot apply {:?}", hv)),
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
        // point→pose promotion (language.md §2)
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

/// Frame applied to a value: dyn/pose → composed dyn; action → InFrame action;
/// array → per-element (§5: ordinary map).
fn apply_frame_val(frame: Pose, child: Val) -> Result<Val, String> {
    match child {
        Val::Action(a) => Ok(Val::Action(Rc::new(ActionV::InFrame { frame, inner: a }))),
        Val::Arr(items) => {
            let out = items
                .iter()
                .map(|c| apply_frame_val(frame, c.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::Arr(Rc::new(out)))
        }
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

fn sf_let(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let Some(Form::Vector(binds)) = items.get(1) else {
        return Err("let: expected binding vector".into());
    };
    if binds.len() % 2 != 0 {
        return Err("let: odd binding vector".into());
    }
    let mut e = env.clone();
    for c in binds.chunks(2) {
        let Form::Sym(name) = &c[0] else {
            return Err("let: bad binding name (destructuring unimplemented)".into());
        };
        let v = evaluate(&c[1], &e, ctx)?;
        e = e.bind(name.clone(), v);
    }
    match items.len() - 2 {
        1 => evaluate(&items[2], &e, ctx),
        _ => Ok(Val::Action(Rc::new(ActionV::Seq {
            items: items[2..].to_vec().into(),
            env: e,
        }))),
    }
}

fn sf_dotimes(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let Some(Form::Vector(spec)) = items.get(1) else {
        return Err("dotimes: expected binding vector".into());
    };
    let mut every_ticks: u64 = 0;
    let mut pairs: Vec<(&Form, &Form)> = Vec::new();
    let mut k = 0;
    while k < spec.len() {
        if let Form::Kw(kw) = &spec[k] {
            if &**kw == "every" {
                let secs = evaluate(&spec[k + 1], env, ctx)?.num()?;
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
    let n = evaluate(counter.1, env, ctx)?.num()?;
    let seq_binds = rest
        .iter()
        .map(|(name, src)| {
            let Form::Sym(nm) = name else {
                return Err("dotimes: bad seq binding name".to_string());
            };
            Ok((nm.clone(), evaluate(src, env, ctx)?))
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

fn sf_loop(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
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
        inits.push(evaluate(&c[1], env, ctx)?);
    }
    Ok(Val::Action(Rc::new(ActionV::Loop {
        names,
        inits,
        body: items[2..].to_vec().into(),
        env: env.clone(),
    })))
}

fn sf_vel(items: &[Form], env: &Env, _ctx: &mut Ctx) -> Result<Val, String> {
    // (vel c[..]) / (vel p[..]): components as forms over slot-bound t.
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
    Ok(Val::Dyn(Rc::new(DynNode::Vel {
        a: comps[0].clone(),
        b: comps[1].clone(),
        polar,
        env: env.clone(),
    })))
}

fn sf_spawn(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let dv = evaluate(&items[1], env, ctx)?;
    let meta = match items.get(2) {
        Some(m) => evaluate(m, env, ctx)?,
        None => Val::Map(Rc::new(vec![])),
    };
    let mut dyns = Vec::new();
    flatten_dyns(dv, &mut dyns)?;
    let styles = resolve_styles(&meta, dyns.len())?;
    Ok(Val::Action(Rc::new(ActionV::Spawn { dyns, styles })))
}

fn flatten_dyns(v: Val, out: &mut Vec<Rc<DynNode>>) -> Result<(), String> {
    match v {
        Val::Arr(items) => {
            for i in items.iter() {
                flatten_dyns(i.clone(), out)?;
            }
            Ok(())
        }
        other => {
            out.push(as_dyn(other)?);
            Ok(())
        }
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

/// Resolve `:style` meta for n elements: axis arrays zip cyclically against
/// the (flattened, leading-axis-major) element index — prototype simplification
/// of the §5 leading-axis rule.
fn resolve_styles(meta: &Val, n: usize) -> Result<Vec<Style>, String> {
    let style = map_get(meta, "style").unwrap_or(Val::Map(Rc::new(vec![])));
    let get_axis = |key: &str, k: usize| -> String {
        match map_get(&style, key) {
            Some(Val::Arr(items)) if !items.is_empty() => kw_str(&items[k % items.len()]),
            Some(v) => kw_str(&v),
            None => String::new(),
        }
    };
    Ok((0..n)
        .map(|k| Style {
            family: get_axis("family", k),
            color: get_axis("color", k),
            variant: get_axis("variant", k),
        })
        .collect())
}

/// Pose-array formation with optional trailing child (frame sugar).
fn formation(
    poses: Vec<Pose>,
    child: Option<&Form>,
    env: &Env,
    ctx: &mut Ctx,
) -> Result<Val, String> {
    let arr = Val::Arr(Rc::new(poses.into_iter().map(Val::Pose).collect()));
    match child {
        None => Ok(arr),
        Some(cf) => {
            let child = evaluate(cf, env, ctx)?;
            apply_frame_arr(&arr, child)
        }
    }
}

fn sf_circle(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx)?.num()? as usize;
    if n == 0 {
        return Err("circle: zero elements".into());
    }
    let poses = (0..n)
        .map(|k| Pose { x: 0.0, y: 0.0, th: k as f64 * 360.0 / n as f64 })
        .collect();
    formation(poses, items.get(2), env, ctx)
}

fn sf_arrow(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    // (arrow n back side child?): chevron {(-back*|j|, side*j)}, j centered.
    let n = evaluate(&items[1], env, ctx)?.num()? as i64;
    let back = evaluate(&items[2], env, ctx)?.num()?;
    let side = evaluate(&items[3], env, ctx)?.num()?;
    let half = (n - 1) / 2;
    let poses = (-half..=(n - 1 - half))
        .map(|j| Pose { x: -back * (j.abs() as f64), y: side * j as f64, th: 0.0 })
        .collect();
    formation(poses, items.get(4), env, ctx)
}

fn sf_fan(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    // (fan n step child?): centered angular fan, step degrees apart.
    let n = evaluate(&items[1], env, ctx)?.num()? as i64;
    let step = evaluate(&items[2], env, ctx)?.num()?;
    let mid = (n - 1) as f64 / 2.0;
    let poses = (0..n)
        .map(|k| Pose { x: 0.0, y: 0.0, th: (k as f64 - mid) * step })
        .collect();
    formation(poses, items.get(3), env, ctx)
}

// ---------------------------------------------------------------------------
// Builtins.

/// Polymorphic addition: numbers, points, poses (translation), arrays (map),
/// dyns (Translate node) — the `+` of the two-op algebra (§4).
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
        (a, b) => Err(format!("+: cannot add {:?} and {:?}", a, b)),
    }
}

fn builtin(name: &str, args: &[Val]) -> Result<Val, String> {
    let n = |i: usize| -> Result<f64, String> {
        args.get(i)
            .ok_or_else(|| format!("{}: missing argument {}", name, i))?
            .num()
    };
    let fold_num = |init: f64, f: fn(f64, f64) -> f64| -> Result<Val, String> {
        let mut acc = if args.is_empty() { init } else { args[0].num()? };
        if args.len() == 1 {
            acc = f(init, acc); // unary negate / reciprocal
        }
        for a in &args[1..] {
            acc = f(acc, a.num()?);
        }
        Ok(Val::Num(acc))
    };
    match name {
        "+" => {
            let mut acc = args.first().cloned().unwrap_or(Val::Num(0.0));
            for a in &args[1..] {
                acc = add2(acc, a.clone())?;
            }
            Ok(acc)
        }
        "-" => fold_num(0.0, |a, b| a - b),
        "*" => fold_num(1.0, |a, b| a * b),
        "/" => fold_num(1.0, |a, b| a / b),
        "mod" => Ok(Val::Num(n(0)?.rem_euclid(n(1)?))),
        "pow" => Ok(Val::Num(n(0)?.powf(n(1)?))),
        "inc" => Ok(Val::Num(n(0)? + 1.0)),
        "dec" => Ok(Val::Num(n(0)? - 1.0)),
        "=" => Ok(Val::Bool((n(0)? - n(1)?).abs() < 1e-9)),
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
            // DMK sine(period, amp, x): amp * sin(2π x / period)
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
        "iota" => {
            let count = n(0)? as usize;
            Ok(Val::Arr(Rc::new(
                (0..count).map(|k| Val::Num(k as f64)).collect(),
            )))
        }
        "nth" => match &args[0] {
            Val::Arr(items) if !items.is_empty() => {
                let i = n(1)? as i64;
                let len = items.len() as i64;
                Ok(items[(i.rem_euclid(len)) as usize].clone())
            }
            v => Err(format!("nth: expected non-empty array, got {:?}", v)),
        },
        "stutter" => {
            // (stutter n xs): each element repeated n times; with cyclic
            // indexing this is exactly floor-division indexing.
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
        _ => Err(format!("unknown function '{}'", name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edn::read_one;

    fn ev(src: &str) -> Val {
        let f = read_one(src).unwrap();
        evaluate(&f, &Env::empty(), &mut Ctx::default()).unwrap()
    }

    #[test]
    fn arithmetic_and_math_macro() {
        let f = read_one("m\"0.2*(i+1)*(i+2)\"").unwrap();
        let env = Env::empty().bind("i".into(), Val::Num(3.0));
        let v = evaluate(&f, &env, &mut Ctx::default()).unwrap();
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
        let p = dyn_pose(&d, 1.0, &st).unwrap();
        assert!(p.x.abs() < 1e-9 && (p.y - 4.0).abs() < 1e-9, "rotated 90°: {:?}", p);
    }

    #[test]
    fn circle_with_child_is_product() {
        let Val::Arr(items) = ev("(circle 5 (linear c[4 0]))") else { panic!() };
        assert_eq!(items.len(), 5);
        let Val::Dyn(d) = &items[1] else { panic!() };
        let st = MotionState::new();
        let p = dyn_pose(d, 1.0, &st).unwrap();
        let (ex, ey) = (
            4.0 * (72f64).to_radians().cos(),
            4.0 * (72f64).to_radians().sin(),
        );
        assert!((p.x - ex).abs() < 1e-9 && (p.y - ey).abs() < 1e-9);
    }

    #[test]
    fn closed_polar_dyn() {
        // (polar m"2*t" m"20*t") — the 060 spiral, slot-bound t (F12)
        let Val::Dyn(d) = ev("(polar m\"2*t\" m\"20*t\")") else { panic!() };
        let st = MotionState::new();
        let p = dyn_pose(&d, 1.0, &st).unwrap();
        let (ex, ey) = (2.0 * (20f64).to_radians().cos(), 2.0 * (20f64).to_radians().sin());
        assert!((p.x - ex).abs() < 1e-9 && (p.y - ey).abs() < 1e-9, "{:?}", p);
        // without t, polar is a plain point
        assert!(matches!(ev("p[2 90]"), Val::Vec2 { .. }));
    }

    #[test]
    fn vel_integrates() {
        // constant velocity integrates to linear motion
        let Val::Dyn(d) = ev("(vel c[4 0])") else { panic!() };
        let mut st = MotionState::new();
        let dt = 1.0 / TICK_RATE;
        for k in 0..120 {
            step_motion(&d, k as f64 * dt, dt, &mut st).unwrap();
        }
        let p = dyn_pose(&d, 1.0, &st).unwrap();
        assert!((p.x - 4.0).abs() < 1e-6, "integrated x: {}", p.x);
        assert!(is_scanned(&d));
    }

    #[test]
    fn aim_is_ambient_relative() {
        let mut ctx = Ctx { player: (0.0, -4.0), ambient: Pose::IDENTITY };
        let f = read_one("(aim player)").unwrap();
        let Val::Pose(p) = evaluate(&f, &Env::empty(), &mut ctx).unwrap() else {
            panic!()
        };
        assert!((p.th - -90.0).abs() < 1e-9, "aim down: {}", p.th);
    }

    #[test]
    fn plus_translates_formations() {
        // (+ c[-7 0] (arrow 3 1.0 0.5)): rotational back-offset (080)
        let Val::Arr(items) = ev("(+ c[-7 0] (arrow 3 1.0 0.5))") else { panic!() };
        assert_eq!(items.len(), 3);
        let Val::Pose(center) = &items[1] else { panic!() };
        assert!((center.x - -7.0).abs() < 1e-9 && center.y.abs() < 1e-9);
    }
}
