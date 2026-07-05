use super::*;

/// Sample a laser's world-space curve at `tau` (shared by render and the
/// collision pass — the beam you see is the beam that hits).
pub fn sample_laser(b: &Bullet, tau: f64, sig: &SigEnv) -> Option<Vec<(f64, f64)>> {
    sample_laser_frac(b, tau, sig, 1.0)
}

/// Sample the beam up to `frac` of its length (slow lasers: the hot
/// front's reach). frac 1.0 = the whole path.
pub fn sample_laser_frac(
    b: &Bullet,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let Kind::Laser { shape, u_max, u_max_sig, resolution, .. } = &b.kind else {
        return None;
    };
    if frac <= 0.0 {
        return None;
    }
    let anchor = dyn_pose(&b.motion, tau, &b.state, sig).ok()?;
    let u_max = match u_max_sig {
        Some((f, e)) => eval_sig(f, e, sig, tau, 0.0, None, None)
            .and_then(|v| v.num())
            .unwrap_or(*u_max)
            .max(0.01),
        None => *u_max,
    } * frac.min(1.0);
    let steps = ((u_max / resolution).ceil() as usize).clamp(2, 400);
    let mut pts = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let u = u_max * k as f64 / steps as f64;
        let local = match shape {
            Some(sh) => dyn_pose_u(sh, tau, u, &b.state, sig).ok()?,
            None => Pose { x: u, y: 0.0, th: 0.0 }, // straight along +x
        };
        let w = anchor.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

/// A slow laser's hot fraction at age tau: 0 before the warn ends, then
/// sweeping to 1 over the :fill window. Lasers without :fill are hot in
/// full the moment the warn ends.
pub(super) fn hot_frac(kind: &Kind, tau: f64, sig: &SigEnv) -> f64 {
    let Kind::Laser { warn, fill, fill_sig, .. } = kind else { return 1.0 };
    if let Some((f, e)) = fill_sig {
        // signal :fill = swept fraction as a function of laser age
        return eval_sig(f, e, sig, tau, 0.0, None, None)
            .and_then(|v| v.num())
            .map(|x| x.clamp(0.0, 1.0))
            .unwrap_or(1.0);
    }
    match fill {
        Some(d) if *d > 0.0 => ((tau - warn) / d).clamp(0.0, 1.0),
        _ => 1.0,
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

impl Sim {
    /// Collision pass, detect-then-resolve over card-defined contact rules.
    /// Checks are hot, shape-only data; contacts are rare callback code.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        const LASER_R: f64 = 0.08; // beam half-width for collision

        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: world-space collider anchors + contact velocities
        let n = self.world.bullets.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        let mut scl: Vec<f64> = Vec::with_capacity(n);
        for b in &self.world.bullets {
            if !b.alive {
                pos.push(None);
                scl.push(1.0);
                continue;
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_pose(&b.motion, tau, &b.state, &sig)?;
            pos.push(Some((p.x, p.y)));
            scl.push(self.sample_sig(&b.sigs.scale, tau, 1.0));
        }

        // squared distance from a target point to bullet i's collision
        // anchor: points measure center distance; active lasers measure
        // distance to the sampled beam (capsule chain)
        let target_d2 = |b: &Bullet, i: usize, to: (f64, f64)| -> Option<f64> {
            let (bx, by) = pos[i]?;
            match &b.kind {
                Kind::Point => Some((bx - to.0).powi(2) + (by - to.1).powi(2)),
                Kind::Laser { warn, width, .. } => {
                    let tau = (tick - b.birth) as f64 / TICK_RATE;
                    if tau < *warn {
                        return None; // warn phase: no hitbox yet
                    }
                    // slow lasers: only the swept-out prefix is hot
                    let pts = sample_laser_frac(b, tau, &sig, hot_frac(&b.kind, tau, &sig))?;
                    let d = dist_to_chain(to, &pts)?;
                    Some((d - LASER_R * width).max(0.0).powi(2))
                }
                // the trail IS the hitbox: a capsule chain over the
                // recorded window
                Kind::Pather { .. } => {
                    let d = dist_to_chain(to, &b.trail)?;
                    Some((d - LASER_R).max(0.0).powi(2))
                }
            }
        };

        let rules = self.world.contacts.clone();
        let mut contacts: Vec<Vec<(usize, usize)>> = Vec::with_capacity(rules.len());
        for rule in &rules {
            let mut pairs = Vec::new();
            for (i, a) in self.world.bullets.iter().enumerate() {
                if !a.alive || pos[i].is_none() {
                    continue;
                }
                for (j, b) in self.world.bullets.iter().enumerate() {
                    if i == j || !b.alive {
                        continue;
                    }
                    let Some(at) = pos[j] else { continue };
                    let Some(d2) = target_d2(a, i, at) else { continue };
                    for ac in a.colliders.iter().filter(|c| c.layer.as_ref() == rule.a.as_ref()) {
                        for bc in b.colliders.iter().filter(|c| c.layer.as_ref() == rule.b.as_ref()) {
                            let r = ac.r * scl[i] + bc.r * scl[j];
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
                if !self.world.bullets[i].alive || !self.world.bullets[j].alive {
                    continue;
                }
                if let Some(col) = &rule.once {
                    if self.world.bullets[i].col_get(col).is_some() {
                        continue;
                    }
                }
                if let Some(skip) = &rule.skip_if {
                    let side = if skip.on_b { j } else { i };
                    let lhs = self.world.bullets[side].col_get(&skip.col).unwrap_or(0.0);
                    let rhs = match &skip.rhs {
                        SkipRhs::Tick => tick as f64,
                        SkipRhs::Num(n) => *n,
                    };
                    if (skip.gt && lhs > rhs) || (!skip.gt && lhs < rhs) {
                        continue;
                    }
                }
                let a_id = self.world.bullets[i].id;
                let b_id = self.world.bullets[j].id;
                apply_fn(
                    rule.callback.clone(),
                    &[Val::Handle(a_id), Val::Handle(b_id)],
                    &mut self.ctx,
                    &mut self.world,
                    true,
                )?;
                if let Some(col) = &rule.once {
                    // dead-inclusive: the callback may have culled A, and the
                    // latch must still stick (find() only sees live bullets)
                    if let Some(b) = self.world.bullets.iter_mut().find(|b| b.id == a_id) {
                        b.col_set(col, 1.0);
                    }
                }
            }
        }
        // Contact callbacks read :vel through bullet views, which finite-
        // difference against prev_pos. Updating prev_pos before resolution
        // would zero every contact velocity.
        for (b, p) in self.world.bullets.iter_mut().zip(pos.iter()) {
            b.prev_pos = *p;
        }
        Ok(())
    }

    /// Standing triggers: per entity, per rule, when `col ≤ leq` first
    /// holds, fire (event + optional cull). The latch is a column, so it
    /// snapshots/scrubs; order is canonical (entity index, rule index).
    pub(super) fn fire_triggers(&mut self) {
        let tick = self.world.tick;
        for i in 0..self.world.bullets.len() {
            let n_rules = self.world.bullets[i].triggers.len();
            for r in 0..n_rules {
                let b = &self.world.bullets[i];
                if !b.alive {
                    break;
                }
                let rule = &b.triggers[r];
                let armed = b.col_get(&rule.latch).is_none();
                let holds = b.col_get(&rule.col).map(|v| v <= rule.leq).unwrap_or(false);
                if !(armed && holds) {
                    continue;
                }
                let (latch, name, cull) = (rule.latch.clone(), rule.name.clone(), rule.cull);
                let name: Rc<str> = name;
                let at = self.world.bullets[i].prev_pos;
                let b = &mut self.world.bullets[i];
                b.col_set(&latch, 1.0);
                if cull {
                    b.alive = false;
                }
                self.world.push_event(Event { tick, name, pos: at });
            }
        }
    }
}
