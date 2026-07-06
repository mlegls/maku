use super::*;
use super::collision::hot_frac;

pub enum RenderItem {
    Dot { x: f64, y: f64, th: f64, style: Style, hue: f64, scale: f64, alpha: f64 },
    Polyline { pts: Vec<(f64, f64)>, style: Style, active: bool, hue: f64, alpha: f64 },
}

impl Sim {
    /// Sample one render-signal tag at entity-local t (default when absent).
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

    fn sample_hue(&self, b: &Entity, tau: f64) -> f64 {
        self.sample_sig(&b.sigs.hue, tau, 0.0)
    }

    fn materialize_render_slot(
        &self,
        b: &Entity,
        slot: &DynRender,
        tau: f64,
        sig: &SigEnv,
    ) -> Vec<RenderData> {
        match slot.repr() {
            RenderDynRepr::CurveCompat(projection) => {
                let hot = hot_frac(&projection.activity, tau, sig);
                let partly = tau >= projection.activity.warn && hot < 1.0;
                let mut out = Vec::new();
                match sample_curve(b, tau, sig) {
                    Some(points) => out.push(RenderData::Polyline {
                        points,
                        // a filling curve's full path stays a telegraph
                        active: tau >= projection.activity.warn && !partly,
                    }),
                    None => out.push(RenderData::None),
                }
                if partly {
                    match sample_curve_frac(b, tau, sig, hot) {
                        Some(points) => out.push(RenderData::Polyline { points, active: true }),
                        None => out.push(RenderData::None),
                    }
                }
                out
            }
        }
    }

    pub fn render(&self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for b in &self.world.entities {
            if !b.alive {
                continue;
            }
            let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
            match b.dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    if b.cache_policy.trace.is_some() {
                        if b.trail.len() >= 2 {
                            out.push(RenderItem::Polyline {
                                pts: b.trail.iter().map(|p| (p.x, p.y)).collect(),
                                style: b.style.clone(),
                                active: true,
                                hue: self.sample_hue(b, tau),
                                alpha: self.sample_sig(&b.sigs.opacity, tau, 1.0),
                            });
                        }
                    } else if let Ok(p) = dyn_figure_pose(&b.dyn_figure, tau, &b.state, sig) {
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
                FigureDynRepr::Curve { .. } => {
                    let alpha = self.sample_sig(&b.sigs.opacity, tau, 1.0);
                    for data in b
                        .renderers
                        .iter()
                        .flat_map(|slot| self.materialize_render_slot(b, slot, tau, sig))
                    {
                        match data {
                            RenderData::None => {}
                            RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                                pts: points,
                                style: b.style.clone(),
                                active,
                                hue: self.sample_hue(b, tau),
                                alpha,
                            }),
                        }
                    }
                }
            }
        }
        out
    }
}
