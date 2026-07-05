use super::*;

impl Sim {
    pub(super) fn refresh_channels(&mut self, inputs: &Inputs) {
        let mut ch = (*self.ctx.sig.channels).clone();
        // host channels verbatim — passed by name (§3)
        for (k, v) in &inputs.vals {
            ch.insert(k.to_string(), v.clone());
        }
        // defaults for the conventional names when the host omits them
        // (once present — from host, default, or a previous tick — they stay)
        for (name, v) in [
            ("move-x", Val::Num(0.0)),
            ("move-y", Val::Num(0.0)),
            ("focus-firing", Val::Num(0.0)),
            ("bomb", Val::Num(0.0)),
            ("boss-hp", Val::Num(0.0)),
            ("player", Val::Vec2 { x: 0.0, y: -4.0 }),
            ("nearest-enemy", Val::Vec2 { x: 0.0, y: 3.0 }),
        ] {
            ch.entry(name.to_string()).or_insert(v);
        }
        // $tick: the world clock as a channel (refreshed at step start, so
        // control-layer reads see the current tick). Absolute time is what
        // lets deadline columns be plain data — (invuln …) in the stdlib is
        // a set-col of :iframe-until = $tick + window, not an engine verb.
        ch.insert("tick".into(), Val::Num(self.world.tick as f64));
        // $player-k DERIVES from piloted rig entities keyed by the :pilot
        // column's VALUE; $player aliases pilot 1 (card-integrated movement
        // overrides the host mock). Per-pilot homing targets too.
        let mut pilots: Vec<(i64, (f64, f64))> = Vec::new();
        for b in &self.world.bullets {
            if !b.alive {
                continue;
            }
            if let Some(k) = b.col_get("pilot") {
                let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
                if let Ok(p) = dyn_pose(&b.motion, tau, &b.state, &self.ctx.sig) {
                    pilots.push((k as i64, (p.x, p.y)));
                }
            }
        }
        for (k, (x, y)) in &pilots {
            ch.insert(format!("player-{}", k), Val::Vec2 { x: *x, y: *y });
            if let Some((nx, ny)) = self.nearest("enemy", (*x, *y)) {
                ch.insert(format!("nearest-enemy-{}", k), Val::Vec2 { x: nx, y: ny });
            }
            if *k == 1 {
                ch.insert("player".into(), Val::Vec2 { x: *x, y: *y });
            }
        }
        let player_pos = match ch.get("player") {
            Some(Val::Vec2 { x, y }) => (*x, *y),
            _ => (0.0, -4.0),
        };
        // $nearest-enemy relative to $player (derived when :enemy entities
        // exist; the host-provided value is the mock fallback)
        if let Some((x, y)) = self.nearest("enemy", player_pos) {
            ch.insert("nearest-enemy".into(), Val::Vec2 { x, y });
        }
        // $nearest-pilot: nearest player entity to the boss anchor (for
        // boss aim in multi-pilot cards)
        if let Some((x, y)) = pilots
            .iter()
            .map(|(_, p)| *p)
            .min_by(|a, b| {
                let da = (a.0 - self.world.boss.x).powi(2) + (a.1 - self.world.boss.y).powi(2);
                let db = (b.0 - self.world.boss.x).powi(2) + (b.1 - self.world.boss.y).powi(2);
                da.partial_cmp(&db).unwrap()
            })
        {
            ch.insert("nearest-pilot".into(), Val::Vec2 { x, y });
        }
        // gameplay counters as signals
        ch.insert("graze".into(), Val::Num(self.world.graze as f64));
        ch.insert(
            "enemies".into(),
            Val::Num(
                self.world
                    .bullets
                    .iter()
                    .filter(|b| b.alive && b.team.as_deref() == Some("enemy"))
                    .count() as f64,
            ),
        );
        // lives: per pilot ($lives-k), plus $lives from the first
        // player-body (compat with pilotless mouse rigs)
        for b in &self.world.bullets {
            if !b.alive {
                continue;
            }
            if let (Some(k), Some(l)) = (b.col_get("pilot"), b.col_get("lives")) {
                ch.insert(format!("lives-{}", k as i64), Val::Num(l));
            }
        }
        if let Some(l) = self
            .world
            .bullets
            .iter()
            .find(|b| b.alive && b.team.as_deref() == Some("player-body"))
            .and_then(|b| b.col_get("lives"))
        {
            ch.insert("lives".into(), Val::Num(l));
        }
        // boss anchor (the move-action target — engine state, not an entity)
        ch.insert("boss".into(), Val::Vec2 { x: self.world.boss.x, y: self.world.boss.y });
        // :expose rules — entity columns published as channels; a dead or
        // absent entity reads 0, so hp gates fire (cards declare these:
        // {:expose {:hp :boss-hp}})
        for (chan, id, col) in &self.world.exposes {
            let v = self
                .world
                .bullets
                .iter()
                .find(|b| b.alive && b.id == *id)
                .and_then(|b| b.col_get(col))
                .unwrap_or(0.0);
            ch.insert(chan.to_string(), Val::Num(v));
        }
        // (export cell) — pattern cells published as read-only channels
        for (name, id) in self.ctx.sig.exports.borrow().iter() {
            if let Some((_, v)) = self.ctx.sig.cells.borrow().get(id) {
                ch.insert(name.clone(), v.clone());
            }
        }
        self.ctx.sig.channels = Rc::new(ch);
    }

    /// Nearest alive entity with the given team tag, by position.
    fn nearest(&self, team: &str, to: (f64, f64)) -> Option<(f64, f64)> {
        let sig = &self.ctx.sig;
        let mut best: Option<((f64, f64), f64)> = None;
        for b in &self.world.bullets {
            if !b.alive || b.team.as_deref() != Some(team) {
                continue;
            }
            let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
            let Ok(p) = dyn_pose(&b.motion, tau, &b.state, sig) else { continue };
            let d2 = (p.x - to.0).powi(2) + (p.y - to.1).powi(2);
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some(((p.x, p.y), d2));
            }
        }
        best.map(|(p, _)| p)
    }
}
