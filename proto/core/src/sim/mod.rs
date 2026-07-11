//! The deterministic sim: fixed-tick scheduler over inert Action trees +
//! bullet/entity world. design.md §4: step(inputs) → events; render getters.

use crate::edn::{read_all, Form};
use crate::interp::*;
use std::rc::Rc;

mod channels;
mod collision;
mod exec;
mod render;
mod slots;

#[cfg(test)]
mod tests;

use exec::{new_task, step_task, Task, TF};

const PLAYFIELD: f64 = 12.0; // cull margin (units)

#[derive(Clone, Default)]
pub struct Inputs {
    pub vals: Vec<(Rc<str>, Val)>,
}

impl Inputs {
    /// The classic mock pair (tests, simple hosts).
    pub fn classic(player: (f64, f64), nearest_enemy: (f64, f64)) -> Inputs {
        let mut i = Inputs::default();
        i.set_vec2("player", player.0, player.1);
        i.set_vec2("nearest-enemy", nearest_enemy.0, nearest_enemy.1);
        i
    }

    pub fn set(&mut self, name: &str, v: Val) {
        match self.vals.iter_mut().find(|(k, _)| &**k == name) {
            Some((_, slot)) => *slot = v,
            None => self.vals.push((name.into(), v)),
        }
    }

    pub fn set_num(&mut self, name: &str, v: f64) {
        self.set(name, Val::Num(v));
    }

    pub fn set_vec2(&mut self, name: &str, x: f64, y: f64) {
        self.set(name, Val::Pose(Pose::point(x, y)));
    }

    pub fn set_flag(&mut self, name: &str, b: bool) {
        self.set_num(name, if b { 1.0 } else { 0.0 });
    }

    pub fn get(&self, name: &str) -> Option<&Val> {
        self.vals.iter().find(|(k, _)| &**k == name).map(|(_, v)| v)
    }
}

pub struct Sim {
    pub world: World,
    tasks: Vec<Task>,
    ctx: Ctx,
    collider_scratch: ColliderScratch,
    render_scratch: render::RenderScratch,
    /// (defchannel $name expr) rules from the loaded card (stdlib included),
    /// evaluated once per tick at the end of refresh_channels.
    card_channels: Vec<(Rc<str>, Form)>,
}

#[derive(Clone, Default)]
struct ColliderScratch {
    rows: Vec<ColliderData>,
    ranges: Vec<std::ops::Range<usize>>,
    defs: Vec<DynCollider>,
}

impl ColliderScratch {
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

    fn row(&self, entity_row: usize) -> &[ColliderData] {
        let range = self
            .ranges
            .get(entity_row)
            .cloned()
            .unwrap_or_else(|| self.rows.len()..self.rows.len());
        &self.rows[range]
    }
}

fn install_tick_rules(card: &Card, ctx: &mut Ctx, world: &mut World) -> Result<(), String> {
    for form in &card.tick_rules {
        evaluate(form, &Env::empty(), ctx, world)?;
    }
    Ok(())
}

/// Snapshot = clone: everything is Rc-shared immutable or plain data, except
/// control cells, which are mutable and must deep-copy (a scrubbed-back sim
/// must not see future cell writes). This is what makes scrubbing "restore
/// nearest snapshot + re-step the input tape" (design.md §11).
impl Clone for Sim {
    fn clone(&self) -> Sim {
        let mut ctx = self.ctx.clone();
        ctx.sig.cells =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.cells.borrow().clone()));
        ctx.sig.exports =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.exports.borrow().clone()));
        ctx.sig.bound_channels =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.bound_channels.borrow().clone()));
        Sim {
            world: self.world.clone(),
            tasks: self.tasks.clone(),
            ctx,
            collider_scratch: ColliderScratch::default(),
            render_scratch: render::RenderScratch::default(),
            card_channels: self.card_channels.clone(),
        }
    }
}
impl Sim {
    /// Load a card FILE (resolving imports) and instantiate a pattern.
    pub fn load_file(path: &std::path::Path, pattern: Option<&str>) -> Result<Sim, String> {
        let src = crate::edn::expand_card(path)?;
        Sim::load(&src, pattern)
    }

