use super::*;
use super::collision::hot_frac;

pub enum RenderItem {
    Dot { x: f64, y: f64, th: f64, style: Style, hue: f64, scale: f64, alpha: f64 },
    Polyline { pts: Vec<(f64, f64)>, style: Style, active: bool, hue: f64, alpha: f64 },
}

impl Sim {
    /// Sample one render-signal tag at bullet-local t (default when absent).
    pub(super) fn sample_sig(&self, s: &Option<MetaSig>, tau: f64, default: f64) -> f64 {
        let Some(h) = s else { return default };
        let env = h.env.bind("t".into(), Val::Num(tau));
        let mut ctx = Ctx {
            sig: self.ctx.sig.clone(),
            ambient: Pose::IDENTITY,
            scan: None,
            patterns: self.ctx.patterns.clone(),
            macros: self.ctx.macros.clone(),
            deferred: Vec::new(),
        };
        let mut w = World::default();
        match evaluate(&h.form, &env, &mut ctx, &mut w) {
            Ok(Val::Num(x)) => x,
            Ok(Val::Arr(items)) if !items.is_empty() => {
                items[h.idx % items.len()].num().unwrap_or(default)
            }
            _ => default,
        }
    }

    fn sample_hue(&self, b: &Bullet, tau: f64) -> f64 {
        self.sample_sig(&b.sigs.hue, tau, 0.0)
    }

    pub fn render(&self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for b in &self.world.bullets {
            if !b.alive || b.team.as_deref() == Some("player-body") {
                continue; // the host draws its own player marker
            }
            let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
            match &b.kind {
                Kind::Point => {
                    if let Ok(p) = dyn_pose(&b.motion, tau, &b.state, sig) {
                        out.push(RenderItem::Dot {
                            x: p.x,
                            y: p.y,
                            // :facing overrides the motion direction
                            th: self.sample_sig(&b.sigs.facing, tau, p.th),
                            style: b.style.clone(),
                            hue: self.sample_hue(b, tau),
                            scale: self.sample_sig(&b.sigs.scale, tau, 1.0),
                            alpha: self.sample_sig(&b.sigs.opacity, tau, 1.0),
                        });
                    }
                }
                Kind::Pather { .. } => {
                    if b.trail.len() >= 2 {
                        out.push(RenderItem::Polyline {
                            pts: b.trail.clone(),
                            style: b.style.clone(),
                            active: true,
                            hue: self.sample_hue(b, tau),
                            alpha: self.sample_sig(&b.sigs.opacity, tau, 1.0),
                        });
                    }
                }
                Kind::Laser { warn, .. } => {
                    let hot = hot_frac(&b.kind, tau, sig);
                    let partly = tau >= *warn && hot < 1.0;
                    let alpha = self.sample_sig(&b.sigs.opacity, tau, 1.0);
                    if let Some(pts) = sample_laser(b, tau, sig) {
                        out.push(RenderItem::Polyline {
                            pts,
                            style: b.style.clone(),
                            // a filling laser's full path stays a telegraph
                            active: tau >= *warn && !partly,
                            hue: self.sample_hue(b, tau),
                            alpha,
                        });
                    }
                    // slow laser: the hot prefix renders bright on top
                    if partly {
                        if let Some(pts) = sample_laser_frac(b, tau, sig, hot) {
                            out.push(RenderItem::Polyline {
                                pts,
                                style: b.style.clone(),
                                active: true,
                                hue: self.sample_hue(b, tau),
                                alpha,
                            });
                        }
                    }
                }
            }
        }
        out
    }
}
