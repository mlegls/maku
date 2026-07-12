//! The deterministic sim: fixed-tick scheduler over inert Action trees +
//! bullet/entity world. design.md §4: step(inputs) → events; render getters.

use crate::edn::{read_all, Form};
use crate::interp::*;
use crate::model::RenderItem;
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
    vel_batch: VelBatchScratch,
    closed_pose: ClosedPoseScratch,
    /// Host inputs the loaded card requires: the (from-host :name) sites
    /// collected by the load-time schema pass, in first-use order.
    host_manifest: Vec<String>,
    /// Load-time lints from the schema pass (advisory, never errors).
    load_warnings: Vec<String>,
}

/// Scan-step batching (compiled-dyn milestone B): rows whose figure is a
/// chain of constant wrappers over one compiled-integrand Vel node step as
/// lanes of a single batched program run, grouped by program-pair address.
/// Rebuilt every tick; groups recycle through `pool` to keep lane capacity.
#[derive(Default)]
struct VelBatchScratch {
    groups: Vec<VelBatchGroup>,
    /// (a-program ptr, b-program ptr, polar) → index into `groups`, valid
    /// for one tick (the plans' Rcs keep the keyed programs alive for the
    /// tick). Polar is part of the key: programs are structurally INTERNED,
    /// so a `(vel (polar s d))` and a `(vel (cart s d))` site can share the
    /// same program pair while needing different component math.
    index: crate::fxhash::FxHashMap<(usize, usize, bool), usize>,
    /// Last (key, group) pushed: spawn groups occupy contiguous rows, so
    /// consecutive lanes almost always hit the same group without hashing.
    last: Option<((usize, usize, bool), usize)>,
    pool: Vec<VelBatchGroup>,
    regs: Vec<f64>,
}

struct VelBatchGroup {
    plan: VelStepPlan,
    /// (row, n2 slot) per lane.
    rows: Vec<(usize, usize)>,
    tau: Vec<f64>,
    pos: Vec<[f64; 2]>,
    /// Per-lane capture vectors at stride `plan.ap.n_inputs` (empty when
    /// the group's programs take no inputs).
    caps: Vec<f64>,
    /// Oracle only: each lane's own Vel node — capture-slot lanes share
    /// programs across per-entity node clones, so the interpreter re-run
    /// must key state and substitute caps through the LANE's node, not the
    /// group plan's.
    nodes: Vec<Rc<DynNode>>,
    va: Vec<f64>,
    vb: Vec<f64>,
}

impl VelBatchScratch {
    fn begin_tick(&mut self) {
        self.index.clear();
        self.last = None;
        self.pool.extend(self.groups.drain(..).map(|mut g| {
            g.rows.clear();
            g.tau.clear();
            g.pos.clear();
            g.caps.clear();
            g.nodes.clear();
            g.va.clear();
            g.vb.clear();
            g
        }));
    }

    fn push_lane(
        &mut self,
        plan: VelStepPlanRef<'_>,
        row: usize,
        slot: usize,
        tau: f64,
        pos: [f64; 2],
    ) {
        let key = (Rc::as_ptr(plan.ap) as usize, Rc::as_ptr(plan.bp) as usize, plan.polar);
        let idx = match self.last {
            Some((k, idx)) if k == key => idx,
            _ => {
                let idx = *self.index.entry(key).or_insert_with(|| {
                    let owned = plan.to_plan();
                    let mut g = self.pool.pop().unwrap_or_else(|| VelBatchGroup {
                        plan: owned.clone(),
                        rows: Vec::new(),
                        tau: Vec::new(),
                        pos: Vec::new(),
                        caps: Vec::new(),
                        nodes: Vec::new(),
                        va: Vec::new(),
                        vb: Vec::new(),
                    });
                    g.plan = owned;
                    self.groups.push(g);
                    self.groups.len() - 1
                });
                self.last = Some((key, idx));
                idx
            }
        };
        let g = &mut self.groups[idx];
        // group identity = program pair, so every lane's capture width is
        // the shared n_inputs (0 for cap-free programs)
        debug_assert_eq!(plan.caps.len(), g.plan.ap.n_inputs);
        g.rows.push((row, slot));
        g.tau.push(tau);
        g.pos.push(pos);
        g.caps.extend_from_slice(plan.caps);
        if oracle_enabled() {
            g.nodes.push(plan.vel.clone());
        }
    }
}

/// Batched pos-only pose fill for wrapper-chain-over-ClosedPt rows
/// (milestone B): grouped by interned program-pair address, one
/// lane-batched run per component per phase (collide fill, cull), then
/// per-row wrapper composition — the same value the per-row pos_only walk
/// produces, lane-bit-identical by `run_lanes` construction and
/// oracle-checked per lane.
#[derive(Default)]
struct ClosedPoseScratch {
    groups: Vec<ClosedPoseGroup>,
    index: crate::fxhash::FxHashMap<(usize, usize, bool), usize>,
    /// Contiguous spawn groups hit the same group without hashing.
    last: Option<((usize, usize, bool), usize)>,
    pool: Vec<ClosedPoseGroup>,
    regs: Vec<f64>,
    /// ClosedPt programs never read pos (allow_pos=false); the lanes API
    /// still wants a slice.
    zero_pos: Vec<[f64; 2]>,
    /// Per-row results for the current phase; left empty when the phase
    /// found no closed rows, so non-closed cards pay nothing.
    out: Vec<Option<Pose>>,
    /// Cross-tick classification cache: per row, (figure root ptr,
    /// is-closed-chain). A row whose figure Rc is unchanged skips the
    /// chain walk — figures change only at spawn/remat.
    class: Vec<(*const DynNode, bool)>,
    /// Closed rows found by the tick's collect pass (collide); the cull
    /// pass re-lanes only these — validated per use against the row's
    /// current figure root, so a remat between the phases falls back to
    /// the per-row path.
    candidates: Vec<(usize, *const DynNode)>,
}

struct ClosedPoseGroup {
    ap: Rc<NumProgram>,
    bp: Rc<NumProgram>,
    polar: bool,
    rows: Vec<usize>,
    tau: Vec<f64>,
    caps: Vec<f64>,
    va: Vec<f64>,
    vb: Vec<f64>,
}

impl ClosedPoseScratch {
    fn begin_pass(&mut self) {
        self.out.clear();
        self.index.clear();
        self.last = None;
        self.pool.extend(self.groups.drain(..).map(|mut g| {
            g.rows.clear();
            g.tau.clear();
            g.caps.clear();
            g.va.clear();
            g.vb.clear();
            g
        }));
    }

