//! Explicit value-to-dyn coercion machinery.
//!
//! This stays interpreter-local for now because deferred dyn values carry
//! interpreter forms and environments. The important boundary is that
//! language coercions are centralized as explicit branches, rather than hidden
//! behind Rust conversion traits.

use super::*;
use std::rc::Rc;

impl DataAtom {
    pub fn from_val(v: Val) -> Result<DataAtom, Val> {
        match v {
            Val::Num(n) => Ok(DataAtom::Num(n)),
            Val::Kw(k) => Ok(DataAtom::Kw(k)),
            Val::Pose(p) => Ok(DataAtom::Figure(Figure::Pose(p))),
            Val::Figure(f) => Ok(DataAtom::Figure(f)),
            Val::Handle(id) => Ok(DataAtom::Handle(id)),
            Val::Nothing => Ok(DataAtom::Nothing),
            other => Err(other),
        }
    }

    pub fn to_val(&self) -> Val {
        match self {
            DataAtom::Num(n) => Val::Num(*n),
            DataAtom::Kw(k) => Val::Kw(k.clone()),
            DataAtom::Figure(Figure::Pose(p)) => Val::Pose(*p),
            DataAtom::Figure(f) => Val::Figure(f.clone()),
            DataAtom::Handle(id) => Val::Handle(*id),
            DataAtom::Nothing => Val::Nothing,
        }
    }
}

/// A value-level structure that can be lifted into a typed dyn at an
/// expected boundary. This is not source syntax and not a typed `Dyn<T>` by
/// itself: `DynLike::Dyn` keeps a deferred expression plus its environment,
/// while typed boundaries such as spawn interpret the structure with `as_*`
/// schema checks. Ordinary static structures still evaluate to ordinary
/// arrays/maps.
#[derive(Clone, Debug)]
pub enum DynLike {
    Atom(DataAtom),
    Dyn(DynVal),
    List(Rc<[DynLike]>),
    Map(Rc<Vec<(DataAtom, DynLike)>>),
}

#[derive(Clone, Debug)]
pub enum DynVal {
    Pose(DynPose),
    Expr { form: Form, env: Env },
}

impl DynLike {
    pub fn is_dynamic(&self) -> bool {
        match self {
            DynLike::Atom(_) => false,
            DynLike::Dyn(_) => true,
            DynLike::List(items) => items.iter().any(DynLike::is_dynamic),
            DynLike::Map(kvs) => kvs.iter().any(|(_, v)| v.is_dynamic()),
        }
    }

