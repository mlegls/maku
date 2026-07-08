use super::*;

const CURVE_R: f64 = 0.08; // compatibility curve half-width for collision

/// Sample a world-space curve at `tau` (shared by render and collision).
pub fn sample_curve(
    dyn_figure: &DynFigure,
    projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
) -> Option<Vec<(f64, f64)>> {
    let projection = first_render_projection(projector, tau, sig)?;
    sample_curve_projection(dyn_figure, tau, sig, 1.0, &projection.sample_set, &projection.u_max_sig)
}

/// Sample the curve up to `frac` of its length. frac 1.0 = the whole path.
pub fn sample_curve_frac(
    dyn_figure: &DynFigure,
    projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let projection = first_render_projection(projector, tau, sig)?;
    sample_curve_projection(dyn_figure, tau, sig, frac, &projection.sample_set, &projection.u_max_sig)
}

pub(crate) fn first_render_projection(
    projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
) -> Option<CurveRenderSlot> {
    let mut defs = Vec::new();
    first_render_projection_into(projector, tau, sig, &mut defs)
}

pub(crate) fn first_render_projection_into(
    projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
    defs: &mut Vec<DynRender>,
) -> Option<CurveRenderSlot> {
    let state = MotionState::new();
    defs.clear();
    materialize_render_defs_into(projector, tau, &state, sig, defs)
        .ok()?;
    defs
        .first()
        .cloned()
        .map(|r| r.polyline().clone())
}

fn sample_curve_collider_frac(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
    projection: &CapsuleChainSlot,
) -> Option<Vec<(f64, f64)>> {
    sample_curve_projection(dyn_figure, tau, sig, frac, &projection.sample_set, &projection.u_max_sig)
}

