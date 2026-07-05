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

/// Entity view handed to contact-time pure functions (:damage fns).
fn contact_map(b: &Bullet, pos: Option<(f64, f64)>, vel: (f64, f64)) -> Val {
    let mut kvs = vec![
        (Val::Kw("vel".into()), Val::Vec2 { x: vel.0, y: vel.1 }),
        (Val::Kw("family".into()), Val::Kw(b.style.family.as_str().into())),
    ];
    if let Some((x, y)) = pos {
        kvs.push((Val::Kw("pos".into()), Val::Vec2 { x, y }));
    }
    for (k, v) in &b.cols {
        kvs.push((Val::Kw(k.as_ref().into()), Val::Num(*v)));
    }
    if let Some(t) = &b.team {
        kvs.push((Val::Kw("team".into()), Val::Kw(t.as_ref().into())));
    }
    Val::Map(Rc::new(kvs))
}

impl Sim {
    /// Collision pass, detect-then-resolve over the layer matrix:
    ///   damage × player-hurtbox → hit;  graze × player-hurtbox → graze;
    ///   shot × hurt → damage resolution.
    /// CHECKS are per-pair-per-tick and shape-only (hot). CONTACTS are rare,
    /// so effect parameters may be card-defined pure functions — :damage as
    /// (fn [self other] n) is evaluated at contact with both entities (pos,
    /// contact velocity via finite difference, hp, team, family) in scope.
    /// Everything writes World, so the gameplay layer scrubs with the
    /// timeline; resolve order is canonical (bullet index) for determinism.
    /// Lasers derive capsule chains from their sampled curve while active.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        const LASER_R: f64 = 0.08; // beam half-width for collision
        const IFRAMES: u64 = 60;

        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: world-space collider anchors + contact velocities
        let n = self.world.bullets.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        let mut vel: Vec<(f64, f64)> = Vec::with_capacity(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        let mut scl: Vec<f64> = Vec::with_capacity(n);
        for b in &self.world.bullets {
            if !b.alive {
                pos.push(None);
                vel.push((0.0, 0.0));
                scl.push(1.0);
                continue;
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_pose(&b.motion, tau, &b.state, &sig)?;
            pos.push(Some((p.x, p.y)));
            vel.push(match b.prev_pos {
                Some((ox, oy)) => ((p.x - ox) * TICK_RATE, (p.y - oy) * TICK_RATE),
                None => (0.0, 0.0),
            });
            scl.push(self.sample_sig(&b.sigs.scale, tau, 1.0));
        }
        for (b, p) in self.world.bullets.iter_mut().zip(pos.iter()) {
            b.prev_pos = *p;
        }

        // player hurtboxes: PlayerHurt colliders on host-mounted entities
        let hurts: Vec<(usize, f64, (f64, f64))> = self
            .world
            .bullets
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.team.as_deref() == Some("player-body") && pos[*i].is_some()
            })
            .flat_map(|(i, b)| {
                let at = pos[i].unwrap();
                b.colliders
                    .iter()
                    .filter(|c| c.layer == Layer::PlayerHurt)
                    .map(move |c| (i, c.r, at))
                    .collect::<Vec<_>>()
            })
            .collect();

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

