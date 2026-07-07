use super::*;
use super::slots::{eval_collider_slot, first_render_projection, materialize_collider_defs};

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

fn materialize_colliders(
    b: &Entity,
    tau: f64,
    sig: &SigEnv,
    scale: f64,
    symbols: &mut SymbolTable,
) -> Result<Vec<ColliderData>, String> {
    let mut defs = materialize_collider_defs(&b.colliders, tau, &b.state, sig, symbols)
        .map_err(|e| format!("colliders: {}", e))?;
    if matches!(b.dyn_figure.repr(), FigureDynRepr::Curve { .. }) {
        if let Some(projection) = first_render_projection(b, tau, sig) {
            let curve_slot = CapsuleChainSlot {
                sample_set: projection.sample_set,
                u_max_sig: projection.u_max_sig,
                width: projection.width,
                activity: projection.activity,
            };
            defs = curve_capsule_slots(defs, &curve_slot);
        }
    }
    let defs = defs
        .into_iter()
        .map(|slot| eval_collider_slot(b, &slot, tau, sig, scale))
        .collect();
    Ok(defs)
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
        let mut colliders: Vec<Vec<ColliderData>> = Vec::with_capacity(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        for b in &self.world.entities {
            if !b.alive {
                pos.push(None);
                colliders.push(Vec::new());
                continue;
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_figure_pose(&b.dyn_figure, tau, &b.state, &sig)?;
            pos.push(Some((p.x, p.y)));
            let scale = self.sample_sig(&b.sigs.scale, tau, 1.0);
            colliders.push(materialize_colliders(b, tau, &sig, scale, &mut self.world.symbols)?);
        }

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
                    if pos[j].is_none() {
                        continue;
                    }
                    for ac in colliders[i].iter().filter(|c| c.layer() == Some(rule.a)) {
                        for bc in colliders[j].iter().filter(|c| c.layer() == Some(rule.b)) {
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
                        .get(a_ref.row)
                        .is_some_and(|b| b.generation == a_ref.generation)
                    {
                        let bi = a_ref.row;
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
                let (latch, name, cull) = (rule.latch.clone(), rule.name, rule.cull);
                let at = self.world.entities[i].prev_pos;
                self.world.col_set_at(i, &latch, 1.0);
                if cull {
                    self.world.cull_at(i);
                }
                self.world.push_event(StoredEvent { tick, name, pos: at });
            }
        }
    }
}
