use super::*;

const NAMES: &[&str] = &[
    "cart", "polar", "pose", "rot", "still", "linear", "angle-of", "mag",
];

pub(crate) fn is_builtin(name: &str) -> bool {
    NAMES.contains(&name)
}

pub(crate) fn builtin(name: &str, args: &[Val]) -> Result<Option<Val>, String> {
    let r = match name {
        "cart" => Ok(Val::Pose(Pose::point(arg_num(name, args, 0)?, arg_num(name, args, 1)?))),
        "polar" => {
            let (r, th) = (arg_num(name, args, 0)?, arg_num(name, args, 1)?);
            let (s, c) = th.to_radians().sin_cos();
            Ok(Val::Pose(Pose::point(r * c, r * s)))
        }
        "pose" => {
            let figure = as_pose(args[0].clone()).map(Val::Pose)?;
            match args.get(1) {
                Some(m) => Ok(wrap_elem_fields(figure, elem_fields_from_builtin_map(name, m)?)),
                None => Ok(figure),
            }
        }
        "rot" => match &args[0] {
            Val::Arr(xs) => Ok(Val::arr(
                xs.iter()
                    .map(|v| v.num().map(|th| Val::Pose(Pose::oriented(0.0, 0.0, th))))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            v => Ok(Val::Pose(Pose::oriented(0.0, 0.0, v.num()?))),
        },
        "still" => Ok(Val::Pose(Pose::IDENTITY)),
        "linear" => {
            let figure = match &args[0] {
                Val::Pose(p) => Ok(Val::DynPose(DynPose::pose_node(Rc::new(DynNode::Linear {
                    vx: p.x,
                    vy: p.y,
                })))),
                v => Err(format!("linear: expected point, got {:?}", v)),
            }?;
            match args.get(1) {
                Some(m) => Ok(wrap_elem_fields(figure, elem_fields_from_builtin_map(name, m)?)),
                None => Ok(figure),
            }
        }
        "angle-of" => match &args[0] {
            Val::Pose(p) => Ok(Val::Num(p.y.atan2(p.x).to_degrees())),
            v => Err(format!("angle-of: expected point, got {:?}", v)),
        },
        "mag" => match &args[0] {
            Val::Pose(p) => Ok(Val::Num((p.x * p.x + p.y * p.y).sqrt())),
            v => Err(format!("mag: expected point, got {:?}", v)),
        },
        _ => return Ok(None),
    };
    r.map(Some)
}

fn elem_fields_from_builtin_map(name: &str, m: &Val) -> Result<Vec<(Rc<str>, FieldSeed)>, String> {
    let Val::Map(kvs) = m else {
        return Err(format!("{}: trailing fields must be a map, got {:?}", name, m));
    };
    let mut out = Vec::new();
    for (k, v) in kvs.iter() {
        let Val::Kw(key) = k else {
            return Err(format!("{}: expected keyword field name, got {:?}", name, k));
        };
        match v {
            Val::Nothing => {}
            Val::Num(n) => out.push((key.clone(), FieldSeed::Num(*n))),
            Val::Kw(s) => out.push((key.clone(), FieldSeed::Sym(s.clone()))),
            Val::DynLike(_) | Val::DynPose(_) | Val::DynFigure(_) => {
                return Err(format!("{}: trailing field :{} cannot capture signal seeds here; use (fields ...) for signal seeds", name, key));
            }
            other => return Err(format!("{}: trailing field :{} expected number or keyword, got {:?}; use (fields ...) for signal seeds", name, key, other)),
        }
    }
    Ok(out)
}