    fn push_lane(&mut self, plan: &ClosedChainRef<'_>, row: usize, tau: f64) {
        let key = (Rc::as_ptr(plan.ap) as usize, Rc::as_ptr(plan.bp) as usize, plan.polar);
        let idx = match self.last {
            Some((k, idx)) if k == key => idx,
            _ => {
                let idx = *self.index.entry(key).or_insert_with(|| {
                    let mut g = self.pool.pop().unwrap_or_else(|| ClosedPoseGroup {
                        ap: plan.ap.clone(),
                        bp: plan.bp.clone(),
                        polar: plan.polar,
                        rows: Vec::new(),
                        tau: Vec::new(),
                        caps: Vec::new(),
                        va: Vec::new(),
                        vb: Vec::new(),
                    });
                    g.ap = plan.ap.clone();
                    g.bp = plan.bp.clone();
                    g.polar = plan.polar;
                    self.groups.push(g);
                    self.groups.len() - 1
                });
                self.last = Some((key, idx));
                idx
            }
        };
        let g = &mut self.groups[idx];
        debug_assert_eq!(plan.caps.len(), g.ap.n_inputs);
        g.rows.push(row);
        g.tau.push(tau);
        g.caps.extend_from_slice(plan.caps);
    }
}

#[derive(Clone, Default)]
struct ColliderScratch {
    rows: Vec<ColliderData>,
    ranges: Vec<std::ops::Range<usize>>,
    defs: Vec<DynCollider>,
    /// Per-pass memo of collider EntityCol name → store slots, keyed by the
    /// name's Rc address (specs stay alive for the whole pass). Cleared at
    /// pass start and after any non-direct materialization, which holds
    /// `&mut World` and could intern new columns.
    field_slots: Vec<(*const u8, FieldSlots)>,
    /// Per-pass memo of projector Rc address → all-Circle fast plan (None =
    /// unclassifiable, take the general path). Same lifetime and staleness
    /// rules as `field_slots` — the plans hold resolved store slots. Keyed
    /// map, not a scan: distinct projectors scale with live spawn groups.
    plans: crate::fxhash::FxHashMap<usize, Option<std::rc::Rc<collision::FastColliderPlan>>>,
}

