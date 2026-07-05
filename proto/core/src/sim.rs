//! The deterministic sim: fixed-tick scheduler over inert Action trees +
//! bullet/entity world. design.md §4: step(inputs) → events; render getters.

use crate::edn::{read_all, Form};
use crate::interp::*;
use std::rc::Rc;

const PLAYFIELD: f64 = 12.0; // cull margin (units)

/// Sample a laser's world-space curve at `tau` (shared by render and the
/// collision pass — the beam you see is the beam that hits).
pub fn sample_laser(b: &Bullet, tau: f64, sig: &SigEnv) -> Option<Vec<(f64, f64)>> {
    sample_laser_frac(b, tau, sig, 1.0)
}

/// Sample the beam up to `frac` of its length (slow lasers: the hot
/// front's reach). frac 1.0 = the whole path.
pub fn sample_laser_frac(
    b: &Bullet,
    tau: f64,
    sig: &SigEnv,
    frac: f64,
) -> Option<Vec<(f64, f64)>> {
    let Kind::Laser { shape, u_max, u_max_sig, resolution, .. } = &b.kind else {
        return None;
    };
    if frac <= 0.0 {
        return None;
    }
    let anchor = dyn_pose(&b.motion, tau, &b.state, sig).ok()?;
    let u_max = match u_max_sig {
        Some((f, e)) => eval_sig(f, e, sig, tau, 0.0, None, None)
            .and_then(|v| v.num())
            .unwrap_or(*u_max)
            .max(0.01),
        None => *u_max,
    } * frac.min(1.0);
    let steps = ((u_max / resolution).ceil() as usize).clamp(2, 400);
    let mut pts = Vec::with_capacity(steps + 1);
    for k in 0..=steps {
        let u = u_max * k as f64 / steps as f64;
        let local = match shape {
            Some(sh) => dyn_pose_u(sh, tau, u, &b.state, sig).ok()?,
            None => Pose { x: u, y: 0.0, th: 0.0 }, // straight along +x
        };
        let w = anchor.compose(&local);
        pts.push((w.x, w.y));
    }
    Some(pts)
}

