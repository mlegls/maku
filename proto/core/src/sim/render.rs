use super::*;

#[derive(Clone, Default)]
pub(super) struct RenderScratch {
    rows: Vec<RenderData>,
    ranges: Vec<std::ops::Range<usize>>,
}

impl RenderScratch {
    fn clear_for_entities(&mut self, len: usize) {
        self.rows.clear();
        self.ranges.clear();
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
    fn stock_style_syms(world: &mut World, i: usize) -> Vec<(Rc<str>, Rc<str>)> {
        let mut syms = Vec::new();
        for key in ["family", "color", "variant"] {
            let Some(value) = world.sym_field_resolved_at(i, key).map(Rc::<str>::from) else {
                continue;
            };
            if let Err(err) = world.render_field_check(key, RenderFieldKind::Sym) {
                debug_assert!(false, "{err}");
                continue;
            }
            syms.push((Rc::<str>::from(key), value));
        }
        syms
    }

    fn push_row(out: &mut Vec<RenderRow>, data: &RenderData, syms: &[(Rc<str>, Rc<str>)]) {
        if matches!(data, RenderData::None) {
            return;
        }
        out.push(RenderRow { data: data.clone(), nums: Vec::new(), syms: syms.to_vec() });
    }

    fn push_stock_dot_rows(
        dyn_figure: &DynFigure,
        pose: Option<Pose>,
        trace: &[Pose],
        traced: bool,
        out: &mut Vec<RenderData>,
    ) {
        match dyn_figure.repr() {
            FigureDynRepr::Curve { .. } => out.push(RenderData::None),
            FigureDynRepr::Pose(_) if traced && !trace.is_empty() => {
                for p in trace {
                    out.push(RenderData::Point {
                        x: p.x,
                        y: p.y,
                        theta: p.angle_or(0.0),
                        scale: 1.0,
                        alpha: 1.0,
                        hue: 0.0,
                    });
                }
            }
            FigureDynRepr::Pose(_) => {
                let Some(pose) = pose else {
                    out.push(RenderData::None);
                    return;
                };
                out.push(RenderData::Point {
                    x: pose.x,
                    y: pose.y,
                    theta: pose.angle_or(0.0),
                    scale: 1.0,
                    alpha: 1.0,
                    hue: 0.0,
                });
            }
        }
    }

    pub fn render(&mut self) -> Vec<RenderRow> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for row in &self.world.render_rows {
            if !matches!(row.data, RenderData::None) {
                out.push(row.as_ref().clone());
            }
        }
        let mut scratch = std::mem::take(&mut self.render_scratch);
        scratch.clear_for_entities(self.world.entities.len());
        for (i, _) in self.world.entities.iter().enumerate() {
            if !self.world.entities.is_alive(i) {
                scratch.push_empty();
                continue;
            }
            let Some(dyn_figure) = self.world.entities.dyn_figure(i).cloned() else {
                scratch.push_empty();
                continue;
            };
            if matches!(dyn_figure.repr(), FigureDynRepr::Curve { .. }) {
                scratch.push_empty();
                continue;
            }
            match self.world.sym_field_resolved_at(i, "render") {
                Some("dot") | None => {}
                Some(_) => {
                    scratch.push_empty();
                    continue;
                }
            }
            let syms = Sim::stock_style_syms(&mut self.world, i);
            let tau = self.world.entity_motion_tau(i, self.world.tick);
            let readers = self.motion_readers(i);
            let state = MotionState::new();
            let pose = dyn_figure_pose_in(
                &dyn_figure,
                tau,
                MotionEvalCtx::with_tick_rate(&state, sig, &readers, self.world.tick_rate()),
            )
            .ok();
            let trace = self.world.entities.trace_samples(i);
            let traced = self.world.entities.is_traced(i);
            let start = scratch.begin_row();
            Sim::push_stock_dot_rows(
                &dyn_figure,
                pose,
                trace,
                traced,
                &mut scratch.rows,
            );
            scratch.finish_row(start);
            for data in scratch.row(i) {
                Sim::push_row(&mut out, data, &syms);
            }
        }
        self.render_scratch = scratch;
        out
    }
}
