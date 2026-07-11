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

/// The extra (non-geometry) fields a batch carries, in emission order.
/// One per compiled render rule; memoized so hosts can key precomputed
/// layouts on `Rc::ptr_eq` (stable once dynamic-kind fields settle).
#[derive(Debug, PartialEq)]
pub struct RenderSchema {
    pub cols: Vec<(Rc<str>, RenderFieldKind)>,
}

/// A geometry column: literal/defaulted slots stay constant, per-row
/// slots materialize.
#[derive(Debug, PartialEq)]
pub enum NumColumn {
    Const(f64),
    Rows(Vec<f64>),
}

impl NumColumn {
    pub fn at(&self, i: usize) -> f64 {
        match self {
            NumColumn::Const(v) => *v,
            NumColumn::Rows(v) => v[i],
        }
    }
}

/// An extra-field column, parallel to `RenderSchema::cols`. Absence per
/// row means the field is not on that row (a `nothing` value).
#[derive(Debug, PartialEq)]
pub enum Column {
    Num(NumColumn),
    /// Per-row nums with presence: value at i is valid iff mask[i].
    NumOpt(Vec<f64>, Vec<bool>),
    SymConst(Rc<str>),
    Syms(Vec<Option<Rc<str>>>),
}

/// One compiled render pass's rows as typed columns (point geometry).
/// Expanding a batch in stream position reproduces the row sequence
/// exactly; `RenderRow` remains the semantic reference form.
#[derive(Debug, PartialEq)]
pub struct RenderBatch {
    pub schema: Rc<RenderSchema>,
    pub len: usize,
    pub x: NumColumn,
    pub y: NumColumn,
    pub theta: NumColumn,
    pub scale: NumColumn,
    pub alpha: NumColumn,
    pub hue: NumColumn,
    pub cols: Vec<Column>,
}

impl RenderBatch {
    pub fn expand_row(&self, i: usize) -> RenderRow {
        let mut row = RenderRow::plain(RenderData::Point {
            x: self.x.at(i),
            y: self.y.at(i),
            theta: self.theta.at(i),
            scale: self.scale.at(i),
            alpha: self.alpha.at(i),
            hue: self.hue.at(i),
        });
        for ((key, _), col) in self.schema.cols.iter().zip(self.cols.iter()) {
            match col {
                Column::Num(c) => row.nums.push((key.clone(), c.at(i))),
                Column::NumOpt(v, mask) => {
                    if mask[i] {
                        row.nums.push((key.clone(), v[i]));
                    }
                }
                Column::SymConst(s) => row.syms.push((key.clone(), s.clone())),
                Column::Syms(v) => {
                    if let Some(s) = &v[i] {
                        row.syms.push((key.clone(), s.clone()));
                    }
                }
            }
        }
        row
    }

    pub fn expand_into(&self, out: &mut Vec<RenderRow>) {
        out.reserve(self.len);
        for i in 0..self.len {
            out.push(self.expand_row(i));
        }
    }
}

/// One position in a tick's render frame: draw order is item order, and a
/// batch's rows sit at its stream position in matched-row order.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderItem {
    Row(Rc<RenderRow>),
    Batch(Rc<RenderBatch>),
}

impl RenderItem {
    /// Rows carried by this item, appended in draw order. `RenderData::None`
    /// rows are skipped (they are placeholders, never drawn).
    pub fn expand_into(&self, out: &mut Vec<RenderRow>) {
        match self {
            RenderItem::Row(row) => {
                if !matches!(row.data, RenderData::None) {
                    out.push(row.as_ref().clone());
                }
            }
            RenderItem::Batch(batch) => batch.expand_into(out),
        }
    }
}
