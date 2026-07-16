use super::*;


pub(crate) fn builtin(name: &str, args: &[Val]) -> Result<Option<Val>, String> {
    let r = match name {
        "iota" => {
            let count = arg_num(name, args, 0)? as usize;
            Ok(Val::arr((0..count).map(|k| Val::Num(k as f64)).collect()))
        }
        "range" => {
            let (a, b) = (arg_num(name, args, 0)?, arg_num(name, args, 1)?);
            let step = if args.len() > 2 { arg_num(name, args, 2)? } else { 1.0 };
            let mut out = Vec::new();
            let mut x = a;
            while (step > 0.0 && x < b) || (step < 0.0 && x > b) {
                out.push(Val::Num(x));
                x += step;
            }
            Ok(Val::arr(out))
        }
        "nth" => {
            let subject = match seq_view(&args[0]) {
                Some(xs) => Val::Arr(xs),
                None => args[0].clone(),
            };
            match (&subject, &args[1]) {
                (Val::DynLike(d), Val::Arr(idxs)) => {
                    let DynLike::List(items) = &**d else {
                        return Err(format!("nth: expected non-empty array, got {:?}", subject));
                    };
                    if items.is_empty() {
                        return Err(format!("nth: expected non-empty array, got {:?}", subject));
                    }
                    let out = idxs
                        .iter()
                        .map(|i| {
                            let k = i.num()? as i64;
                            Ok(items[(k.rem_euclid(items.len() as i64)) as usize].clone())
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok(Val::DynLike(Rc::new(DynLike::List(out.into()))))
                }
                (Val::DynLike(d), i) => {
                    let DynLike::List(items) = &**d else {
                        return Err(format!("nth: expected non-empty array, got {:?}", subject));
                    };
                    if items.is_empty() {
                        return Err(format!("nth: expected non-empty array, got {:?}", subject));
                    }
                    let k = i.num()? as i64;
                    dynlike_to_structural_val(&items[(k.rem_euclid(items.len() as i64)) as usize])
                }
                (Val::Arr(items), Val::Arr(idxs)) if !items.is_empty() => {
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
            let x = arg_num(name, args, 0)?;
            let out = items
                .iter()
                .filter(|v| !matches!(v, Val::Num(y) if (*y - x).abs() < 1e-9))
                .cloned()
                .collect();
            Ok(Val::arr(out))
        }
        "stutter" => {
            let reps = arg_num(name, args, 0)? as usize;
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
            let k = (arg_num(name, args, 0)?.max(0.0)) as usize;
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
        _ => return Ok(None),
    };
    r.map(Some)
}
