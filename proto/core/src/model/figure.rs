//! Semantic geometry types.

use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Degrees, canonical (language.md §11). `None` means the pose only
    /// specifies a point; consumers that need facing derive it from context.
    pub theta: Option<f64>,
}

impl Pose {
    pub const IDENTITY: Pose = Pose { x: 0.0, y: 0.0, theta: Some(0.0) };

    pub const fn point(x: f64, y: f64) -> Pose {
        Pose { x, y, theta: None }
    }

    pub const fn oriented(x: f64, y: f64, theta: f64) -> Pose {
        Pose { x, y, theta: Some(theta) }
    }

    pub fn angle_or(self, default: f64) -> f64 {
        self.theta.unwrap_or(default)
    }

    /// The (sin, cos) of this pose's heading — compose's rotation,
    /// cacheable when the pose is constant.
    pub fn heading_rot(&self) -> (f64, f64) {
        self.angle_or(0.0).to_radians().sin_cos()
    }

    /// SE(2) composition: self ∘ child (child expressed in self's frame).
    pub fn compose(&self, child: &Pose) -> Pose {
        self.compose_with_rot(self.heading_rot(), child)
    }

    /// `compose` with the rotation supplied by the caller — MUST be this
    /// pose's `heading_rot()`, precomputed. Split out so constant frames
    /// can skip the per-eval sincos without diverging from `compose`.
    pub fn compose_with_rot(&self, (s, c): (f64, f64), child: &Pose) -> Pose {
        Pose {
            x: self.x + c * child.x - s * child.y,
            y: self.y + s * child.x + c * child.y,
            theta: match (self.theta, child.theta) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum CurveDomain {
    Range { min: f64, max: f64 },
    Values(Rc<[f64]>),
}

#[derive(Debug, Clone)]
pub enum SampleSet {
    /// Concrete parameter values supplied by the constructor/caller.
    Values(Rc<[f64]>),
    /// Compatibility sampling for ranged curves. Higher-level constructors
    /// should prefer Values when they need an exact concrete curve.
    Step { resolution: f64 },
}

#[derive(Debug, Clone)]
pub enum CurveEval<E> {
    /// Compatibility straight curve along the local +x axis.
    Straight,
    /// Evaluator/program for `(t, u) -> Pose`, chosen by the frontend/backend.
    Expr(E),
}

#[derive(Debug, Clone)]
pub struct ParametricCurve<E> {
    pub eval: CurveEval<E>,
    pub domain: CurveDomain,
}

#[derive(Debug, Clone)]
pub struct Curve<E> {
    pub frame: Pose,
    pub spec: ParametricCurve<E>,
}

#[derive(Debug, Clone)]
pub enum Figure<E> {
    Pose(Pose),
    Curve(Curve<E>),
}
