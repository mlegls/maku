//! The pure function vocabulary.

use super::*;
use std::rc::Rc;

const BUILTINS: &[&str] = &[
    "+", "-", "*", "/", "mod", "pow", "inc", "dec", "=", "<", ">", "<=", ">=", "min", "max",
    "abs", "quot", "ticks", "not", "sin", "cos", "sine", "lssht", "cart", "polar", "pose", "rot", "still",
    "linear", "iota", "range", "nth", "without", "stutter", "lerp", "lerp3", "lerpsmooth",
    "angle-of", "mag", "einsine", "eoutsine", "eiosine",
    "count", "first", "rest", "drop", "take", "concat", "forms", "get",
    "form-type", "form-name", "nothing?", "num?",
];

pub(crate) fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

fn mask(b: bool) -> Val {
    Val::Num(if b { 1.0 } else { 0.0 })
}

/// Broadcast-aware numeric binop (§5: zips cycle; scalars lift).
pub(crate) fn num_bin(a: Val, b: Val, f: fn(f64, f64) -> f64) -> Result<Val, String> {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => Ok(Val::Num(f(x, y))),
        (Val::Arr(xs), Val::Arr(ys)) => {
            let len = xs.len().max(ys.len());
            let out = (0..len)
                .map(|k| num_bin(xs[k % xs.len()].clone(), ys[k % ys.len()].clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        (Val::Arr(xs), y) => {
            let out = xs
                .iter()
                .map(|x| num_bin(x.clone(), y.clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        (x, Val::Arr(ys)) => {
            let out = ys
                .iter()
                .map(|y| num_bin(x.clone(), y.clone(), f))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        (a, b) => Err(format!("numeric op on {:?} and {:?}", a, b)),
    }
}

pub(crate) fn add2(a: Val, b: Val) -> Result<Val, String> {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => Ok(Val::Num(x + y)),
        (Val::Pose(a), Val::Pose(b)) => Ok(Val::Pose(Pose {
            x: a.x + b.x,
            y: a.y + b.y,
            theta: match (a.theta, b.theta) {
                (Some(x), Some(y)) => Some(x + y),
                (Some(x), None) => Some(x),
                (None, Some(y)) => Some(y),
                (None, None) => None,
            },
        })),
        (v @ Val::Pose(_), Val::Arr(items)) | (Val::Arr(items), v @ Val::Pose(_)) => {
            let out = items
                .iter()
                .map(|i| add2(v.clone(), i.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Val::arr(out))
        }
        (Val::Pose(p), Val::Dyn(d)) | (Val::Dyn(d), Val::Pose(p)) => Ok(Val::Dyn(
            DynPose::pose_node(Rc::new(DynNode::Translate {
                dx: p.x,
                dy: p.y,
                child: d.into_node(),
            })),
        )),
        (a @ (Val::Num(_) | Val::Arr(_)), b @ (Val::Num(_) | Val::Arr(_))) => {
            num_bin(a, b, |x, y| x + y)
        }
        (a, b) => Err(format!("+: cannot add {:?} and {:?}", a, b)),
    }
}

/// View a value as a sequence for the generic seq builtins. Arrays view as
/// themselves; a FORM list/vector views as its subforms (each wrapped back
/// up as a form value) — which is what lets macro code take unevaluated
/// clauses apart with the ordinary vocabulary (count/first/rest/nth/…).
pub(crate) fn seq_view(v: &Val) -> Option<Seq> {
    match v {
        Val::Arr(xs) => Some(xs.clone()),
        Val::FormV(f) => match &**f {
            // Forms need one materialization into Val::FormV elements; after
            // that, recursive rest/drop/take over the result is all views.
            Form::List(xs) | Form::Vector(xs) => {
                Some(Seq::from_vec(xs.iter().map(|x| Val::FormV(Rc::new(x.clone()))).collect()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Look a key up in a map VALUE or a map FORM. Form lookups return the
/// value subform unevaluated (macro time); missing/non-map yields None —
/// `get` is total so macro code can probe without pre-checking shapes.
pub(crate) fn get_in(subject: &Val, key: &Val) -> Option<Val> {
    match subject {
        Val::Map(kvs) => kvs
            .iter()
            .find(|(k, _)| map_key_matches(key, k))
            .map(|(_, v)| v.clone()),
        Val::FormV(f) => match &**f {
            Form::Map(kvs) => kvs
                .iter()
                .find(|(k, _)| match (key, k) {
                    (Val::Kw(a), Form::Kw(b)) => a == b,
                    (Val::Kw(a), Form::Str(b)) => a == b,
                    (Val::Num(a), Form::Num(b)) => (a - b).abs() < 1e-9,
                    _ => false,
                })
                .map(|(_, v)| Val::FormV(Rc::new(v.clone()))),
            _ => None,
        },
        _ => None,
    }
}

fn map_key_matches(key: &Val, candidate: &Val) -> bool {
    match (key, candidate) {
        (Val::Kw(a), Val::Kw(b)) => a == b,
        (Val::Num(a), Val::Num(b)) => (a - b).abs() < 1e-9,
        _ => false,
    }
}

pub(crate) fn ease(name: &str, r: f64) -> f64 {
    use std::f64::consts::FRAC_PI_2;
    let r = r.clamp(0.0, 1.0);
    match name {
        "einsine" => 1.0 - (r * FRAC_PI_2).cos(),
        "eoutsine" => (r * FRAC_PI_2).sin(),
        "eiosine" => 0.5 - 0.5 * (r * std::f64::consts::PI).cos(),
        _ => r,
    }
}

pub(crate) fn builtin(name: &str, args: &[Val]) -> Result<Val, String> {
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
            (Some(Val::Pose(a)), 2) => {
                if let Val::Pose(b) = &args[1] {
                    Ok(Val::Pose(Pose {
                        x: a.x - b.x,
                        y: a.y - b.y,
                        theta: match (a.theta, b.theta) {
                            (Some(x), Some(y)) => Some(x - y),
                            (Some(x), None) => Some(x),
                            (None, Some(y)) => Some(-y),
                            (None, None) => None,
                        },
                    }))
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
            (Val::Kw(a), Val::Kw(b)) => Ok(mask(a == b)),
            (Val::Map(a), Val::Map(b)) => Ok(mask(format!("{:?}", a) == format!("{:?}", b))),
            _ => Ok(mask((n(0)? - n(1)?).abs() < 1e-9)),
        },
        "<" => Ok(mask(n(0)? < n(1)?)),
        ">" => Ok(mask(n(0)? > n(1)?)),
        "<=" => Ok(mask(n(0)? <= n(1)?)),
        ">=" => Ok(mask(n(0)? >= n(1)?)),
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
        "cart" => Ok(Val::Pose(Pose::point(n(0)?, n(1)?))),
        "polar" => {
            let (r, th) = (n(0)?, n(1)?);
            let (s, c) = th.to_radians().sin_cos();
            Ok(Val::Pose(Pose::point(r * c, r * s)))
        }
        "pose" => as_pose(args[0].clone()).map(Val::Pose),
        "rot" => match &args[0] {
            // broadcasts: (rot (* 10 (iota 30))) is 30 rotation frames —
            // spawn combinators are arithmetic on pose arrays (§5)
            Val::Arr(xs) => Ok(Val::arr(
                xs.iter()
                    .map(|v| v.num().map(|th| Val::Pose(Pose::oriented(0.0, 0.0, th))))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            v => Ok(Val::Pose(Pose::oriented(0.0, 0.0, v.num()?))),
        },
        "still" => Ok(Val::Pose(Pose::IDENTITY)),
        "not" => Ok(mask(n(0)? == 0.0)),
        "linear" => match &args[0] {
            Val::Pose(p) => Ok(Val::Dyn(DynPose::pose_node(Rc::new(DynNode::Linear {
                vx: p.x,
                vy: p.y,
            })))),
            v => Err(format!("linear: expected point, got {:?}", v)),
        },
        "angle-of" => match &args[0] {
            Val::Pose(p) => Ok(Val::Num(p.y.atan2(p.x).to_degrees())),
            v => Err(format!("angle-of: expected point, got {:?}", v)),
        },
        "mag" => match &args[0] {
            Val::Pose(p) => Ok(Val::Num((p.x * p.x + p.y * p.y).sqrt())),
            v => Err(format!("mag: expected point, got {:?}", v)),
        },
        "iota" => {
            let count = n(0)? as usize;
            Ok(Val::arr(
                (0..count).map(|k| Val::Num(k as f64)).collect(),
            ))
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
            Ok(Val::arr(out))
        }
        // nth also indexes form lists/vectors (macro-time clause access)
        "nth" => {
            let subject = match seq_view(&args[0]) {
                Some(xs) => Val::Arr(xs),
                None => args[0].clone(),
            };
            match (&subject, &args[1]) {
            (Val::Arr(items), Val::Arr(idxs)) if !items.is_empty() => {
                // broadcast: (nth xs (iota n))
                let out = idxs
                    .iter()
                    .map(|i| {
                        let k = i.num()? as i64;
                        Ok(items[(k.rem_euclid(items.len() as i64)) as usize].clone())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Val::arr(out))
            }
            (Val::Arr(items), i) if !items.is_empty() => {
                let k = i.num()? as i64;
                Ok(items[(k.rem_euclid(items.len() as i64)) as usize].clone())
            }
            (v, _) => Err(format!("nth: expected non-empty array, got {:?}", v)),
            }
        }
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
            Ok(Val::arr(out))
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
            Ok(Val::arr(out))
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
        // --- generic seq/form vocabulary (macro-time tooling, but not
        // form-specific: everything also works on ordinary arrays/maps) ---
        "count" => match (&args[0], seq_view(&args[0])) {
            (_, Some(xs)) => Ok(Val::Num(xs.len() as f64)),
            (Val::Map(kvs), _) => Ok(Val::Num(kvs.len() as f64)),
            (Val::FormV(f), _) => match &**f {
                Form::Map(kvs) => Ok(Val::Num(kvs.len() as f64)),
                v => Err(format!("count: not a sequence: {:?}", v)),
            },
            (v, _) => Err(format!("count: not a sequence: {:?}", v)),
        },
        "first" => match seq_view(&args[0]) {
            Some(xs) => Ok(xs.first().cloned().unwrap_or(Val::Nothing)),
            None => Err(format!("first: not a sequence: {:?}", args[0])),
        },
        "rest" => match seq_view(&args[0]) {
            Some(xs) => Ok(Val::Arr(xs.view(1.min(xs.len()), xs.len().saturating_sub(1)))),
            None => Err(format!("rest: not a sequence: {:?}", args[0])),
        },
        "drop" | "take" => {
            let k = (n(0)?.max(0.0)) as usize;
            let xs = seq_view(&args[1])
                .ok_or_else(|| format!("{}: not a sequence: {:?}", name, args[1]))?;
            let out = if name == "drop" {
                xs.view(k.min(xs.len()), xs.len() - k.min(xs.len()))
            } else {
                xs.view(0, k.min(xs.len()))
            };
            Ok(Val::Arr(out))
        }
        "concat" => {
            let mut out = Vec::new();
            for a in args {
                match seq_view(a) {
                    Some(xs) => out.extend(xs.iter().cloned()),
                    None => return Err(format!("concat: not a sequence: {:?}", a)),
                }
            }
            Ok(Val::arr(out))
        }
        // a form list/vector as an array of subform values (what ~@ splices)
        "forms" => match seq_view(&args[0]) {
            Some(xs) => Ok(Val::Arr(xs)),
            None => Err(format!("forms: not a form sequence: {:?}", args[0])),
        },
        // total lookup: map values AND map forms; missing/non-map → default
        // (3rd arg) or nothing — macro code probes without shape checks
        "get" => Ok(get_in(&args[0], &args[1])
            .or_else(|| args.get(2).cloned())
            .unwrap_or(Val::Nothing)),
        // the shape of a form (or the kind of any other value), as a keyword
        "form-type" => Ok(Val::Kw(
            match &args[0] {
                Val::FormV(f) => match &**f {
                    Form::Num(_) => "num",
                    Form::Str(_) => "str",
                    Form::Sym(_) => "sym",
                    Form::Kw(_) => "kw",
                    Form::Bool(_) => "bool",
                    Form::List(_) => "list",
                    Form::Vector(_) => "vector",
                    Form::Map(_) => "map",
                },
                Val::Num(_) => "num",
                Val::Kw(_) => "kw",
                Val::Arr(_) => "arr",
                Val::Map(_) => "map",
                Val::Nothing => "nothing",
                _ => "opaque",
            }
            .into(),
        )),
        // name of a sym/kw (form or value); : otherwise — total on purpose
        "form-name" => Ok(Val::Kw(match &args[0] {
            Val::FormV(f) => match &**f {
                Form::Sym(s) | Form::Kw(s) | Form::Str(s) => s.clone(),
                _ => "".into(),
            },
            Val::Kw(s) => s.clone(),
            _ => "".into(),
        })),
        "nothing?" => Ok(mask(matches!(args[0], Val::Nothing))),
        "num?" => Ok(mask(matches!(args[0], Val::Num(_)))),
        _ => Err(format!("unknown function '{}'", name)),
    }
}