/// A slow laser's hot fraction at age tau: 0 before the warn ends, then
/// sweeping to 1 over the :fill window. Lasers without :fill are hot in
/// full the moment the warn ends.
fn hot_frac(kind: &Kind, tau: f64, sig: &SigEnv) -> f64 {
    let Kind::Laser { warn, fill, fill_sig, .. } = kind else { return 1.0 };
    if let Some((f, e)) = fill_sig {
        // signal :fill = swept fraction as a function of laser age
        return eval_sig(f, e, sig, tau, 0.0, None, None)
            .and_then(|v| v.num())
            .map(|x| x.clamp(0.0, 1.0))
            .unwrap_or(1.0);
    }
    match fill {
        Some(d) if *d > 0.0 => ((tau - warn) / d).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// Distance from a point to a polyline (capsule-chain narrow phase).
fn dist_to_chain(p: (f64, f64), pts: &[(f64, f64)]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for seg in pts.windows(2) {
        let (ax, ay) = seg[0];
        let (bx, by) = seg[1];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((p.0 - ax) * dx + (p.1 - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        let d = ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt();
        best = Some(best.map_or(d, |b: f64| b.min(d)));
    }
    best
}

/// Entity view handed to contact-time pure functions (:damage fns).
fn contact_map(b: &Bullet, pos: Option<(f64, f64)>, vel: (f64, f64)) -> Val {
    let mut kvs = vec![
        (Val::Kw("vel".into()), Val::Vec2 { x: vel.0, y: vel.1 }),
        (Val::Kw("family".into()), Val::Kw(b.style.family.as_str().into())),
    ];
    if let Some((x, y)) = pos {
        kvs.push((Val::Kw("pos".into()), Val::Vec2 { x, y }));
    }
    for (k, v) in &b.cols {
        kvs.push((Val::Kw(k.as_ref().into()), Val::Num(*v)));
    }
    if let Some(t) = &b.team {
        kvs.push((Val::Kw("team".into()), Val::Kw(t.as_ref().into())));
    }
    Val::Map(Rc::new(kvs))
}

pub enum RenderItem {
    Dot { x: f64, y: f64, th: f64, style: Style, hue: f64, scale: f64, alpha: f64 },
    Polyline { pts: Vec<(f64, f64)>, style: Style, active: bool, hue: f64, alpha: f64 },
}

/// One running task = a stack of resumable cursors over Action trees.
#[derive(Clone)]
enum TF {
    Seq { items: Rc<[Form]>, idx: usize, env: Env },
    /// (until pred ...) scope marker: while on the stack, the predicate
    /// cancels this task; forks under it inherit it as a task guard.
    Guard { pred: Form, env: Env },
    Dot {
        var: Rc<str>,
        n: f64,
        seq_binds: Vec<(Rc<str>, Val)>,
        every: u64,
        body: Rc<[Form]>,
        env: Env,
        i: f64,
        started: bool,
    },
    Loop {
        names: Vec<Rc<str>>,
        body: Rc<[Form]>,
        env: Env,
        cur: Vec<Val>,
        idx: usize,
    },
    Frame(FrameSpec),
    /// A running `phases` machine: the trampoline over ordered states.
    /// stage: 0 = enter cur (arm the goto guard, push the body), 1 = body
    /// exited (completed/cancelled) → run finally, 2 = route (goto target
    /// or state order) and loop to 0.
    Phases {
        clauses: Rc<[PhaseClause]>,
        env: Env,
        /// goto-request cell (world-counter id, so it snapshots/replays).
        cell: u64,
        cur: usize,
        stage: u8,
    },
}

#[derive(Clone)]
struct Task {
    stack: Vec<TF>,
    wait: u64,
    wait_pred: Option<(Form, Env)>,
    /// Cancellation guards inherited from enclosing (until ...) scopes at
    /// fork time — structured cancellation: when a guard fires, this task
    /// and every task forked under the same scope die together.
    guards: Vec<(Form, Env)>,
}

fn new_task(stack: Vec<TF>) -> Task {
    Task { stack, wait: 0, wait_pred: None, guards: Vec::new() }
}

/// Host inputs, passed BY NAME (§3's channel principle applied to the input
/// tape): an open vocabulary of named values, not a fixed struct. Names are
/// interned (Rc<str>), so tape entries clone cheaply. Conventional names —
/// "player", "nearest-enemy", "move-x"/"move-y", "focus-firing", "bomb",
/// "p1-move-x"/"p2-move-x"… — are just names; the sim defaults the classic
/// set when the host omits them and derivations override where world
/// sources exist.
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
        self.set(name, Val::Vec2 { x, y });
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
        Sim { world: self.world.clone(), tasks: self.tasks.clone(), ctx }
    }
}

impl Sim {
    /// Load a card FILE (resolving imports) and instantiate a pattern.
    pub fn load_file(path: &std::path::Path, pattern: Option<&str>) -> Result<Sim, String> {
        let src = crate::edn::expand_card(path)?;
        Sim::load(&src, pattern)
    }

    /// Load a card source and instantiate `pattern` (or the first defpattern).
    pub fn load(src: &str, pattern: Option<&str>) -> Result<Sim, String> {
        let forms = read_all(src).map_err(|e| e.to_string())?;
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
        let card_forms = read_all(card_src).map_err(|e| e.to_string())?;
        let mut card = load_card(&card_forms)?;
        let body = read_all(form_src).map_err(|e| e.to_string())?;
        if let Some(Form::List(items)) = body.first() {
            if matches!(items.first(), Some(Form::Sym(s)) if &**s == "defpattern") {
                let sent = load_card(&body)?;
                let first = sent.order.first().cloned().ok_or("no defpattern")?;
                card.patterns.extend(sent.patterns);
                card.defs.extend(sent.defs);
                return Sim::from_pattern(&card, &first);
            }
        }
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        ctx.patterns = Rc::new(card.patterns.clone());
        ctx.macros = Rc::new(card.macros.clone());
        let world = World::default();
        let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        let task = new_task(vec![TF::Seq { items: body.into(), idx: 0, env }]);
        Ok(Sim { world, tasks: vec![task], ctx })
    }

    /// Build a task for new program forms, updating defs. Shared by swap/add.
    fn program_task(&mut self, card_src: &str, form_src: &str) -> Result<Task, String> {
        let card_forms = read_all(card_src).map_err(|e| e.to_string())?;
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
                self.ctx.sig.defs = Rc::new(card.defs.clone());
                self.ctx.patterns = Rc::new(card.patterns.clone());
                self.ctx.macros = Rc::new(card.macros.clone());
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
                let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
                (body_forms.into(), env)
            }
        };
        Ok(new_task(vec![TF::Seq { items: body, idx: 0, env }]))
    }

    /// Generational hot-swap (design.md §11): replace the program, KEEP the
    /// world — in-flight bullets keep the delegates they spawned with; the
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
        let mut env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        for (pname, default) in &pat.params {
            let v = evaluate(default, &env, &mut ctx, &mut world)?;
            env = env.bind(pname.clone(), v);
        }
        let task = new_task(vec![TF::Seq { items: pat.body.clone(), idx: 0, env }]);
        Ok(Sim { world, tasks: vec![task], ctx })
    }

    pub fn tick(&self) -> u64 {
        self.world.tick
    }

    pub fn step(&mut self) -> Result<(), String> {
        self.step_with(&Inputs::default())
    }

    pub fn step_with(&mut self, inputs: &Inputs) -> Result<(), String> {
        let mut ch = (*self.ctx.sig.channels).clone();
        // host channels verbatim — passed by name (§3)
        for (k, v) in &inputs.vals {
            ch.insert(k.to_string(), v.clone());
        }
        // defaults for the conventional names when the host omits them
        // (once present — from host, default, or a previous tick — they stay)
        for (name, v) in [
            ("move-x", Val::Num(0.0)),
            ("move-y", Val::Num(0.0)),
            ("focus-firing", Val::Num(0.0)),
            ("bomb", Val::Num(0.0)),
            ("boss-hp", Val::Num(0.0)),
            ("player", Val::Vec2 { x: 0.0, y: -4.0 }),
            ("nearest-enemy", Val::Vec2 { x: 0.0, y: 3.0 }),
        ] {
            ch.entry(name.to_string()).or_insert(v);
        }
        // $player-k DERIVES from piloted rig entities keyed by the :pilot
        // column's VALUE; $player aliases pilot 1 (card-integrated movement
        // overrides the host mock). Per-pilot homing targets too.
        let mut pilots: Vec<(i64, (f64, f64))> = Vec::new();
        for b in &self.world.bullets {
            if !b.alive {
                continue;
            }
            if let Some(k) = b.col_get("pilot") {
                let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
                if let Ok(p) = dyn_pose(&b.motion, tau, &b.state, &self.ctx.sig) {
                    pilots.push((k as i64, (p.x, p.y)));
                }
            }
        }
        for (k, (x, y)) in &pilots {
            ch.insert(format!("player-{}", k), Val::Vec2 { x: *x, y: *y });
            if let Some((nx, ny)) = self.nearest("enemy", (*x, *y)) {
                ch.insert(format!("nearest-enemy-{}", k), Val::Vec2 { x: nx, y: ny });
            }
            if *k == 1 {
                ch.insert("player".into(), Val::Vec2 { x: *x, y: *y });
            }
        }
        let player_pos = match ch.get("player") {
            Some(Val::Vec2 { x, y }) => (*x, *y),
            _ => (0.0, -4.0),
        };
        // $nearest-enemy relative to $player (derived when :enemy entities
        // exist; the host-provided value is the mock fallback)
        if let Some((x, y)) = self.nearest("enemy", player_pos) {
            ch.insert("nearest-enemy".into(), Val::Vec2 { x, y });
        }
        // $nearest-pilot: nearest player entity to the boss anchor (for
        // boss aim in multi-pilot cards)
        if let Some((x, y)) = pilots
            .iter()
            .map(|(_, p)| *p)
            .min_by(|a, b| {
                let da = (a.0 - self.world.boss.x).powi(2) + (a.1 - self.world.boss.y).powi(2);
                let db = (b.0 - self.world.boss.x).powi(2) + (b.1 - self.world.boss.y).powi(2);
                da.partial_cmp(&db).unwrap()
            })
        {
            ch.insert("nearest-pilot".into(), Val::Vec2 { x, y });
        }
        // gameplay counters as signals
        ch.insert("graze".into(), Val::Num(self.world.graze as f64));
        ch.insert(
            "enemies".into(),
            Val::Num(
                self.world
                    .bullets
                    .iter()
                    .filter(|b| b.alive && b.team.as_deref() == Some("enemy"))
                    .count() as f64,
            ),
        );
        // lives: per pilot ($lives-k), plus $lives from the first
        // player-body (compat with pilotless mouse rigs)
        for b in &self.world.bullets {
            if !b.alive {
                continue;
            }
            if let (Some(k), Some(l)) = (b.col_get("pilot"), b.col_get("lives")) {
                ch.insert(format!("lives-{}", k as i64), Val::Num(l));
            }
        }
        if let Some(l) = self
            .world
            .bullets
            .iter()
            .find(|b| b.alive && b.team.as_deref() == Some("player-body"))
            .and_then(|b| b.col_get("lives"))
        {
            ch.insert("lives".into(), Val::Num(l));
        }
        // boss anchor (the move-action target — engine state, not an entity)
        ch.insert("boss".into(), Val::Vec2 { x: self.world.boss.x, y: self.world.boss.y });
        // :expose rules — entity columns published as channels; a dead or
        // absent entity reads 0, so hp gates fire (cards declare these:
        // {:expose {:hp :boss-hp}})
        for (chan, id, col) in &self.world.exposes {
            let v = self
                .world
                .bullets
                .iter()
                .find(|b| b.alive && b.id == *id)
                .and_then(|b| b.col_get(col))
                .unwrap_or(0.0);
            ch.insert(chan.to_string(), Val::Num(v));
        }
        // (export cell) — pattern cells published as read-only channels
        for (name, id) in self.ctx.sig.exports.borrow().iter() {
            if let Some((_, v)) = self.ctx.sig.cells.borrow().get(id) {
                ch.insert(name.clone(), v.clone());
            }
        }
        self.ctx.sig.channels = Rc::new(ch);
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
        // boss anchor animation (eased)
        if let Some(anim) = world_anim(&self.world) {
            let r = ((self.world.tick - anim.start) as f64 / anim.dur as f64).clamp(0.0, 1.0);
            let e = (r * std::f64::consts::FRAC_PI_2).sin(); // eoutsine
            self.world.boss = Pose {
                x: anim.from.x + e * (anim.to.0 - anim.from.x),
                y: anim.from.y + e * (anim.to.1 - anim.from.y),
                th: anim.from.th,
            };
            if r >= 1.0 {
                self.world.boss_anim = None;
            }
        }
        // integrate Scanned motion
        let dt = 1.0 / TICK_RATE;
        let tick = self.world.tick;
        let sig = self.ctx.sig.clone();
        for b in &mut self.world.bullets {
            if b.scanned {
                let tau = (tick - b.birth) as f64 / TICK_RATE;
                step_motion(&b.motion, tau, dt, &mut b.state, &sig)?;
            }
        }
        // record pather trails (the remembrance window, in ticks)
        {
            let tick = self.world.tick;
            let sig = self.ctx.sig.clone();
            for b in &mut self.world.bullets {
                if let Kind::Pather { window } = &b.kind {
                    let tau = (tick - b.birth) as f64 / TICK_RATE;
                    if let Ok(p) = dyn_pose(&b.motion, tau, &b.state, &sig) {
                        b.trail.push((p.x, p.y));
                        let cap = (window * TICK_RATE).ceil() as usize + 1;
                        if b.trail.len() > cap {
                            let drop = b.trail.len() - cap;
                            b.trail.drain(..drop);
                        }
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
        // cull: off-playfield points; lasers past their active window
        let tick = self.world.tick;
        let mut err = None;
        self.world.bullets.retain(|b| {
            if !b.alive {
                return false;
            }
            if b.team.as_deref() == Some("player-body") {
                return true; // the player rides a channel; never field-culled
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            match &b.kind {
                Kind::Point => match dyn_pose(&b.motion, tau, &b.state, &sig) {
                    Ok(p) => p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD,
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                },
                Kind::Laser { warn, active, .. } => tau <= warn + active,
                Kind::Pather { .. } => match dyn_pose(&b.motion, tau, &b.state, &sig) {
                    Ok(p) => p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD,
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                },
            }
        });
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Collision pass, detect-then-resolve over the layer matrix:
    ///   damage × player-hurtbox → hit;  graze × player-hurtbox → graze;
    ///   shot × hurt → damage resolution.
    /// CHECKS are per-pair-per-tick and shape-only (hot). CONTACTS are rare,
    /// so effect parameters may be card-defined pure functions — :damage as
    /// (fn [self other] n) is evaluated at contact with both entities (pos,
    /// contact velocity via finite difference, hp, team, family) in scope.
    /// Everything writes World, so the gameplay layer scrubs with the
    /// timeline; resolve order is canonical (bullet index) for determinism.
    /// Lasers derive capsule chains from their sampled curve while active.
    fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        const LASER_R: f64 = 0.08; // beam half-width for collision
        const IFRAMES: u64 = 60;

        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: world-space collider anchors + contact velocities
        let n = self.world.bullets.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        let mut vel: Vec<(f64, f64)> = Vec::with_capacity(n);
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        let mut scl: Vec<f64> = Vec::with_capacity(n);
        for b in &self.world.bullets {
            if !b.alive {
                pos.push(None);
                vel.push((0.0, 0.0));
                scl.push(1.0);
                continue;
            }
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_pose(&b.motion, tau, &b.state, &sig)?;
            pos.push(Some((p.x, p.y)));
            vel.push(match b.prev_pos {
                Some((ox, oy)) => ((p.x - ox) * TICK_RATE, (p.y - oy) * TICK_RATE),
                None => (0.0, 0.0),
            });
            scl.push(self.sample_sig(&b.sigs.scale, tau, 1.0));
        }
        for (b, p) in self.world.bullets.iter_mut().zip(pos.iter()) {
            b.prev_pos = *p;
        }

        // player hurtboxes: PlayerHurt colliders on host-mounted entities
        let hurts: Vec<(usize, f64, (f64, f64))> = self
            .world
            .bullets
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                b.team.as_deref() == Some("player-body") && pos[*i].is_some()
            })
            .flat_map(|(i, b)| {
                let at = pos[i].unwrap();
                b.colliders
                    .iter()
                    .filter(|c| c.layer == Layer::PlayerHurt)
                    .map(move |c| (i, c.r, at))
                    .collect::<Vec<_>>()
            })
            .collect();

        // squared distance from a target point to bullet i's collision
        // anchor: points measure center distance; active lasers measure
        // distance to the sampled beam (capsule chain)
        let target_d2 = |b: &Bullet, i: usize, to: (f64, f64)| -> Option<f64> {
            let (bx, by) = pos[i]?;
            match &b.kind {
                Kind::Point => Some((bx - to.0).powi(2) + (by - to.1).powi(2)),
                Kind::Laser { warn, width, .. } => {
                    let tau = (tick - b.birth) as f64 / TICK_RATE;
                    if tau < *warn {
                        return None; // warn phase: no hitbox yet
                    }
                    // slow lasers: only the swept-out prefix is hot
                    let pts = sample_laser_frac(b, tau, &sig, hot_frac(&b.kind, tau, &sig))?;
                    let d = dist_to_chain(to, &pts)?;
                    Some((d - LASER_R * width).max(0.0).powi(2))
                }
                // the trail IS the hitbox: a capsule chain over the
                // recorded window
                Kind::Pather { .. } => {
                    let d = dist_to_chain(to, &b.trail)?;
                    Some((d - LASER_R).max(0.0).powi(2))
                }
            }
        };

        // phase 1: detect — the big set (hostile colliders) tests only
        // against the player's few hurtboxes: O(bullets × few)
        let mut hit_contacts: Vec<(usize, usize)> = Vec::new(); // (bullet, player)
        let mut graze_contacts: Vec<usize> = Vec::new();
        for (i, b) in self.world.bullets.iter().enumerate() {
            if b.team.is_some() || pos[i].is_none() {
                continue;
            }
            for &(pj, pr, at) in &hurts {
                let Some(d2) = target_d2(b, i, at) else { continue };
                for c in b.colliders.iter() {
                    match c.layer {
                        Layer::Damage => {
                            let r = c.r * scl[i] + pr;
                            if d2 < r * r {
                                hit_contacts.push((i, pj));
                            }
                        }
                        Layer::Graze if !b.grazed => {
                            let r = c.r * scl[i] + pr;
                            if d2 < r * r {
                                graze_contacts.push(i);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        // shot × hurt (both sets are small)
        let mut shot_contacts: Vec<(usize, usize)> = Vec::new();
        for (i, shot) in self.world.bullets.iter().enumerate() {
            if pos[i].is_none() || shot.team.as_deref() != Some("player") {
                continue;
            }
            let Some(sc) = shot.colliders.iter().find(|c| c.layer == Layer::Shot) else {
                continue;
            };
            let (sx, sy) = pos[i].unwrap();
            'enemies: for (j, enemy) in self.world.bullets.iter().enumerate() {
                if pos[j].is_none() || enemy.team.as_deref() != Some("enemy") {
                    continue;
                }
                let (ex, ey) = pos[j].unwrap();
                for ec in enemy.colliders.iter() {
                    if ec.layer != Layer::Hurt {
                        continue;
                    }
                    let r = sc.r * scl[i] + ec.r * scl[j];
                    if (sx - ex).powi(2) + (sy - ey).powi(2) < r * r {
                        shot_contacts.push((i, j));
                        break 'enemies; // one shot, one enemy
                    }
                }
            }
        }

        // phase 2: resolve, canonical order. iframes are a PER-ENTITY
        // column (two pilots dodge independently), not a world global.
        for (i, pj) in hit_contacts {
            let until = self.world.bullets[pj].col_get("iframe-until").unwrap_or(0.0);
            if (tick as f64) < until {
                continue;
            }
            let b = &mut self.world.bullets[i];
            if !b.alive {
                continue;
            }
            if matches!(b.kind, Kind::Point) {
                b.alive = false; // beams persist through a hit
            }
            self.world.player_hits += 1;
            // the hit effect is column writes; game-over is the player
            // entity's trigger, not the contact's business
            let player = &mut self.world.bullets[pj];
            // the mercy window is per-entity DATA: an :iframes column
            // (seconds) overrides the engine default
            let window = player
                .col_get("iframes")
                .map(|s| (s * TICK_RATE) as u64)
                .unwrap_or(IFRAMES);
            player.col_set(&"iframe-until".into(), (tick + window) as f64);
            let lives = player.col_get("lives").unwrap_or(0.0);
            player.col_set(&"lives".into(), lives - 1.0);
            self.world.push_event(Event { tick, name: "player-hit".into(), pos: pos[i] });
        }
        for i in graze_contacts {
            let b = &mut self.world.bullets[i];
            if !b.alive || b.grazed {
                continue;
            }
            b.grazed = true;
            self.world.graze += 1;
            self.world.push_event(Event { tick, name: "graze".into(), pos: pos[i] });
        }
        for (i, j) in shot_contacts {
            if !self.world.bullets[i].alive || !self.world.bullets[j].alive {
                continue;
            }
            // resolve damage at contact: numbers pass through; a pure fn
            // gets (self other) contact maps
            let dmg_val = self.world.bullets[i].damage.clone();
            let dmg = match dmg_val {
                Val::Num(n) => n,
                f => {
                    let self_map = contact_map(&self.world.bullets[i], pos[i], vel[i]);
                    let other_map = contact_map(&self.world.bullets[j], pos[j], vel[j]);
                    apply_fn(f, &[self_map, other_map], &mut self.ctx, &mut self.world, false)?
                        .num()
                        .map_err(|e| format!("damage fn: {}", e))?
                }
            };
            self.world.bullets[i].alive = false;
            // invulnerability window: the shot still dies (absorbed), the
            // column write is skipped — same iframe-until both sides honor
            let until = self.world.bullets[j].col_get("iframe-until").unwrap_or(0.0);
            if (tick as f64) < until {
                self.world.push_event(Event { tick, name: "absorbed".into(), pos: pos[j] });
                continue;
            }
            // the effect is a COLUMN WRITE, nothing more — what zero hp
            // means is the enemy's trigger's business, not the contact's
            let enemy = &mut self.world.bullets[j];
            let hp = enemy.col_get("hp").unwrap_or(1.0);
            enemy.col_set(&"hp".into(), hp - dmg);
            self.world.push_event(Event { tick, name: "enemy-hit".into(), pos: pos[j] });
        }
        Ok(())
    }

    /// Standing triggers: per entity, per rule, when `col ≤ leq` first
    /// holds, fire (event + optional cull). The latch is a column, so it
    /// snapshots/scrubs; order is canonical (entity index, rule index).
    fn fire_triggers(&mut self) {
        let tick = self.world.tick;
        for i in 0..self.world.bullets.len() {
            let n_rules = self.world.bullets[i].triggers.len();
            for r in 0..n_rules {
                let b = &self.world.bullets[i];
                if !b.alive {
                    break;
                }
                let rule = &b.triggers[r];
                let armed = b.col_get(&rule.latch).is_none();
                let holds = b.col_get(&rule.col).map(|v| v <= rule.leq).unwrap_or(false);
                if !(armed && holds) {
                    continue;
                }
                let (latch, name, cull) = (rule.latch.clone(), rule.name.clone(), rule.cull);
                let name: Rc<str> = name;
                let at = self.world.bullets[i].prev_pos;
                let b = &mut self.world.bullets[i];
                b.col_set(&latch, 1.0);
                if cull {
                    b.alive = false;
                }
                self.world.push_event(Event { tick, name, pos: at });
            }
        }
    }

    /// Nearest alive entity with the given team tag, by position.
    fn nearest(&self, team: &str, to: (f64, f64)) -> Option<(f64, f64)> {
        let sig = &self.ctx.sig;
        let mut best: Option<((f64, f64), f64)> = None;
        for b in &self.world.bullets {
            if !b.alive || b.team.as_deref() != Some(team) {
                continue;
            }
            let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
            let Ok(p) = dyn_pose(&b.motion, tau, &b.state, sig) else { continue };
            let d2 = (p.x - to.0).powi(2) + (p.y - to.1).powi(2);
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some(((p.x, p.y), d2));
            }
        }
        best.map(|(p, _)| p)
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

    /// Read the retained event window without cloning it.
    pub fn with_events<R>(&self, f: impl FnOnce(&std::collections::VecDeque<Event>) -> R) -> R {
        f(&self.world.log.borrow().entries)
    }

    /// Retained events, cloned (tests, casual host use).
    pub fn events_vec(&self) -> Vec<Event> {
        self.world.log.borrow().entries.iter().cloned().collect()
    }

    /// After restoring this sim as a snapshot: drop shared-log events its
    /// timeline hasn't emitted yet (re-stepping re-emits them).
    pub fn rewind_events(&mut self) {
        self.world.log.borrow_mut().truncate_to(self.world.cursor);
    }

    /// Sample one render-signal tag at bullet-local t (default when absent).
    fn sample_sig(&self, s: &Option<MetaSig>, tau: f64, default: f64) -> f64 {
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

fn world_anim(w: &World) -> Option<BossAnim> {
    w.boss_anim
}

fn truthy_pub(v: &Val) -> bool {
    !matches!(v, Val::Bool(false) | Val::Nothing)
}

/// Ambient frame = composition of Frame entries on the task stack, rooted at
/// the boss anchor. Dyn-valued frames (unexpressed guides) resolve their pose
/// from whichever live bullet shares the node's scan state (§5 sharing).
fn ambient(stack: &[TF], world: &World, sig: &SigEnv) -> Pose {
    let mut p = world.boss;
    for tf in stack {
        if let TF::Frame(fs) = tf {
            match fs {
                FrameSpec::World => p = Pose::IDENTITY, // escape the caller anchor
                FrameSpec::Const(fp) => p = p.compose(fp),
                FrameSpec::Node(node) => p = p.compose(&resolve_node_pose(node, world, sig)),
            }
        }
    }
    p
}

fn resolve_node_pose(node: &Rc<DynNode>, world: &World, sig: &SigEnv) -> Pose {
    let key = Rc::as_ptr(node) as usize;
    for b in &world.bullets {
        if b.alive && b.state.contains_key(&key) {
            let tau = (world.tick - b.birth) as f64 / TICK_RATE;
            if let Ok(p) = dyn_pose(node, tau, &b.state, sig) {
                return p;
            }
        }
    }
    // no carrier yet (or stateless node): evaluate with empty state at t=0
    dyn_pose(node, 0.0, &MotionState::new(), sig).unwrap_or(Pose::IDENTITY)
}

/// Step one task until it blocks (wait) or completes. Returns true if done.
fn active_guards(task: &Task) -> Vec<(Form, Env)> {
    let mut gs = task.guards.clone();
    for tf in &task.stack {
        if let TF::Guard { pred, env } = tf {
            gs.push((pred.clone(), env.clone()));
        }
    }
    gs
}

fn step_task(
    task: &mut Task,
    ctx: &mut Ctx,
    world: &mut World,
    new_tasks: &mut Vec<Task>,
) -> Result<bool, String> {
    // structured cancellation: guards inherited at fork time kill the whole
    // task (the fork lives entirely inside the cancelled scope)…
    for (pred, env) in task.guards.clone() {
        if truthy_pub(&evaluate(&pred, &env, ctx, world)?) {
            return Ok(true);
        }
    }
    // …while a stack guard cancels its SCOPE: unwind to the guard frame and
    // resume the enclosing frame (so (seq (until p a) b) continues with b,
    // and a phase machine regains control to run finalizers). Outermost
    // fired scope wins.
    let mut fired: Option<usize> = None;
    for (i, tf) in task.stack.iter().enumerate() {
        if let TF::Guard { pred, env } = tf {
            if truthy_pub(&evaluate(pred, env, ctx, world)?) {
                fired = Some(i);
                break;
            }
        }
    }
    if let Some(i) = fired {
        task.stack.truncate(i);
        // any pending block was issued inside the cancelled scope
        task.wait = 0;
        task.wait_pred = None;
    }
    if task.wait > 0 {
        task.wait -= 1;
        if task.wait > 0 {
            return Ok(false);
        }
    }
    if let Some((pred, env)) = &task.wait_pred {
        let (pred, env) = (pred.clone(), env.clone());
        let v = evaluate(&pred, &env, ctx, world)?;
        if !truthy_pub(&v) {
            return Ok(false); // still parked (DMK whiletrue = pause)
        }
        task.wait_pred = None;
    }
    let mut fuel: u32 = 100_000;
    loop {
        fuel -= 1;
        if fuel == 0 {
            return Err("control-layer fuel exhausted this tick".into());
        }
        let Some(top) = task.stack.last_mut() else {
            return Ok(true);
        };
        let next: Option<(Form, Env)> = match top {
            TF::Guard { .. } => {
                task.stack.pop(); // body done: scope closes
                continue;
            }
            TF::Frame(_) => {
                task.stack.pop();
                continue;
            }
            TF::Seq { items, idx, env } => {
                if *idx >= items.len() {
                    task.stack.pop();
                    continue;
                }
                let f = items[*idx].clone();
                *idx += 1;
                Some((f, env.clone()))
            }
            TF::Dot { var, n, seq_binds, every, body, env, i, started } => {
                if *i >= *n {
                    task.stack.pop();
                    continue;
                }
                if *started && *every > 0 {
                    *started = false;
                    task.wait = *every;
                    return Ok(false);
                }
                let mut e = env.bind(var.clone(), Val::Num(*i));
                let idx_i = *i as i64;
                for (nm, src) in seq_binds.iter() {
                    let v = match src {
                        Val::Arr(items) if !items.is_empty() => {
                            items[(idx_i.rem_euclid(items.len() as i64)) as usize].clone()
                        }
                        other => other.clone(),
                    };
                    e = e.bind(nm.clone(), v);
                }
                *i += 1.0;
                *started = true;
                let body = body.clone();
                task.stack.push(TF::Seq { items: body, idx: 0, env: e });
                continue;
            }
            TF::Loop { names, body, env, cur, idx } => {
                if *idx >= body.len() {
                    task.stack.pop();
                    continue;
                }
                let mut e = env.clone();
                for (nm, v) in names.iter().zip(cur.iter()) {
                    e = e.bind(nm.clone(), v.clone());
                }
                let f = body[*idx].clone();
                *idx += 1;
                Some((f, e))
            }
            TF::Phases { clauses, env, cell, cur, stage } => {
                match *stage {
                    // enter the current state: clear any stale goto request,
                    // arm the goto guard over the body scope (forks under it
                    // inherit it), push the body
                    0 => {
                        if *cur >= clauses.len() {
                            task.stack.pop(); // fell off the end: complete
                            continue;
                        }
                        let c = clauses[*cur].clone();
                        let cell = *cell;
                        let benv = env.bind("#phase-cell".into(), Val::Num(cell as f64));
                        *stage = 1;
                        ctx.sig
                            .cells
                            .borrow_mut()
                            .insert(cell, ("#goto".to_string(), Val::Nothing));
                        let pred = Form::list(vec![
                            Form::sym("phase-goto?"),
                            Form::Num(cell as f64),
                        ]);
                        task.stack.push(TF::Guard { pred, env: benv.clone() });
                        task.stack.push(TF::Seq { items: c.body.clone(), idx: 0, env: benv });
                        continue;
                    }
                    // body exited (completed or goto'd): the finalizer runs
                    // OUTSIDE the state's guard, on every path
                    1 => {
                        let c = clauses[*cur].clone();
                        let cell = *cell;
                        *stage = 2;
                        if !c.finally.is_empty() {
                            let benv =
                                env.bind("#phase-cell".into(), Val::Num(cell as f64));
                            task.stack.push(TF::Seq {
                                items: c.finally.clone(),
                                idx: 0,
                                env: benv,
                            });
                        }
                        continue;
                    }
                    // route: a goto target wins; bare goto and body
                    // completion take the default successor (state order)
                    _ => {
                        let next = match ctx.sig.cells.borrow().get(cell) {
                            Some((_, Val::Kw(l))) => Some(l.clone()),
                            _ => None,
                        };
                        match next {
                            Some(l) => {
                                let Some(i) =
                                    clauses.iter().position(|c| c.label == l)
                                else {
                                    return Err(format!(
                                        "goto: no state :{} in this machine",
                                        l
                                    ));
                                };
                                *cur = i;
                            }
                            None => *cur += 1,
                        }
                        *stage = 0;
                        continue;
                    }
                }
            }
        };
        let Some((form, env)) = next else { continue };
        ctx.ambient = ambient(&task.stack, world, &ctx.sig.clone());
        let v = evaluate(&form, &env, ctx, world)?;
        if let Val::Action(a) = v {
            if run_action(&a, task, ctx, world, new_tasks)? {
                return Ok(false);
            }
        }
    }
}

/// Execute an evaluated action inside a task. Returns true if the task blocked.
fn run_action(
    a: &ActionV,
    task: &mut Task,
    ctx: &mut Ctx,
    world: &mut World,
    new_tasks: &mut Vec<Task>,
) -> Result<bool, String> {
    match a {
        ActionV::Nothing
        | ActionV::Event { .. }
        | ActionV::Cull { .. }
        | ActionV::CullHostile
        | ActionV::Export { .. }
        | ActionV::Remat { .. }
        | ActionV::SetCol { .. }
        | ActionV::Invuln { .. }
        | ActionV::SetStyle { .. }
        | ActionV::Manipulate { .. }
        | ActionV::Spawn { .. } => {
            ctx.ambient = ambient(&task.stack, world, &ctx.sig.clone());
            exec_instant(a, ctx, world)?;
            // forks issued inside the instant (callback timed work) are
            // adopted here, inheriting this task's guards
            for inner in std::mem::take(&mut ctx.deferred) {
                let mut child = new_task(Vec::new());
                child.guards = active_guards(task);
                run_action(&inner, &mut child, ctx, world, new_tasks)?;
                new_tasks.push(child);
            }
            Ok(false)
        }
        ActionV::Wait { ticks } => {
            task.wait = *ticks;
            Ok(*ticks > 0)
        }
        ActionV::WaitFor { pred, env } => {
            let v = evaluate(pred, env, ctx, world)?;
            if truthy_pub(&v) {
                Ok(false)
            } else {
                task.wait_pred = Some((pred.clone(), env.clone()));
                Ok(true)
            }
        }
        ActionV::DefVar { .. } | ActionV::SetVar { .. } => {
            exec_instant(a, ctx, world)?;
            Ok(false)
        }
        ActionV::Move { dur_ticks, dest } => {
            world.boss_anim = Some(BossAnim {
                from: world.boss,
                to: *dest,
                start: world.tick,
                dur: (*dur_ticks).max(1),
            });
            task.wait = *dur_ticks;
            Ok(*dur_ticks > 0)
        }
        ActionV::Seq { items, env } => {
            task.stack.push(TF::Seq { items: items.clone(), idx: 0, env: env.clone() });
            Ok(false)
        }
        ActionV::Let { binds, body, env } => {
            // action-valued bindings execute here, inside the ambient frame
            ctx.ambient = ambient(&task.stack, world, &ctx.sig.clone());
            let mut e = env.clone();
            for (name, v) in binds {
                let bound = match v {
                    Val::Action(a) => exec_instant(a, ctx, world)?,
                    other => other.clone(),
                };
                e = e.bind(name.clone(), bound);
            }
            task.stack.push(TF::Seq { items: body.clone(), idx: 0, env: e });
            Ok(false)
        }
        ActionV::InFrame { frame, inner } => {
            task.stack.push(TF::Frame(frame.clone()));
            run_action(inner, task, ctx, world, new_tasks)
        }
        ActionV::Dotimes { var, n, seq_binds, every_ticks, body, env } => {
            task.stack.push(TF::Dot {
                var: var.clone(),
                n: *n,
                seq_binds: seq_binds.clone(),
                every: *every_ticks,
                body: body.clone(),
                env: env.clone(),
                i: 0.0,
                started: false,
            });
            Ok(false)
        }
        ActionV::Loop { names, inits, body, env } => {
            task.stack.push(TF::Loop {
                names: names.clone(),
                body: body.clone(),
                env: env.clone(),
                cur: inits.clone(),
                idx: 0,
            });
            Ok(false)
        }
        ActionV::Recur(vals) => {
            while let Some(tf) = task.stack.last_mut() {
                if let TF::Loop { cur, idx, .. } = tf {
                    *cur = vals.clone();
                    *idx = 0;
                    return Ok(false);
                }
                task.stack.pop();
            }
            Err("recur outside loop".into())
        }
        ActionV::Fork(inner) => {
            // children keep the frame STACK (dyn frames stay live), not a snapshot
            let stack: Vec<TF> = task
                .stack
                .iter()
                .filter_map(|tf| match tf {
                    TF::Frame(f) => Some(TF::Frame(f.clone())),
                    _ => None,
                })
                .collect();
            let mut child = new_task(stack);
            child.guards = active_guards(task);
            run_action(inner, &mut child, ctx, world, new_tasks)?;
            new_tasks.push(child);
            Ok(false)
        }
        ActionV::Par(kids) => {
            for k in kids {
                let stack: Vec<TF> = task
                    .stack
                    .iter()
                    .filter_map(|tf| match tf {
                        TF::Frame(f) => Some(TF::Frame(f.clone())),
                        _ => None,
                    })
                    .collect();
                let mut child = new_task(stack);
                child.guards = active_guards(task);
                run_action(k, &mut child, ctx, world, new_tasks)?;
                new_tasks.push(child);
            }
            Ok(false)
        }
        ActionV::Until { pred, body, env } => {
            task.stack.push(TF::Guard { pred: pred.clone(), env: env.clone() });
            task.stack.push(TF::Seq { items: body.clone(), idx: 0, env: env.clone() });
            Ok(false)
        }
        ActionV::Phases { clauses, env } => {
            // allocate the goto-request cell from the world counter (ids
            // are deterministic, so requests snapshot and replay)
            let cell = world.next_id;
            world.next_id += 1;
            ctx.sig.cells.borrow_mut().insert(cell, ("#goto".to_string(), Val::Nothing));
            task.stack.push(TF::Phases {
                clauses: clauses.clone(),
                env: env.clone(),
                cell,
                cur: 0,
                stage: 0,
            });
            Ok(false)
        }
        ActionV::Goto { cell, .. } => {
            exec_instant(a, ctx, world)?; // file the request (first wins)
            // in the machine's own task the exit is immediate: unwind to the
            // machine frame — its finalizer + routing stages take over. From
            // a fork there's no frame to find; the write is enough (the
            // phase guard fires on the machine's next step, and this forked
            // task dies with the scope it inherited).
            let owns = task
                .stack
                .iter()
                .any(|tf| matches!(tf, TF::Phases { cell: c, .. } if c == cell));
            if owns {
                while let Some(tf) = task.stack.last() {
                    if matches!(tf, TF::Phases { cell: c, .. } if c == cell) {
                        break;
                    }
                    task.stack.pop();
                }
            }
            Ok(false)
        }
        ActionV::CallPattern { params, body, args, caller_cells, fresh_cells } => {
            // the §10 embedding adapter: fresh cells by default (isolated
            // defvar state per instance), the caller's for (inline …)
            let cells = if *fresh_cells {
                fresh_cell_scope()
            } else {
                caller_cells.clone().unwrap_or_else(fresh_cell_scope)
            };
            let mut env = Env::empty().bind(CELLS_KEY.into(), cells);
            for (i, (pname, default)) in params.iter().enumerate() {
                let v = match args.get(i) {
                    Some(v) => v.clone(),
                    None => evaluate(default, &env, ctx, world)?,
                };
                env = env.bind(pname.clone(), v);
            }
            task.stack.push(TF::Seq { items: body.clone(), idx: 0, env });
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Conformance: the real translation files, loaded verbatim from disk.
    #[test]
    fn translations_run() {
        let cases: &[(&str, &str, usize)] = &[
            ("../../cards/translations/130_bowap.dmk", "bowap", 300),
            ("../../cards/translations/130_bowap.dmk", "bowap-fold", 300),
            ("../../cards/translations/020_gsrepeat.dmk", "gsrepeat-demo", 300),
            ("../../cards/translations/040_spread.dmk", "spread-demo", 300),
            ("../../cards/translations/060_polar.dmk", "polar-demo", 300),
            ("../../cards/translations/080_aimed.dmk", "aimed-demo", 400),
            ("../../cards/translations/070_dynamic_lasers.dmk", "lasers-demo", 300),
            ("../../cards/translations/110_exploding_stars.dmk", "exploding-stars", 400),
            ("../../cards/translations/200_cradle.dmk", "cradle", 300),
            ("../../cards/translations/player_homing.dmk", "reimu-free-fire", 300),
            ("../../cards/translations/player_homing.dmk", "reimu-focus", 400),
            ("../../cards/translations/player_homing.dmk", "fantasy-seal", 700),
            ("../../cards/translations/ph_boss2_spell2.dmk", "spell-2", 900),
        ];
        for (path, pattern, ticks) in cases {
            let src = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{}: {}", path, e));
            let mut sim = Sim::load(&src, Some(pattern))
                .unwrap_or_else(|e| panic!("{} [{}]: {}", path, pattern, e));
            for _ in 0..*ticks {
                sim.step()
                    .unwrap_or_else(|e| panic!("{} [{}]: {}", path, pattern, e));
            }
            assert!(
                !sim.world.bullets.is_empty(),
                "{} [{}]: no bullets after {} ticks",
                path,
                pattern,
                ticks
            );
        }
    }

    const BOWAP: &str = r#"
(defpattern bowap [speed 4.0
                   arms  5
                   period (ticks 8)]
  ((pose c[0 2])
    (dotimes [i inf :every period]
      (spawn ((rot m"0.2*(i+1)*(i+2)")
               (circle arms (linear c[speed 0])))
             {:style {:family :gem :variant :w
                      :color [:yellow :orange :red :pink :purple]}}))))
"#;

    #[test]
    fn bowap_headless() {
        let mut sim = Sim::load(BOWAP, Some("bowap")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.bullets.len(), 15 * 5, "15 volleys × 5 arms");

        let sig = SigEnv::default();
        let b = &sim.world.bullets[0];
        assert_eq!(b.birth, 0);
        assert_eq!(b.style.family, "gem");
        assert_eq!(b.style.color, "yellow");
        let p = dyn_pose(&b.motion, 1.0, &b.state, &sig).unwrap();
        let ang = (0.4f64).to_radians();
        assert!((p.x - 4.0 * ang.cos()).abs() < 1e-9, "x: {}", p.x);
        assert!((p.y - (2.0 + 4.0 * ang.sin())).abs() < 1e-9, "y: {}", p.y);

        assert_eq!(sim.world.bullets[1].style.color, "orange");
        assert_eq!(sim.world.bullets[4].style.color, "purple");

        let b5 = &sim.world.bullets[5];
        assert_eq!(b5.birth, 8);
    }

    #[test]
    fn bowap_fold_version_matches() {
        const BOWAP_B: &str = r#"
(defpattern bowap-fold [speed 4.0
                        arms  5
                        period (ticks 8)]
  ((pose c[0 2])
    (loop [increment 0.4
           base      0.4]
      (spawn ((rot base)
               (circle arms (linear c[speed 0])))
             {:style {:family :gem :variant :w
                      :color [:yellow :orange :red :pink :purple]}})
      (wait period)
      (recur (+ increment 0.4)
             (+ base increment 0.4)))))
"#;
        let mut sa = Sim::load(BOWAP, Some("bowap")).unwrap();
        let mut sb = Sim::load(BOWAP_B, Some("bowap-fold")).unwrap();
        for _ in 0..240 {
            sa.step().unwrap();
            sb.step().unwrap();
        }
        assert_eq!(sa.world.bullets.len(), sb.world.bullets.len());
        let sig = SigEnv::default();
        for (a, b) in sa.world.bullets.iter().zip(sb.world.bullets.iter()) {
            assert_eq!(a.birth, b.birth);
            let pa = dyn_pose(&a.motion, 0.7, &a.state, &sig).unwrap();
            let pb = dyn_pose(&b.motion, 0.7, &b.state, &sig).unwrap();
            assert!(
                (pa.x - pb.x).abs() < 1e-6 && (pa.y - pb.y).abs() < 1e-6,
                "A/B diverged: {:?} vs {:?}",
                pa,
                pb
            );
        }
    }

    /// 110's mechanism end-to-end: let-bound spawn handles + scheduled
    /// manipulate with explode-and-cull.
    #[test]
    fn handles_and_manipulate() {
        const CARD: &str = r#"
(defpattern boom []
  ((pose c[0 1])
    (let [stars (spawn (circle 4 (linear c[1 0])) {:style {:family :lstar}})]
      (seq
        (wait 0.5)
        (manipulate (nth stars 0)
          (fn [b]
            (spawn (+ (pos b) (circle 8 (linear c[2 0])))
                   {:style {:family :star}})
            (cull b :soft)))))))
"#;
        let mut sim = Sim::load(CARD, Some("boom")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap();
        }
        let lstars = sim.world.bullets.iter().filter(|b| b.style.family == "lstar").count();
        let stars = sim.world.bullets.iter().filter(|b| b.style.family == "star").count();
        assert_eq!(lstars, 3, "one big star culled");
        assert_eq!(stars, 8, "explosion ring spawned");
        // ring anchored at the culled star's position at t≈0.5 (x = 0.5 from
        // anchor (0,1)); fn bodies drop the ambient frame, so no double anchor
        let sig = SigEnv::default();
        let ring: Vec<_> =
            sim.world.bullets.iter().filter(|b| b.style.family == "star").collect();
        let p = dyn_pose(&ring[0].motion, 0.0, &ring[0].state, &sig).unwrap();
        assert!((p.x - 0.5).abs() < 0.02 && (p.y - 1.0).abs() < 0.02, "ring anchor: {:?}", p);
    }

    /// Snapshot determinism: clone mid-run, step both with identical inputs,
    /// worlds stay identical (the scrubbing contract).
    #[test]
    fn snapshot_determinism() {
        let src = std::fs::read_to_string("../../cards/translations/ph_boss2_spell2.dmk").unwrap();
        let mut a = Sim::load(&src, Some("spell-2")).unwrap();
        for _ in 0..200 {
            a.step().unwrap();
        }
        let mut b = a.clone();
        let inputs = Inputs::classic((1.5, -3.0), (-2.0, 2.0));
        for _ in 0..300 {
            a.step_with(&inputs).unwrap();
            b.step_with(&inputs).unwrap();
        }
        assert_eq!(a.world.bullets.len(), b.world.bullets.len());
        let sig = SigEnv::default();
        for (x, y) in a.world.bullets.iter().zip(b.world.bullets.iter()) {
            assert_eq!(x.id, y.id);
            let tau = (a.world.tick - x.birth) as f64 / TICK_RATE;
            let px = dyn_pose(&x.motion, tau, &x.state, &sig).unwrap();
            let py = dyn_pose(&y.motion, tau, &y.state, &sig).unwrap();
            assert!(
                (px.x - py.x).abs() < 1e-12 && (px.y - py.y).abs() < 1e-12,
                "diverged: {:?} vs {:?}",
                px,
                py
            );
        }
    }

    /// Hostile fire hits the tiny player hitbox once, then iframes absorb
    /// the follow-up; the bullet that hit is culled.
    #[test]
    fn player_hit_and_iframes() {
        // two bullets aimed straight down the player's column, 10 ticks apart
        const CARD: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]
          :cols {:lives 3}
          :triggers [{:col :lives :leq 0 :event :game-over}]}))
(defpattern atk []
  (par (rig)
    (dotimes [i 2 :every (ticks 10)]
      (spawn (in-frame (pose c[0 3]) (vel c[0 -6]))))))
"#;
        let mut sim = Sim::load(CARD, Some("atk")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.world.player_hits, 1, "second bullet fell in iframes");
        let hits: Vec<_> =
            sim.events_vec().into_iter().filter(|e| &*e.name == "player-hit").collect();
        assert_eq!(hits.len(), 1);
        // the iframed bullet passed through (grazing) and is still flying
        assert_eq!(sim.world.bullets.iter().filter(|b| b.team.is_none()).count(), 1);
        assert_eq!(sim.world.graze, 2, "graze ring precedes the hitbox; iframes graze too");
        // the hit effect is a column write; $lives is a channel
        assert!(matches!(sim.channel_val("lives"), Some(Val::Num(n)) if n == 2.0));
    }

    /// A bullet passing beside the player grazes exactly once.
    #[test]
    fn graze_counts_once() {
        const CARD: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]}))
(defpattern g []
  (par (rig) (spawn (in-frame (pose c[0.25 3]) (vel c[0 -6])))))
"#;
        let mut sim = Sim::load(CARD, Some("g")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.world.player_hits, 0, "0.25 off-axis misses the 0.06 hitbox");
        assert_eq!(sim.world.graze, 1, "graze latches once per bullet");
        // and the counter is a channel patterns can read
        assert!(matches!(sim.channel_val("graze"), Some(Val::Num(n)) if n == 1.0));
    }

    /// Player fire decrements :hp; at zero the enemy dies with an event and
    /// the $enemies channel reflects it.
    #[test]
    fn enemy_hp_and_death() {
        const CARD: &str = r#"
(defpattern duel []
  (seq
    (spawn (pose c[0 2]) {:team :enemy :hp 2 :hitbox 0.3})
    (dotimes [i 3 :every (ticks 30)]
      (spawn (in-frame (pose c[0 0]) (vel c[0 4]))
             {:team :player :damage 1}))))
"#;
        let mut sim = Sim::load(CARD, Some("duel")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // shot 1 (fired tick 0, 4 u/s) reaches the enemy ring at ~tick 47
        for _ in 0..55 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "enemy-hit").count(), 1);
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 1.0));
        // shot 2 kills at ~tick 77; shot 3 flies through empty space
        for _ in 0..55 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.events_vec().iter().filter(|e| &*e.name == "died").count(), 1);
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 0.0));
    }

    /// The gameplay layer lives in World, so it scrubs: rewind to before a
    /// graze and the counter rewinds with it; re-step and it recurs.
    #[test]
    fn gameplay_scrubs() {
        use crate::session::Session;
        const CARD: &str = r#"
(defpattern g [] (spawn (in-frame (pose c[0.25 3]) (vel c[0 -6]))))
"#;
        let mut sess = Session::default();
        sess.rig = Some(
            "(defpattern rig [] (spawn (live $player) {:team :player-body \
             :colliders [{:layer :player-hurt :r 0.06}]}))"
                .into(),
        );
        sess.last_inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        sess.start(Sim::load(CARD, Some("g")).unwrap());
        for _ in 0..120 {
            sess.advance(CARD).unwrap();
        }
        assert_eq!(sess.sim.as_ref().unwrap().world.graze, 1);
        sess.seek(CARD, 10).unwrap();
        assert_eq!(sess.sim.as_ref().unwrap().world.graze, 0, "rewound past the graze");
        sess.seek(CARD, 120).unwrap();
        let sim = sess.sim.as_ref().unwrap();
        assert_eq!(sim.world.graze, 1, "replay re-grazes, not double-counts");
        assert_eq!(
            sim.events_vec().iter().filter(|e| &*e.name == "graze").count(),
            1,
            "the shared log was truncated at restore and re-populated"
        );
    }

    /// The player is an ordinary entity: lives is a column decremented by
    /// the hit effect; game-over is its (non-culling) trigger.
    #[test]
    fn lives_and_game_over() {
        const CARD: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]
          :cols {:lives 2}
          :triggers [{:col :lives :leq 0 :event :game-over}]}))