fn sample_curve_projection(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
    sample_set: &SampleSet,
    u_max_sig: &Option<DynNum>,
) -> Option<Vec<(f64, f64)>> {
    let state = MotionState::new();
    let Figure::Curve(curve) = eval_dyn_figure(dyn_figure, tau, &state, sig).ok()? else {
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
                Some(d) => eval_dyn(d, tau, &state, sig).unwrap_or(*max),
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
        let local = eval_curve_pose(&curve.spec.eval, tau, u, &state, sig).ok()?;
        let w = curve.frame.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

pub fn materialize_collider_defs_into(
    projector: &ColliderProjector,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
    symbols: &mut SymbolTable,
    out: &mut Vec<DynCollider>,
) -> Result<(), String> {
    for list in projector.specs.iter() {
        match &list.expr {
            ColliderProjectorExpr::Stable(slots) => {
                out.extend(slots.iter().cloned());
            }
            ColliderProjectorExpr::DeferredBody { body, env } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let env = env.clone()
                    .bind("e".into(), e_bound.clone())
                    .bind("ctx".into(), ctx_bound.clone());
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                let mut run_world = World::default();
                run_world.symbols = symbols.clone();
                let mut last = Val::Nothing;
                for form in body.iter() {
                    last = evaluate(form, &env, &mut run_ctx, &mut run_world)?;
                }
                *symbols = run_world.symbols;
                match last {
                    Val::ColliderProjectorSpecs(spec) => {
                        materialize_collider_defs_into(
                            &ColliderProjector { specs: vec![spec.as_ref().clone()].into() },
                            tau,
                            state,
                            sig,
                            Some(&e_bound),
                            Some(&ctx_bound),
                            symbols,
                            out,
                        )?;
                    }
                    other => return Err(format!("defcollider: expected collider projector, got {:?}", other)),
                }
            }
            ColliderProjectorExpr::Sum(specs) => {
                materialize_collider_defs_into(
                    &ColliderProjector { specs: specs.clone() },
                    tau,
                    state,
                    sig,
                    e_view,
                    ctx_view,
                    symbols,
                    out,
                )?;
            }
            ColliderProjectorExpr::ActiveWhen { pred, env, child } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let env = env.clone()
                    .bind("e".into(), e_bound.clone())
                    .bind("ctx".into(), ctx_bound.clone());
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                let mut run_world = World::default();
                run_world.symbols = symbols.clone();
                let enabled = truthy_pub(&evaluate(pred, &env, &mut run_ctx, &mut run_world)?);
                *symbols = run_world.symbols;
                if enabled {
                    materialize_collider_defs_into(
                        &ColliderProjector { specs: vec![child.as_ref().clone()].into() },
                        tau,
                        state,
                        sig,
                        Some(&e_bound),
                        Some(&ctx_bound),
                        symbols,
                        out,
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub fn materialize_render_defs_into(
    projector: &RenderProjector,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    out: &mut Vec<DynRender>,
) -> Result<(), String> {
    for list in projector.specs.iter() {
        let val = list.eval(tau, state, sig)?;
        let dynlike = DynLike::from_val(val)?;
        as_stable_render_slots_into(&dynlike, out)?;
    }
    Ok(())
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
    dyn_figure: &DynFigure,
    slot: &DynCollider,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
) -> ColliderData {
    match slot.repr() {
        ColliderDynRepr::Slot(projection) => match &projection.shape {
            ColliderSlotShape::Circle { radius } => {
                let state = MotionState::new();
                let radius = eval_dyn(radius, tau, &state, sig).unwrap_or(0.0);
                match dyn_figure.repr() {
                    FigureDynRepr::Pose(_) if traced => {
                        let points: Vec<(f64, f64)> = trace.iter().map(|p| (p.x, p.y)).collect();
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
                    FigureDynRepr::Pose(_) => ColliderData::Circle {
                        layer: projection.layer.clone(),
                        center: (pose.x, pose.y),
                        radius: radius * scale,
                    },
                    FigureDynRepr::Curve { .. } => ColliderData::None,
                }
            }
            ColliderSlotShape::CapsuleChain { radius, slot: curve_slot } => {
                let state = MotionState::new();
                let radius = eval_dyn(radius, tau, &state, sig).unwrap_or(0.0);
                if tau < curve_slot.activity.warn {
                    return ColliderData::None;
                }
                match dyn_figure.repr() {
                    FigureDynRepr::Pose(_) if traced => {
                        let points: Vec<(f64, f64)> = trace.iter().map(|p| (p.x, p.y)).collect();
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
                            dyn_figure,
                            tau,
                            sig,
                            hot_frac(&curve_slot.activity, tau, sig),
                            curve_slot,
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

pub fn eval_render_slot_into(
    dyn_figure: &DynFigure,
    slot: &DynRender,
    tau: f64,
    sig: &SigEnv,
    out: &mut Vec<RenderData>,
) {
    match slot.repr() {
        RenderDynRepr::Polyline(projection) => {
            let hot = hot_frac(&projection.activity, tau, sig);
            let partly = tau >= projection.activity.warn && hot < 1.0;
            match sample_curve_projection(dyn_figure, tau, sig, 1.0, &projection.sample_set, &projection.u_max_sig) {
                Some(points) => out.push(RenderData::Polyline {
                    points,
                    // a filling curve's full path stays a telegraph
                    active: tau >= projection.activity.warn && !partly,
                }),
                None => out.push(RenderData::None),
            }
            if partly {
                match sample_curve_projection(dyn_figure, tau, sig, hot, &projection.sample_set, &projection.u_max_sig) {
                    Some(points) => out.push(RenderData::Polyline { points, active: true }),
                    None => out.push(RenderData::None),
                }
            }
        }
    }
}

pub fn eval_render_list_into(
    dyn_figure: &DynFigure,
    projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
    slots: &mut Vec<DynRender>,
    out: &mut Vec<RenderData>,
) {
    let state = MotionState::new();
    slots.clear();
    if materialize_render_defs_into(projector, tau, &state, sig, slots).is_err() {
        return;
    }
    for slot in slots.iter() {
        eval_render_slot_into(dyn_figure, slot, tau, sig, out);
    }
}
