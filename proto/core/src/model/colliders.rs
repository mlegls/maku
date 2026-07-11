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

/// Distance from a point to a polyline (capsule-chain narrow phase).
fn dist_to_chain(p: (f64, f64), pts: &[(f64, f64)]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for seg in pts.windows(2) {
        let (ax, ay) = seg[0];
        let (bx, by) = seg[1];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((p.0 - ax) * dx + (p.1 - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        let d = ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt();
        best = Some(best.map_or(d, |b: f64| b.min(d)));
    }
    best
}

fn dist2_points(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn segment_distance(a0: (f64, f64), a1: (f64, f64), b0: (f64, f64), b1: (f64, f64)) -> f64 {
    let samples = [a0, a1, b0, b1];
    let mut best = f64::INFINITY;
    if let Some(d) = dist_to_chain(a0, &[b0, b1]) {
        best = best.min(d);
    }
    if let Some(d) = dist_to_chain(a1, &[b0, b1]) {
        best = best.min(d);
    }
    if let Some(d) = dist_to_chain(b0, &[a0, a1]) {
        best = best.min(d);
    }
    if let Some(d) = dist_to_chain(b1, &[a0, a1]) {
        best = best.min(d);
    }
    if best.is_finite() {
        best
    } else {
        samples
            .windows(2)
            .map(|w| dist2_points(w[0], w[1]).sqrt())
            .fold(f64::INFINITY, f64::min)
    }
}

fn chain_distance(a: &[(f64, f64)], b: &[(f64, f64)]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for aseg in a.windows(2) {
        for bseg in b.windows(2) {
            let d = segment_distance(aseg[0], aseg[1], bseg[0], bseg[1]);
            best = Some(best.map_or(d, |cur| cur.min(d)));
        }
    }
    best
}

pub fn collider_overlap(a: &ColliderData, b: &ColliderData) -> bool {
    match (a, b) {
        (ColliderData::None, _) | (_, ColliderData::None) => false,
        (
            ColliderData::Circle { center: ac, radius: ar, .. },
            ColliderData::Circle { center: bc, radius: br, .. },
        ) => dist2_points(*ac, *bc) < (ar + br).powi(2),
        (
            ColliderData::CapsuleChain { points, radius: ar, .. },
            ColliderData::Circle { center, radius: br, .. },
        )
        | (
            ColliderData::Circle { center, radius: br, .. },
            ColliderData::CapsuleChain { points, radius: ar, .. },
        ) => dist_to_chain(*center, points).is_some_and(|d| d < ar + br),
        (
            ColliderData::CapsuleChain { points: ap, radius: ar, .. },
            ColliderData::CapsuleChain { points: bp, radius: br, .. },
        ) => chain_distance(ap, bp).is_some_and(|d| d < ar + br),
    }
}
