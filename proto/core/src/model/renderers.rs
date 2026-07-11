//! Runtime render boundary rows.

use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum RenderData {
    None,
    Point { x: f64, y: f64, theta: f64, scale: f64, alpha: f64, hue: f64 },
    Polyline { points: Vec<(f64, f64)>, active: bool },
}

/// Open host-facing render row: structural geometry plus schema-checked
/// keyed fields. Field vocabulary is card/host policy, not core semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRow {
    pub data: RenderData,
    pub nums: Vec<(Rc<str>, f64)>,
    pub syms: Vec<(Rc<str>, Rc<str>)>,
}

impl RenderRow {
    pub fn plain(data: RenderData) -> RenderRow {
        RenderRow { data, nums: Vec::new(), syms: Vec::new() }
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.nums.iter().find_map(|(k, v)| (&**k == key).then_some(*v))
    }

    pub fn sym(&self, key: &str) -> Option<&str> {
        self.syms.iter().find_map(|(k, v)| (&**k == key).then_some(v.as_ref()))
    }
}

/// Field kinds for the per-world render row schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderFieldKind { Num, Sym }
