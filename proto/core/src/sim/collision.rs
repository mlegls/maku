use super::*;

/// Sample a world-space curve at `tau` (shared by render and the collision
/// pass — the curve you see is the curve that hits).
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
    let projection = b.renderers.iter().find_map(DynRender::curve_compat)?;
    sample_curve_projection(b, tau, sig, frac, &projection.sample_set, &projection.u_max_sig)
}

pub(super) fn sample_curve_collider_frac(
    b: &Entity,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let projection = b.colliders.iter().find_map(DynCollider::curve_compat)?;
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
pub(super) fn hot_frac(activity: &CurveSlotActivityCompat, tau: f64, sig: &SigEnv) -> f64 {
    if let Some(d) = &activity.hot_frac_sig {
        return eval_dyn(d, tau, &MotionState::new(), sig)
            .map(|x| x.clamp(0.0, 1.0))
            .unwrap_or(1.0);
    }
    1.0
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

impl Sim {
    /// Collision pass, detect-then-resolve over card-defined contact rules.
    /// Checks are hot, geometry-only data; contacts are rare callback code.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        const CURVE_R: f64 = 0.08; // curve half-width for collision

        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: world-space collider anchors + contact velocities
        let n = self.world.entities.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        let mut scl: Vec<f64> = Vec::with_capacity(n);
        for b in &self.world.entities {
            if !b.alive {
                pos.push(None);
                scl.push(1.0);
                continue;
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_figure_pose(&b.dyn_figure, tau, &b.state, &sig)?;
            pos.push(Some((p.x, p.y)));
            scl.push(self.sample_sig(&b.sigs.scale, tau, 1.0));
        }

        // squared distance from a target point to entity i's collision
        // anchor: points measure center distance; active curves measure
        // distance to the sampled polyline (capsule chain)
        let target_d2 = |b: &Entity, i: usize, to: (f64, f64)| -> Option<f64> {
            let (bx, by) = pos[i]?;
            match b.dyn_figure.repr() {
                DynRepr::Pose(_) if b.cache_policy.trace.is_some() => {
                    let pts: Vec<(f64, f64)> = b.trail.iter().map(|p| (p.x, p.y)).collect();
                    let d = dist_to_chain(to, &pts)?;
                    Some((d - CURVE_R).max(0.0).powi(2))
                }
                DynRepr::Pose(_) => Some((bx - to.0).powi(2) + (by - to.1).powi(2)),
                DynRepr::FigureCurve { .. } => {
                    let Some(projection) = b.colliders.iter().find_map(DynCollider::curve_compat) else { return None };
                    let tau = (tick - b.birth) as f64 / TICK_RATE;
                    if tau < projection.activity.warn {
                        return None; // warn phase: no hitbox yet
                    }
                    // filled curves: only the swept-out prefix is hot
                    let pts = sample_curve_collider_frac(
                        b,
                        tau,
                        &sig,
                        hot_frac(&projection.activity, tau, &sig),
                    )?;
                    let d = dist_to_chain(to, &pts)?;
                    Some((d - CURVE_R * projection.width).max(0.0).powi(2))
                }
                _ => unreachable!("internal type error: expected Dyn<Figure>"),
            }
        };

        let rules = self.world.contacts.clone();
        let mut contacts: Vec<Vec<(usize, usize)>> = Vec::with_capacity(rules.len());
        for rule in &rules {
            let mut pairs = Vec::new();
            for (i, a) in self.world.entities.iter().enumerate() {
                if !a.alive || pos[i].is_none() {
                    continue;
                }
                for (j, b) in self.world.entities.iter().enumerate() {
                    if i == j || !b.alive {
                        continue;
                    }
                    let Some(at) = pos[j] else { continue };
                    let Some(d2) = target_d2(a, i, at) else { continue };
                    for ac in a.colliders.iter().filter(|c| c.layer() == Some(rule.a.as_ref())) {
                        let Some(ar) = ac.circle_radius() else { continue };
                        for bc in b.colliders.iter().filter(|c| c.layer() == Some(rule.b.as_ref())) {
                            let Some(br) = bc.circle_radius() else { continue };
                            let r = ar * scl[i] + br * scl[j];
                            if d2 < r * r {
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
                if !self.world.entities[i].alive || !self.world.entities[j].alive {
                    continue;
                }
                if let Some(col) = &rule.once {
                    if self.world.col_get_at(i, col).is_some() {
                        continue;
                    }
                }
                if let Some(skip) = &rule.skip_if {
                    let side = if skip.on_b { j } else { i };
                    let lhs = self.world.col_get_at(side, &skip.col).unwrap_or(0.0);
                    let rhs = match &skip.rhs {
                        SkipRhs::Tick => tick as f64,
                        SkipRhs::Num(n) => *n,
                    };
                    if (skip.gt && lhs > rhs) || (!skip.gt && lhs < rhs) {
                        continue;
                    }
                }
                let a_id = self.world.entities[i].id;
                let b_id = self.world.entities[j].id;
                apply_fn(
                    rule.callback.clone(),
                    &[Val::Handle(a_id), Val::Handle(b_id)],
                    &mut self.ctx,
                    &mut self.world,
                    true,
                )?;
                if let Some(col) = &rule.once {
                    // dead-inclusive: the callback may have culled A, and the
                    // latch must still stick (find() only sees live entities)
                    if let Some(bi) = self.world.entities.iter().position(|b| b.id == a_id) {
                        self.world.col_set_at(bi, col, 1.0);
                    }
                }
            }
        }
        // Contact callbacks read :vel through entity views, which finite-
        // difference against prev_pos. Updating prev_pos before resolution
        // would zero every contact velocity.
        for (b, p) in self.world.entities.iter_mut().zip(pos.iter()) {
            b.prev_pos = *p;
        }
        Ok(())
    }

    /// Standing triggers: per entity, per rule, when `col ≤ leq` first
    /// holds, fire (event + optional cull). The latch is a column, so it
    /// snapshots/scrubs; order is canonical (entity index, rule index).
    pub(super) fn fire_triggers(&mut self) {
        let tick = self.world.tick;
        for i in 0..self.world.entities.len() {
            let n_rules = self.world.entities[i].triggers.len();
            for r in 0..n_rules {
                if !self.world.entities[i].alive {
                    break;
                }
                let rule = self.world.entities[i].triggers[r].clone();
                let armed = self.world.col_get_at(i, &rule.latch).is_none();
                let holds = self.world.col_get_at(i, &rule.col).map(|v| v <= rule.leq).unwrap_or(false);
                if !(armed && holds) {
                    continue;
                }
                let (latch, name, cull) = (rule.latch.clone(), rule.name.clone(), rule.cull);
                let name: Rc<str> = name;
                let at = self.world.entities[i].prev_pos;
                self.world.col_set_at(i, &latch, 1.0);
                if cull {
                    self.world.entities[i].alive = false;
                }
                self.world.push_event(Event { tick, name, pos: at });
            }
        }
    }
}
