//! The deterministic sim: fixed-tick scheduler over inert Action trees +
//! bullet/entity world. design.md §4: step(inputs) → events; render getters.

use crate::edn::{read_all, Form};
use crate::interp::*;
use std::rc::Rc;

const PLAYFIELD: f64 = 12.0; // cull margin (units)

pub enum RenderItem {
    Dot { x: f64, y: f64, th: f64, style: Style, hue: f64 },
    Polyline { pts: Vec<(f64, f64)>, style: Style, active: bool, hue: f64 },
}

/// One running task = a stack of resumable cursors over Action trees.
enum TF {
    Seq { items: Rc<[Form]>, idx: usize, env: Env },
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
}

struct Task {
    stack: Vec<TF>,
    wait: u64,
    wait_pred: Option<(Form, Env)>,
}

fn new_task(stack: Vec<TF>) -> Task {
    Task { stack, wait: 0, wait_pred: None }
}

pub struct Inputs {
    pub player: (f64, f64),
    pub nearest_enemy: (f64, f64),
}

impl Default for Inputs {
    fn default() -> Self {
        Inputs { player: (0.0, -4.0), nearest_enemy: (0.0, 3.0) }
    }
}

pub struct Sim {
    pub world: World,
    tasks: Vec<Task>,
    ctx: Ctx,
}

impl Sim {
    /// Load a card source and instantiate `pattern` (or the first defpattern).
    pub fn load(src: &str, pattern: Option<&str>) -> Result<Sim, String> {
        let forms = read_all(src).map_err(|e| e.to_string())?;
        let card = load_card(&forms)?;
        let name = match pattern {
            Some(n) => n.to_string(),
            None => card.order.first().cloned().ok_or("card has no defpattern")?,
        };
        let pat = card
            .patterns
            .get(&name)
            .ok_or_else(|| format!("no pattern '{}' in card", name))?;

        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(card.defs.clone());
        let mut world = World::default();
        let mut env = Env::empty();
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
        ch.insert("player".into(), Val::Vec2 { x: inputs.player.0, y: inputs.player.1 });
        ch.insert(
            "nearest-enemy".into(),
            Val::Vec2 { x: inputs.nearest_enemy.0, y: inputs.nearest_enemy.1 },
        );
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
        self.world.tick += 1;
        // cull: off-playfield points; lasers past their active window
        let tick = self.world.tick;
        let mut err = None;
        self.world.bullets.retain(|b| {
            if !b.alive {
                return false;
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
            }
        });
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn sample_hue(&self, b: &Bullet, tau: f64) -> f64 {
        let Some(h) = &b.hue else { return 0.0 };
        let env = h.env.bind("t".into(), Val::Num(tau));
        let mut ctx = Ctx { sig: self.ctx.sig.clone(), ambient: Pose::IDENTITY, scan: None };
        let mut w = World::default();
        match evaluate(&h.form, &env, &mut ctx, &mut w) {
            Ok(Val::Num(x)) => x,
            Ok(Val::Arr(items)) if !items.is_empty() => {
                items[h.idx % items.len()].num().unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }

    pub fn render(&self) -> Vec<RenderItem> {
        let sig = &self.ctx.sig;
        let mut out = Vec::new();
        for b in &self.world.bullets {
            if !b.alive {
                continue;
            }
            let tau = (self.world.tick - b.birth) as f64 / TICK_RATE;
            match &b.kind {
                Kind::Point => {
                    if let Ok(p) = dyn_pose(&b.motion, tau, &b.state, sig) {
                        out.push(RenderItem::Dot {
                            x: p.x,
                            y: p.y,
                            th: p.th,
                            style: b.style.clone(),
                            hue: self.sample_hue(b, tau),
                        });
                    }
                }
                Kind::Laser { shape, warn, active: _, u_max, u_max_sig, resolution } => {
                    let Ok(anchor) = dyn_pose(&b.motion, tau, &b.state, sig) else {
                        continue;
                    };
                    let u_max = match u_max_sig {
                        Some((f, e)) => eval_sig(f, e, sig, tau, 0.0, None, None)
                            .and_then(|v| v.num())
                            .unwrap_or(*u_max)
                            .max(0.01),
                        None => *u_max,
                    };
                    let steps = ((u_max / resolution).ceil() as usize).clamp(2, 400);
                    let mut pts = Vec::with_capacity(steps + 1);
                    let mut broke = false;
                    for k in 0..=steps {
                        let u = u_max * k as f64 / steps as f64;
                        let local = match shape {
                            Some(sh) => match dyn_pose_u(sh, tau, u, &b.state, sig) {
                                Ok(p) => p,
                                Err(_) => {
                                    broke = true;
                                    break;
                                }
                            },
                            None => Pose { x: u, y: 0.0, th: 0.0 }, // straight along +x
                        };
                        let w = anchor.compose(&local);
                        pts.push((w.x, w.y));
                    }
                    if !broke {
                        out.push(RenderItem::Polyline {
                            pts,
                            style: b.style.clone(),
                            active: tau >= *warn,
                            hue: self.sample_hue(b, tau),
                        });
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
            let f = match fs {
                FrameSpec::Const(fp) => *fp,
                FrameSpec::Node(node) => resolve_node_pose(node, world, sig),
            };
            p = p.compose(&f);
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
fn step_task(
    task: &mut Task,
    ctx: &mut Ctx,
    world: &mut World,
    new_tasks: &mut Vec<Task>,
) -> Result<bool, String> {
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
        | ActionV::Manipulate { .. }
        | ActionV::Spawn { .. } => {
            ctx.ambient = ambient(&task.stack, world, &ctx.sig.clone());
            exec_instant(a, ctx, world)?;
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
                run_action(k, &mut child, ctx, world, new_tasks)?;
                new_tasks.push(child);
            }
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
            ("../../translations/130_bowap.edn", "bowap", 300),
            ("../../translations/130_bowap.edn", "bowap-fold", 300),
            ("../../translations/020_gsrepeat.edn", "gsrepeat-demo", 300),
            ("../../translations/040_spread.edn", "spread-demo", 300),
            ("../../translations/060_polar.edn", "polar-demo", 300),
            ("../../translations/080_aimed.edn", "aimed-demo", 400),
            ("../../translations/070_dynamic_lasers.edn", "lasers-demo", 300),
            ("../../translations/110_exploding_stars.edn", "exploding-stars", 400),
            ("../../translations/200_cradle.edn", "cradle", 300),
            ("../../translations/player_homing.edn", "reimu-free-fire", 300),
            ("../../translations/player_homing.edn", "reimu-focus", 400),
            ("../../translations/player_homing.edn", "fantasy-seal", 700),
            ("../../translations/ph_boss2_spell2.edn", "spell-2", 900),
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
}
