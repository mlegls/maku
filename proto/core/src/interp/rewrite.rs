use super::{is_builtin, Card};
use crate::edn::Form;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub(crate) const VALUE_OR_INTRINSIC: &str = "%value-or";

#[derive(Clone)]
struct TrivialDef {
    params: Vec<Rc<str>>,
    body: Form,
}

pub(crate) fn rewrite_card(card: &mut Card) {
    let empty = HashMap::new();
    rewrite_card_forms(card, &empty);

    let trivial = collect_trivial_defs(&card.defs);
    rewrite_card_forms(card, &trivial);
}

fn rewrite_card_forms(card: &mut Card, trivial: &HashMap<String, TrivialDef>) {
    let root_bound = builtin_shadows(card);
    for f in card.defs.values_mut() {
        *f = rewrite_form(f.clone(), &mut root_bound.clone(), trivial);
    }
    for pat in card.patterns.values_mut() {
        for (_, default) in &mut pat.params {
            *default = rewrite_form(default.clone(), &mut root_bound.clone(), trivial);
        }
        pat.body = rewrite_body(&pat.body, &mut root_bound.clone(), trivial);
    }
    for (_, f) in &mut card.channels {
        *f = rewrite_form(f.clone(), &mut root_bound.clone(), trivial);
    }
    for f in &mut card.tick_rules {
        *f = rewrite_form(f.clone(), &mut root_bound.clone(), trivial);
    }
}

fn builtin_shadows(card: &Card) -> HashSet<String> {
    card.defs
        .keys()
        .filter(|name| is_builtin(name))
        .cloned()
        .collect()
}

fn rewrite_body(
    body: &Rc<[Form]>,
    bound: &mut HashSet<String>,
    trivial: &HashMap<String, TrivialDef>,
) -> Rc<[Form]> {
    body.iter()
        .cloned()
        .map(|f| rewrite_form(f, bound, trivial))
        .collect::<Vec<_>>()
        .into()
}

fn rewrite_form(
    form: Form,
    bound: &mut HashSet<String>,
    trivial: &HashMap<String, TrivialDef>,
) -> Form {
    let rewritten = match form {
        Form::List(items) => rewrite_list(&items, bound, trivial),
        Form::Vector(items) => Form::Vector(
            items
                .iter()
                .cloned()
                .map(|f| rewrite_form(f, bound, trivial))
                .collect::<Vec<_>>()
                .into(),
        ),
        Form::Map(kvs) => Form::Map(
            kvs.iter()
                .cloned()
                .map(|(k, v)| (rewrite_form(k, bound, trivial), rewrite_form(v, bound, trivial)))
                .collect::<Vec<_>>()
                .into(),
        ),
        atom => atom,
    };
    rewrite_value_or_shape(rewritten, bound)
}

