//! The pure function vocabulary.

use super::*;
use std::rc::Rc;

#[path = "builtins/array.rs"]
mod array;
#[path = "builtins/geometry.rs"]
mod geometry;
#[path = "builtins/language.rs"]
mod language;
#[path = "builtins/math.rs"]
mod math;

pub(crate) fn is_builtin(name: &str) -> bool {
    math::is_builtin(name)
        || geometry::is_builtin(name)
        || array::is_builtin(name)
        || language::is_builtin(name)
}

pub(crate) fn mask(b: bool) -> Val {
    Val::Num(if b { 1.0 } else { 0.0 })
}

pub(crate) fn arg_num(name: &str, args: &[Val], i: usize) -> Result<f64, String> {
    args.get(i)
        .ok_or_else(|| format!("{}: missing argument {}", name, i))?
        .num()
}

pub(crate) fn fold_num(
    args: &[Val],
    init: f64,
    f: fn(f64, f64) -> f64,
) -> Result<Val, String> {
    let mut acc = if args.is_empty() { Val::Num(init) } else { args[0].clone() };
    if args.len() == 1 {
        acc = num_bin(Val::Num(init), acc, f)?;
    }
    for a in &args[1..] {
        acc = num_bin(acc, a.clone(), f)?;
    }
    Ok(acc)
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
        (Val::Pose(p), Val::DynPose(d)) | (Val::DynPose(d), Val::Pose(p)) => Ok(Val::DynPose(
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

pub(crate) fn val_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(a), Val::Num(b)) => (a - b).abs() < 1e-9,
        (Val::Kw(a), Val::Kw(b)) => a == b,
        (Val::Nothing, Val::Nothing) => true,
        (Val::Pose(a), Val::Pose(b)) => {
            (a.x - b.x).abs() < 1e-9
                && (a.y - b.y).abs() < 1e-9
                && match (a.theta, b.theta) {
                    (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                    (None, None) => true,
                    _ => false,
                }
        }
        (Val::Handle(a), Val::Handle(b)) => a == b,
        (Val::Arr(a), Val::Arr(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| val_eq(a, b))
        }
        (Val::Map(a), Val::Map(b)) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|((ak, av), (bk, bv))| {
                    val_eq(ak, bk) && val_eq(av, bv)
                })
        }
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

pub(crate) fn builtin_with_tick_rate(name: &str, args: &[Val], tick_rate: f64) -> Result<Val, String> {
    if let Some(r) = math::builtin(name, args, tick_rate)? {
        return Ok(r);
    }
    if let Some(r) = geometry::builtin(name, args)? {
        return Ok(r);
    }
    if let Some(r) = array::builtin(name, args)? {
        return Ok(r);
    }
    if let Some(r) = language::builtin(name, args)? {
        return Ok(r);
    }
    Err(format!("unknown function '{}'", name))
}
