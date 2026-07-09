//! Runtime render boundary rows.

#[derive(Clone, Debug)]
pub enum RenderData {
    None,
    Point { x: f64, y: f64, theta: f64, scale: f64, alpha: f64, hue: f64 },
    Polyline { points: Vec<(f64, f64)>, active: bool },
}
