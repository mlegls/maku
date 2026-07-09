use super::*;
use super::slots::{eval_collider_slot, first_render_projection_into, materialize_collider_defs_into};

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

fn collider_overlap(a: &ColliderData, b: &ColliderData) -> bool {
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

fn materialize_colliders_into(
    dyn_figure: &DynFigure,
    projector: &ColliderProjector,
    render_projector: &RenderProjector,
    tau: f64,
    sig: &SigEnv,
    e_view: &Val,
    ctx_view: &Val,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
    symbols: &mut SymbolTable,
    defs: &mut Vec<DynCollider>,
    render_defs: &mut Vec<DynRender>,
    out: &mut Vec<ColliderData>,
) -> Result<(), String> {
    let state = MotionState::new();
    defs.clear();
    materialize_collider_defs_into(projector, tau, &state, sig, Some(e_view), Some(ctx_view), symbols, defs)
        .map_err(|e| format!("colliders: {}", e))?;
    let curve_slot = if matches!(dyn_figure.repr(), FigureDynRepr::Curve { .. }) {
        first_render_projection_into(render_projector, tau, sig, Some(e_view), Some(ctx_view), render_defs).map(
            |projection| CapsuleChainSlot {
                sample_set: projection.sample_set,
                u_max_sig: projection.u_max_sig,
                width: projection.width,
                activity: projection.activity,
            },
        )
    } else {
        None
    };
    out.extend(
        defs.drain(..).map(|slot| {
            let slot = match &curve_slot {
                Some(curve_slot) => curve_capsule_slot(slot, curve_slot),
                None => slot,
            };
            eval_collider_slot(dyn_figure, &slot, tau, sig, scale, pose, trace, traced)
        }),
    );
    Ok(())
}

fn curve_capsule_slot(collider: DynCollider, curve_slot: &CapsuleChainSlot) -> DynCollider {
    let slot = collider.slot();
    match &slot.shape {
        ColliderSlotShape::Circle { radius } => DynCollider::collider_capsule_chain(
            slot.layer,
            radius.clone(),
            curve_slot.clone(),
        ),
        ColliderSlotShape::CapsuleChain { .. } => collider,
    }
}

impl Sim {
    /// Collision pass, detect-then-resolve over card-defined contact rules.
    /// Checks are hot, geometry-only data; contacts are rare callback code.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: materialized collider data + contact velocities
        let n = self.world.entities.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        self.collider_scratch.clear_for_entities(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                self.world.entities.set_sampled_pose(i, tick, None);
                pos.push(None);
                self.collider_scratch.push_empty();
                continue;
            }
            let tau = self.world.entities.tau(i, tick);
            let p = {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(i)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?;
                let readers = self.motion_readers(i);
                let state = MotionState::new();
                dyn_figure_pose_in(dyn_figure, tau, MotionEvalCtx::new(&state, &sig, &readers))?
            };
            self.world.entities.set_sampled_pose(i, tick, Some(p));
            pos.push(Some((p.x, p.y)));
            let scale = {
                let Some(render_projector) = self.world.entities.render_projector(i) else {
                    self.collider_scratch.push_empty();
                    continue;
                };
                self.sample_sig(&render_projector.sigs.scale, tau, 1.0)
            };
            let dyn_figure = self
                .world
                .entities
                .dyn_figure(i)
                .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?
                .clone();
            let trace = self.world.entities.trace_samples(i);
            let traced = self.world.entities.is_traced(i);
            let collider_projector = self
                .world
                .entities
                .collider_projector(i)
                .ok_or_else(|| format!("colliders: missing projector for row {i}"))?
                .clone();
            let render_projector = self
                .world
                .entities
                .render_projector(i)
                .ok_or_else(|| format!("colliders: missing render projector for row {i}"))?
                .clone();
            let start = self.collider_scratch.begin_row();
            let e_view = entity_view(i, &self.world, &sig)?;
            let ctx_view = Val::Map(std::rc::Rc::new(vec![
                (Val::Kw("age".into()), Val::Num(tau)),
                (Val::Kw("t".into()), Val::Num(tau)),
                (Val::Kw("tick".into()), Val::Num(tick as f64)),
            ]));
            materialize_colliders_into(
                &dyn_figure,
                &collider_projector,
                &render_projector,
                tau,
                &sig,
                &e_view,
                &ctx_view,
                scale,
                p,
                trace,
                traced,
                &mut self.world.symbols,
                &mut self.collider_scratch.defs,
                &mut self.collider_scratch.render_defs,
                &mut self.collider_scratch.rows,
            )?;
            self.collider_scratch.finish_row(start);
        }

        let rules = self.world.contacts.clone();
        let mut contacts: Vec<Vec<(usize, usize)>> = Vec::with_capacity(rules.len());
        for rule in &rules {
            let mut pairs = Vec::new();
            for (i, _a) in self.world.entities.iter().enumerate() {
                if !self.world.entities.is_alive(i) || pos[i].is_none() {
                    continue;
                }
                for (j, _b) in self.world.entities.iter().enumerate() {
                    if i == j || !self.world.entities.is_alive(j) {
                        continue;
                    }
                    if pos[j].is_none() {
                        continue;
                    }
                    for ac in self
                        .collider_scratch
                        .row(i)
                        .iter()
                        .filter(|c| c.layer() == Some(rule.a))
                    {
                        for bc in self
                            .collider_scratch
                            .row(j)
                            .iter()
                            .filter(|c| c.layer() == Some(rule.b))
                        {
                            if collider_overlap(ac, bc) {
                                pairs.push((i, j));
                            }
                        }
                    }
                }
            }
            contacts.push(pairs);
        }

        for (rule, pairs) in rules.iter().zip(contacts.iter()) {
            for &(i, j) in pairs {
                if !self.world.entities.is_alive(i) || !self.world.entities.is_alive(j) {
                    continue;
                }
                if let Some(col) = &rule.once {
                    if self.world.col_get_sym_at(i, *col).is_some() {
                        continue;
                    }
                }
                if let Some(skip) = &rule.skip_if {
                    let side = if skip.on_b { j } else { i };
                    let lhs = self.world.col_get_sym_at(side, skip.col).unwrap_or(0.0);
                    let rhs = match &skip.rhs {
                        SkipRhs::Tick => tick as f64,
                        SkipRhs::Num(n) => *n,
                    };
                    if (skip.gt && lhs > rhs) || (!skip.gt && lhs < rhs) {
                        continue;
                    }
                }
                let a_ref = self.world.entity_ref(i);
                let b_ref = self.world.entity_ref(j);
                apply_fn(
                    rule.callback.clone(),
                    &[Val::Handle(a_ref), Val::Handle(b_ref)],
                    &mut self.ctx,
                    &mut self.world,
                    true,
                )?;
                if let Some(col) = &rule.once {
                    // dead-inclusive: the callback may have culled A, and the
                    // latch must still stick (find() only sees live entities)
                    if self
                        .world
                        .entities
                        .generation(a_ref.row)
                        .is_some_and(|generation| generation == a_ref.generation)
                    {
                        let bi = a_ref.row;
                        self.world.col_set_sym_at(bi, *col, 1.0);
                    }
                }
            }
        }
        Ok(())
    }

    /// Standing triggers: per entity, per rule, when `col ≤ leq` first
    /// holds, fire (event + optional cull). The latch is a column, so it
    /// snapshots/scrubs; order is canonical (entity index, rule index).
    pub(super) fn fire_triggers(&mut self) {
        let tick = self.world.tick;
        for i in 0..self.world.entities.len() {
            let rules = self.world.entities.triggers(i);
            for rule in rules.iter().cloned() {
                if !self.world.entities.is_alive(i) {
                    break;
                }
                let armed = self.world.col_get_sym_at(i, rule.latch).is_none();
                let holds = self.world.col_get_sym_at(i, rule.col).map(|v| v <= rule.leq).unwrap_or(false);
                if !(armed && holds) {
                    continue;
                }
                let (latch, name, cull) = (rule.latch, rule.name, rule.cull);
                let at = self.world.entities.sampled_pos(i, tick);
                self.world.col_set_sym_at(i, latch, 1.0);
                if cull {
                    self.world.cull_at(i);
                }
                self.world.push_event(StoredEvent { tick, name, pos: at });
            }
        }
    }
}