(defpattern atk []
  (par (rig)
    (dotimes [i 5 :every (ticks 70)]
      (spawn (in-frame (pose c[0 3]) (vel c[0 -6]))))))
"#;
        let mut sim = Sim::load(CARD, Some("atk")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..300 {
            sim.step_with(&inputs).unwrap();
        }
        // 70-tick cadence clears the 60-tick iframes: all 4 arrivals hit
        let count = |n: &str| sim.events_vec().iter().filter(|e| &*e.name == n).count();
        assert_eq!(count("player-hit"), 4);
        assert_eq!(count("game-over"), 1, "trigger edge-fires once at lives 0, latched");
        // the column keeps counting (what game-over MEANS is host policy)
        assert!(matches!(sim.channel_val("lives"), Some(Val::Num(n)) if n == -2.0));
        // non-culling: the player entity is still there (host decides)
        assert!(sim.world.bullets.iter().any(|b| b.team.as_deref() == Some("player-body")));
    }

    /// Death is not special: :triggers replaces the synthesized default,
    /// so an entity can gate a phase event at low hp and die at zero —
    /// same mechanism, two thresholds, each edge-fires exactly once.
    #[test]
    fn trigger_thresholds() {
        const CARD: &str = r#"
(defpattern gates []
  (seq
    (spawn (pose c[0 2])
           {:team :enemy :hp 3 :hitbox 0.3
            :triggers [{:col :hp :leq 1 :event :low-hp}
                       {:col :hp :leq 0 :event :died :cull true}]})
    (dotimes [i 3 :every (ticks 30)]
      (spawn (in-frame (pose c[0 0]) (vel c[0 4]))
             {:team :player :damage 1}))))
"#;
        let mut sim = Sim::load(CARD, Some("gates")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..200 {
            sim.step_with(&inputs).unwrap();
        }
        let count = |n: &str| sim.events_vec().iter().filter(|e| &*e.name == n).count();
        assert_eq!(count("enemy-hit"), 3, "every contact writes the column");
        assert_eq!(count("low-hp"), 1, "gate fired once at hp 1, latched");
        assert_eq!(count("died"), 1, "death is just the second threshold");
        assert!(matches!(sim.channel_val("enemies"), Some(Val::Num(n)) if n == 0.0));
    }

    /// :damage can be a pure function of both contact entities — here,
    /// damage = |contact velocity| one-shots a 3hp enemy at speed 4.
    #[test]
    fn damage_fn_at_contact() {
        const CARD: &str = r#"
(defpattern duel []
  (seq
    (spawn (pose c[0 2]) {:team :enemy :hp 3 :hitbox 0.3})
    (spawn (in-frame (pose c[0 0]) (vel c[0 4]))
           {:team :player :damage (fn [self other] (mag (:vel self)))})))
"#;
        let mut sim = Sim::load(CARD, Some("duel")).unwrap();
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(
            sim.events_vec().iter().filter(|e| &*e.name == "died").count(),
            1,
            "vel-magnitude damage (≈4) beats hp 3 in one contact"
        );
    }

    /// Active lasers collide as capsule chains sampled from the same curve
    /// the renderer draws; beams persist through a hit (no cull).
    #[test]
    fn laser_hitbox() {
        const CARD: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]}))