        // phase 1: detect — the big set (hostile colliders) tests only
        // against the player's few hurtboxes: O(bullets × few)
        let mut hit_contacts: Vec<(usize, usize)> = Vec::new(); // (bullet, player)
        let mut graze_contacts: Vec<usize> = Vec::new();
        for (i, b) in self.world.bullets.iter().enumerate() {
            if b.team.is_some() || pos[i].is_none() {
                continue;
            }
            for &(pj, pr, at) in &hurts {
                let Some(d2) = target_d2(b, i, at) else { continue };
                for c in b.colliders.iter() {
                    match c.layer {
                        Layer::Damage => {
                            let r = c.r * scl[i] + pr;
                            if d2 < r * r {
                                hit_contacts.push((i, pj));
                            }
                        }
                        Layer::Graze if !b.grazed => {
                            let r = c.r * scl[i] + pr;
                            if d2 < r * r {
                                graze_contacts.push(i);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        // shot × hurt (both sets are small)
        let mut shot_contacts: Vec<(usize, usize)> = Vec::new();
        for (i, shot) in self.world.bullets.iter().enumerate() {
            if pos[i].is_none() || shot.team.as_deref() != Some("player") {
                continue;
            }
            let Some(sc) = shot.colliders.iter().find(|c| c.layer == Layer::Shot) else {
                continue;
            };
            let (sx, sy) = pos[i].unwrap();
            'enemies: for (j, enemy) in self.world.bullets.iter().enumerate() {
                if pos[j].is_none() || enemy.team.as_deref() != Some("enemy") {
                    continue;
                }
                let (ex, ey) = pos[j].unwrap();
                for ec in enemy.colliders.iter() {
                    if ec.layer != Layer::Hurt {
                        continue;
                    }
                    let r = sc.r * scl[i] + ec.r * scl[j];
                    if (sx - ex).powi(2) + (sy - ey).powi(2) < r * r {
                        shot_contacts.push((i, j));
                        break 'enemies; // one shot, one enemy
                    }
                }
            }
        }

        // phase 2: resolve, canonical order. iframes are a PER-ENTITY
        // column (two pilots dodge independently), not a world global.
        for (i, pj) in hit_contacts {
            let until = self.world.bullets[pj].col_get("iframe-until").unwrap_or(0.0);
            if (tick as f64) < until {
                continue;
            }
            let b = &mut self.world.bullets[i];
            if !b.alive {
                continue;
            }
            if matches!(b.kind, Kind::Point) {
                b.alive = false; // beams persist through a hit
            }
            self.world.player_hits += 1;
            // the hit effect is column writes; game-over is the player
            // entity's trigger, not the contact's business
            let player = &mut self.world.bullets[pj];
            // the mercy window is per-entity DATA: an :iframes column
            // (seconds) overrides the engine default
            let window = player
                .col_get("iframes")
                .map(|s| (s * TICK_RATE) as u64)
                .unwrap_or(IFRAMES);
            player.col_set(&"iframe-until".into(), (tick + window) as f64);
            let lives = player.col_get("lives").unwrap_or(0.0);
            player.col_set(&"lives".into(), lives - 1.0);
            self.world.push_event(Event { tick, name: "player-hit".into(), pos: pos[i] });
        }
        for i in graze_contacts {
            let b = &mut self.world.bullets[i];
            if !b.alive || b.grazed {
                continue;
            }
            b.grazed = true;
            self.world.graze += 1;
            self.world.push_event(Event { tick, name: "graze".into(), pos: pos[i] });
        }
        for (i, j) in shot_contacts {
            if !self.world.bullets[i].alive || !self.world.bullets[j].alive {
                continue;
            }
            // resolve damage at contact: numbers pass through; a pure fn
            // gets (self other) contact maps
            let dmg_val = self.world.bullets[i].damage.clone();
            let dmg = match dmg_val {
                Val::Num(n) => n,
                f => {
                    let self_map = contact_map(&self.world.bullets[i], pos[i], vel[i]);
                    let other_map = contact_map(&self.world.bullets[j], pos[j], vel[j]);
                    apply_fn(f, &[self_map, other_map], &mut self.ctx, &mut self.world, false)?
                        .num()
                        .map_err(|e| format!("damage fn: {}", e))?
                }
            };
            self.world.bullets[i].alive = false;
            // invulnerability window: the shot still dies (absorbed), the
            // column write is skipped — same iframe-until both sides honor
            let until = self.world.bullets[j].col_get("iframe-until").unwrap_or(0.0);
            if (tick as f64) < until {
                self.world.push_event(Event { tick, name: "absorbed".into(), pos: pos[j] });
                continue;
            }
            // the effect is a COLUMN WRITE, nothing more — what zero hp
            // means is the enemy's trigger's business, not the contact's
            let enemy = &mut self.world.bullets[j];
            let hp = enemy.col_get("hp").unwrap_or(1.0);
            enemy.col_set(&"hp".into(), hp - dmg);
            self.world.push_event(Event { tick, name: "enemy-hit".into(), pos: pos[j] });
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
