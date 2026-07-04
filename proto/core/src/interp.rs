//! Control-layer interpreter + prototype signal representation.
//!
//! Per language.md §2: Actions are inert data; the scheduler (sim.rs) walks
//! them with an explicit stack. Expressions evaluate instantly and purely;
//! only Action leaves (wait/spawn/event) interact with time. Seq bodies are
//! LAZY (each item form evaluates when reached), which gives loop-var and
//! cell timing for free.

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

/// Prototype dyn: enough structure for the Closed subset of the corpus.
#[derive(Debug)]
pub enum DynNode {
    Const(Pose),
    /// pos = v·τ in the local frame; θ = heading.
    Linear { vx: f64, vy: f64 },
    Frame(Rc<DynNode>, Rc<DynNode>),
}

pub fn dyn_pose(d: &DynNode, tau: f64) -> Pose {
    match d {
        DynNode::Const(p) => *p,
        DynNode::Linear { vx, vy } => Pose {
            x: vx * tau,
            y: vy * tau,
            th: vy.atan2(*vx).to_degrees(),
        },
        DynNode::Frame(parent, child) => dyn_pose(parent, tau).compose(&dyn_pose(child, tau)),
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

pub struct Ctx {
    // reserved: RNG, channels, control cells
}

pub fn evaluate(form: &Form, env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    match form {
        Form::Num(n) => Ok(Val::Num(*n)),
        Form::Bool(b) => Ok(Val::Bool(*b)),
        Form::Str(s) => Ok(Val::Str(s.clone())),
        Form::Kw(k) => Ok(Val::Kw(k.clone())),
        Form::Sym(s) => match &**s {
            "inf" => Ok(Val::Num(f64::INFINITY)),
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
        if env.lookup(name).is_none() {
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
    // [i n, x xs, ... :every dt]
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

fn sf_circle(items: &[Form], env: &Env, ctx: &mut Ctx) -> Result<Val, String> {
    let n = evaluate(&items[1], env, ctx)?.num()? as usize;
    if n == 0 {
        return Err("circle: zero elements".into());
    }
    let poses: Vec<Val> = (0..n)
        .map(|k| Val::Pose(Pose { x: 0.0, y: 0.0, th: k as f64 * 360.0 / n as f64 }))
        .collect();
    let arr = Val::Arr(Rc::new(poses));
    match items.get(2) {
        None => Ok(arr),
        Some(childf) => {
            // trailing-child sugar
            let child = evaluate(childf, env, ctx)?;
            apply_frame_arr(&arr, child)
        }
    }
}

// ---------------------------------------------------------------------------
// Builtins.

fn builtin(name: &str, args: &[Val]) -> Result<Val, String> {
    let n = |i: usize| -> Result<f64, String> {
        args.get(i)
            .ok_or_else(|| format!("{}: missing argument {}", name, i))?
            .num()
    };
    let fold = |init: f64, f: fn(f64, f64) -> f64| -> Result<Val, String> {
        let mut acc = if args.is_empty() { init } else { args[0].num()? };
        if args.len() == 1 && matches!(name, "-" | "/") {
            acc = f(init, acc); // unary negate / reciprocal
        }
        for a in &args[1..] {
            acc = f(acc, a.num()?);
        }
        Ok(Val::Num(acc))
    };
    match name {
        "+" => fold(0.0, |a, b| a + b),
        "-" => fold(0.0, |a, b| a - b),
        "*" => fold(1.0, |a, b| a * b),
        "/" => fold(1.0, |a, b| a / b),
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
        "lerp" => {
            // (lerp a b ctrl v1 v2): pure pointwise, controller bounds
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
        evaluate(&f, &Env::empty(), &mut Ctx {}).unwrap()
    }

    #[test]
    fn arithmetic_and_math_macro() {
        let f = read_one("m\"0.2*(i+1)*(i+2)\"").unwrap();
        let env = Env::empty().bind("i".into(), Val::Num(3.0));
        let v = evaluate(&f, &env, &mut Ctx {}).unwrap();
        assert!((v.num().unwrap() - 0.2 * 4.0 * 5.0).abs() < 1e-9);
    }

    #[test]
    fn cyclic_nth_and_iota() {
        assert_eq!(ev("(nth [10 20 30] 7)").num().unwrap(), 20.0);
        assert_eq!(ev("(nth [10 20 30] -1)").num().unwrap(), 30.0);
        let Val::Arr(items) = ev("(iota 4)") else { panic!() };
        assert_eq!(items.len(), 4);
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
        // ((rot 90) (linear c[4 0])) — dyn composed under a rotated frame
        let Val::Dyn(d) = ev("((rot 90) (linear c[4 0]))") else {
            panic!("expected dyn")
        };
        let p = dyn_pose(&d, 1.0);
        assert!(p.x.abs() < 1e-9 && (p.y - 4.0).abs() < 1e-9, "rotated 90°: {:?}", p);
    }

    #[test]
    fn circle_with_child_is_product() {
        let Val::Arr(items) = ev("(circle 5 (linear c[4 0]))") else { panic!() };
        assert_eq!(items.len(), 5);
        let Val::Dyn(d) = &items[1] else { panic!() };
        let p = dyn_pose(d, 1.0);
        let (ex, ey) = (
            4.0 * (72f64).to_radians().cos(),
            4.0 * (72f64).to_radians().sin(),
        );
        assert!((p.x - ex).abs() < 1e-9 && (p.y - ey).abs() < 1e-9);
    }
}
