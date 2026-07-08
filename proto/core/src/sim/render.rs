use super::*;
use super::slots::eval_render_list_into;

pub enum RenderItem {
    Dot { x: f64, y: f64, th: f64, style: Style, hue: f64, scale: f64, alpha: f64 },
    Polyline { pts: Vec<(f64, f64)>, style: Style, active: bool, hue: f64, alpha: f64 },
}

#[derive(Clone, Default)]
pub(super) struct RenderScratch {
    rows: Vec<RenderData>,
    ranges: Vec<std::ops::Range<usize>>,
    defs: Vec<DynRender>,
}

impl RenderScratch {
    fn clear_for_entities(&mut self, len: usize) {
        self.rows.clear();
        self.ranges.clear();
        self.defs.clear();
        if self.ranges.capacity() < len {
            self.ranges.reserve_exact(len - self.ranges.capacity());
        }
    }

    fn push_empty(&mut self) {
        let at = self.rows.len();
        self.ranges.push(at..at);
    }

    fn begin_row(&self) -> usize {
        self.rows.len()
    }

    fn finish_row(&mut self, start: usize) {
        self.ranges.push(start..self.rows.len());
    }

    fn row(&self, entity_row: usize) -> &[RenderData] {
        let range = self
            .ranges
            .get(entity_row)
            .cloned()
            .unwrap_or_else(|| self.rows.len()..self.rows.len());
        &self.rows[range]
    }
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

    fn sample_hue(&self, projector: &RenderProjector, tau: f64) -> f64 {
        self.sample_sig(&projector.sigs.hue, tau, 0.0)
    }

    pub fn render(&mut self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        let mut scratch = std::mem::take(&mut self.render_scratch);
        scratch.clear_for_entities(self.world.entities.len());
        for (i, _) in self.world.entities.iter().enumerate() {
            if !self.world.entities.is_alive(i) {
                scratch.push_empty();
                continue;
            }
            let Some(render_projector) = self.world.entities.render_projector(i) else {
                scratch.push_empty();
                continue;
            };
            let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                scratch.push_empty();
                continue;
            };
            let tau = self.world.entities.tau(i, self.world.tick);
            match dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    scratch.push_empty();
                    if self.world.entities.is_traced(i) {
                        let trace = self.world.entities.trace_samples(i);
                        if trace.len() >= 2 {
                            out.push(RenderItem::Polyline {
                                pts: trace.iter().map(|p| (p.x, p.y)).collect(),
                                style: render_projector.style.clone(),
                                active: true,
                                hue: self.sample_hue(render_projector, tau),
                                alpha: self.sample_sig(&render_projector.sigs.opacity, tau, 1.0),
                            });
                        }
                    } else {
                        let readers = self.motion_readers(i);
                        let state = MotionState::new();
                        if let Ok(p) = dyn_figure_pose_in(dyn_figure, tau, MotionEvalCtx::new(&state, sig, &readers)) {
                            out.push(RenderItem::Dot {
                                x: p.x,
                                y: p.y,
                                // :facing overrides the motion direction
                                th: self.sample_sig(&render_projector.sigs.facing, tau, p.angle_or(0.0)),
                                style: render_projector.style.clone(),
                                hue: self.sample_hue(render_projector, tau),
                                scale: self.sample_sig(&render_projector.sigs.scale, tau, 1.0),
                                alpha: self.sample_sig(&render_projector.sigs.opacity, tau, 1.0),
                            });
                        }
                    }
                }
                FigureDynRepr::Curve { .. } => {
                    let alpha = self.sample_sig(&render_projector.sigs.opacity, tau, 1.0);
                    let start = scratch.begin_row();
                    eval_render_list_into(
                        dyn_figure,
                        render_projector,
                        tau,
                        sig,
                        &mut scratch.defs,
                        &mut scratch.rows,
                    );
                    scratch.finish_row(start);
                    for data in scratch.row(i) {
                        match data {
                            RenderData::None => {}
                            RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                                pts: points.clone(),
                                style: render_projector.style.clone(),
                                active: *active,
                                hue: self.sample_hue(render_projector, tau),
                                alpha,
                            }),
                        }
                    }
                }
            }
        }
        self.render_scratch = scratch;
        out
    }
}
