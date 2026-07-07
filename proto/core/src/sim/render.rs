use super::*;
use super::slots::eval_render_list;

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
        self.sample_sig(&b.render_projector.sigs.hue, tau, 0.0)
    }

    pub fn render(&self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for (i, b) in self.world.entities.iter().enumerate() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            let tau = self.world.entities.tau(i, self.world.tick);
            match b.dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    if self.world.entities.is_traced(i) {
                        let trace = self.world.entities.trace_samples(i);
                        if trace.len() >= 2 {
                            out.push(RenderItem::Polyline {
                                pts: trace.iter().map(|p| (p.x, p.y)).collect(),
                                style: b.render_projector.style.clone(),
                                active: true,
                                hue: self.sample_hue(b, tau),
                                alpha: self.sample_sig(&b.render_projector.sigs.opacity, tau, 1.0),
                            });
                        }
                    } else {
                        let readers = self.motion_readers(i);
                        let state = MotionState::new();
                        if let Ok(p) = dyn_figure_pose_in(&b.dyn_figure, tau, MotionEvalCtx::new(&state, sig, &readers)) {
                            out.push(RenderItem::Dot {
                                x: p.x,
                                y: p.y,
                                // :facing overrides the motion direction
                                th: self.sample_sig(&b.render_projector.sigs.facing, tau, p.angle_or(0.0)),
                                style: b.render_projector.style.clone(),
                                hue: self.sample_hue(b, tau),
                                scale: self.sample_sig(&b.render_projector.sigs.scale, tau, 1.0),
                                alpha: self.sample_sig(&b.render_projector.sigs.opacity, tau, 1.0),
                            });
                        }
                    }
                }
                FigureDynRepr::Curve { .. } => {
                    let alpha = self.sample_sig(&b.render_projector.sigs.opacity, tau, 1.0);
                    for data in eval_render_list(b, &b.render_projector, tau, sig) {
                        match data {
                            RenderData::None => {}
                            RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                                pts: points,
                                style: b.render_projector.style.clone(),
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
