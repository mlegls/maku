use super::*;

const CURVE_R: f64 = 0.08; // compatibility curve half-width for collision

/// Sample a world-space curve at `tau` (shared by render and collision).
pub fn sample_curve(b: &Entity, tau: f64, sig: &SigEnv) -> Option<Vec<(f64, f64)>> {
    sample_curve_frac(b, tau, sig, 1.0)
}

/// Sample the curve up to `frac` of its length. frac 1.0 = the whole path.
pub fn sample_curve_frac(
    b: &Entity,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let projection = b.renderers.first().map(DynRender::polyline)?;
    sample_curve_projection(b, tau, sig, frac, &projection.sample_set, &projection.u_max_sig)
}

fn sample_curve_collider_frac(
    b: &Entity,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let (_, projection, _) = b.colliders.iter().find_map(DynCollider::capsule_chain)?;
    sample_curve_projection(b, tau, sig, frac, &projection.sample_set, &projection.u_max_sig)
}

fn sample_curve_projection(
    b: &Entity,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
    sample_set: &SampleSet,
    u_max_sig: &Option<DynNum>,
) -> Option<Vec<(f64, f64)>> {
    let Figure::Curve(curve) = eval_dyn_figure(&b.dyn_figure, tau, &b.state, sig).ok()? else {
        return None;
    };
    if frac <= 0.0 {
        return None;
    }
    let us: Vec<f64> = match (&curve.spec.domain, sample_set) {
        (_, SampleSet::Values(vals)) => {
            if vals.is_empty() {
                return None;
            }
            let n = ((vals.len() as f64) * frac.min(1.0)).ceil() as usize;
            vals.iter().take(n.max(2).min(vals.len())).copied().collect()
        }
        (CurveDomain::Values(vals), SampleSet::Step { .. }) => {
            if vals.is_empty() {
                return None;
            }
            let n = ((vals.len() as f64) * frac.min(1.0)).ceil() as usize;
            vals.iter().take(n.max(2).min(vals.len())).copied().collect()
        }
        (CurveDomain::Range { min, max }, SampleSet::Step { resolution }) => {
            let min = *min;
            let max = match u_max_sig {
                Some(d) => eval_dyn(d, tau, &b.state, sig).unwrap_or(*max),
                None => *max,
            };
            let end = min + (max - min) * frac.min(1.0);
            let span = (end - min).abs().max(0.01);
            let steps = ((span / resolution).ceil() as usize).clamp(2, 400);
            (0..=steps).map(|k| min + (end - min) * k as f64 / steps as f64).collect()
        }
    };
    let mut pts = Vec::with_capacity(us.len());
    for u in us {
        let local = eval_curve_pose(&curve.spec.eval, tau, u, &b.state, sig).ok()?;
        let w = curve.frame.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

/// A curve's hot fraction at age tau. Curves without :fill are hot in full
/// the moment the warn ends. :fill itself is a fraction signal; helpers like
/// fill-linear live in card/library code.
pub fn hot_frac(activity: &SlotActivity, tau: f64, sig: &SigEnv) -> f64 {
    if let Some(d) = &activity.hot_frac_sig {
        return eval_dyn(d, tau, &MotionState::new(), sig)
            .map(|x| x.clamp(0.0, 1.0))
            .unwrap_or(1.0);
    }
    1.0
}

pub fn eval_collider_slot(
    b: &Entity,
    slot: &DynCollider,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
) -> ColliderData {
    match slot.repr() {
        ColliderDynRepr::Slot(projection) => match &projection.shape {
            ColliderSlotShape::Circle { radius } => {
                let radius = eval_dyn(radius, tau, &b.state, sig).unwrap_or(0.0);
                match b.dyn_figure.repr() {
                    FigureDynRepr::Pose(_) if b.cache_policy.trace.is_some() => {
                        let points: Vec<(f64, f64)> = b.trail.iter().map(|p| (p.x, p.y)).collect();
                        if points.len() < 2 {
                            ColliderData::None
                        } else {
                            ColliderData::CapsuleChain {
                                layer: projection.layer.clone(),
                                points,
                                radius: CURVE_R + radius * scale,
                            }
                        }
                    }
                    FigureDynRepr::Pose(_) => match dyn_figure_pose(&b.dyn_figure, tau, &b.state, sig) {
                        Ok(p) => ColliderData::Circle {
                            layer: projection.layer.clone(),
                            center: (p.x, p.y),
                            radius: radius * scale,
                        },
                        Err(_) => ColliderData::None,
                    },
                    FigureDynRepr::Curve { .. } => ColliderData::None,
                }
            }
            ColliderSlotShape::CapsuleChain { radius, slot: curve_slot } => {
                let radius = eval_dyn(radius, tau, &b.state, sig).unwrap_or(0.0);
                if tau < curve_slot.activity.warn {
                    return ColliderData::None;
                }
                match b.dyn_figure.repr() {
                    FigureDynRepr::Pose(_) if b.cache_policy.trace.is_some() => {
                        let points: Vec<(f64, f64)> = b.trail.iter().map(|p| (p.x, p.y)).collect();
                        if points.len() < 2 {
                            ColliderData::None
                        } else {
                            ColliderData::CapsuleChain {
                                layer: projection.layer.clone(),
                                points,
                                radius: CURVE_R * curve_slot.width + radius * scale,
                            }
                        }
                    }
                    FigureDynRepr::Curve { .. } => {
                        let Some(points) = sample_curve_collider_frac(
                            b,
                            tau,
                            sig,
                            hot_frac(&curve_slot.activity, tau, sig),
                        ) else {
                            return ColliderData::None;
                        };
                        ColliderData::CapsuleChain {
                            layer: projection.layer.clone(),
                            points,
                            radius: CURVE_R * curve_slot.width + radius * scale,
                        }
                    }
                    FigureDynRepr::Pose(_) => ColliderData::None,
                }
            }
        },
    }
}

pub fn eval_render_slot(
    b: &Entity,
    slot: &DynRender,
    tau: f64,
    sig: &SigEnv,
) -> Vec<RenderData> {
    match slot.repr() {
        RenderDynRepr::Polyline(projection) => {
            let hot = hot_frac(&projection.activity, tau, sig);
            let partly = tau >= projection.activity.warn && hot < 1.0;
            let mut out = Vec::new();
            match sample_curve(b, tau, sig) {
                Some(points) => out.push(RenderData::Polyline {
                    points,
                    // a filling curve's full path stays a telegraph
                    active: tau >= projection.activity.warn && !partly,
                }),
                None => out.push(RenderData::None),
            }
            if partly {
                match sample_curve_frac(b, tau, sig, hot) {
                    Some(points) => out.push(RenderData::Polyline { points, active: true }),
                    None => out.push(RenderData::None),
                }
            }
            out
        }
    }
}
