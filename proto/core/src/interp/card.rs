//! Card loading: top-level defs, defns, defpatterns (callable).

use crate::edn::Form;
use std::collections::HashMap;
use std::rc::Rc;


// ---------------------------------------------------------------------------
// Card: top-level definitions.

pub struct Pattern {
    pub name: String,
    pub params: Vec<(Rc<str>, Form)>,
    pub body: Rc<[Form]>,
}

pub struct Card {
    pub patterns: HashMap<String, Pattern>,
    pub order: Vec<String>,
    pub defs: HashMap<String, Form>,
}

pub fn load_card(forms: &[Form]) -> Result<Card, String> {
    let mut patterns = HashMap::new();
    let mut order = Vec::new();
    let mut defs = HashMap::new();
    for f in forms {
        if let Form::List(items) = f {
            match items.first() {
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
                    // patterns are callable: synthesize (fn [] (let [p d ...]
                    // (seq body...))) so (par (bowap) (other)) composes.
                    // (Prototype: defaults only; §10 scope adapters later.)
                    if !defs.contains_key(&name) {
                        let mut binds = Vec::new();
                        for (pn, dflt) in &params {
                            binds.push(Form::Sym(pn.clone()));
                            binds.push(dflt.clone());
                        }
                        let mut letf = vec![Form::sym("let"), Form::Vector(binds.into())];
                        let mut seqf = vec![Form::sym("seq")];
                        seqf.extend(items[3..].iter().cloned());
                        letf.push(Form::list(seqf));
                        defs.insert(
                            name.clone(),
                            Form::list(vec![
                                Form::sym("fn"),
                                Form::Vector(Vec::new().into()),
                                Form::list(letf),
                            ]),
                        );
                    }
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
                _ => {}
            }
        }
    }
    Ok(Card { patterns, order, defs })
}
