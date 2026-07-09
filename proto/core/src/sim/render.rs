use super::*;
use super::slots::eval_render_list_into;

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

    pub fn render(&mut self) -> Vec<RenderRow> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for row in &self.world.render_rows {
            if !matches!(row.data, RenderData::None) {
                out.push(row.clone());
            }
        }
        let mut scratch = std::mem::take(&mut self.render_scratch);
        scratch.clear_for_entities(self.world.entities.len());
        for (i, _) in self.world.entities.iter().enumerate() {
            if !self.world.entities.is_alive(i) {
                scratch.push_empty();
                continue;
            }
            let Some(render_projector) = self.world.entities.render_projector(i).cloned() else {
                scratch.push_empty();
                continue;
            };
            let Some(dyn_figure) = self.world.entities.dyn_figure(i).cloned() else {
                scratch.push_empty();
                continue;
            };
            let syms = Sim::stock_style_syms(&mut self.world, i);
            let tau = self.world.entities.tau(i, self.world.tick);
            let readers = self.motion_readers(i);
            let state = MotionState::new();
            let pose = dyn_figure_pose_in(&dyn_figure, tau, MotionEvalCtx::new(&state, sig, &readers)).ok();
            let trace = self.world.entities.trace_samples(i);
            let traced = self.world.entities.is_traced(i);
            let start = scratch.begin_row();
            let e_view = entity_view(i, &self.world, sig).ok();
            let ctx_view = Val::Map(std::rc::Rc::new(vec![
                (Val::Kw("age".into()), Val::Num(tau)),
                (Val::Kw("t".into()), Val::Num(tau)),
                (Val::Kw("tick".into()), Val::Num(self.world.tick as f64)),
            ]));
            eval_render_list_into(
                &dyn_figure,
                &render_projector,
                tau,
                sig,
                e_view.as_ref(),
                Some(&ctx_view),
                pose,
                trace,
                traced,
                &mut scratch.defs,
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