fn rewrite_list(
    items: &Rc<[Form]>,
    bound: &mut HashSet<String>,
    trivial: &HashMap<String, TrivialDef>,
) -> Form {
    let Some(Form::Sym(head)) = items.first() else {
        return Form::List(
            items
                .iter()
                .cloned()
                .map(|f| rewrite_form(f, bound, trivial))
                .collect::<Vec<_>>()
                .into(),
        );
    };

    match head.as_ref() {
        "fn" => {
            let mut out = Vec::with_capacity(items.len());
            out.push(items[0].clone());
            if let Some(params) = items.get(1) {
                out.push(params.clone());
                let names = param_names(params);
                with_bound(bound, &names, |bound| {
                    out.extend(items[2..].iter().cloned().map(|f| rewrite_form(f, bound, trivial)));
                });
            }
            Form::List(out.into())
        }
        "let" => {
            if let Some(Form::Vector(binds)) = items.get(1) {
                let mut local = bound.clone();
                let mut new_binds = Vec::with_capacity(binds.len());
                for pair in binds.chunks(2) {
                    if pair.len() == 2 {
                        new_binds.push(pair[0].clone());
                        new_binds.push(rewrite_form(pair[1].clone(), &mut local, trivial));
                        for name in binding_names(&pair[0]) {
                            local.insert(name);
                        }
                    }
                }
                let mut out = vec![items[0].clone(), Form::Vector(new_binds.into())];
                out.extend(items[2..].iter().cloned().map(|f| rewrite_form(f, &mut local, trivial)));
                Form::List(out.into())
            } else {
                Form::List(items.to_vec().into())
            }
        }
        "loop" => {
            if let Some(Form::Vector(binds)) = items.get(1) {
                let names = binds
                    .chunks(2)
                    .filter_map(|pair| match pair.first() {
                        Some(Form::Sym(s)) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let mut out_binds = Vec::with_capacity(binds.len());
                for pair in binds.chunks(2) {
                    if pair.len() == 2 {
                        out_binds.push(pair[0].clone());
                        out_binds.push(rewrite_form(pair[1].clone(), bound, trivial));
                    }
                }
                let mut out = vec![items[0].clone(), Form::Vector(out_binds.into())];
                with_bound(bound, &names, |bound| {
                    out.extend(items[2..].iter().cloned().map(|f| rewrite_form(f, bound, trivial)));
                });
                Form::List(out.into())
            } else {
                Form::List(items.to_vec().into())
            }
        }
        _ => {
            let out: Vec<Form> =
                items.iter().cloned().map(|f| rewrite_form(f, bound, trivial)).collect();
            if let Some(Form::Sym(name)) = out.first() {
                if !bound.contains(name.as_ref()) {
                    if let Some(def) = trivial.get(name.as_ref()) {
                        if out.len() == def.params.len() + 1
                            && out[1..].iter().all(|arg| is_pure(arg, bound))
                        {
                            let args = def
                                .params
                                .iter()
                                .cloned()
                                .zip(out[1..].iter().cloned())
                                .collect::<HashMap<_, _>>();
                            return substitute(&def.body, &args);
                        }
                    }
                }
            }
            Form::List(out.into())
        }
    }
}

fn rewrite_value_or_shape(form: Form, bound: &HashSet<String>) -> Form {
    let Form::List(items) = &form else {
        return form;
    };
    if items.len() != 4 || !sym_is(&items[0], "if") || bound.contains("if") {
        return form;
    }
    let Form::List(cond) = &items[1] else {
        return form;
    };
    if cond.len() != 2 || !sym_is(&cond[0], "nothing?") || bound.contains("nothing?") {
        return form;
    }
    let x = &cond[1];
    if x == &items[3] && is_pure(x, bound) {
        return Form::list(vec![
            Form::Sym(VALUE_OR_INTRINSIC.into()),
            x.clone(),
            items[2].clone(),
        ]);
    }
    form
}

fn collect_trivial_defs(defs: &HashMap<String, Form>) -> HashMap<String, TrivialDef> {
    defs.iter()
        .filter_map(|(name, form)| trivial_def(form).map(|def| (name.clone(), def)))
        .collect()
}

fn trivial_def(form: &Form) -> Option<TrivialDef> {
    let Form::List(items) = form else { return None };
    if items.len() != 3 || !sym_is(&items[0], "fn") {
        return None;
    }
    let Form::Vector(params) = &items[1] else { return None };
    let params = params
        .iter()
        .map(|p| match p {
            Form::Sym(s) => Some(s.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let body = items[2].clone();
    let Form::List(call) = &body else { return None };
    let Some(Form::Sym(head)) = call.first() else { return None };
    if head.as_ref() != VALUE_OR_INTRINSIC && !is_builtin(head) {
        return None;
    }
    if call.len() != params.len() + 1 {
        return None;
    }
    let mut seen = HashSet::new();
    for arg in &call[1..] {
        let Form::Sym(s) = arg else { return None };
        if !params.iter().any(|p| p.as_ref() == s.as_ref()) || !seen.insert(s.to_string()) {
            return None;
        }
    }
    if seen.len() != params.len() {
        return None;
    }
    Some(TrivialDef { params, body })
}

fn substitute(form: &Form, args: &HashMap<Rc<str>, Form>) -> Form {
    match form {
        Form::Sym(s) => args.get(s).cloned().unwrap_or_else(|| form.clone()),
        Form::List(items) => {
            Form::List(items.iter().map(|f| substitute(f, args)).collect::<Vec<_>>().into())
        }
        Form::Vector(items) => {
            Form::Vector(items.iter().map(|f| substitute(f, args)).collect::<Vec<_>>().into())
        }
        Form::Map(kvs) => Form::Map(
            kvs.iter()
                .map(|(k, v)| (substitute(k, args), substitute(v, args)))
                .collect::<Vec<_>>()
                .into(),
        ),
        _ => form.clone(),
    }
}

fn is_pure(form: &Form, bound: &HashSet<String>) -> bool {
    match form {
        Form::Num(_) | Form::Str(_) | Form::Kw(_) | Form::Bool(_) => true,
        Form::Sym(_) => true,
        Form::Vector(items) => items.iter().all(|f| is_pure(f, bound)),
        Form::Map(kvs) => kvs.iter().all(|(k, v)| is_pure(k, bound) && is_pure(v, bound)),
        Form::List(items) => {
            let Some(head) = items.first() else { return true };
            match head {
                Form::Kw(_) => items.len() == 2 && is_pure(&items[1], bound),
                Form::Sym(s) if is_builtin(s) && !bound.contains(s.as_ref()) => {
                    items[1..].iter().all(|f| is_pure(f, bound))
                }
                _ => false,
            }
        }
    }
}

fn param_names(form: &Form) -> Vec<String> {
    match form {
        Form::Vector(params) => params.iter().flat_map(binding_names).collect(),
        _ => Vec::new(),
    }
}

fn binding_names(form: &Form) -> Vec<String> {
    match form {
        Form::Sym(s) if s.as_ref() != "&" => vec![s.to_string()],
        Form::Vector(items) => items.iter().flat_map(binding_names).collect(),
        Form::Map(kvs) => kvs
            .iter()
            .flat_map(|(k, v)| {
                if matches!(k, Form::Kw(kw) if kw.as_ref() == "keys") {
                    binding_names(v)
                } else {
                    Vec::new()
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn with_bound<T>(
    bound: &mut HashSet<String>,
    names: &[String],
    f: impl FnOnce(&mut HashSet<String>) -> T,
) -> T {
    let inserted = names
        .iter()
        .filter_map(|name| bound.insert(name.clone()).then(|| name.clone()))
        .collect::<Vec<_>>();
    let out = f(bound);
    for name in inserted {
        bound.remove(&name);
    }
    out
}

fn sym_is(form: &Form, name: &str) -> bool {
    matches!(form, Form::Sym(s) if s.as_ref() == name)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn rewrite_for_test(src: &str) -> Form {
    let form = crate::edn::read_one(src).unwrap();
    rewrite_form(form, &mut HashSet::new(), &HashMap::new())
}
