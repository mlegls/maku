use super::*;
use super::slots::{eval_collider_slot, materialize_collider_defs_into};

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
    tau: f64,
    sig: &SigEnv,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
    scale: f64,
    pose: Pose,
    trace: &[Pose],
    traced: bool,
    symbols: &mut SymbolTable,
    defs: &mut Vec<DynCollider>,
    out: &mut Vec<ColliderData>,
    tick_rate: f64,
) -> Result<(), String> {
    let state = MotionState::new();
    defs.clear();
    materialize_collider_defs_into(projector, tau, &state, sig, e_view, ctx_view, symbols, defs, tick_rate)
        .map_err(|e| format!("colliders: {}", e))?;
    out.extend(
        defs.drain(..)
            .map(|slot| eval_collider_slot(dyn_figure, &slot, tau, sig, scale, pose, trace, traced, tick_rate)),
    );
    Ok(())
}

impl Sim {
    /// Collision pass: materialize collider rows and record current-tick
    /// collision facts for `(collisions :a :b)` domain queries.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: materialized collider data + contact velocities
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
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
            let tau = self.world.entity_motion_tau(i, tick);
            let p = {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(i)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?;
                let readers = self.motion_readers(i);
                let state = MotionState::new();
                // pos_only: the sampled-pose cache and colliders read x/y
                // (velocity-from-samples, exit snapshots re-derive heading)
                dyn_figure_pose_in(
                    dyn_figure,
                    tau,
                    MotionEvalCtx::with_tick_rate(&state, &sig, &readers, self.world.tick_rate())
                        .pos_only(),
                )?
            };
            self.world.entities.set_sampled_pose(i, tick, Some(p));
            pos.push(Some((p.x, p.y)));
            let scale = self.world.col_get_at(i, "scale").unwrap_or(1.0);
            let dyn_figure = self
                .world
                .entities
                .dyn_figure(i)
                .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?
                .clone();
            let trace = self.world.entities.trace_samples(i);
            let traced = self.world.entities.is_traced(i);
            let tick_rate = self.world.tick_rate();
            let collider_projector = self
                .world
                .entities
                .collider_projector(i)
                .ok_or_else(|| format!("colliders: missing projector for row {i}"))?
                .clone();
            let start = self.collider_scratch.begin_row();
            let (e_view, ctx_view) = if collider_projector.needs_views() {
                (Some(entity_view(i, &self.world, &sig)?), Some(Val::Map(std::rc::Rc::new(vec![
                    (Val::Kw("age".into()), Val::Num(tau)),
                    (Val::Kw("t".into()), Val::Num(tau)),
                    (Val::Kw("tick".into()), Val::Num(tick as f64)),
                ]))))
            } else {
                (None, None)
            };
            materialize_colliders_into(
                &dyn_figure,
                &collider_projector,
                tau,
                &sig,
                e_view.as_ref(),
                ctx_view.as_ref(),
                scale,
                p,
                trace,
                traced,
                &mut self.world.symbols,
                &mut self.collider_scratch.defs,
                &mut self.collider_scratch.rows,
                tick_rate,
            )?;
            self.collider_scratch.finish_row(start);
        }

        if let Some(f) = probe {
            crate::interp::profile::close("phase:collide-mat", f);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        self.collider_scratch.broadphase.clear();
        for i in 0..n {
            if !self.world.entities.is_alive(i) || pos[i].is_none() {
                continue;
            }
            let mut bounds = (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
            for collider in self.collider_scratch.row(i) {
                if collider.layer().is_none() {
                    continue;
                }
                match collider {
                    ColliderData::Circle { center, radius, .. } => {
                        bounds.0 = bounds.0.min(center.0 - radius);
                        bounds.1 = bounds.1.max(center.0 + radius);
                        bounds.2 = bounds.2.min(center.1 - radius);
                        bounds.3 = bounds.3.max(center.1 + radius);
                    }
                    ColliderData::CapsuleChain { points, radius, .. } => {
                        for point in points {
                            bounds.0 = bounds.0.min(point.0 - radius);
                            bounds.1 = bounds.1.max(point.0 + radius);
                            bounds.2 = bounds.2.min(point.1 - radius);
                            bounds.3 = bounds.3.max(point.1 + radius);
                        }
                    }
                    ColliderData::None => {}
                }
            }
            if !bounds.0.is_nan() && !bounds.1.is_nan() && !bounds.2.is_nan() && !bounds.3.is_nan() {
                self.collider_scratch.broadphase.push((bounds.0, bounds.1, bounds.2, bounds.3, i));
            }
        }
        self.collider_scratch.broadphase.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.4.cmp(&b.4)));
        let mut facts = Vec::new();
        for k in 0..self.collider_scratch.broadphase.len() {
            let a = self.collider_scratch.broadphase[k];
            for m in k + 1..self.collider_scratch.broadphase.len() {
                let b = self.collider_scratch.broadphase[m];
                // Conservative AABBs may only prune strictly disjoint projections.
                if b.0 > a.1 {
                    break;
                }
                if a.3 < b.2 || b.3 < a.2 {
                    continue;
                }
                let (i, j) = if a.4 < b.4 { (a.4, b.4) } else { (b.4, a.4) };
                let mut mirrored = Vec::new();
                // Overlap is symmetric, so each unordered collider pair is evaluated once.
                for (ra, ac) in self.collider_scratch.row(i).iter().enumerate() {
                    let Some(layer_a) = ac.layer() else { continue };
                    for (rb, bc) in self.collider_scratch.row(j).iter().enumerate() {
                        let Some(layer_b) = bc.layer() else { continue };
                        if collider_overlap(ac, bc) {
                            facts.push(CollisionFact { a: layer_a, b: layer_b, i, j });
                            mirrored.push((rb, ra, CollisionFact { a: layer_b, b: layer_a, i: j, j: i }));
                        }
                    }
                }
                mirrored.sort_by_key(|(rb, ra, _)| (*rb, *ra));
                facts.extend(mirrored.into_iter().map(|(_, _, fact)| fact));
            }
        }
        facts.sort_by_key(|fact| (fact.i, fact.j));
        self.world.collision_facts = facts;
        if oracle_enabled() {
            let expected = brute_force_collision_facts(&self.world, &pos, &self.collider_scratch);
            assert_eq!(self.world.collision_facts, expected);
        }
        if let Some(f) = probe {
            crate::interp::profile::close("phase:collide-pairs", f);
        }
        Ok(())
    }
}

pub(super) fn brute_force_collision_facts(
    world: &World,
    pos: &[Option<(f64, f64)>],
    scratch: &ColliderScratch,
) -> Vec<CollisionFact> {
    let mut facts = Vec::new();
    for (i, _) in world.entities.iter().enumerate() {
        if !world.entities.is_alive(i) || pos[i].is_none() { continue; }
        for (j, _) in world.entities.iter().enumerate() {
            if i == j || !world.entities.is_alive(j) || pos[j].is_none() { continue; }
            for ac in scratch.row(i) {
                let Some(a) = ac.layer() else { continue };
                for bc in scratch.row(j) {
                    let Some(b) = bc.layer() else { continue };
                    if collider_overlap(ac, bc) {
                        facts.push(CollisionFact { a, b, i, j });
                    }
                }
            }
        }
    }
    facts
}
