//! Runtime render boundary rows.

#[derive(Clone, Debug)]
pub enum RenderData {
    None,
    Polyline { points: Vec<(f64, f64)>, active: bool },
}