    pub fn eval(&self, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Val, String> {
        match self {
            DynLike::Atom(v) => Ok(v.to_val()),
            DynLike::Dyn(DynVal::Pose(d)) => eval_dyn(d, tau, state, sig).map(Val::Pose),
            DynLike::Dyn(DynVal::Expr { form, env }) => {
                eval_sig(form, env, sig, tau, 0.0, Some(read_scan(state, 0)), None)
            }
            DynLike::List(items) => items
                .iter()
                .map(|v| v.eval(tau, state, sig))
                .collect::<Result<Vec<_>, _>>()
                .map(Val::arr),
            DynLike::Map(kvs) => kvs
                .iter()
                .map(|(k, v)| Ok((k.to_val(), v.eval(tau, state, sig)?)))
                .collect::<Result<Vec<_>, String>>()
                .map(|pairs| Val::Map(Rc::new(pairs))),
        }
    }

    pub fn from_val(v: Val) -> Result<DynLike, String> {
        match v {
            Val::DynLike(d) => Ok((*d).clone()),
            Val::Dyn(d) => Ok(DynLike::Dyn(DynVal::Pose(d))),
            Val::Arr(items) => items
                .iter()
                .cloned()
                .map(DynLike::from_val)
                .collect::<Result<Vec<_>, _>>()
                .map(|items| DynLike::List(items.into())),
            Val::Map(kvs) => kvs
                .iter()
                .map(|(k, v)| {
                    Ok((
                        data_atom_from_key(k.clone())?,
                        DynLike::from_val(v.clone())?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(|pairs| DynLike::Map(Rc::new(pairs))),
            other => DataAtom::from_val(other)
                .map(DynLike::Atom)
                .map_err(|v| format!("unsupported dyn data atom: {:?}", v)),
        }
    }
}

pub(crate) fn data_atom_from_key(v: Val) -> Result<DataAtom, String> {
    DataAtom::from_val(v).map_err(|v| format!("unsupported dyn map key: {:?}", v))
}

pub(crate) fn dynlike_arr(v: &DynLike) -> Option<Vec<DynLike>> {
    match v {
        DynLike::List(items) => Some(items.iter().cloned().collect()),
        _ => None,
    }
}

pub(crate) fn as_dynlike_list(v: &DynLike, what: &str) -> Result<Vec<DynLike>, String> {
    dynlike_arr(v).ok_or_else(|| format!("{}: expected array", what))
}

pub(crate) fn dynlike_to_val(v: &DynLike) -> Result<Val, String> {
    if v.is_dynamic() {
        Ok(Val::DynLike(Rc::new(v.clone())))
    } else {
        v.eval(0.0, &MotionState::new(), &SigEnv::default())
    }
}

pub(crate) fn dynlike_to_structural_val(v: &DynLike) -> Result<Val, String> {
    match v {
        DynLike::Atom(atom) => Ok(atom.to_val()),
        DynLike::Dyn(DynVal::Pose(d)) => Ok(Val::Dyn(d.clone())),
        DynLike::Dyn(DynVal::Expr { .. }) => Ok(Val::DynLike(Rc::new(v.clone()))),
        DynLike::List(items) => items
            .iter()
            .map(dynlike_to_structural_val)
            .collect::<Result<Vec<_>, _>>()
            .map(Val::arr),
        DynLike::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((k.to_val(), dynlike_to_structural_val(v)?)))
            .collect::<Result<Vec<_>, String>>()
            .map(|pairs| Val::Map(Rc::new(pairs))),
    }
}

pub(crate) fn dynlike_meta_pairs(v: &DynLike) -> Result<Vec<(Val, Val)>, String> {
    match v {
        DynLike::Map(kvs) => kvs
            .iter()
            .map(|(k, v)| Ok((k.to_val(), dynlike_to_val(v)?)))
            .collect(),
        _ => Err("spawn meta: expected map".into()),
    }
}

pub(crate) fn dynlike_map_get(m: &DynLike, key: &str) -> Option<DynLike> {
    match m {
        DynLike::Map(kvs) => {
            for (k, v) in kvs.iter() {
                if matches!(k, DataAtom::Kw(kw) if &**kw == key) {
                    return Some(v.clone());
                }
            }
            None
        }
        _ => None,
    }
}

pub(crate) fn dynlike_kw(v: &DynLike) -> Option<Rc<str>> {
    match v {
        DynLike::Atom(DataAtom::Kw(k)) => Some(k.clone()),
        _ => None,
    }
}

pub(crate) fn as_dyn_num(v: &DynLike) -> Result<DynNum, String> {
    match v {
        DynLike::Dyn(DynVal::Expr { form, env }) => Ok(DynNum::num_expr(form.clone(), env.clone())),
        DynLike::Atom(DataAtom::Num(n)) => Ok(DynNum::num(*n)),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

pub(crate) fn as_static_num(v: &DynLike) -> Result<f64, String> {
    match v {
        DynLike::Atom(DataAtom::Num(n)) => Ok(*n),
        DynLike::Dyn(_) => Err("expected static number".into()),
        _ => Err(format!("expected number, got {:?}", v)),
    }
}

pub(crate) fn dynlike_map_as_static_num(
    m: &DynLike,
    key: &str,
    default: f64,
) -> Result<f64, String> {
    match dynlike_map_get(m, key) {
        Some(v) => as_static_num(&v),
        None => Ok(default),
    }
}

pub(crate) fn dynlike_map_as_dyn_num_any(
    m: &DynLike,
    keys: &[&str],
    default: f64,
) -> Result<DynNum, String> {
    for key in keys {
        if let Some(v) = dynlike_map_get(m, key) {
            return as_dyn_num(&v);
        }
    }
    Ok(DynNum::num(default))
}
