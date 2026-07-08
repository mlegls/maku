use super::*;

const NAMES: &[&str] = &[
    "+", "-", "*", "/", "mod", "pow", "inc", "dec", "=", "<", ">", "<=", ">=", "min",
    "max", "abs", "quot", "ticks", "not", "sin", "cos", "sine", "lssht", "lerp", "lerp3",
    "lerpsmooth", "einsine", "eoutsine", "eiosine",
];

pub(crate) fn is_builtin(name: &str) -> bool {
    NAMES.contains(&name)
}

pub(crate) fn builtin(name: &str, args: &[Val]) -> Result<Option<Val>, String> {
    let r = match name {
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
            _ => fold_num(args, 0.0, |a, b| a - b),
        },
        "*" => fold_num(args, 1.0, |a, b| a * b),
        "/" => fold_num(args, 1.0, |a, b| a / b),
        "mod" => Ok(Val::Num(arg_num(name, args, 0)?.rem_euclid(arg_num(name, args, 1)?))),
        "pow" => Ok(Val::Num(arg_num(name, args, 0)?.powf(arg_num(name, args, 1)?))),
        "inc" => Ok(Val::Num(arg_num(name, args, 0)? + 1.0)),
        "dec" => Ok(Val::Num(arg_num(name, args, 0)? - 1.0)),
        "=" => Ok(mask(val_eq(&args[0], &args[1]))),
        "<" => Ok(mask(arg_num(name, args, 0)? < arg_num(name, args, 1)?)),
        ">" => Ok(mask(arg_num(name, args, 0)? > arg_num(name, args, 1)?)),
        "<=" => Ok(mask(arg_num(name, args, 0)? <= arg_num(name, args, 1)?)),
        ">=" => Ok(mask(arg_num(name, args, 0)? >= arg_num(name, args, 1)?)),
        "min" => Ok(Val::Num(arg_num(name, args, 0)?.min(arg_num(name, args, 1)?))),
        "max" => Ok(Val::Num(arg_num(name, args, 0)?.max(arg_num(name, args, 1)?))),
        "abs" => Ok(Val::Num(arg_num(name, args, 0)?.abs())),
        "quot" => Ok(Val::Num((arg_num(name, args, 0)? / arg_num(name, args, 1)?).trunc())),
        "ticks" => Ok(Val::Num(arg_num(name, args, 0)? / TICK_RATE)),
        "not" => Ok(mask(arg_num(name, args, 0)? == 0.0)),
        "sin" => Ok(Val::Num(arg_num(name, args, 0)?.to_radians().sin())),
        "cos" => Ok(Val::Num(arg_num(name, args, 0)?.to_radians().cos())),
        "sine" => {
            let (period, amp, x) = (
                arg_num(name, args, 0)?,
                arg_num(name, args, 1)?,
                arg_num(name, args, 2)?,
            );
            Ok(Val::Num(amp * (std::f64::consts::TAU * x / period).sin()))
        }
        "lerp" => {
            let (a, b, ctrl, v1, v2) = (
                arg_num(name, args, 0)?,
                arg_num(name, args, 1)?,
                arg_num(name, args, 2)?,
                arg_num(name, args, 3)?,
                arg_num(name, args, 4)?,
            );
            let r = ((ctrl - a) / (b - a)).clamp(0.0, 1.0);
            Ok(Val::Num(v1 + r * (v2 - v1)))
        }
        "lerp3" => {
            let (a1, b1, a2, b2, ctrl) = (
                arg_num(name, args, 0)?,
                arg_num(name, args, 1)?,
                arg_num(name, args, 2)?,
                arg_num(name, args, 3)?,
                arg_num(name, args, 4)?,
            );
            let (v1, v2, v3) = (
                arg_num(name, args, 5)?,
                arg_num(name, args, 6)?,
                arg_num(name, args, 7)?,
            );
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
            let ename = match &args[0] {
                Val::Builtin(nm) => nm.to_string(),
                v => return Err(format!("lerpsmooth: expected easing fn, got {:?}", v)),
            };
            let (a, b, ctrl, v1, v2) = (
                arg_num(name, args, 1)?,
                arg_num(name, args, 2)?,
                arg_num(name, args, 3)?,
                arg_num(name, args, 4)?,
                arg_num(name, args, 5)?,
            );
            let r = ((ctrl - a) / (b - a)).clamp(0.0, 1.0);
            Ok(Val::Num(v1 + ease(&ename, r) * (v2 - v1)))
        }
        "einsine" | "eoutsine" | "eiosine" => Ok(Val::Num(ease(name, arg_num(name, args, 0)?))),
        "lssht" => {
            let (c, pv, f1, f2) = (
                arg_num(name, args, 0)?,
                arg_num(name, args, 1)?,
                arg_num(name, args, 2)?,
                arg_num(name, args, 3)?,
            );
            let w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
            let _ = w;
            let k = c;
            let m = (k * f1).exp() + (k * f2).exp();
            Ok(Val::Num(m.ln() / k))
        }
        _ => return Ok(None),
    };
    r.map(Some)
}