(defpattern beam []
  (par (rig) (spawn ((pose c[-2 0]) (laser {:warn 0.5 :active 2 :u-max 6})))))
"#;
        let mut sim = Sim::load(CARD, Some("beam")).unwrap();
        // player parked ON the beam line, 2 units along it
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // warn phase: no hitbox
        for _ in 0..50 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.world.player_hits, 0, "warn phase doesn't hit");
        for _ in 0..30 {
            sim.step_with(&inputs).unwrap();
        }
        assert_eq!(sim.world.player_hits, 1, "active beam hits");
        assert_eq!(
            sim.world.bullets.iter().filter(|b| b.team.is_none()).count(),
            1,
            "the beam persists through the hit"
        );
        assert_eq!(sim.world.graze, 1, "beam grazed on the way in");
    }

    /// The duel-card bug: aim inside an expression-level frame must aim
    /// FROM that frame's position (the frame is ambient for its body),
    /// not from the world origin. Player just below the source → bullets
    /// head down at the player, not up.
    /// Lifecycle trees: handles + per-bullet forked timelines express
    /// multi-stage lifecycles with no queries — (for [b handles] …)
    /// iterates an array in the lead binding.
    #[test]
    fn lifecycle_tree_via_handles() {
        const CARD: &str = r#"
(defpattern p []
  (let [ring (spawn (circle 4 (linear p[1.5 0]))
                    {:style {:family :circle}})]
    (for [b ring, i (iota 4)]
      (fork
        (seq
          (wait 0.5)
          (seq
            ((pose (pos b))
              (spawn (nth [(circle 6 (linear p[2 0]))
                           (fan 3 20 (linear p[2 0]))]
                          i)
                     {:style {:family (nth [:gem :star] i)}}))
            (cull b)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..90 {
            sim.step().unwrap();
        }
        let count = |f: &str| sim.world.bullets.iter().filter(|b| b.style.family == f).count();
        assert_eq!(count("circle"), 0, "stage-1 bullets consumed");
        assert_eq!(count("gem"), 12, "even indices: two 6-rings");
        assert_eq!(count("star"), 6, "odd indices: two 3-fans");
    }

    /// Invulnerability windows: (invuln b dur) writes iframe-until, which
    /// BOTH resolve paths honor — shots are absorbed (die, no hp write)
    /// while a boss is invulnerable, and hp flows again after expiry.
    #[test]
    fn invuln_window_absorbs_damage() {
        const CARD: &str = r#"
(defpattern p []
  (let [boss (spawn (pose c[0 3]) {:team :enemy :hp 10})]
    (seq
      (invuln (nth boss 0) 1)
      (fork
        (for [i inf :every (ticks 30)]
          ((pose c[0 0])
            (spawn (vel c[0 6]) {:team :player :damage 1})))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        // boss at y=3, shots at 6/s reach it in ~60 ticks; invuln covers
        // the first second (120 ticks) — early shots are absorbed
        for _ in 0..115 {
            sim.step().unwrap();
        }
        let hp = |sim: &Sim| {
            sim.world
                .bullets
                .iter()
                .find(|b| b.team.as_deref() == Some("enemy"))
                .and_then(|b| b.col_get("hp"))
                .unwrap()
        };
        assert_eq!(hp(&sim), 10.0, "shots absorbed during the window");
        assert!(
            sim.world.log.borrow().entries.iter().any(|e| &*e.name == "absorbed"),
            "absorption is observable"
        );
        for _ in 0..240 {
            sim.step().unwrap();
        }
        assert!(hp(&sim) < 10.0, "damage flows after the window expires");
    }

    /// Curves are values: (sample curve t u) evaluates a u-parameterized
    /// dyn without expressing an entity — pose plus tangent heading.
    #[test]
    fn sample_evaluates_curves() {
        const CARD: &str = r#"
(defpattern p []
  (let [curve (polar m"2 * u" 0)]
    (spawn ((pose (sample curve 0 1)) (pose c[0 0]))
           {:style {:family :gem}})))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let (x, y) = sim.world.bullets[0].prev_pos.unwrap_or((f64::NAN, f64::NAN));
        assert!((x - 2.0).abs() < 1e-6 && y.abs() < 1e-6, "point at u=1 on a straight radial curve: ({}, {})", x, y);
    }

    /// Slow lasers: the telegraph shows the whole path immediately, but
    /// the hitbox sweeps out from the source over the :fill window.
    #[test]
    fn slow_laser_fills() {
        const CARD: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]}))
(defpattern beam []
  (par (rig)
       (spawn ((pose c[-2 0])
                (laser {:warn 0.5 :active 6 :u-max 6 :fill 2}))
              {:style {:family :laser :color :red}})))
"#;
        let mut sim = Sim::load(CARD, Some("beam")).unwrap();
        // player parked on the beam line at u = 2 (x = 0); 120 ticks/s
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        // warn ends at 0.5s (tick 60); the front reaches u=2 at
        // tau = 0.5 + (2/6)*2 ≈ 1.17s (tick ~140, less the capsule radii)
        for _ in 0..100 {
            sim.step_with(&inputs).unwrap(); // t ≈ 0.83s: front at u ≈ 1.0
        }
        assert_eq!(sim.world.player_hits, 0, "front hasn't reached the player");
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap(); // t ≈ 1.33s: front at u = 2.5
        }
        assert_eq!(sim.world.player_hits, 1, "the sweeping front arrived");
        // full path is still telegraphed while filling: dim + bright polylines
        let mut sim2 = Sim::load(CARD, Some("beam")).unwrap();
        for _ in 0..90 {
            sim2.step_with(&inputs).unwrap(); // t = 0.75s: mid-fill
        }
        let polys: Vec<bool> = sim2
            .render()
            .iter()
            .filter_map(|r| match r {
                RenderItem::Polyline { active, .. } => Some(*active),
                _ => None,
            })
            .collect();
        assert_eq!(polys, vec![false, true], "dim full path + bright hot prefix");
    }

    #[test]
    fn aim_sees_expression_frame_ambient() {
        const CARD: &str = r#"
(defpattern nested []
  (spawn (in-frame (pose c[0 3]) ((aim $player) (linear p[2 0])))))
(defpattern flat []
  (spawn (in-frame (pose c[0 3]) (aim $player) (linear p[2 0]))))
"#;
        for pat in ["nested", "flat"] {
            let mut sim = Sim::load(CARD, Some(pat)).unwrap();
            // player below the source: pre-fix, aim measured from (0,0)
            // and fired UP toward (0,1); the bullet must head DOWN
            let inputs = Inputs::classic((0.0, 1.0), (0.0, 1.0));
            for _ in 0..60 {
                sim.step_with(&inputs).unwrap();
            }
            let b = &sim.world.bullets[0];
            let sig = SigEnv::default();
            let p = dyn_pose(&b.motion, 0.5, &b.state, &sig).unwrap();
            assert!(
                p.x.abs() < 1e-9 && (p.y - 2.0).abs() < 1e-9,
                "{}: fired from (0,3) toward the player below: {:?}",
                pat,
                p
            );
        }
    }

    /// (in-frame f1 f2 body) folds the frame monoid: same pose as nesting.
    #[test]
    fn in_frame_variadic() {
        const CARD: &str = r#"
(defpattern flat []
  (spawn (in-frame (pose c[0 1]) (rot 90) (linear c[1 0]))))
(defpattern nested []
  (spawn (in-frame (pose c[0 1]) (in-frame (rot 90) (linear c[1 0])))))
"#;
        let mut a = Sim::load(CARD, Some("flat")).unwrap();
        let mut b = Sim::load(CARD, Some("nested")).unwrap();
        for _ in 0..60 {
            a.step().unwrap();
            b.step().unwrap();
        }
        let sig = SigEnv::default();
        let pa = dyn_pose(&a.world.bullets[0].motion, 0.5, &a.world.bullets[0].state, &sig).unwrap();
        let pb = dyn_pose(&b.world.bullets[0].motion, 0.5, &b.world.bullets[0].state, &sig).unwrap();
        assert!((pa.x - pb.x).abs() < 1e-12 && (pa.y - pb.y).abs() < 1e-12);
        // rot 90 turns +x motion into +y, from anchor (0,1): at t=0.5 → (0, 1.5)
        assert!(pa.x.abs() < 1e-9 && (pa.y - 1.5).abs() < 1e-9, "got {:?}", pa);
    }

    /// The event log is bounded: old events prune once past the size
    /// threshold, keeping snapshot cost O(world), not O(elapsed time).
    #[test]
    fn event_log_bounded() {
        const CARD: &str = r#"
(defpattern chatty [] (dotimes [i inf :every (ticks 1)] (event :ping)))
"#;
        let mut sim = Sim::load(CARD, Some("chatty")).unwrap();
        for _ in 0..6000 {
            sim.step().unwrap();
        }
        let events = sim.events_vec();
        assert!(events.len() < 4200, "pruned: {}", events.len());
        let newest = events.last().unwrap().tick;
        assert!(newest >= 5990, "recent events kept");
    }

    /// (import "path") splices recursively, include-once: importing two
    /// files that both import a common base yields one copy of the base.
    #[test]
    fn imports_expand_once() {
        let dir = std::env::temp_dir().join("dmk-import-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.dmk"), "(def shared 7)\n").unwrap();
        std::fs::write(
            dir.join("a.dmk"),
            "(import \"base.dmk\")\n(defpattern a [] (spawn (pose c[shared 0])))\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.dmk"),
            "(import \"a.dmk\")\n(import \"base.dmk\") ; already included\n\
             (defpattern m [] (a))\n",
        )
        .unwrap();
        let src = crate::edn::expand_card(&dir.join("main.dmk")).unwrap();
        assert_eq!(src.matches("(def shared 7)").count(), 1, "include-once");
        let mut sim = Sim::load(&src, Some("m")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 1, "imported defs resolve through layers");
    }

    /// (until pred body): the tick the predicate holds, the body's whole
    /// task subtree — including forks — dies. §8 phase-end cancellation.
    #[test]
    fn until_cancels_subtree() {
        const CARD: &str = r#"
(defpattern u []
  (defvar stop 0)
  (par
    (until (= stop 1)
      (par (fork (dotimes [i inf :every (ticks 5)]
                   (spawn (linear c[0.01 0]))))
           (dotimes [j inf :every (ticks 5)]
             (spawn (linear c[0 0.01])))))
    (seq (wait (ticks 52)) (set! stop 1))))
"#;
        let mut sim = Sim::load(CARD, Some("u")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        let at_cancel = sim.world.bullets.len();
        assert!(at_cancel >= 20, "both spawners ran: {}", at_cancel);
        for _ in 0..200 {
            sim.step().unwrap();
        }
        assert_eq!(
            sim.world.bullets.len(),
            at_cancel,
            "cancelled subtree (loop AND its fork) spawns nothing more"
        );
    }

    /// (clamp lo hi dyn) clamps the INTEGRATOR state: pushing a wall banks
    /// no phantom distance — reversing moves away immediately.
    #[test]
    fn clamp_slides_not_banks() {
        const CARD: &str = r#"
(defpattern c []
  (spawn (clamp c[-2 -2] c[2 2]
           (in-frame c[0 -1] (vel c[(* 4 (live $move-x)) 0])))
         {:team :player-body :colliders [{:layer :player-hurt :r 0.05}]
          :cols {:pilot 1}}))
"#;
        let mut sim = Sim::load(CARD, Some("c")).unwrap();
        let mut inputs = Inputs::default();
        // push left 480 ticks (would travel 16 units unclamped)
        inputs.set_num("move-x", -1.0);
        for _ in 0..480 {
            sim.step_with(&inputs).unwrap();
        }
        let x_wall = match sim.channel_val("player") {
            Some(Val::Vec2 { x, .. }) => x,
            v => panic!("bad player channel: {:?}", v),
        };
        assert!((x_wall + 2.0).abs() < 0.05, "parked at the wall: {}", x_wall);
        // reverse for half a second: must move ~2 units immediately
        inputs.set_num("move-x", 1.0);
        for _ in 0..60 {
            sim.step_with(&inputs).unwrap();
        }
        let x_back = match sim.channel_val("player") {
            Some(Val::Vec2 { x, .. }) => x,
            _ => unreachable!(),
        };
        assert!(x_back > -0.2, "no banked phantom distance: {}", x_back);
    }

    /// Macros: unevaluated arguments, backtick templates, splicing; the
    /// expansion evaluates in the caller's scope.
    #[test]
    fn macros_expand() {
        const CARD: &str = r#"
(defmacro where [expr] `(fn [b] ~expr))
(defmacro ring-every [n dt body]
  `(for [vol inf :every ~dt] (spawn (circle ~n ~body))))
(defpattern p []
  (par
    (ring-every 6 0.5 (linear p[2 0]))
    (fork (for [i inf :every (ticks 5)]
      (manip {:where (where (> b.t 0.8))} (fn [b] (cull b)))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..200 {
            sim.step().unwrap();
        }
        // rings keep spawning; the where-sugar control ages them out
        let n = sim.world.bullets.len();
        assert!(n >= 6 && n <= 18, "steady state through macro sugar: {}", n);
    }

    /// Per-element columns (:cols arrays bind like style axes) and
    /// deferred forks (timed work scheduled from inside a callback).
    #[test]
    fn cols_per_element_and_deferred_fork() {
        const CARD: &str = r#"
(defpattern p []
  (seq
    (spawn (circle 4 (linear c[0.5 0]))
           {:style {:family :seed} :cols {:ci (iota 4)}})
    (wait (ticks 2))
    (manipulate {:family :seed :where (fn [b] (> b.ci 2.5))}
      (fn [b]
        (seq
          (fork (seq (wait (ticks 10))
                     (spawn (circle 6 (linear c[1 0]))
                            {:style {:family :burst}})))
          (cull b))))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        // only the seed with ci=3 matched the query and died
        assert_eq!(
            sim.world.bullets.iter().filter(|b| b.style.family == "seed").count(),
            3,
            "per-element column selected exactly one seed"
        );
        assert_eq!(sim.world.bullets.iter().filter(|b| b.style.family == "burst").count(), 0);
        for _ in 0..15 {
            sim.step().unwrap();
        }
        // the deferred fork's timed spawn landed after its wait
        assert_eq!(
            sim.world.bullets.iter().filter(|b| b.style.family == "burst").count(),
            6,
            "callback-forked timed work ran as an adopted task"
        );
    }

    /// Accessor sugar: dotted symbols are keyword chains (reader-level);
    /// they read handles (live bullet view), maps, and vectors; m-strings
    /// add postfix indexing with array gather.
    #[test]
    fn accessor_sugar() {
        const CARD: &str = r#"
(defpattern p []
  (seq
    (defvar probe 0)
    (export probe)
    (spawn (pose c[3 4]) {:style {:family :circle}})
    (manipulate {:family :circle :where (fn [b] (> b.pos.y 1))}
      (fn [b] (set! probe b.pos.y)))))
(defpattern gather []
  (spawn ((rot m"(30 * iota(12)).[iota(3)]") (linear c[1 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        for _ in 0..3 {
            sim.step().unwrap();
        }
        assert!(
            matches!(sim.channel_val("probe"), Some(Val::Num(n)) if n == 4.0),
            "handle field through :where and callback: {:?}",
            sim.channel_val("probe")
        );
        let mut sim = Sim::load(CARD, Some("gather")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 3, "m-string postfix array gather");
    }

    /// Nested meta arrays resolve structurally: depth = axis along the
    /// element's path, cycling per level, scalars broadcasting down.
    #[test]
    fn nested_meta_structural() {
        const CARD: &str = r#"
(defpattern p []
  (spawn ((rot m"30 * iota(10)")
           ((rot m"4 * iota(3)")
             ((pose c[1 0]) (linear p[2 0]))))
         {:style {:family :arrow
                  :color [[:red :blue] :green :purple]}}))
"#;
        let mut sim = Sim::load(CARD, Some("p")).unwrap();
        sim.step().unwrap();
        let col = |g: usize, i: usize| sim.world.bullets[g * 3 + i].style.color.clone();
        assert_eq!(
            (col(0, 0), col(0, 1), col(0, 2)),
            ("red".into(), "blue".into(), "red".into()),
            "nested element cycles the inner axis"
        );
        assert_eq!(col(1, 0), "green", "scalar element broadcasts its group");
        assert_eq!(col(2, 2), "purple");
        assert_eq!(col(3, 1), "blue", "outer level cycles over the groups");
    }

    /// Tutorial cards are doctests: every example pattern in every
    /// cards/tutorials/*.dmk must load and run (the docs can't rot).
    #[test]
    fn tutorial_cards_run() {
        let dir = std::path::Path::new("../../cards/tutorials");
        let mut swept = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("dmk") {
                continue;
            }
            let src = crate::edn::expand_card(&path).unwrap();
            let card = load_card(&read_all(&src).unwrap()).unwrap();
            for name in &card.order {
                let mut sim = Sim::load(&src, Some(name))
                    .unwrap_or_else(|e| panic!("{:?} [{}]: {}", path, name, e));
                for k in 0..240 {
                    sim.step().unwrap_or_else(|e| {
                        panic!("{:?} [{}] tick {}: {}", path, name, k, e)
                    });
                }
                assert!(
                    !sim.world.bullets.is_empty() || sim.world.cursor > 0,
                    "{:?} [{}]: example did nothing visible",
                    path,
                    name
                );
                swept += 1;
            }
        }
        assert!(swept >= 9, "tutorial patterns swept: {}", swept);
    }

    /// The §10 embedding adapters: pattern instances get ISOLATED cells by
    /// default (two embeddings of the same pattern don't share defvar
    /// state); (inline …) shares the caller's scope; defns called from a
    /// pattern see its cells dynamically (spell-2's guide-rig idiom).
    #[test]
    fn embedding_adapters() {
        const CARD: &str = r#"
(defn helper-reads [] (spawn (circle (live n) (linear c[1 0]))))
(defpattern counter [start 1]
  (seq
    (defvar n start)
    (set! n (+ (live n) 1))
    (helper-reads)))                      ; defn sees THIS instance's n
(defpattern outer []
  (seq
    (defvar n 100)
    (export n)
    (par (counter 1) (counter 5))         ; isolated: 2 and 6, not shared
    (wait (ticks 2))
    (inline (bump))))                     ; inline: mutates OUR n
(defpattern bump []
  (set! n 200))
"#;
        let mut sim = Sim::load(CARD, Some("outer")).unwrap();
        for _ in 0..5 {
            sim.step().unwrap();
        }
        // two counter instances spawned rings of 2 and 6 — isolated cells
        // (shared cells would give 2 and 3, or collide with outer's 100)
        let mut sizes: Vec<usize> = Vec::new();
        let counts = sim.world.bullets.len();
        assert_eq!(counts, 8, "2 + 6 bullets: {}", counts);
        sizes.push(counts);
        // inline (bump) wrote through to OUTER's exported cell
        assert!(
            matches!(sim.channel_val("n"), Some(Val::Num(v)) if v == 200.0),
            "inline shares the caller's cells: {:?}",
            sim.channel_val("n")
        );
    }

    /// :expose publishes an entity column as a channel (0 after death, so
    /// hp gates fire); (export cell) publishes a pattern cell read-only.
    #[test]
    fn expose_and_export() {
        const CARD: &str = r#"
(defpattern e []
  (seq
    (defvar phase 1)
    (export phase)
    (spawn (pose c[0 2])
           {:team :enemy :hp 2 :hitbox 0.3 :expose {:hp $target-hp}})
    (spawn (in-frame (pose c[0 0]) (vel c[0 4])) {:team :player :damage 1})
    (wait-for (<= $target-hp 1))
    (set! phase 2)
    (spawn (in-frame (pose c[0 0]) (vel c[0 4])) {:team :player :damage 1})))
"#;
        let mut sim = Sim::load(CARD, Some("e")).unwrap();
        for _ in 0..40 {
            sim.step().unwrap();
        }
        assert!(matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 2.0));
        assert!(matches!(sim.channel_val("phase"), Some(Val::Num(n)) if n == 1.0));
        for _ in 0..40 {
            sim.step().unwrap(); // first shot lands ~tick 47; second ~95
        }
        assert!(matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 1.0));
        assert!(
            matches!(sim.channel_val("phase"), Some(Val::Num(n)) if n == 2.0),
            "exported cell tracks the pattern's set!"
        );
        for _ in 0..220 {
            sim.step().unwrap(); // second shot kills; entity culled
        }
        assert!(
            matches!(sim.channel_val("target-hp"), Some(Val::Num(n)) if n == 0.0),
            "dead entity reads 0, not stale"
        );
    }

    /// Two pilots: distinct input channels move distinct rigs, channels
    /// derive per pilot-value, and iframes are per-entity — both pilots
    /// can be hit in the same window.
    #[test]
    fn two_players() {
        let rig = std::fs::read_to_string("../../cards/coop.dmk").unwrap();
        let mut sim = Sim::load(&rig, Some("coop")).unwrap();
        let mut inputs = Inputs::default();
        // p1 pushes right, p2 pushes left — they cross
        inputs.set_num("p1-move-x", 1.0);
        inputs.set_num("p1-move-y", 0.0);
        inputs.set_num("p2-move-x", -1.0);
        inputs.set_num("p2-move-y", 0.0);
        for _ in 0..120 {
            sim.step_with(&inputs).unwrap();
        }
        let p1 = match sim.channel_val("player-1") {
            Some(Val::Vec2 { x, .. }) => x,
            v => panic!("no $player-1: {:?}", v),
        };
        let p2 = match sim.channel_val("player-2") {
            Some(Val::Vec2 { x, .. }) => x,
            v => panic!("no $player-2: {:?}", v),
        };
        assert!(p1 > -1.5 && p2 < 1.5, "rigs moved on their own channels: {} {}", p1, p2);
        assert!(matches!(sim.channel_val("lives-1"), Some(Val::Num(n)) if n == 3.0));
        assert!(sim.channel_val("nearest-pilot").is_some());

        // per-entity iframes: park both pilots in the aimed spray column —
        // over time BOTH lose lives (a global iframe would shield one)
        let mut inputs = Inputs::default();
        inputs.set_num("p1-move-x", 0.35); // drift toward center
        inputs.set_num("p1-move-y", 0.0);
        inputs.set_num("p2-move-x", -0.35);
        inputs.set_num("p2-move-y", 0.0);
        let mut sim = Sim::load(&rig, Some("coop")).unwrap();
        for _ in 0..1400 {
            sim.step_with(&inputs).unwrap();
        }
        let l1 = match sim.channel_val("lives-1") { Some(Val::Num(n)) => n, _ => 99.0 };
        let l2 = match sim.channel_val("lives-2") { Some(Val::Num(n)) => n, _ => 99.0 };
        assert!(l1 < 3.0 && l2 < 3.0, "both pilots hit independently: {} {}", l1, l2);
    }

    /// The full-stack card: piloted rig (raw axes -> vel-domain movement),
    /// focus, bombs (raw button + control-layer stock), boss hp phases via
    /// triggers, spell-2 embedded. One scripted run hits every mechanism.
    #[test]
    fn reimu_vs_mima_plays() {
        // load_file resolves the card's imports (spell-2 + seal-orb come
        // from the translations)
        let mut sim = Sim::load_file(
            std::path::Path::new("../../cards/reimu_vs_mima.dmk"),
            Some("reimu-vs-mima"),
        )
        .unwrap();
        let mut inputs = Inputs::default();
        let mut saw_needles = false;
        for k in 0..4500u64 {
            // net-zero wiggle with the raw axes; bomb once; focus mid-fight
            inputs.set_num("move-x", if k % 200 < 100 { 0.6 } else { -0.6 });
            inputs.set_flag("bomb", (900..930).contains(&k));
            inputs.set_flag("focus-firing", (400..600).contains(&k));
            sim.step_with(&inputs).unwrap();
            if !saw_needles {
                saw_needles = sim
                    .world
                    .bullets
                    .iter()
                    .any(|b| b.team.as_deref() == Some("player") && b.style.family == "gem");
            }
        }
        assert!(saw_needles, "focus switched the fire mode to needles");
        let names: Vec<String> =
            sim.events_vec().iter().map(|e| e.name.to_string()).collect();
        let count = |n: &str| names.iter().filter(|x| x == &n).count();
        assert_eq!(count("spell"), 1, "non-spell broke into spell-2");
        assert_eq!(count("bomb"), 1, "one bomb consumed");
        assert_eq!(count("died"), 1, "boss down");
        // the piloted rig moved off its start: $player is entity-derived
        if let Some(Val::Vec2 { x, y }) = sim.channel_val("player") {
            assert!(x.abs() > 0.01 || (y + 3.0).abs() > 0.01, "rig integrated the axes");
        } else {
            panic!("no $player channel");
        }
        // field quiets after the kill (rig + parked guides only)
        assert!(sim.world.bullets.len() <= 6, "post-fight field: {}", sim.world.bullets.len());
    }

    /// The playable demo card exercises the whole gameplay layer at once:
    /// hostile spray hits/grazes, autofire kills drones.
    #[test]
    fn duel_card_plays() {
        let src = std::fs::read_to_string("../../cards/duel.dmk").unwrap();
        let rig = std::fs::read_to_string("../../cards/player-rig.dmk").unwrap();
        let mut sim = Sim::load(&src, Some("duel")).unwrap();
        // the host layers the stock rig; boss/stage cards stay player-free
        sim.add_forms(&src, &format!("{}\n(player-rig)", rig)).unwrap();
        let inputs = Inputs::classic((0.0, -2.0), (0.0, -2.0));
        for _ in 0..1200 {
            sim.step_with(&inputs).unwrap();
        }
        assert!(sim.world.player_hits > 0, "aimed spray reaches a stationary player");
        assert!(sim.world.graze > 0, "fan neighbors graze");
        assert!(
            sim.events_vec().iter().any(|e| &*e.name == "died"),
            "autofire kills drones"
        );
    }

    /// F20: $nearest-enemy derives from :team :enemy entities when present.
    #[test]
    fn derived_nearest_enemy() {
        const CARD: &str = r#"
(defpattern hunt []
  (seq
    (spawn (pose c[2 3]) {:style {:family :dummy} :team :enemy})
    (spawn (vel p[3 (slew 720 90 (angle-of (- (live $nearest-enemy) pos)))])
           {:style {:family :amulet}})))
"#;
        let mut sim = Sim::load(CARD, Some("hunt")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap(); // mock target defaults to (0, 3)
        }
        match sim.channel_val("nearest-enemy") {
            Some(Val::Vec2 { x, y }) => {
                assert!((x - 2.0).abs() < 1e-9 && (y - 3.0).abs() < 1e-9, "derived: {} {}", x, y);
            }
            v => panic!("bad channel: {:?}", v),
        }
        let sig = SigEnv::default();
        let b = sim.world.bullets.iter().find(|b| b.style.family == "amulet").unwrap();
        let tau = (sim.world.tick - b.birth) as f64 / TICK_RATE;
        let p = dyn_pose(&b.motion, tau, &b.state, &sig).unwrap();
        assert!(p.x > 0.3, "homed toward derived enemy: {:?}", p);
    }

    /// Generational hot-swap: bullets persist, program changes.
    #[test]
    fn swap_keeps_world() {
        const CARD: &str = r#"
(defpattern a [] (spawn (circle 6 (linear c[0.5 0]))))
"#;
        let mut sim = Sim::load(CARD, Some("a")).unwrap();
        for _ in 0..60 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.bullets.len(), 6);
        sim.swap_forms(CARD, "(spawn (circle 3 (linear c[0.2 0])))").unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 9, "old 6 keep flying + new 3");
        assert_eq!(sim.tick(), 61, "clock continues");
    }

    /// Layering starts on the ADD tick, not tick 0: a delayed add fires its
    /// pattern's timeline relative to when it was added.
    #[test]
    fn add_anchors_at_add_tick() {
        const CARD: &str = r#"
(defpattern a [] (dotimes [i inf :every (ticks 60)]
  (spawn (circle 2 (linear c[1 0])) {:style {:family :x}})))
(defpattern b [] (seq (wait (ticks 30))
  (spawn (circle 3 (linear c[1 0])) {:style {:family :y}})))
"#;
        let mut sim = Sim::load(CARD, Some("a")).unwrap();
        for _ in 0..100 {
            sim.step().unwrap();
        }
        sim.add_forms(CARD, "(b)").unwrap(); // added at tick 100
        for _ in 0..30 {
            sim.step().unwrap();
        }
        // b waits 30 ticks from ITS start: nothing through tick 129
        assert_eq!(sim.world.bullets.iter().filter(|b| b.style.family == "y").count(), 0);
        sim.step().unwrap(); // the step processing tick 130 = add(100) + 30
        let ys: Vec<_> =
            sim.world.bullets.iter().filter(|b| b.style.family == "y").collect();
        assert_eq!(ys.len(), 3);
        assert_eq!(ys[0].birth, 130, "b's clock anchored at the add tick");
        // a kept its own cadence meanwhile (volleys at ticks 0, 60, 120)
        assert_eq!(sim.world.bullets.iter().filter(|b| b.style.family == "x").count(), 6);
    }

    /// Patterns are callable: (par (a) (b)) plays two patterns in parallel.
    #[test]
    fn parallel_patterns() {
        const CARD: &str = r#"
(defpattern a [n 4] (spawn (circle n (linear c[1 0])) {:style {:family :x}}))
(defpattern b [] (seq (wait 0.1) (spawn (circle 3 (linear c[2 0])) {:style {:family :y}})))
"#;
        let mut sim = Sim::load_forms(CARD, "(par (a) (b))").unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let x = sim.world.bullets.iter().filter(|b| b.style.family == "x").count();
        let y = sim.world.bullets.iter().filter(|b| b.style.family == "y").count();
        assert_eq!((x, y), (4, 3), "both patterns ran in parallel");
    }

    /// Anonymous forms run with the card's defs in scope (the REPL path).
    #[test]
    fn load_forms_anonymous() {
        let card = r#"
(def spd 3.0)
(defpattern unused [] (spawn (circle 3 (linear c[1 0]))))
"#;
        let mut sim =
            Sim::load_forms(card, "(spawn (circle 8 (linear c[spd 0])))").unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 8);
        let mut sim2 = Sim::load_forms(
            card,
            "(defpattern ring [n 5] (spawn (circle n (linear c[spd 0]))))",
        )
        .unwrap();
        sim2.step().unwrap();
        assert_eq!(sim2.world.bullets.len(), 5);
    }

    /// F15 in the sim: 200's variant (axis 0, len 3) and color (axis 1 via
    /// explicit length 6) must bind to their axes, not the flat index.
    #[test]
    fn leading_axis_meta() {
        const CARD: &str = r#"
(defpattern axes []
  (spawn (map (fn [idx] ((rot m"15*idx") (circle 6 (linear c[1 0]))))
              (iota 3))
         {:style {:family :x
                  :variant [:b :c :w]
                  :color (nth [:blue :green :teal] (iota 6))}}))
"#;
        let mut sim = Sim::load(CARD, Some("axes")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 18);
        let b = |k: usize| &sim.world.bullets[k].style;
        assert_eq!(b(0).variant, "b");
        assert_eq!(b(6).variant, "c");
        assert_eq!(b(12).variant, "w");
        assert_eq!(b(0).color, "blue");
        assert_eq!(b(3).color, "blue"); // cycles within the ring axis
        assert_eq!(b(7).color, "green");
    }

    /// Render-affecting signal tags (§7): :scale/:facing/:opacity sampled at
    /// bullet-local t like :hue; :scale also multiplies collider radii.
    #[test]
    fn render_signal_tags() {
        const CARD: &str = r#"
(defpattern tags []
  (spawn (still)
         {:scale (+ 1 t) :opacity (- 1 (* 0.5 t)) :facing (* 90 t)}))
"#;
        let mut sim = Sim::load(CARD, Some("tags")).unwrap();
        for _ in 0..120 {
            sim.step().unwrap(); // t = 1s
        }
        let RenderItem::Dot { th, scale, alpha, .. } = &sim.render()[0] else {
            panic!("expected a dot");
        };
        assert!((scale - 2.0).abs() < 0.02, "scale(1s) = 2: {}", scale);
        assert!((alpha - 0.5).abs() < 0.02, "opacity(1s) = 0.5: {}", alpha);
        assert!((th - 90.0).abs() < 1.0, "facing(1s) = 90°: {}", th);

        // collision: a bullet whose base radius misses the player connects
        // once :scale grows the collider (constant-valued tags work too —
        // a constant is just a constant signal)
        const HIT: &str = r#"
(defpattern rig []
  (spawn (live $player)
         {:team :player-body :colliders [{:layer :player-hurt :r 0.06}]}))
(defpattern scaled [s 1]
  (par (rig)
       (spawn ((pose c[0.5 0]) (still))
              {:colliders [{:layer :damage :r 0.1}] :scale s})))
"#;
        let inputs = Inputs::classic((0.0, 0.0), (0.0, 0.0));
        let mut near = Sim::load_forms(HIT, "(scaled 1)").unwrap();
        for _ in 0..10 {
            near.step_with(&inputs).unwrap();
        }
        assert_eq!(near.world.player_hits, 0, "base radius misses at 0.5");
        let mut big = Sim::load_forms(HIT, "(scaled 6)").unwrap();
        for _ in 0..10 {
            big.step_with(&inputs).unwrap();
        }
        assert_eq!(big.world.player_hits, 1, "scaled collider connects");
    }

    /// §8 scope semantics under the guard-unwind rule: cancellation kills
    /// the scope, and the TASK CONTINUES after it — (seq (until p a) b)
    /// reaches b.
    #[test]
    fn until_cancels_scope_not_task() {
        const CARD: &str = r#"
(defpattern uc []
  (seq
    (defvar stop 0)
    (fork (seq (wait 0.1) (set! stop 1)))
    (until (> stop 0)
      (for [i inf :every (ticks 2)]
        (spawn (still))))
    (event :after-until)))
"#;
        let mut sim = Sim::load(CARD, Some("uc")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let n = sim.world.bullets.len();
        assert!((5..=8).contains(&n), "spawner ran ~0.1s then died: {}", n);
        assert!(
            sim.world.log.borrow().entries.iter().any(|e| &*e.name == "after-until"),
            "the task resumed after the cancelled scope"
        );
    }

    /// The phases FSM: routing goto skips states, a timeout expressed as
    /// body code (fork + wait + bare goto) ends a looping body, finalizers
    /// run on the way out, and fall-through completes the machine.
    #[test]
    fn phases_trampoline() {
        const CARD: &str = r#"
(defpattern m []
  (seq
    (phases
      (:opening (goto :b))
      (:a (spawn (circle 3 (still))))            ; skipped by the goto
      (:b
        (fork (seq (wait 0.05) (goto)))          ; timeout: exit to successor
        (for [i inf :every 1] (spawn (circle 5 (still))))
        (finally (event :b-done))))
    (event :machine-done)))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        sim.step().unwrap();
        assert_eq!(sim.world.bullets.len(), 5, ":opening routed straight to :b");
        for _ in 0..20 {
            sim.step().unwrap();
        }
        assert_eq!(sim.world.bullets.len(), 5, "the :b loop died at the timeout");
        let names: Vec<String> =
            sim.world.log.borrow().entries.iter().map(|e| e.name.to_string()).collect();
        let b_done = names.iter().position(|n| n == "b-done");
        let m_done = names.iter().position(|n| n == "machine-done");
        assert!(b_done.is_some(), "finalizer ran on timeout exit");
        assert!(m_done.is_some(), "falling off the end completed the machine");
        assert!(b_done < m_done, "finalizer before machine completion");
    }

    /// goto is a scoped exit: from a fork inside the state body it cancels
    /// the whole state scope — including a nested (until …) guard the body
    /// wrapped itself in — and re-enters at the target label.
    #[test]
    fn phases_goto_from_fork_and_until() {
        const CARD: &str = r#"
(defpattern m []
  (seq
    (defvar hp 10)
    (export hp)
    (phases
      (:spell
        (fork (seq (wait 0.05) (goto :post)))
        (until (<= $hp 0)                        ; the hp gate, as body code
          (for [i inf :every (ticks 2)] (spawn (still))))
        (finally (event :spell-out)))
      (:post (event :post)))))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        for _ in 0..30 {
            sim.step().unwrap();
        }
        let n = sim.world.bullets.len();
        assert!((3..=6).contains(&n), "spawner died at the goto: {}", n);
        let names: Vec<String> =
            sim.world.log.borrow().entries.iter().map(|e| e.name.to_string()).collect();
        assert!(names.iter().any(|n| n == "spell-out"), "finalizer ran on goto exit");
        assert!(names.iter().any(|n| n == "post"), "re-entered at the target label");
    }

    /// Labels are values: computed goto routing makes the machine a Markov
    /// chain (here over the deterministic world rng).
    #[test]
    fn phases_markov_routing() {
        const CARD: &str = r#"
(defpattern m []
  (phases
    (:a (event :in-a)
        (wait (ticks 4))
        (goto (nth [:a :b] (rand-int 0 2))))
    (:b (event :in-b)
        (wait (ticks 4))
        (goto (nth [:a :b] (rand-int 0 2))))))
"#;
        let mut sim = Sim::load(CARD, Some("m")).unwrap();
        for _ in 0..200 {
            sim.step().unwrap();
        }
        let names: Vec<String> =
            sim.world.log.borrow().entries.iter().map(|e| e.name.to_string()).collect();
        let a = names.iter().filter(|n| *n == "in-a").count();
        let b = names.iter().filter(|n| *n == "in-b").count();
        assert!(a + b >= 40, "the chain kept walking: {} + {}", a, b);
        assert!(a > 0 && b > 0, "both states visited: a={} b={}", a, b);
    }

    /// goto outside any phases machine is an error, and machines are
    /// lexically scoped: a pattern invoked from a phase body has no
    /// enclosing machine in ITS text, so its goto fails too.
    #[test]
    fn goto_scoping() {
        let mut sim =
            Sim::load_forms("(defpattern p [] (still))", "(goto :anywhere)").unwrap();
        assert!(sim.step().is_err(), "goto outside phases errors");

        const CARD: &str = r#"
(defpattern callee [] (goto :a))
(defpattern m []
  (phases
    (:a (callee))))
"#;
        let mut sim2 = Sim::load(CARD, Some("m")).unwrap();
        assert!(
            sim2.step().is_err(),
            "called patterns don't inherit the machine scope (goto is lexical)"
        );
    }
}