    /// Load a card source and instantiate `pattern` (or the first defpattern).
    /// Imports are expanded (bare names hit the library, paths the cwd);
    /// already-expanded sources pass through untouched.
    pub fn load(src: &str, pattern: Option<&str>) -> Result<Sim, String> {
        let src = crate::edn::expand_src(src)?;
        let forms = read_all(&src).map_err(|e| e.to_string())?;
        let card = load_card(&forms)?;
        let name = match pattern {
            Some(n) => n.to_string(),
            None => card.order.first().cloned().ok_or("card has no defpattern")?,
        };
        Sim::from_pattern(&card, &name)
    }

    /// Run arbitrary action-valued forms as an anonymous pattern, with the
    /// given card's defs (and defpatterns) in scope — the REPL entry point.
    /// A leading (defpattern ...) form registers and runs itself.
    pub fn load_forms(card_src: &str, form_src: &str) -> Result<Sim, String> {
        let card_src = crate::edn::expand_src(card_src)?;
        let card_forms = read_all(&card_src).map_err(|e| e.to_string())?;
        let mut card = load_card(&card_forms)?;
        let body = read_all(form_src).map_err(|e| e.to_string())?;
        if let Some(Form::List(items)) = body.first() {
            if matches!(items.first(), Some(Form::Sym(s)) if &**s == "defpattern") {
                let sent = load_card(&body)?;
                let first = sent.order.first().cloned().ok_or("no defpattern")?;
                card.patterns.extend(sent.patterns);
                card.defs.extend(sent.defs);
                card.tick_rules.extend(sent.tick_rules);
                return Sim::from_pattern(&card, &first);
            }
        }
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.patterns = Rc::new(card.patterns.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let mut world = World::default();
        install_tick_rules(&card, &mut ctx, &mut world)?;
        let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        let task = new_task(vec![TF::Seq { items: body.into(), idx: 0, env }]);
        Ok(Sim {
            world,
            tasks: vec![task],
            ctx,
            collider_scratch: ColliderScratch::default(),
            render_scratch: render::RenderScratch::default(),
            card_channels: card.channels,
        })
    }

    /// Build a task for new program forms, updating defs. Shared by swap/add.
    /// Wire sources stay self-contained for FILE imports; library imports
    /// resolve from the engine-embedded stdlib on any host.
    fn program_task(&mut self, card_src: &str, form_src: &str) -> Result<Task, String> {
        let card_src = crate::edn::expand_src(card_src)?;
        let card_forms = read_all(&card_src).map_err(|e| e.to_string())?;
        let mut card = load_card(&card_forms)?;
        let body_forms = read_all(form_src).map_err(|e| e.to_string())?;
        let (body, env): (Rc<[Form]>, Env) = match body_forms.first() {
            Some(Form::List(items))
                if matches!(items.first(), Some(Form::Sym(s)) if &**s == "defpattern") =>
            {
                let sent = load_card(&body_forms)?;
                let first = sent.order.first().cloned().ok_or("no defpattern")?;
                card.patterns.extend(sent.patterns);
                card.defs.extend(sent.defs);
                card.tick_rules.extend(sent.tick_rules);
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
                self.card_channels = card.channels.clone();
                install_tick_rules(&card, &mut self.ctx, &mut self.world)?;
                let pat = &self.ctx.patterns.clone()[&first];
                let mut env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
                let mut w = World::default();
                for (pname, default) in &pat.params {
                    let v = evaluate(default, &env, &mut self.ctx, &mut w)?;
                    env = env.bind(pname.clone(), v);
                }
                (pat.body.clone(), env)
            }
            _ => {
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
                self.card_channels = card.channels.clone();
                install_tick_rules(&card, &mut self.ctx, &mut self.world)?;
                let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
                (body_forms.into(), env)
            }
        };
        Ok(new_task(vec![TF::Seq { items: body, idx: 0, env }]))
    }

    /// Generational hot-swap (design.md §11): replace the program, KEEP the
    /// world — in-flight entities keep the delegates they spawned with; the
    /// new pattern's control tree starts now. Cells persist.
    pub fn swap_forms(&mut self, card_src: &str, form_src: &str) -> Result<(), String> {
        let task = self.program_task(card_src, form_src)?;
        self.tasks = vec![task];
        Ok(())
    }

    /// Layer a pattern onto the running sim: existing tasks and world are
    /// untouched; the added pattern's local clocks anchor at THIS tick
    /// (waits are relative countdowns — §3's action-local clock rule).
    pub fn add_forms(&mut self, card_src: &str, form_src: &str) -> Result<(), String> {
        let task = self.program_task(card_src, form_src)?;
        self.tasks.push(task);
        Ok(())
    }

    fn from_pattern(card: &Card, name: &str) -> Result<Sim, String> {
        let pat = card
            .patterns
            .get(name)
            .ok_or_else(|| format!("no pattern '{}' in card", name))?;
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.patterns = Rc::new(card.patterns.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let mut world = World::default();
        install_tick_rules(card, &mut ctx, &mut world)?;
        let mut env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        for (pname, default) in &pat.params {
            let v = evaluate(default, &env, &mut ctx, &mut world)?;
            env = env.bind(pname.clone(), v);
        }
        let task = new_task(vec![TF::Seq { items: pat.body.clone(), idx: 0, env }]);
        Ok(Sim {
            world,
            tasks: vec![task],
            ctx,
            collider_scratch: ColliderScratch::default(),
            render_scratch: render::RenderScratch::default(),
            card_channels: card.channels.clone(),
        })
    }

    pub fn tick(&self) -> u64 {
        self.world.tick
    }

    pub fn resize_entity_capacity(&mut self, max_entities: usize) -> Result<(), String> {
        self.world.resize_entity_capacity(max_entities)
    }

    pub(crate) fn motion_readers(&self, row: usize) -> MotionReaders {
        let dense_n2 = Rc::new(self.world.entities.state_n2_snapshot(row));
        let dense_dyn = Rc::new(self.world.entities.state_dyn_snapshot(row));
        let dense_val = Rc::new(self.world.entities.state_val_snapshot(row));
        let node_ids = self
            .world
            .entities
            .motion_schema(row)
            .map(|schema| schema.shared_node_ids())
            .unwrap_or_default();
        MotionReaders {
            n2: Rc::new(move |key| dense_n2.get(&key).copied()),
            dyns: Rc::new(move |key| dense_dyn.get(&key).cloned()),
            vals: Rc::new(move |key| dense_val.get(&key).cloned()),
            node_ids,
        }
    }

    pub(crate) fn channel_u64(&self, name: &str) -> u64 {
        self.ctx
            .sig
            .channel(name)
            .and_then(|v| v.num().ok())
            .map(|v| v as u64)
            .unwrap_or(0)
    }

    fn refresh_dyn_cols(&mut self) -> Result<(), String> {
        let tick = self.world.tick;
        let sig = self.ctx.sig.clone();
        let state = MotionState::new();
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            let tau = self.world.entity_tau(i, tick);
            for (col, dyn_num) in self.world.entities.dyn_cols(i).iter() {
                let value = eval_dyn_with_tick_rate(dyn_num, tau, &state, &sig, self.world.tick_rate())
                    .map_err(|e| format!("dyn meta field: {}", e))?;
                self.world.col_set_sym_at(i, *col, value);
            }
        }
        Ok(())
    }

    fn exec_tick_value(&mut self, v: Val) -> Result<(), String> {
        match v {
            Val::Action(action) => {
                exec_instant(&action, &mut self.ctx, &mut self.world)?;
            }
            Val::Arr(items) => {
                for item in items.iter() {
                    self.exec_tick_value(item.clone())?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn drain_pending_writes(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.world.pending_writes);
        if pending.is_empty() {
            return Ok(());
        }
        let closed_sig = SigEnv { defs: self.ctx.sig.defs.clone(), ..SigEnv::default() };
        let mut fuel: u32 = 100_000;
        for write in pending {
            match write {
                PendingWrite::Field { target, col, f } => {
                    Self::bill_pending_write(&mut fuel)?;
                    self.apply_pending_field(target, col, f, &closed_sig)?;
                }
                PendingWrite::Remat { target, spec } => {
                    let Some(row) = self.world.find(target) else {
                        continue;
                    };
                    if let Some(motion) = spec.motion {
                        Self::bill_pending_write(&mut fuel)?;
                        self.apply_pending_motion(row, motion, &closed_sig)?;
                    }
                    for (col, f) in spec.fields {
                        Self::bill_pending_write(&mut fuel)?;
                        self.apply_pending_field(target, col, f, &closed_sig)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn bill_pending_write(fuel: &mut u32) -> Result<(), String> {
        *fuel -= 1;
        if *fuel == 0 {
            return Err("pending write: fuel exhausted while draining pending writes".into());
        }
        Ok(())
    }

    fn closed_call_ctx(&self, sig: SigEnv) -> Ctx {
        Ctx {
            sig,
            ambient: Pose::IDENTITY,
            scan: None,
            patterns: Rc::new(std::collections::HashMap::new()),
            macros: Rc::new(std::collections::HashMap::new()),
            deferred: Vec::new(),
            projector_scope: None,
            signal_scope: false,
        }
    }

    fn apply_pending_field(
        &mut self,
        target: EntityRef,
        col: ColName,
        f: Val,
        closed_sig: &SigEnv,
    ) -> Result<(), String> {
        let Some(row) = self.world.find(target) else {
            return Ok(());
        };
        let next = match f {
            Val::Fn { .. } | Val::Builtin(_) => {
                let cur = self
                    .world
                    .col_get_sym_at(row, col)
                    .map(Val::Num)
                    .or_else(|| {
                        self.world.sym_field_value_at(row, col).and_then(|sym| {
                            self.world.symbols.resolve(sym).map(|name| Val::Kw(name.into()))
                        })
                    })
                    .unwrap_or(Val::Nothing);
                let mut call_ctx = self.closed_call_ctx(closed_sig.clone());
                let mut call_world = World::for_eval(self.world.tick_rate());
                apply_fn(f, &[cur], &mut call_ctx, &mut call_world, false)?
            }
            constant => constant,
        };
        let Some(row) = self.world.find(target) else {
            return Ok(());
        };
        let col_label = self.world.symbols.resolve(col).unwrap_or("<unknown>").to_string();
        match next {
            Val::Num(n) => {
                self.world.sym_field_clear_at(row, col);
                self.world.col_set_sym_at(row, col, n);
            }
            Val::Kw(v) => {
                self.world.col_clear_sym_at(row, col);
                let value = self.world.symbols.intern(v.as_ref());
                self.world.sym_field_set_at(row, col, value);
            }
            other => {
                return Err(format!(
                    "change-col: :{} expected number or keyword value, got {:?}",
                    col_label, other
                ));
            }
        }
        Ok(())
    }

    fn apply_pending_motion(
        &mut self,
        row: usize,
        motion: Val,
        closed_sig: &SigEnv,
    ) -> Result<(), String> {
        let (exit, anchor) = {
            let dyn_figure = self
                .world
                .entities
                .dyn_figure(row)
                .cloned()
                .ok_or_else(|| format!("remat: missing dyn figure for row {row}"))?;
            let tau = self.world.entity_motion_tau(row, self.world.tick);
            let readers = entity_motion_readers(row, &self.world);
            let state = MotionState::new();
            let p = dyn_figure_pose_in(
                &dyn_figure,
                tau,
                MotionEvalCtx::with_tick_rate(&state, &self.ctx.sig, &readers, self.world.tick_rate()),
            )?;
            let vel = self.world.entity_velocity_from_samples(row, self.world.tick);
            let heading = if vel.0 == 0.0 && vel.1 == 0.0 {
                p.angle_or(0.0)
            } else {
                vel.1.atan2(vel.0).to_degrees()
            };
            let exit = Val::Map(Rc::new(vec![
                (Val::Kw("pos".into()), Val::Pose(Pose::point(p.x, p.y))),
                (Val::Kw("vel".into()), Val::Pose(Pose::point(vel.0, vel.1))),
                (Val::Kw("t".into()), Val::Num(tau)),
            ]));
            (exit, Pose::oriented(p.x, p.y, heading))
        };
        let new_dyn = match motion {
            Val::Fn { .. } | Val::Builtin(_) => {
                let mut call_ctx = self.closed_call_ctx(closed_sig.clone());
                let mut call_world = World::for_eval(self.world.tick_rate());
                as_dyn_pose(apply_fn(motion, &[exit], &mut call_ctx, &mut call_world, false)?)?
            }
            direct => as_dyn_pose(direct)?,
        };
        let dyn_figure = DynFigure::pose(DynPose::pose_node(Rc::new(DynNode::Frame(
            Rc::new(DynNode::Const(anchor)),
            new_dyn.into_node(),
        ))));
        let scanned = is_scanned_figure(&dyn_figure);
        let motion_schema = Rc::new(collect_motion_state_schema(&dyn_figure));
        self.world.entities.set_motion_schema(row, motion_schema);
        self.world.entities.set_sampled_pose(row, self.world.tick, Some(anchor));
        self.world.entities.set_scanned(row, scanned);
        self.world.entities.set_dyn_figure(row, dyn_figure);
        self.world.entities.reset_motion_birth(row, self.world.tick);
        Ok(())
    }

    fn run_standing_rules(&mut self) -> Result<(), String> {
        let rules = self.world.standing_rules.clone();
        for rule in rules {
            for form in rule.body.iter() {
                let value = evaluate(form, &rule.env, &mut self.ctx, &mut self.world)
                    .map_err(|e| format!("deftick: {}", e))?;
                self.exec_tick_value(value)?;
            }
        }
        Ok(())
    }

    pub fn step(&mut self) -> Result<(), String> {
        self.step_with(&Inputs::default())
    }

    pub fn step_with(&mut self, inputs: &Inputs) -> Result<(), String> {
        self.drain_pending_writes()?;
        self.refresh_channels(inputs)?;
        self.world.render_rows.clear();
        // control layer
        let mut i = 0;
        while i < self.tasks.len() {
            let mut task = std::mem::replace(&mut self.tasks[i], new_task(vec![]));
            let mut new_tasks = Vec::new();
            let done = step_task(&mut task, &mut self.ctx, &mut self.world, &mut new_tasks)?;
            if done {
                self.tasks.remove(i);
            } else {
                self.tasks[i] = task;
                i += 1;
            }
            self.tasks.extend(new_tasks);
        }
        // integrate Scanned motion
        let dt = self.world.tick_dt();
        let tick = self.world.tick;
        let sig = self.ctx.sig.clone();
        for i in 0..self.world.entities.len() {
            if self.world.entities.is_alive(i) && self.world.entities.is_scanned(i) {
                let tau = self.world.entity_motion_tau(i, tick);
                let Some(dyn_figure) = self.world.entities.dyn_figure(i).cloned() else {
                    continue;
                };
                let readers = self.motion_readers(i);
                let mut state = MotionState::new();
                let mut n2_writes = Vec::new();
                let mut val_writes = Vec::new();
                let mut write_n2 = |key, value| n2_writes.push((key, value));
                let mut ignore_dyn = |_, _| {};
                let mut write_val = |key, value| val_writes.push((key, value));
                let tick_rate = self.world.tick_rate();
                let mut motion = MotionStepCtx {
                    state: &mut state,
                    sig: &sig,
                    world: Some(&mut self.world),
                    readers: &readers,
                    tick_rate,
                    mirror_legacy: false,
                    write_n2: &mut write_n2,
                    write_dyn: &mut ignore_dyn,
                    write_val: &mut write_val,
                };
                step_dyn_figure_in(&dyn_figure, tau, dt, &mut motion)?;
                for (key, value) in n2_writes {
                    self.world.entities.set_state_n2(i, key, value);
                }
                for (key, value) in val_writes {
                    self.world.entities.set_state_val(i, key, value);
                }
            }
        }
        // Evaluate dyn-owned entity fields after control/motion updates and
        // before any collision/render/rule projector reads entity views.
        self.refresh_dyn_cols()?;
        // record traced curves: a dynamic integer sample domain over the
        // retained history window
        {
            let tick = self.world.tick;
            let sig = self.ctx.sig.clone();
            for i in 0..self.world.entities.len() {
                if !self.world.entities.is_alive(i) {
                    continue;
                }
                let tau = self.world.entity_motion_tau(i, tick);
                let readers = self.motion_readers(i);
                if let Some(window) = self.world.entities.trace_window(i) {
                    let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                        continue;
                    };
                    let state = MotionState::new();
                    if let Ok(p) = dyn_figure_pose_in(
                        dyn_figure,
                        tau,
                        MotionEvalCtx::with_tick_rate(&state, &sig, &readers, self.world.tick_rate()),
                    ) {
                        let cap = (window * self.world.tick_rate()).ceil() as usize + 1;
                        self.world.entities.push_trace_sample(i, p, cap);
                    }
                }
            }
        }
        self.collide(inputs)?;
        self.run_standing_rules()?;
        // bound the retained event window. The log is SHARED across
        // snapshots (they store only a cursor), so this is display
        // history, not snapshot data: restores truncate the tail and
        // re-stepping re-emits — the pruned front is never needed.
        const EVENT_KEEP: u64 = 1200; // 10s of history for host/patterns
        if self.world.log.borrow().entries.len() > 4096 {
            let cutoff = self.world.tick.saturating_sub(EVENT_KEEP);
            self.world.log.borrow_mut().prune(cutoff);
        }
        self.world.tick += 1;
        self.refresh_dyn_cols()?;
        // cull: off-playfield poses/traces; curve lifetime is card/library policy
        let tick = self.world.tick;
        let mut err = None;
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            if self.world.sym_field_matches_at(i, "team", "player-body") {
                continue; // the player rides a channel; never field-culled
            }
            let tau = self.world.entity_motion_tau(i, tick);
            let readers = self.motion_readers(i);
            let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                continue;
            };
            let keep = match dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    let state = MotionState::new();
                    match dyn_figure_pose_in(
                        dyn_figure,
                        tau,
                        MotionEvalCtx::with_tick_rate(&state, &sig, &readers, self.world.tick_rate())
                            .pos_only(),
                    ) {
                    Ok(p) => p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD,
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                    }
                }
                FigureDynRepr::Curve { .. } => true,
            };
            if !keep {
                self.world.cull_at(i);
            }
        }
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    /// Current value of a channel (for host UI, e.g. scrub indicators).
    pub fn channel_val(&self, name: &str) -> Option<Val> {
        self.ctx.sig.channel(name)
    }

    /// DEBUG/tooling read of the pattern-scoped control cells (defcell).
    /// Cells are deliberately NOT part of the host game contract — the
    /// export surface is channels/events/tags (§3) — but an inspector
    /// wants to see them (sorted for stable display).
    pub fn cells_snapshot(&self) -> Vec<(String, Val)> {
        let mut out: Vec<(String, Val)> = self
            .ctx
            .sig
            .cells
            .borrow()
            .values()
            .cloned()
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Read the retained event window with event symbols resolved for host use.
    pub fn with_events<R>(&self, f: impl FnOnce(Vec<Event>) -> R) -> R {
        f(self.events_vec())
    }

    /// Retained events, cloned (tests, casual host use).
    pub fn events_vec(&self) -> Vec<Event> {
        self.world
            .log
            .borrow()
            .entries
            .iter()
            .map(|e| self.world.resolve_event(e))
            .collect()
    }

    /// After restoring this sim as a snapshot: drop shared-log events its
    /// timeline hasn't emitted yet (re-stepping re-emits them).
    pub fn rewind_events(&mut self) {
        self.world.log.borrow_mut().truncate_to(self.world.cursor);
    }
}

fn truthy_pub(v: &Val) -> bool {
    match v {
        Val::Num(n) => *n != 0.0,
        Val::Nothing => false,
        _ => false,
    }
}
