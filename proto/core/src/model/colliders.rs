//! Runtime collider boundary rows.

use super::Symbol;

#[derive(Clone, Debug)]
pub enum ColliderData {
    None,
    Circle { layer: Symbol, center: (f64, f64), radius: f64 },
    CapsuleChain { layer: Symbol, points: Vec<(f64, f64)>, radius: f64 },
}

impl ColliderData {
    pub fn layer(&self) -> Option<Symbol> {
        match self {
            ColliderData::None => None,
            ColliderData::Circle { layer, .. } | ColliderData::CapsuleChain { layer, .. } => {
                Some(*layer)
            }
        }
    }
}
