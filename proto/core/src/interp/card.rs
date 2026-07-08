//! Card loading: top-level defs, defns, defpatterns (callable).

use crate::edn::Form;
use std::collections::HashMap;
use std::rc::Rc;


// ---------------------------------------------------------------------------
// Card: top-level definitions.

#[derive(Clone)]
pub struct Pattern {
    pub name: String,
    pub params: Vec<(Rc<str>, Form)>,
    pub body: Rc<[Form]>,
}

/// (defmacro name [params…] body…): a tree transformation (§10). Arguments
/// arrive UNEVALUATED as forms; the body (usually quasiquoted) returns a
/// form, which evaluates in the caller's scope. Unhygienic in the classic
/// way — templates should use unusual names for introduced bindings.
#[derive(Clone)]
pub struct Macro {
    pub params: Vec<Rc<str>>,
    pub body: Rc<[Form]>,
}

pub struct Card {
    pub patterns: HashMap<String, Pattern>,
    pub order: Vec<String>,
    pub defs: HashMap<String, Form>,
    pub macros: HashMap<String, Macro>,
    /// (defchannel $name expr) — derived channels, evaluated once per tick
    /// during channel refresh, in definition order. Card code, not engine:
    /// this is how the stdlib publishes $enemies/$nearest-enemy.
    pub channels: Vec<(Rc<str>, Form)>,
    /// (defcontact [:a :b] opts? f) — evaluated when the card is installed,
    /// registering contact rules into World data.
    pub contacts: Vec<Form>,
}

pub fn load_card(forms: &[Form]) -> Result<Card, String> {
    let mut patterns = HashMap::new();
    let mut order = Vec::new();
    let mut defs = HashMap::new();
    let mut macros = HashMap::new();
    let mut channels: Vec<(Rc<str>, Form)> = Vec::new();
    let mut contacts: Vec<Form> = Vec::new();
    for f in forms {
        if let Form::List(items) = f {
            match items.first() {
                Some(Form::Sym(s)) if &**s == "defmacro" => {
                    let name = match items.get(1) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defmacro: expected name".into()),
                    };
                    let params: Vec<Rc<str>> = match items.get(2) {
                        Some(Form::Vector(ps)) => ps
                            .iter()
                            .map(|p| match p {
                                Form::Sym(n) => Ok(n.clone()),
                                _ => Err("defmacro: params must be symbols".to_string()),
                            })
                            .collect::<Result<_, _>>()?,
                        _ => return Err("defmacro: expected param vector".into()),
                    };
                    macros.insert(name, Macro { params, body: items[3..].to_vec().into() });
                }
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
                    order.retain(|n| n != &name);
                    order.push(name.clone());
                    patterns.insert(name.clone(), Pattern { name, params, body });
                }
                Some(Form::Sym(s)) if &**s == "def" => {
                    if let Some(Form::Sym(n)) = items.get(1) {
                        defs.insert(n.to_string(), items[2].clone());
                    }
                }
                Some(Form::Sym(s)) if &**s == "defn" => {
                    // (defn name [params] body...) → def name (fn [params] body...)
                    if let Some(Form::Sym(n)) = items.get(1) {
                        let mut fnform = vec![Form::sym("fn")];
                        fnform.extend(items[2..].iter().cloned());
                        defs.insert(n.to_string(), Form::list(fnform));
                    }
                }
                Some(Form::Sym(s)) if &**s == "defcollider" => {
                    // (defcollider name [e ctx] body...)
                    //
                    // Register a collider projector value. Evaluation first
                    // tries to elaborate the body into the projector algebra;
                    // unsupported forms fall back to the callable bridge.
                    let name = match items.get(1) {
                        Some(Form::Sym(n)) => n.to_string(),
                        _ => return Err("defcollider: expected name".into()),
                    };
                    let params: Vec<Form> = match items.get(2) {
                        Some(Form::Vector(ps)) if ps.len() == 2 => {
                            ps.iter()
                                .map(|p| match p {
                                    Form::Sym(_) => Ok(p.clone()),
                                    _ => Err("defcollider: params must be symbols".to_string()),
                                })
                                .collect::<Result<_, _>>()?
                        }
                        Some(Form::Vector(_)) => {
                            return Err("defcollider: expected two parameters".into());
                        }
                        _ => return Err("defcollider: expected parameter vector".into()),
                    };
                    if items.len() < 4 {
                        return Err("defcollider: expected body".into());
                    }
                    let mut projector = vec![
                        Form::sym("__collider-projector"),
                        Form::Vector(params.into()),
                    ];
                    projector.extend(items[3..].iter().cloned());
                    let form = Form::list(vec![Form::sym("__defcollider"), Form::list(projector)]);
                    defs.insert(name, form);
                }
                Some(Form::Sym(s)) if &**s == "defchannel" => {
                    // (defchannel $name expr): a per-tick derived channel.
                    // Redefinition replaces (imports first, card later —
                    // ordinary shadowing), order otherwise preserved.
                    let Some(Form::Sym(n)) = items.get(1) else {
                        return Err("defchannel: expected a $channel name".into());
                    };
                    let Some(name) = n.strip_prefix('$') else {
                        return Err("defchannel: name must start with $".into());
                    };
                    let Some(expr) = items.get(2) else {
                        return Err(format!("defchannel ${}: expected an expression", name));
                    };
                    let name: Rc<str> = name.into();
                    channels.retain(|(k, _)| *k != name);
                    channels.push((name, expr.clone()));
                }
                Some(Form::Sym(s)) if &**s == "defcontact" => {
                    contacts.push(f.clone());
                }
                _ => {}
            }
        }
    }
    Ok(Card { patterns, order, defs, macros, channels, contacts })
}