impl ColliderScratch {
    fn clear_for_entities(&mut self, len: usize) {
        self.rows.clear();
        self.ranges.clear();
        self.defs.clear();
        self.field_slots.clear();
        self.plans.clear();
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

}

/// The Vel integrator's n2 slot in a wrapper-chain-over-Vel row's schema.
/// Such a tree has exactly one stateful node, so the common case is the
/// whole schema being that single n2 cell at slot 0 — resolved without
/// hashing. Anything else takes the keyed lookup.
fn vel_chain_n2_slot(schema: &MotionStateSchema, vel_ptr: usize) -> Option<usize> {
    if schema.n2_keys.len() == 1 && schema.dyn_keys.is_empty() && schema.val_keys.is_empty() {
        debug_assert_eq!(
            schema
                .node_ids
                .get(&vel_ptr)
                .and_then(|id| schema.n2_slots.get(&MotionStateKey::Node(*id)))
                .map(|s| s.0 as usize),
            Some(0),
            "single-cell schema's n2 slot is not the Vel integrator"
        );
        return Some(0);
    }
    let id = schema.node_ids.get(&vel_ptr).copied()?;
    Some(schema.n2_slots.get(&MotionStateKey::Node(id))?.0 as usize)
}

fn install_tick_rules(card: &Card, ctx: &mut Ctx, world: &mut World) -> Result<(), String> {
    for form in &card.tick_rules {
        evaluate(form, &Env::empty(), ctx, world)?;
    }
    Ok(())
}

/// Install top-level `(def $name ...)` streams and run top-level
/// bind!/export! forms, in card order — producers attach before any pattern
/// executes ("defs in order, then bound producers"). Re-install (swap/add)
/// keeps stream identity: an existing name keeps its id and value; a
/// rebound producer replaces in place.
fn install_streams(card: &Card, ctx: &mut Ctx, world: &mut World) -> Result<(), String> {
    for (name, init) in &card.streams {
        if ctx.sig.streams.borrow().contains_key(&**name) {
            continue;
        }
        let v = match init {
            Some(f) => evaluate(f, &Env::empty(), ctx, world)
                .map_err(|e| format!("def ${}: {}", name, e))?,
            None => Val::Nothing,
        };
        let id = world.next_id;
        world.next_id += 1;
        ctx.sig.cells.borrow_mut().insert(id, (name.to_string(), v));
        ctx.sig.streams.borrow_mut().insert(name.to_string(), id);
    }
    for form in &card.stream_forms {
        let v = evaluate(form, &Env::empty(), ctx, world)?;
        if let Val::Action(a) = v {
            crate::interp::exec_instant(&a, ctx, world)?;
        }
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
        ctx.sig.streams =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.streams.borrow().clone()));
        ctx.sig.host_streams =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.host_streams.borrow().clone()));
        ctx.sig.producers =
            Rc::new(std::cell::RefCell::new(self.ctx.sig.producers.borrow().clone()));
        Sim {
            world: self.world.clone(),
            tasks: self.tasks.clone(),
            ctx,
            collider_scratch: ColliderScratch::default(),
            render_scratch: render::RenderScratch::default(),
            vel_batch: VelBatchScratch::default(),
            closed_pose: ClosedPoseScratch::default(),
            host_manifest: self.host_manifest.clone(),
            load_warnings: self.load_warnings.clone(),
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
        let schema = collect_card_schema(&card)?;
        install_tick_rules(&card, &mut ctx, &mut world)?;
        install_streams(&card, &mut ctx, &mut world)?;
        let env = Env::empty();
        let task = new_task(vec![TF::Seq { items: body.into(), idx: 0, env }]);
        Ok(Sim {
            world,
            tasks: vec![task],
            ctx,
            collider_scratch: ColliderScratch::default(),
            render_scratch: render::RenderScratch::default(),
            vel_batch: VelBatchScratch::default(),
            closed_pose: ClosedPoseScratch::default(),
            host_manifest: schema.host_channels,
            load_warnings: schema.warnings,
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
        // Card-definition heads (the forms load_card consumes). A form_src
        // carrying any defpattern is a card FRAGMENT: its definitions merge
        // into the card (rig layered over card — later definitions shadow,
        // load_card's defchannel rule), and the task body is the fragment's
        // trailing action forms — or the first sent pattern when there are
        // none (live-swapped bare defpatterns auto-play).
        const DEF_HEADS: [&str; 7] =
            ["def", "defn", "defmacro", "defpattern", "defcollider", "defchannel", "deftick"];
        let head_in = |form: &Form, heads: &[&str]| {
            matches!(form, Form::List(items)
                if matches!(items.first(), Some(Form::Sym(s)) if heads.contains(&s.as_ref())))
        };
        let (body, env): (Rc<[Form]>, Env) = match body_forms
            .iter()
            .any(|form| head_in(form, &["defpattern"]))
        {
            true => {
                let sent = load_card(&body_forms)?;
                let first = sent.order.first().cloned().ok_or("no defpattern")?;
                card.patterns.extend(sent.patterns);
                card.defs.extend(sent.defs);
                card.macros.extend(sent.macros);
                for (name, init) in sent.streams {
                    card.streams.retain(|(k, _)| *k != name);
                    card.streams.push((name, init));
                }
                card.stream_forms.extend(sent.stream_forms);
                card.tick_rules.extend(sent.tick_rules);
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
                let schema = collect_card_schema(&card)?;
                for name in schema.host_channels {
                    if !self.host_manifest.contains(&name) {
                        self.host_manifest.push(name);
                    }
                }
                for w in schema.warnings {
                    if !self.load_warnings.contains(&w) {
                        self.load_warnings.push(w);
                    }
                }
                install_tick_rules(&card, &mut self.ctx, &mut self.world)?;
                install_streams(&card, &mut self.ctx, &mut self.world)?;
                let actions: Vec<Form> = body_forms
                    .iter()
                    .filter(|form| !head_in(form, &DEF_HEADS))
                    .cloned()
                    .collect();
                if actions.is_empty() {
                    let pat = &self.ctx.patterns.clone()[&first];
                    let mut env = Env::empty();
                    let mut w = World::default();
                    for (pname, default) in &pat.params {
                        let v = evaluate(default, &env, &mut self.ctx, &mut w)?;
                        env = env.bind(pname.clone(), v);
                    }
                    (pat.body.clone(), env)
                } else {
                    let env = Env::empty();
                    (actions.into(), env)
                }
            }
            false => {
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
                let schema = collect_card_schema(&card)?;
                for name in schema.host_channels {
                    if !self.host_manifest.contains(&name) {
                        self.host_manifest.push(name);
                    }
                }
                for w in schema.warnings {
                    if !self.load_warnings.contains(&w) {
                        self.load_warnings.push(w);
                    }
                }
                install_tick_rules(&card, &mut self.ctx, &mut self.world)?;
                install_streams(&card, &mut self.ctx, &mut self.world)?;
                let env = Env::empty();
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
        let schema = collect_card_schema(card)?;
        install_tick_rules(card, &mut ctx, &mut world)?;
        install_streams(card, &mut ctx, &mut world)?;
        let mut env = Env::empty();
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
            vel_batch: VelBatchScratch::default(),
            closed_pose: ClosedPoseScratch::default(),
            host_manifest: schema.host_channels,
            load_warnings: schema.warnings,
        })
    }

    pub fn tick(&self) -> u64 {
        self.world.tick
    }

    pub fn resize_entity_capacity(&mut self, max_entities: usize) -> Result<(), String> {
        self.world.resize_entity_capacity(max_entities)
    }

    pub(crate) fn motion_readers(&self, row: usize) -> MotionReaders {
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        let r = self.motion_readers_inner(row);
        if let Some(f) = probe {
            crate::interp::profile::close("sim:motion-readers", f);
        }
        r
    }

    fn motion_readers_inner(&self, row: usize) -> MotionReaders {
        self.world.entities.row_motion_readers(row)
    }

    /// pos_only pose fast path: a wrapper-chain-over-Vel row's position is
    /// its integrator state pushed through the constant wrappers — read
    /// directly from the n2 column, no readers or dispatch. Bit-identical
    /// to `dyn_figure_pose_in` with pos_only (same ops, same order); the
    /// oracle asserts exactly that.
    pub(crate) fn fast_pos_pose(&self, row: usize, tau: f64, sig: &SigEnv) -> Option<Pose> {
        let fig = self.world.entities.dyn_figure(row)?;
        let ptr = vel_chain_ptr(fig)?;
        let schema = self.world.entities.motion_schema(row)?;
        let slot = vel_chain_n2_slot(schema, ptr)?;
        let state = self.world.entities.state_n2_at_slot(slot, row);
        let p = wrapper_chain_pos_pose(fig.pose_dyn(), state);
        if oracle_enabled() {
            let readers = self.motion_readers(row);
            let mstate = MotionState::default();
            let want = dyn_figure_pose_in(
                fig,
                tau,
                MotionEvalCtx::with_tick_rate(&mstate, sig, &readers, self.world.tick_rate())
                    .pos_only(),
            )
            .ok()?;
            assert_eq!(p, want, "fast pos_only pose diverged from interpreter for row {row}");
        }
        Some(p)
    }

    /// Batched pos-only pose fill, collect pass (collide phase 0): walk
    /// every row once, classify through the cross-tick (figure ptr →
    /// is-closed) cache, collect closed-chain lanes and this tick's
    /// candidate list, run the groups. Non-closed cards pay one pointer
    /// compare per row and skip everything else.
    fn fill_closed_poses(&mut self, tick: u64, sig: &SigEnv) -> Result<(), String> {
        let n = self.world.entities.len();
        let mut s = std::mem::take(&mut self.closed_pose);
        // Cards with no closed rows skip the scan except a rediscovery
        // sweep every 16 ticks (tick-keyed: deterministic, replay-safe).
        // Newly spawned closed rows go unbatched for at most 15 ticks —
        // the per-row path is bit-identical, so only wall time differs.
        if s.candidates.is_empty() && tick % 16 != 0 {
            s.begin_pass();
            self.closed_pose = s;
            return Ok(());
        }
        s.begin_pass();
        s.candidates.clear();
        s.class.resize(n, (std::ptr::null(), false));
        for i in 0..n {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            let Some(fig) = self.world.entities.dyn_figure(i) else {
                continue;
            };
            let root = Rc::as_ptr(fig.pose_dyn());
            let closed = if s.class[i].0 == root {
                s.class[i].1
            } else {
                let closed = closed_chain_plan(fig, sig).is_some();
                s.class[i] = (root, closed);
                closed
            };
            if !closed {
                continue;
            }
            // re-derive the plan (cheap for closed rows: the OnceCell is
            // warm); classification above only cached the boolean
            let Some(plan) = closed_chain_plan(fig, sig) else {
                continue;
            };
            let tau = self.world.entity_motion_tau(i, tick);
            s.push_lane(&plan, i, tau);
            s.candidates.push((i, root));
        }
        self.run_closed_groups(s, sig)
    }

    /// Re-lane pass (cull, after the tick advanced): only this tick's
    /// candidates, validated against the row's current figure root — a
    /// remat between the phases falls back to the per-row path. Cards
    /// with no closed rows skip this entirely.
    fn refill_closed_poses(&mut self, tick: u64, sig: &SigEnv) -> Result<(), String> {
        let mut s = std::mem::take(&mut self.closed_pose);
        let candidates = std::mem::take(&mut s.candidates);
        s.begin_pass();
        for &(i, root) in &candidates {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            let Some(fig) = self.world.entities.dyn_figure(i) else {
                continue;
            };
            if Rc::as_ptr(fig.pose_dyn()) != root {
                continue;
            }
            let Some(plan) = closed_chain_plan(fig, sig) else {
                continue;
            };
            let tau = self.world.entity_motion_tau(i, tick);
            s.push_lane(&plan, i, tau);
        }
        s.candidates = candidates;
        self.run_closed_groups(s, sig)
    }

    /// Shared batch-run half: one lane run per component per group, then
    /// per-row wrapper composition into the sparse out vec.
    fn run_closed_groups(&mut self, mut s: ClosedPoseScratch, sig: &SigEnv) -> Result<(), String> {
        if s.groups.is_empty() {
            self.closed_pose = s;
            return Ok(());
        }
        s.out.resize(self.world.entities.len(), None);
        let oracle = oracle_enabled();
        let tick_rate = self.world.tick_rate();
        let mut regs = std::mem::take(&mut s.regs);
        for g in &mut s.groups {
            let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
            s.zero_pos.clear();
            s.zero_pos.resize(g.rows.len(), [0.0; 2]);
            g.va.clear();
            g.vb.clear();
            run_lanes(&g.ap, 0.0, &g.tau, &s.zero_pos, &g.caps, &mut regs, &mut g.va);
            run_lanes(&g.bp, 0.0, &g.tau, &s.zero_pos, &g.caps, &mut regs, &mut g.vb);
            for l in 0..g.rows.len() {
                let (x, y) = if g.polar {
                    let (sn, cs) = g.vb[l].to_radians().sin_cos();
                    (g.va[l] * cs, g.va[l] * sn)
                } else {
                    (g.va[l], g.vb[l])
                };
                let row = g.rows[l];
                let fig = self
                    .world
                    .entities
                    .dyn_figure(row)
                    .ok_or_else(|| format!("closed pose fill: missing dyn figure for row {row}"))?;
                let p = wrapper_chain_pos_pose(fig.pose_dyn(), [x, y]);
                if oracle {
                    let readers = self.motion_readers(row);
                    let mstate = MotionState::default();
                    // an interpreted error leaves the row unfilled so the
                    // per-row path surfaces it (fast_pos_pose's stance)
                    let Ok(want) = dyn_figure_pose_in(
                        fig,
                        g.tau[l],
                        MotionEvalCtx::with_tick_rate(&mstate, sig, &readers, tick_rate).pos_only(),
                    ) else {
                        continue;
                    };
                    assert_eq!(p, want, "batched closed pose diverged from per-row for row {row}");
                }
                s.out[row] = Some(p);
            }
            if let Some(f) = probe {
                crate::interp::profile::close("dyn:closed-batch", f);
            }
        }
        s.regs = regs;
        self.closed_pose = s;
        Ok(())
    }

    /// This phase's batched closed-chain pose for a row, if it was filled.
    /// Inline: called per row in the collide/cull hot loops, and the
    /// common non-closed card must pay only the empty-vec check.
    #[inline(always)]
    pub(crate) fn closed_pose_at(&self, row: usize) -> Option<Pose> {
        self.closed_pose.out.get(row).copied().flatten()
    }

    #[inline(always)]
    pub(crate) fn has_closed_poses(&self) -> bool {
        !self.closed_pose.out.is_empty()
    }

    /// Classify a row for the batched Vel step: the plan plus the Vel
    /// node's n2 slot resolved through the row's schema. None falls back
    /// to the general per-row walk.
    fn vel_batch_lane<'a>(
        &self,
        dyn_figure: &'a DynFigure,
        row: usize,
        sig: &SigEnv,
    ) -> Option<(VelStepPlanRef<'a>, usize)> {
        let plan = vel_step_plan(dyn_figure, sig)?;
        let schema = self.world.entities.motion_schema(row)?;
        let slot = vel_chain_n2_slot(schema, Rc::as_ptr(plan.vel) as usize)?;
        Some((plan, slot))
    }

    /// Run the tick's collected Vel batches: per group, one lane-batched
    /// program run per component, then `state += v·dt` written straight to
    /// the n2 columns — the same value and write the per-row compiled arm
    /// of `step_motion_in` produces for each lane.
    fn run_vel_batches(&mut self, dt: f64, sig: &SigEnv) -> Result<(), String> {
        if self.vel_batch.groups.is_empty() {
            return Ok(());
        }
        let tick_rate = self.world.tick_rate();
        let oracle = oracle_enabled();
        let mut groups = std::mem::take(&mut self.vel_batch.groups);
        let mut regs = std::mem::take(&mut self.vel_batch.regs);
        for g in &mut groups {
            let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
            run_lanes(&g.plan.ap, 0.0, &g.tau, &g.pos, &g.caps, &mut regs, &mut g.va);
            run_lanes(&g.plan.bp, 0.0, &g.tau, &g.pos, &g.caps, &mut regs, &mut g.vb);
            for l in 0..g.rows.len() {
                let (av, bv) = (g.va[l], g.vb[l]);
                let (vx, vy) = if g.plan.polar {
                    let (s, c) = bv.to_radians().sin_cos();
                    (av * c, av * s)
                } else {
                    (av, bv)
                };
                let (row, slot) = g.rows[l];
                let [x, y] = g.pos[l];
                if oracle {
                    let readers = self.motion_readers(row);
                    oracle_check_vel_step(
                        &g.nodes[l],
                        g.tau[l],
                        dt,
                        (x, y),
                        (vx, vy),
                        sig,
                        &readers,
                        tick_rate,
                    )?;
                }
                self.world
                    .entities
                    .set_state_n2_at_slot(slot, row, [x + vx * dt, y + vy * dt]);
            }
            if let Some(f) = probe {
                crate::interp::profile::close("dyn:vel-batch", f);
            }
        }
        self.vel_batch.groups = groups;
        self.vel_batch.regs = regs;
        Ok(())
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
        let state = MotionState::default();
        // Shared array-valued meta signals (AxisSel) evaluate once per
        // (form, env, tau) group per refresh; each row then selects only
        // its own lane — the SS5 interchange at the Val level. Identity
        // keys are sound because forms/envs are immutable and clones share
        // Rcs; tau joins the key across different-birth spawn groups.
        let mut shared: Option<crate::fxhash::FxHashMap<(usize, usize, u64), Val>> = None;
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            let tau = self.world.entity_tau(i, tick);
            for (col, dyn_num) in self.world.entities.dyn_cols(i).iter() {
                let tick_rate = self.world.tick_rate();
                let value = match dyn_num.repr() {
                    NumDynRepr::AxisSel { form, env, path, flat } => {
                        let key = form_identity(form).map(|f| (f, env.identity(), tau.to_bits()));
                        let hit = key.and_then(|k| shared.as_ref().and_then(|m| m.get(&k).cloned()));
                        let v = match hit {
                            Some(v) => Ok(v),
                            None => {
                                let v = eval_sig_at_rate(form, env, &sig, tau, 0.0, None, None, tick_rate);
                                if let (Some(k), Ok(v)) = (key, &v) {
                                    shared.get_or_insert_with(Default::default).insert(k, v.clone());
                                }
                                v
                            }
                        };
                        v.and_then(|v| axis_select_val(&v, path, *flat).num())
                    }
                    _ => eval_dyn_with_tick_rate(dyn_num, tau, &state, &sig, tick_rate),
                }
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
                            self.world.symbols.resolve_rc(sym).map(|name| Val::Kw(name.clone()))
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
            let state = MotionState::default();
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
            for (form, compiled) in rule.body.iter().zip(rule.compiled.iter()) {
                let result = (|| -> Result<(), String> {
                    if let Some(compiled) = compiled {
                        let before = self.world.render_rows.len();
                        if self.run_compiled_tick_form(compiled)?.is_none() {
                            self.world.render_rows.truncate(before);
                            let value = evaluate(form, &rule.env, &mut self.ctx, &mut self.world)?;
                            self.exec_tick_value(value)
                        } else if oracle_enabled() {
                            let actual = self.world.render_rows.split_off(before);
                            let value = evaluate(form, &rule.env, &mut self.ctx, &mut self.world)?;
                            self.exec_tick_value(value)?;
                            // compare EXPANDED rows: the compiled pass may have
                            // emitted a column batch where the interpreter
                            // emits rows — expansion is the semantic reference
                            let mut compiled_rows = Vec::new();
                            for item in &actual {
                                item.expand_into(&mut compiled_rows);
                            }
                            let mut interp_rows = Vec::new();
                            for item in &self.world.render_rows[before..] {
                                item.expand_into(&mut interp_rows);
                            }
                            assert_eq!(compiled_rows, interp_rows,
                                "compiled deftick render rows mismatch for {:?}", form);
                            Ok(())
                        } else {
                            Ok(())
                        }
                    } else {
                        let value = evaluate(form, &rule.env, &mut self.ctx, &mut self.world)?;
                        self.exec_tick_value(value)
                    }
                })();
                result.map_err(|e| format!("deftick: {}", e))?;
            }
        }
        Ok(())
    }

    fn run_compiled_tick_form(&mut self, form: &CompiledTickForm) -> Result<Option<()>, String> {
        let tests = form.predicate.resolve(&self.world);
        let bail_at = infallible_suffix_start(&tests);
        // The whole predicate scan runs before any row body, mirroring the
        // interpreted phase order (entities-where completes before map), so
        // a body error cannot preempt a later row's predicate bail.
        let mut rows = std::mem::take(&mut self.render_scratch.match_rows);
        rows.clear();
        if !resolved_tests_match_nothing(&tests, bail_at) {
            for row in 0..self.world.entities.len() {
                if !self.world.entities.is_alive(row) {
                    continue;
                }
                let Some(matches) = resolved_row_tests_match(&tests, bail_at, row, &self.world) else {
                    self.render_scratch.match_rows = rows;
                    return Ok(None);
                };
                if matches {
                    rows.push(row);
                }
            }
        }
        // field names resolve once per pass; entity fields cannot change
        // mid-pass (writes are pending until the next tick boundary)
        let fields: Vec<(&Rc<str>, RenderKey, ResolvedRowVal)> = form
            .fields
            .iter()
            .map(|(key, slot, value)| (key, *slot, value.resolve(&self.world)))
            .collect();
        let mut checked: Vec<(Rc<str>, RenderFieldKind)> = Vec::new();
        let result = (|| {
            if let Some(plan) = CompiledRowPlan::from_fields(&fields) {
                if rows.is_empty() {
                    return Ok(Some(()));
                }
                // small passes stay rows: below this, per-batch column
                // allocs cost more than a few pooled row boxes (either
                // path is exact — the frame is a mixed stream by design)
                const RENDER_BATCH_MIN: usize = 16;
                if rows.len() >= RENDER_BATCH_MIN {
                    if let Some(batch) = self.try_render_batch(form, &plan, &rows) {
                        self.world.render_rows.push(RenderItem::Batch(batch));
                        return Ok(Some(()));
                    }
                }
                // batch abort (an error or a per-row kind surprise): the
                // per-row loop below reproduces interpreted error semantics
                // exactly — evaluation is pure and the batch's schema
                // checks were staged, so the world is untouched
                for &row in &rows {
                    let pose = form.needs_pose
                        .then(|| entity_pose_at(row, &self.world, &self.ctx.sig))
                        .transpose()?;
                    let pose = pose.as_ref();
                    // coercion order matches finish_checked: x, y, theta, scale,
                    // alpha, hue — the first bad slot surfaces the same error
                    let data = RenderData::Point {
                        x: self.compiled_num_slot(plan.x, row, pose, 0.0)?,
                        y: self.compiled_num_slot(plan.y, row, pose, 0.0)?,
                        theta: self.compiled_num_slot(plan.theta, row, pose, 0.0)?,
                        scale: self.compiled_num_slot(plan.scale, row, pose, 1.0)?,
                        alpha: self.compiled_num_slot(plan.alpha, row, pose, 1.0)?,
                        hue: self.compiled_num_slot(plan.hue, row, pose, 0.0)?,
                    };
                    let mut rc = self.render_scratch.take_row();
                    let rendered = Rc::get_mut(&mut rc).expect("pooled render row is uniquely owned");
                    rendered.data = data;
                    for (key, value) in &plan.extras {
                        match self.eval_compiled_row_val(value, row, pose) {
                            Val::Num(n) => {
                                render_field_checked(&mut self.world, key, RenderFieldKind::Num, &mut checked)?;
                                rendered.nums.push(((*key).clone(), n));
                            }
                            Val::Kw(sym) => {
                                render_field_checked(&mut self.world, key, RenderFieldKind::Sym, &mut checked)?;
                                rendered.syms.push(((*key).clone(), sym));
                            }
                            Val::Nothing => {}
                            _ => return Err(format!("render: field :{key} must be a number or keyword")),
                        }
                    }
                    self.world.render_rows.push(RenderItem::Row(rc));
                }
                return Ok(Some(()));
            }
            for &row in &rows {
                let pose = form.needs_pose
                    .then(|| entity_pose_at(row, &self.world, &self.ctx.sig))
                    .transpose()?;
                let mut row_fields = RenderRowFields::default();
                for (key, slot, value) in &fields {
                    row_fields.push_slot(*slot, key, self.eval_compiled_row_val(value, row, pose.as_ref()));
                }
                let rendered = row_fields.finish_checked(&mut self.world, &self.ctx.sig, Some(&mut checked))?;
                self.world.render_rows.push(RenderItem::Row(Rc::new(rendered)));
            }
            Ok(Some(()))
        })();
        self.render_scratch.match_rows = rows;
        result
    }

    fn compiled_num_slot(
        &self,
        slot: Option<&ResolvedRowVal>,
        row: usize,
        pose: Option<&Pose>,
        default: f64,
    ) -> Result<f64, String> {
        match slot {
            Some(value) => self.eval_compiled_row_val(value, row, pose).num(),
            None => Ok(default),
        }
    }

    /// One compiled point-rule pass as a column batch (SoA render output,
    /// openspec/specs/render-rows/spec.md). None aborts to the per-row
    /// path on any error or per-row kind surprise; evaluation is pure and
    /// schema checks are staged, so an abort leaves the world untouched
    /// and the rerun reproduces interpreted behavior exactly.
    fn try_render_batch(
        &mut self,
        form: &CompiledTickForm,
        plan: &CompiledRowPlan,
        rows: &[usize],
    ) -> Option<Rc<crate::model::RenderBatch>> {
        let mut poses = std::mem::take(&mut self.render_scratch.pose_rows);
        poses.clear();
        let ok = self.fill_pose_rows(form, plan, rows, &mut poses);
        let batch = if ok { self.fill_render_batch(form, plan, rows, &poses) } else { None };
        poses.clear();
        self.render_scratch.pose_rows = poses;
        batch
    }

    fn fill_pose_rows(
        &self,
        form: &CompiledTickForm,
        plan: &CompiledRowPlan,
        rows: &[usize],
        poses: &mut Vec<Pose>,
    ) -> bool {
        if !form.needs_pose {
            return true;
        }
        // when no lowered value reads :th the theta component is never
        // consumed, so the pos_only fast pose (exact in x/y) is legal
        let needs_theta = plan.reads_theta();
        for &row in rows {
            let fast = (!needs_theta)
                .then(|| {
                    let tau = self.world.entity_motion_tau(row, self.world.tick);
                    self.fast_pos_pose(row, tau, &self.ctx.sig)
                })
                .flatten();
            let pose = match fast {
                Some(p) => p,
                None => match entity_pose_at(row, &self.world, &self.ctx.sig) {
                    Ok(p) => p,
                    Err(_) => return false,
                },
            };
            poses.push(pose);
        }
        true
    }

    fn batch_num_col(
        &self,
        slot: Option<&ResolvedRowVal>,
        default: f64,
        rows: &[usize],
        poses: &[Pose],
    ) -> Option<crate::model::NumColumn> {
        use crate::model::NumColumn;
        match slot {
            None => Some(NumColumn::Const(default)),
            Some(ResolvedRowVal::Num(v)) => Some(NumColumn::Const(*v)),
            Some(value) => {
                if let Some(col) = self.gather_num_rows(value, rows, poses) {
                    return Some(NumColumn::Rows(col));
                }
                let mut col = Vec::with_capacity(rows.len());
                for (k, &row) in rows.iter().enumerate() {
                    col.push(self.eval_compiled_row_val(value, row, poses.get(k)).num().ok()?);
                }
                Some(NumColumn::Rows(col))
            }
        }
    }

    /// Direct numeric gather for reads that provably cannot yield a
    /// keyword: a field name with no sym-field slot only ever reads its
    /// num column (`entity_field_at_slots` checks sym first), and pose
    /// components come from the pose pass. None = shape not covered; the
    /// caller's generic Val loop keeps the error/abort semantics.
    fn gather_num_rows(
        &self,
        value: &ResolvedRowVal,
        rows: &[usize],
        poses: &[Pose],
    ) -> Option<Vec<f64>> {
        match value {
            ResolvedRowVal::PoseX => Some((0..rows.len()).map(|k| poses[k].x).collect()),
            ResolvedRowVal::PoseY => Some((0..rows.len()).map(|k| poses[k].y).collect()),
            ResolvedRowVal::PoseTheta => {
                Some((0..rows.len()).map(|k| poses[k].angle_or(0.0)).collect())
            }
            ResolvedRowVal::Field(slots) if slots.sym.is_none() => {
                // a missing value is Nothing → the interpreted read errors;
                // bail so the generic loop surfaces it
                let mut out = Vec::with_capacity(rows.len());
                for &row in rows {
                    out.push(self.world.col_at_slot(row, slots.num)?);
                }
                Some(out)
            }
            ResolvedRowVal::FieldOr(slots, default) if slots.sym.is_none() => {
                let col = |k: usize| self.world.col_at_slot(rows[k], slots.num);
                match &**default {
                    ResolvedRowVal::Num(d) => {
                        Some((0..rows.len()).map(|k| col(k).unwrap_or(*d)).collect())
                    }
                    ResolvedRowVal::PoseX => {
                        Some((0..rows.len()).map(|k| col(k).unwrap_or_else(|| poses[k].x)).collect())
                    }
                    ResolvedRowVal::PoseY => {
                        Some((0..rows.len()).map(|k| col(k).unwrap_or_else(|| poses[k].y)).collect())
                    }
                    ResolvedRowVal::PoseTheta => Some(
                        (0..rows.len())
                            .map(|k| col(k).unwrap_or_else(|| poses[k].angle_or(0.0)))
                            .collect(),
                    ),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn fill_render_batch(
        &mut self,
        form: &CompiledTickForm,
        plan: &CompiledRowPlan,
        rows: &[usize],
        poses: &[Pose],
    ) -> Option<Rc<crate::model::RenderBatch>> {
        use crate::model::{Column, NumColumn, RenderBatch, RenderSchema};
        let x = self.batch_num_col(plan.x, 0.0, rows, poses)?;
        let y = self.batch_num_col(plan.y, 0.0, rows, poses)?;
        let theta = self.batch_num_col(plan.theta, 0.0, rows, poses)?;
        let scale = self.batch_num_col(plan.scale, 1.0, rows, poses)?;
        let alpha = self.batch_num_col(plan.alpha, 1.0, rows, poses)?;
        let hue = self.batch_num_col(plan.hue, 0.0, rows, poses)?;
        let mut pending: Vec<(FieldName, RenderFieldKind)> = Vec::new();
        let mut cols: Vec<(Rc<str>, RenderFieldKind, Column)> = Vec::with_capacity(plan.extras.len());
        for (key, value) in &plan.extras {
            match value {
                ResolvedRowVal::Num(v) => {
                    self.world.render_field_check_staged(key, RenderFieldKind::Num, &mut pending).ok()?;
                    cols.push(((*key).clone(), RenderFieldKind::Num, Column::Num(NumColumn::Const(*v))));
                }
                ResolvedRowVal::Kw(k) => {
                    self.world.render_field_check_staged(key, RenderFieldKind::Sym, &mut pending).ok()?;
                    cols.push(((*key).clone(), RenderFieldKind::Sym, Column::SymConst(k.clone())));
                }
                value => {
                    enum Fill {
                        Empty,
                        Nums(Vec<f64>, Vec<bool>, bool),
                        Syms(Vec<Option<Rc<str>>>),
                    }
                    let mut fill = Fill::Empty;
                    for (k, &row) in rows.iter().enumerate() {
                        match self.eval_compiled_row_val(value, row, poses.get(k)) {
                            Val::Nothing => match &mut fill {
                                Fill::Empty => {}
                                Fill::Nums(vals, mask, all) => {
                                    vals.push(0.0);
                                    mask.push(false);
                                    *all = false;
                                }
                                Fill::Syms(vals) => vals.push(None),
                            },
                            Val::Num(v) => match &mut fill {
                                Fill::Empty => {
                                    let mut vals = vec![0.0; k];
                                    let mut mask = vec![false; k];
                                    vals.push(v);
                                    mask.push(true);
                                    fill = Fill::Nums(vals, mask, k == 0);
                                }
                                Fill::Nums(vals, mask, _) => {
                                    vals.push(v);
                                    mask.push(true);
                                }
                                Fill::Syms(_) => return None,
                            },
                            Val::Kw(s) => match &mut fill {
                                Fill::Empty => {
                                    let mut vals: Vec<Option<Rc<str>>> = vec![None; k];
                                    vals.push(Some(s));
                                    fill = Fill::Syms(vals);
                                }
                                Fill::Syms(vals) => vals.push(Some(s)),
                                Fill::Nums(..) => return None,
                            },
                            _ => return None,
                        }
                    }
                    match fill {
                        // a field that is nothing on every row contributes
                        // no column (no row would have carried it)
                        Fill::Empty => {}
                        Fill::Nums(vals, mask, all) => {
                            self.world.render_field_check_staged(key, RenderFieldKind::Num, &mut pending).ok()?;
                            let col = if all {
                                Column::Num(NumColumn::Rows(vals))
                            } else {
                                Column::NumOpt(vals, mask)
                            };
                            cols.push(((*key).clone(), RenderFieldKind::Num, col));
                        }
                        Fill::Syms(vals) => {
                            self.world.render_field_check_staged(key, RenderFieldKind::Sym, &mut pending).ok()?;
                            cols.push(((*key).clone(), RenderFieldKind::Sym, Column::Syms(vals)));
                        }
                    }
                }
            }
        }
        let schema_cols: Vec<(Rc<str>, RenderFieldKind)> =
            cols.iter().map(|(k, kind, _)| (k.clone(), *kind)).collect();
        let mut memo = form.schema.borrow_mut();
        let schema = match memo.as_ref() {
            Some(s) if s.cols == schema_cols => s.clone(),
            _ => {
                let s = Rc::new(RenderSchema { cols: schema_cols });
                *memo = Some(s.clone());
                s
            }
        };
        drop(memo);
        self.world.render_field_commit(&pending);
        Some(Rc::new(RenderBatch {
            schema,
            len: rows.len(),
            x,
            y,
            theta,
            scale,
            alpha,
            hue,
            cols: cols.into_iter().map(|(_, _, c)| c).collect(),
        }))
    }

    fn eval_compiled_row_val(&self, value: &ResolvedRowVal, row: usize, pose: Option<&Pose>) -> Val {
        match value {
            ResolvedRowVal::Num(n) => Val::Num(*n),
            ResolvedRowVal::Kw(k) => Val::Kw(k.clone()),
            ResolvedRowVal::PoseX => Val::Num(pose.expect("lowered pose read").x),
            ResolvedRowVal::PoseY => Val::Num(pose.expect("lowered pose read").y),
            ResolvedRowVal::PoseTheta => Val::Num(pose.expect("lowered pose read").angle_or(0.0)),
            ResolvedRowVal::Field(slots) => entity_field_at_slots(row, *slots, &self.world),
            ResolvedRowVal::FieldOr(slots, default) => {
                let present = entity_field_at_slots(row, *slots, &self.world);
                if matches!(present, Val::Nothing) {
                    self.eval_compiled_row_val(default, row, pose)
                } else {
                    present
                }
            }
        }
    }

    pub fn step(&mut self) -> Result<(), String> {
        self.step_with(&Inputs::default())
    }

    pub fn step_with(&mut self, inputs: &Inputs) -> Result<(), String> {
        self.drain_pending_writes()?;
        self.refresh_channels(inputs)?;
        self.render_scratch.recycle_rows(&mut self.world.render_rows);
        // control layer
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
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
        if let Some(f) = probe {
            crate::interp::profile::close("phase:control", f);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        // integrate Scanned motion
        let dt = self.world.tick_dt();
        let tick = self.world.tick;
        let sig = self.ctx.sig.clone();
        // Batchable rows (constant wrappers over one compiled-integrand Vel;
        // see VelBatchScratch) collect lanes instead of stepping inline;
        // everything else takes the general per-row walk. A step only ever
        // touches its own row's state cells, so running the batches after
        // the walk is unobservable.
        self.vel_batch.begin_tick();
        for i in 0..self.world.entities.len() {
            if self.world.entities.is_alive(i) && self.world.entities.is_scanned(i) {
                let tau = self.world.entity_motion_tau(i, tick);
                let Some(dyn_figure) = self.world.entities.dyn_figure(i).cloned() else {
                    continue;
                };
                if let Some((plan, slot)) = self.vel_batch_lane(&dyn_figure, i, &sig) {
                    let pos = self.world.entities.state_n2_at_slot(slot, i);
                    self.vel_batch.push_lane(plan, i, slot, tau, pos);
                    continue;
                }
                let readers = self.motion_readers(i);
                let mut state = MotionState::default();
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
        self.run_vel_batches(dt, &sig)?;
        if let Some(f) = probe {
            crate::interp::profile::close("phase:scan-step", f);
        }
        // Evaluate dyn-owned entity fields after control/motion updates and
        // before any collision/render/rule projector reads entity views.
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        self.refresh_dyn_cols()?;
        if let Some(f) = probe {
            crate::interp::profile::close("phase:dyn-cols", f);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
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
                if let Some(window) = self.world.entities.trace_window(i) {
                    let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                        continue;
                    };
                    // readers only for traced rows — construction is the
                    // dominant per-row fixed cost on untraced cards
                    let readers = self.motion_readers(i);
                    let state = MotionState::default();
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
        if let Some(f) = probe {
            crate::interp::profile::close("phase:trace", f);
        }
        // collide instruments its own phases (phase:collide-mat/-pairs)
        self.collide(inputs)?;
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        self.run_standing_rules()?;
        if let Some(f) = probe {
            crate::interp::profile::close("phase:rules", f);
        }
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
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        self.refresh_dyn_cols()?;
        if let Some(f) = probe {
            crate::interp::profile::close("phase:dyn-cols", f);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        // cull: off-playfield poses/traces; curve lifetime is card/library policy
        let tick = self.world.tick;
        // refill: the tick advanced since the collide-phase fill, so
        // closed-chain poses re-sample at the new tau
        self.refill_closed_poses(tick, &sig)?;
        let closed_any = self.has_closed_poses();
        let mut err = None;
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                continue;
            }
            if self.world.sym_field_matches_at(i, "team", "player-body") {
                continue; // the player rides a channel; never field-culled
            }
            let tau = self.world.entity_motion_tau(i, tick);
            let keep = if let Some(p) = if closed_any { self.closed_pose_at(i) } else { None } {
                p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD
            } else if let Some(p) = self.fast_pos_pose(i, tau, &sig) {
                p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD
            } else {
                let readers = self.motion_readers(i);
                let Some(dyn_figure) = self.world.entities.dyn_figure(i) else {
                    continue;
                };
                match dyn_figure.repr() {
                    FigureDynRepr::Pose(_) => {
                        let state = MotionState::default();
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
                }
            };
            if !keep {
                self.world.cull_at(i);
            }
        }
        if let Some(f) = probe {
            crate::interp::profile::close("phase:cull", f);
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

    /// Host inputs the loaded card requires — the (from-host :name) sites
    /// collected by the load-time schema pass.
    pub fn host_manifest(&self) -> &[String] {
        &self.host_manifest
    }

    /// Load-time lints from the schema pass (advisory).
    pub fn load_warnings(&self) -> &[String] {
        &self.load_warnings
    }

    /// The load-time host-manifest check: every channel the card claims
    /// with (from-host ...) must be in `provided`. Hosts call this right
    /// after load, so a missing channel fails before tick 0, never mid-run.
    pub fn verify_host_channels(&self, provided: &[&str]) -> Result<(), String> {
        for name in &self.host_manifest {
            if !provided.iter().any(|p| p == name) {
                return Err(format!("host does not provide channel {}", name));
            }
        }
        Ok(())
    }

    /// DEBUG/tooling read of the stream store.
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

/// Per-pass slot assignment for compiled point/dot render rows. The field
/// list is fixed for the whole pass and named slots are first-write-wins,
/// so each slot resolves to at most one field up front and every row build
/// skips the RenderRowFields staging (a Val per slot plus an extras vec).
/// None when the shape isn't a static :point/:dot keyword — the generic
/// finish_checked path keeps its error semantics for those forms.
struct CompiledRowPlan<'a> {
    x: Option<&'a ResolvedRowVal>,
    y: Option<&'a ResolvedRowVal>,
    theta: Option<&'a ResolvedRowVal>,
    scale: Option<&'a ResolvedRowVal>,
    alpha: Option<&'a ResolvedRowVal>,
    hue: Option<&'a ResolvedRowVal>,
    extras: Vec<(&'a Rc<str>, &'a ResolvedRowVal)>,
}

fn row_val_reads_theta(value: &ResolvedRowVal) -> bool {
    match value {
        ResolvedRowVal::PoseTheta => true,
        ResolvedRowVal::FieldOr(_, default) => row_val_reads_theta(default),
        _ => false,
    }
}

impl<'a> CompiledRowPlan<'a> {
    /// Whether any lowered value consumes the pose angle; when none does,
    /// the pos_only fast pose is exact for this plan.
    fn reads_theta(&self) -> bool {
        [self.x, self.y, self.theta, self.scale, self.alpha, self.hue]
            .iter()
            .flatten()
            .any(|v| row_val_reads_theta(v))
            || self.extras.iter().any(|(_, v)| row_val_reads_theta(v))
    }

    fn from_fields(fields: &'a [(&'a Rc<str>, RenderKey, ResolvedRowVal)]) -> Option<CompiledRowPlan<'a>> {
        let mut shape = None;
        let mut x = None;
        let mut y = None;
        let mut theta = None;
        let mut facing = None;
        let mut scale = None;
        let mut alpha = None;
        let mut opacity = None;
        let mut hue = None;
        let mut extras = Vec::new();
        let set_first = |slot: &mut Option<&'a ResolvedRowVal>, value: &'a ResolvedRowVal| {
            if slot.is_none() {
                *slot = Some(value);
            }
        };
        for (key, slot, value) in fields {
            match slot {
                RenderKey::Shape => set_first(&mut shape, value),
                RenderKey::X => set_first(&mut x, value),
                RenderKey::Y => set_first(&mut y, value),
                RenderKey::Theta => set_first(&mut theta, value),
                RenderKey::Facing => set_first(&mut facing, value),
                RenderKey::Scale => set_first(&mut scale, value),
                RenderKey::Alpha => set_first(&mut alpha, value),
                RenderKey::Opacity => set_first(&mut opacity, value),
                RenderKey::Hue => set_first(&mut hue, value),
                // point data never reads these; finish_checked drops them
                RenderKey::Points | RenderKey::Pts | RenderKey::Active => {}
                RenderKey::Extra => extras.push((*key, value)),
            }
        }
        let Some(ResolvedRowVal::Kw(kw)) = shape else {
            return None;
        };
        if !matches!(kw.as_ref(), "point" | "dot") {
            return None;
        }
        Some(CompiledRowPlan {
            x,
            y,
            theta: theta.or(facing),
            scale,
            alpha: alpha.or(opacity),
            hue,
            extras,
        })
    }
}

/// `render_field_check` behind the same per-pass (key, kind) memo
/// finish_checked keeps: the schema only accretes within a rule pass, so an
/// accepted pair cannot conflict later in the pass.
fn render_field_checked(
    world: &mut World,
    key: &Rc<str>,
    kind: RenderFieldKind,
    checked: &mut Vec<(Rc<str>, RenderFieldKind)>,
) -> Result<(), String> {
    // plan keys are the same Rc every row, so the pointer check hits
    if checked.iter().any(|(k, seen)| *seen == kind && (Rc::ptr_eq(k, key) || k == key)) {
        return Ok(());
    }
    world.render_field_check(key, kind)?;
    checked.push((key.clone(), kind));
    Ok(())
}

fn truthy_pub(v: &Val) -> bool {
    match v {
        Val::Num(n) => *n != 0.0,
        Val::Nothing => false,
        _ => false,
    }
}

/// Memo identity for a shared signal form: the Rc allocation address of
/// its payload (clones share it). Scalar forms have no useful identity.
fn form_identity(f: &Form) -> Option<usize> {
    match f {
        Form::List(items) | Form::Vector(items) => Some(items.as_ptr() as usize),
        Form::Map(kvs) => Some(kvs.as_ptr() as usize),
        _ => None,
    }
}
