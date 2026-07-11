use super::*;

const CURVE_R: f64 = 0.08; // compatibility curve half-width for collision

fn sample_curve_collider_frac(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    projection: &CapsuleChainSlot,
    tick_rate: f64,
) -> Option<Vec<(f64, f64)>> {
    sample_curve_projection(dyn_figure, tau, sig, 1.0, &projection.sample_set, projection.u_max, tick_rate)
}

fn sample_curve_projection(
    dyn_figure: &DynFigure,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
    sample_set: &SampleSet,
    u_max: f64,
    tick_rate: f64,
) -> Option<Vec<(f64, f64)>> {
    let state = MotionState::default();
    let Figure::Curve(curve) = eval_dyn_with_tick_rate(dyn_figure, tau, &state, sig, tick_rate).ok()? else {
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
            let max = if u_max.is_finite() { u_max } else { *max };
            let end = min + (max - min) * frac.min(1.0);
            let span = (end - min).abs().max(0.01);
            let steps = ((span / resolution).ceil() as usize).clamp(2, 400);
            (0..=steps).map(|k| min + (end - min) * k as f64 / steps as f64).collect()
        }
    };
    let mut pts = Vec::with_capacity(us.len());
    for u in us {
        let local = eval_curve_pose_with_tick_rate(&curve.spec.eval, tau, u, &state, sig, tick_rate).ok()?;
        let w = curve.frame.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

fn bind_projector_scope(
    env: &Env,
    scope: Option<&ProjectorScope>,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
) -> Env {
    let Some(scope) = scope else {
        return env.clone();
    };
    let e_bound = e_view
        .cloned()
        .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
    let ctx_bound = ctx_view
        .cloned()
        .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
    env.clone()
        .bind(scope.entity.clone(), e_bound)
        .bind(scope.context.clone(), ctx_bound)
}

pub fn materialize_collider_defs_into(
    projector: &ColliderProjector,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
    world: &mut World,
    row: Option<usize>,
    out: &mut Vec<DynCollider>,
    tick_rate: f64,
) -> Result<(), String> {
    for list in projector.projectors.iter() {
        match &list.expr {
            ColliderProjectorExpr::Stable(slots) => {
                out.extend(slots.iter().cloned());
            }
            ColliderProjectorExpr::Circle(spec) => {
                let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                out.push(materialize_circle_projector(spec, &env, sig, world, row)?);
            }
            ColliderProjectorExpr::CapsuleChain(spec) => {
                let env = bind_projector_scope(&spec.env, spec.scope.as_ref(), e_view, ctx_view);
                out.push(materialize_capsule_chain_projector(spec, &env, sig, world, row)?);
            }
            ColliderProjectorExpr::Callable { params, body, env } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let mut env = env.clone();
                if let Some(param) = params.first() {
                    env = env.bind(param.clone(), e_bound.clone());
                }
                if let Some(param) = params.get(1) {
                    env = env.bind(param.clone(), ctx_bound.clone());
                }
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                run_ctx.projector_scope = match (params.first(), params.get(1)) {
                    (Some(entity), Some(context)) => Some(ProjectorScope {
                        entity: entity.clone(),
                        context: context.clone(),
                        figure: list.figure,
                    }),
                    _ => None,
                };
                let mut run_world = World::for_eval(tick_rate);
                run_world.symbols = world.symbols.clone();
                let mut last = Val::Nothing;
                for form in body.iter() {
                    last = evaluate(form, &env, &mut run_ctx, &mut run_world)?;
                }
                world.symbols = run_world.symbols;
                let specs = flatten_collider_projectors("collider", last, Some(list.figure))?;
                materialize_collider_defs_into(
                    &ColliderProjector { projectors: specs },
                    tau,
                    state,
                    sig,
                    Some(&e_bound),
                    Some(&ctx_bound),
                    world,
                    row,
                    out,
                    tick_rate,
                )?;
            }
            ColliderProjectorExpr::Cond { clauses, env, scope } => {
                let e_bound = e_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let ctx_bound = ctx_view
                    .cloned()
                    .unwrap_or_else(|| Val::Map(std::rc::Rc::new(Vec::new())));
                let env = bind_projector_scope(env, scope.as_ref(), Some(&e_bound), Some(&ctx_bound));
                let mut run_ctx = Ctx::default();
                run_ctx.sig = sig.clone();
                run_ctx.projector_scope = scope.clone();
                let mut run_world = World::for_eval(tick_rate);
                run_world.symbols = world.symbols.clone();
                for (pred, child) in clauses.iter() {
                    let enabled = match pred {
                        Some(pred) => truthy_pub(&evaluate(pred, &env, &mut run_ctx, &mut run_world)?),
                        None => true,
                    };
                    if enabled {
                        world.symbols = run_world.symbols;
                        materialize_collider_defs_into(
                            &ColliderProjector { projectors: child.clone() },
                            tau,
                            state,
                            sig,
                            Some(&e_bound),
                            Some(&ctx_bound),
                            world,
                            row,
                            out,
                            tick_rate,
                        )?;
                        return Ok(());
                    }
                }
                world.symbols = run_world.symbols;
            }
        }
    }
    Ok(())
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
    tick_rate: f64,
) -> ColliderData {
    match slot.repr() {
        ColliderDynRepr::Slot(projection) => match &projection.shape {
            ColliderSlotShape::Circle { radius } => {
                let state = MotionState::default();
                let radius = eval_dyn_with_tick_rate(radius, tau, &state, sig, tick_rate).unwrap_or(0.0);
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
                let state = MotionState::default();
                let radius = eval_dyn_with_tick_rate(radius, tau, &state, sig, tick_rate).unwrap_or(0.0);
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
                            curve_slot,
                            tick_rate,
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
