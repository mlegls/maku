use super::*;


pub(crate) fn builtin(name: &str, args: &[Val]) -> Result<Option<Val>, String> {
    let r = match name {
        "forms" => match seq_view(&args[0]) {
            Some(xs) => Ok(Val::Arr(xs)),
            None => Err(format!("forms: not a form sequence: {:?}", args[0])),
        },
        "get" => Ok(get_in(&args[0], &args[1])
            .or_else(|| args.get(2).cloned())
            .unwrap_or(Val::Nothing)),
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
        "seq?" => Ok(mask(seq_view(&args[0]).is_some())),
        _ => return Ok(None),
    };
    r.map(Some)
}
