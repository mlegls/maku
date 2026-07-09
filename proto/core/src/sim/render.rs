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
    fn style_at(&self, i: usize) -> Style {
        Style {
            family: self.world.sym_field_resolved_at(i, "family").unwrap_or("").to_string(),
            color: self.world.sym_field_resolved_at(i, "color").unwrap_or("").to_string(),
            variant: self.world.sym_field_resolved_at(i, "variant").unwrap_or("").to_string(),
        }
    }

    pub fn render(&mut self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for row in &self.world.render_rows {
            match row {
                RenderData::None => {}
                RenderData::Point { x, y, theta, scale, alpha, hue } => out.push(RenderItem::Dot {
                    x: *x,
                    y: *y,
                    th: *theta,
                    style: Style::default(),
                    hue: *hue,
                    scale: *scale,
                    alpha: *alpha,
                }),
                RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                    pts: points.clone(),
                    style: Style::default(),
                    active: *active,
                    hue: 0.0,
                    alpha: 1.0,
                }),
            }
        }
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
            if !self.world.render_rows.is_empty()
                && matches!(dyn_figure.repr(), FigureDynRepr::Pose(_))
                && self.world.sym_field_matches_at(i, "render", "touhou-sprite")
            {
                scratch.push_empty();
                continue;
            }
            let tau = self.world.entities.tau(i, self.world.tick);
            match dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    let style = self.style_at(i);
                    let start = scratch.begin_row();
                    let e_view = entity_view(i, &self.world, sig).ok();
                    let ctx_view = Val::Map(std::rc::Rc::new(vec![
                        (Val::Kw("age".into()), Val::Num(tau)),
                        (Val::Kw("t".into()), Val::Num(tau)),
                        (Val::Kw("tick".into()), Val::Num(self.world.tick as f64)),
                    ]));
                    eval_render_list_into(
                        dyn_figure,
                        render_projector,
                        tau,
                        sig,
                        e_view.as_ref(),
                        Some(&ctx_view),
                        &mut scratch.defs,
                        &mut scratch.rows,
                    );
                    scratch.finish_row(start);
                    if !scratch.row(i).is_empty() {
                        for data in scratch.row(i) {
                            match data {
                                RenderData::None => {}
                                RenderData::Point { x, y, theta, scale, alpha, hue } => out.push(RenderItem::Dot {
                                    x: *x,
                                    y: *y,
                                    th: *theta,
                                    style: style.clone(),
                                    hue: *hue,
                                    scale: *scale,
                                    alpha: *alpha,
                                }),
                                RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                                    pts: points.clone(),
                                    style: style.clone(),
                                    active: *active,
                                    hue: 0.0,
                                    alpha: 1.0,
                                }),
                            }
                        }
                        continue;
                    }
                    if self.world.entities.is_traced(i) {
                        let trace = self.world.entities.trace_samples(i);
                        if trace.len() >= 2 {
                            out.push(RenderItem::Polyline {
                                pts: trace.iter().map(|p| (p.x, p.y)).collect(),
                                style: style.clone(),
                                active: true,
                                hue: 0.0,
                                alpha: 1.0,
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
                                th: p.angle_or(0.0),
                                style: style.clone(),
                                hue: 0.0,
                                scale: 1.0,
                                alpha: 1.0,
                            });
                        }
                    }
                }
                FigureDynRepr::Curve { .. } => {
                    let alpha = 1.0;
                    let style = self.style_at(i);
                    let start = scratch.begin_row();
                    let e_view = entity_view(i, &self.world, sig).ok();
                    let ctx_view = Val::Map(std::rc::Rc::new(vec![
                        (Val::Kw("age".into()), Val::Num(tau)),
                        (Val::Kw("t".into()), Val::Num(tau)),
                        (Val::Kw("tick".into()), Val::Num(self.world.tick as f64)),
                    ]));
                    eval_render_list_into(
                        dyn_figure,
                        render_projector,
                        tau,
                        sig,
                        e_view.as_ref(),
                        Some(&ctx_view),
                        &mut scratch.defs,
                        &mut scratch.rows,
                    );
                    scratch.finish_row(start);
                    for data in scratch.row(i) {
                        match data {
                            RenderData::None => {}
                            RenderData::Point { x, y, theta, scale, alpha, hue } => out.push(RenderItem::Dot {
                                x: *x,
                                y: *y,
                                th: *theta,
                                style: style.clone(),
                                hue: *hue,
                                scale: *scale,
                                alpha: *alpha,
                            }),
                            RenderData::Polyline { points, active } => out.push(RenderItem::Polyline {
                                pts: points.clone(),
                                style: style.clone(),
                                active: *active,
                                hue: 0.0,
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
