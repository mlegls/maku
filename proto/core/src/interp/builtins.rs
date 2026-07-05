//! The pure function vocabulary.

use super::*;
use std::rc::Rc;

const BUILTINS: &[&str] = &[
    "+", "-", "*", "/", "mod", "pow", "inc", "dec", "=", "<", ">", "<=", ">=", "min", "max",
    "abs", "quot", "ticks", "and", "or", "not", "sin", "cos", "sine", "lssht", "cart", "polar", "pose", "rot", "still",
    "linear", "iota", "range", "nth", "without", "stutter", "lerp", "lerp3", "lerpsmooth",
    "angle-of", "mag", "einsine", "eoutsine", "eiosine",
    "count", "first", "rest", "drop", "take", "concat", "forms", "get",
    "form-type", "form-name", "nothing?",
];

pub(crate) fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
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

pub(crate) fn add2(a: Val, b: Val) -> Result<Val, String> {
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

/// View a value as a sequence for the generic seq builtins. Arrays view as
/// themselves; a FORM list/vector views as its subforms (each wrapped back
/// up as a form value) — which is what lets macro code take unevaluated
/// clauses apart with the ordinary vocabulary (count/first/rest/nth/…).
fn seq_view(v: &Val) -> Option<Vec<Val>> {
    match v {
        Val::Arr(xs) => Some((**xs).clone()),
        Val::FormV(f) => match &**f {
            Form::List(xs) | Form::Vector(xs) => {
                Some(xs.iter().map(|x| Val::FormV(Rc::new(x.clone()))).collect())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Look a key up in a map VALUE or a map FORM. Form lookups return the
/// value subform unevaluated (macro time); missing/non-map yields None —
/// `get` is total so macro code can probe without pre-checking shapes.
fn get_in(subject: &Val, key: &Val) -> Option<Val> {
    match subject {
        Val::Map(_) => match key {
            Val::Kw(k) | Val::Str(k) => super::spawn::map_get(subject, k),
            _ => None,
        },
        Val::FormV(f) => match &**f {
            Form::Map(kvs) => kvs
                .iter()
                .find(|(k, _)| match (key, k) {
                    (Val::Kw(a), Form::Kw(b)) => a == b,
                    (Val::Str(a), Form::Str(b)) => a == b,
                    (Val::Num(a), Form::Num(b)) => (a - b).abs() < 1e-9,
                    _ => false,
                })
                .map(|(_, v)| Val::FormV(Rc::new(v.clone()))),
            _ => None,
        },
        _ => None,
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
        "rot" => match &args[0] {
            // broadcasts: (rot (* 10 (iota 30))) is 30 rotation frames —
            // spawn combinators are arithmetic on pose arrays (§5)
            Val::Arr(xs) => Ok(Val::Arr(Rc::new(
                xs.iter()
                    .map(|v| v.num().map(|th| Val::Pose(Pose { x: 0.0, y: 0.0, th })))
                    .collect::<Result<Vec<_>, _>>()?,
            ))),
            v => Ok(Val::Pose(Pose { x: 0.0, y: 0.0, th: v.num()? })),
        },
        "still" => Ok(Val::Pose(Pose::IDENTITY)),
        "and" => Ok(Val::Bool(args.iter().all(truthy))),
        "or" => Ok(Val::Bool(args.iter().any(truthy))),
        "not" => Ok(Val::Bool(!truthy(&args[0]))),
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
        // nth also indexes form lists/vectors (macro-time clause access)
        "nth" => {
            let subject = match seq_view(&args[0]) {
                Some(xs) => Val::Arr(Rc::new(xs)),
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
                Ok(Val::Arr(Rc::new(out)))
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
            Some(xs) => Ok(Val::Arr(Rc::new(xs.get(1..).unwrap_or(&[]).to_vec()))),
            None => Err(format!("rest: not a sequence: {:?}", args[0])),
        },
        "drop" | "take" => {
            let k = (n(0)?.max(0.0)) as usize;
            let xs = seq_view(&args[1])
                .ok_or_else(|| format!("{}: not a sequence: {:?}", name, args[1]))?;
            let out = if name == "drop" {
                xs.get(k.min(xs.len())..).unwrap_or(&[]).to_vec()
            } else {
                xs.get(..k.min(xs.len())).unwrap_or(&[]).to_vec()
            };
            Ok(Val::Arr(Rc::new(out)))
        }
        "concat" => {
            let mut out = Vec::new();
            for a in args {
                match seq_view(a) {
                    Some(xs) => out.extend(xs),
                    None => return Err(format!("concat: not a sequence: {:?}", a)),
                }
            }
            Ok(Val::Arr(Rc::new(out)))
        }
        // a form list/vector as an array of subform values (what ~@ splices)
        "forms" => match seq_view(&args[0]) {
            Some(xs) => Ok(Val::Arr(Rc::new(xs))),
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
                Val::Str(_) => "str",
                Val::Kw(_) => "kw",
                Val::Bool(_) => "bool",
                Val::Arr(_) => "arr",
                Val::Map(_) => "map",
                Val::Nothing => "nothing",
                _ => "opaque",
            }
            .into(),
        )),
        // name of a sym/kw (form or value); "" otherwise — total on purpose
        "form-name" => Ok(Val::Str(match &args[0] {
            Val::FormV(f) => match &**f {
                Form::Sym(s) | Form::Kw(s) | Form::Str(s) => s.clone(),
                _ => "".into(),
            },
            Val::Kw(s) | Val::Str(s) => s.clone(),
            _ => "".into(),
        })),
        "nothing?" => Ok(Val::Bool(matches!(args[0], Val::Nothing))),
        _ => Err(format!("unknown function '{}'", name)),
    }
}

