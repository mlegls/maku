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

pub use render::RenderItem;
pub use slots::{sample_curve, sample_curve_frac};

use exec::{new_task, step_task, Task, TF};
use slots::{first_render_projection, materialize_collider_defs, materialize_render_defs};

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
    /// (defchannel $name expr) rules from the loaded card (stdlib included),
    /// evaluated once per tick at the end of refresh_channels.
    card_channels: Vec<(Rc<str>, Form)>,
}

#[derive(Clone, Default)]
struct ColliderScratch {
    rows: Vec<ColliderData>,
    ranges: Vec<std::ops::Range<usize>>,
}

impl ColliderScratch {
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

    fn row(&self, entity_row: usize) -> &[ColliderData] {
        let range = self
            .ranges
            .get(entity_row)
            .cloned()
            .unwrap_or_else(|| self.rows.len()..self.rows.len());
        &self.rows[range]
    }
}

fn install_contacts(card: &Card, ctx: &mut Ctx, world: &mut World) -> Result<(), String> {
    for form in &card.contacts {
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
                card.contacts.extend(sent.contacts);
                return Sim::from_pattern(&card, &first);
            }
        }
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.patterns = Rc::new(card.patterns.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let mut world = World::default();
        install_contacts(&card, &mut ctx, &mut world)?;
        let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        let task = new_task(vec![TF::Seq { items: body.into(), idx: 0, env }]);
        Ok(Sim {
            world,
            tasks: vec![task],
            ctx,
            collider_scratch: ColliderScratch::default(),
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
                card.contacts.extend(sent.contacts);
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
                self.card_channels = card.channels.clone();
                install_contacts(&card, &mut self.ctx, &mut self.world)?;
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
                install_contacts(&card, &mut self.ctx, &mut self.world)?;
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
        install_contacts(card, &mut ctx, &mut world)?;
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
        let node_ids = Rc::new(std::cell::RefCell::new(
            self.world
                .entities
                .motion_schema(row)
                .map(|schema| schema.node_ids.clone())
                .unwrap_or_default(),
        ));
        MotionReaders {
            n2: Rc::new(move |key| dense_n2.get(&key).copied()),
            dyns: Rc::new(move |key| dense_dyn.get(&key).cloned()),
            node_ids,
            stable_required: true,
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

    pub fn step(&mut self) -> Result<(), String> {
        self.step_with(&Inputs::default())
    }

    pub fn step_with(&mut self, inputs: &Inputs) -> Result<(), String> {
        self.refresh_channels(inputs)?;
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
        let dt = 1.0 / TICK_RATE;
        let tick = self.world.tick;
        let sig = self.ctx.sig.clone();
        for i in 0..self.world.entities.len() {
            if self.world.entities.is_alive(i) && self.world.entities.is_scanned(i) {
                let tau = self.world.entities.tau(i, tick);
                let Some(dyn_figure) = self.world.entities.dyn_figure(i).cloned() else {
                    continue;
                };
                let readers = self.motion_readers(i);
                let mut state = MotionState::new();
                let mut n2_writes = Vec::new();
                let mut dyn_writes = Vec::new();
                let mut write_n2 = |key, value| n2_writes.push((key, value));
                let mut write_dyn = |key, value| dyn_writes.push((key, value));
                let mut motion = MotionStepCtx {
                    state: &mut state,
                    sig: &sig,
                    readers: &readers,
                    mirror_legacy: false,
                    write_n2: &mut write_n2,
                    write_dyn: &mut write_dyn,
                };
                step_dyn_figure_in(&dyn_figure, tau, dt, &mut motion)?;
                for (key, value) in &dyn_writes {
                    for (ptr, id) in self.world.entities.extend_motion_schema_for_lazy_stage(i, *key, value)? {
                        readers.node_ids.borrow_mut().insert(ptr, id);
                    }
                }
                for (key, value) in n2_writes {
                    self.world.entities.set_state_n2(i, key, value);
                }
                for (key, value) in dyn_writes {
                    self.world.entities.set_state_dyn(i, key, value);
                }
            }
        }
        // record traced curves: a dynamic integer sample domain over the
        // retained history window
        {
            let tick = self.world.tick;
            let sig = self.ctx.sig.clone();
            for i in 0..self.world.entities.len() {
                if !self.world.entities.is_alive(i) {
                    continue;
                }
                let tau = self.world.entities.tau(i, tick);
                let readers = self.motion_readers(i);
                if let Some(window) = self.world.entities.trace_window(i) {
                    let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                        continue;
                    };
                    let state = MotionState::new();
                    if let Ok(p) = dyn_figure_pose_in(dyn_figure, tau, MotionEvalCtx::new(&state, &sig, &readers)) {
                        let cap = (window * TICK_RATE).ceil() as usize + 1;
                        self.world.entities.push_trace_sample(i, p, cap);
                    }
                }
            }
        }
        self.collide(inputs)?;
        self.fire_triggers();
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
        // cull: off-playfield poses/traces; compatibility curves past their active window
        let tick = self.world.tick;
        let mut err = None;
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            if self.world.sym_field_matches_at(i, "team", "player-body") {
                continue; // the player rides a channel; never field-culled
            }
            let tau = self.world.entities.tau(i, tick);
            let readers = self.motion_readers(i);
            let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                continue;
            };
            let keep = match dyn_figure.repr() {
                FigureDynRepr::Pose(_) => {
                    let state = MotionState::new();
                    match dyn_figure_pose_in(dyn_figure, tau, MotionEvalCtx::new(&state, &sig, &readers)) {
                    Ok(p) => p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD,
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                    }
                }
                FigureDynRepr::Curve { .. } => {
                    let state = MotionState::new();
                    let render_slots = self
                        .world
                        .entities
                        .render_projector(i)
                        .and_then(|projector| materialize_render_defs(projector, tau, &state, &sig).ok())
                        .unwrap_or_default();
                    let render_live = render_slots
                        .first()
                        .map(DynRender::polyline)
                        .map(|projection| tau <= projection.activity.warn + projection.activity.active);
                    let collider_live = || {
                        let Some(render_projector) = self.world.entities.render_projector(i).cloned() else {
                            return None;
                        };
                        let Some(projector) = self.world.entities.collider_projector(i).cloned() else {
                            return None;
                        };
                        let mut slots = materialize_collider_defs(
                            &projector,
                            tau,
                            &state,
                            &sig,
                            &mut self.world.symbols,
                        )
                            .ok()?;
                        if let Some(projection) = first_render_projection(&render_projector, tau, &sig) {
                            let curve_slot = CapsuleChainSlot {
                                sample_set: projection.sample_set,
                                u_max_sig: projection.u_max_sig,
                                width: projection.width,
                                activity: projection.activity,
                            };
                            slots = curve_capsule_slots(slots, &curve_slot);
                        }
                        slots.iter().find_map(DynCollider::capsule_chain).map(
                            |(_, projection, _)| {
                                tau <= projection.activity.warn + projection.activity.active
                            },
                        )
                    };
                    render_live.or_else(collider_live).unwrap_or(true)
                }
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

    /// DEBUG/tooling read of the pattern-scoped control cells (defvar).
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
