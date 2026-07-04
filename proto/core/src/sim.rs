//! The deterministic sim: fixed-tick scheduler over inert Action trees +
//! bullet pool. design.md §4: step(inputs) → events; bulk render getters.

use crate::edn::{read_all, Form};
use crate::interp::*;
use std::rc::Rc;

const PLAYFIELD: f64 = 12.0; // cull margin (units)

pub struct Bullet {
    pub motion: Rc<DynNode>,
    pub birth: u64,
    pub style: Style,
    pub alive: bool,
}

pub struct RenderBullet<'a> {
    pub x: f64,
    pub y: f64,
    pub th: f64,
    pub style: &'a Style,
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
    Frame(Pose),
}

struct Task {
    stack: Vec<TF>,
    wait: u64,
}

pub struct Sim {
    pub tick: u64,
    tasks: Vec<Task>,
    pub bullets: Vec<Bullet>,
    pub events: Vec<(u64, String)>,
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

        let mut ctx = Ctx {};
        let mut env = Env::empty();
        for (pname, default) in &pat.params {
            let v = evaluate(default, &env, &mut ctx)?;
            env = env.bind(pname.clone(), v);
        }
        let task = Task {
            stack: vec![TF::Seq { items: pat.body.clone(), idx: 0, env }],
            wait: 0,
        };
        Ok(Sim { tick: 0, tasks: vec![task], bullets: Vec::new(), events: Vec::new(), ctx })
    }

    pub fn step(&mut self) -> Result<(), String> {
        // run control layer
        let mut i = 0;
        while i < self.tasks.len() {
            let mut task = std::mem::replace(
                &mut self.tasks[i],
                Task { stack: vec![], wait: 0 },
            );
            let mut new_tasks = Vec::new();
            let done = step_task(
                &mut task,
                self.tick,
                &mut self.bullets,
                &mut self.events,
                &mut new_tasks,
                &mut self.ctx,
            )?;
            if done {
                self.tasks.remove(i);
            } else {
                self.tasks[i] = task;
                i += 1;
            }
            self.tasks.extend(new_tasks);
        }
        self.tick += 1;
        // cull
        let tick = self.tick;
        self.bullets.retain(|b| {
            let tau = (tick - b.birth) as f64 / TICK_RATE;
            let p = dyn_pose(&b.motion, tau);
            b.alive && p.x.abs() <= PLAYFIELD && p.y.abs() <= PLAYFIELD
        });
        Ok(())
    }

    fn pose_of(&self, b: &Bullet, tick: u64) -> Pose {
        let tau = (tick - b.birth) as f64 / TICK_RATE;
        dyn_pose(&b.motion, tau)
    }

    pub fn render(&self) -> Vec<RenderBullet<'_>> {
        self.bullets
            .iter()
            .filter(|b| b.alive)
            .map(|b| {
                let p = self.pose_of(b, self.tick);
                RenderBullet { x: p.x, y: p.y, th: p.th, style: &b.style }
            })
            .collect()
    }
}

/// Ambient frame = composition of Frame entries on the task stack.
fn ambient(stack: &[TF]) -> Pose {
    let mut p = Pose::IDENTITY;
    for tf in stack {
        if let TF::Frame(f) = tf {
            p = p.compose(f);
        }
    }
    p
}

/// Step one task until it blocks (wait) or completes. Returns true if done.
fn step_task(
    task: &mut Task,
    tick: u64,
    bullets: &mut Vec<Bullet>,
    events: &mut Vec<(u64, String)>,
    new_tasks: &mut Vec<Task>,
    ctx: &mut Ctx,
) -> Result<bool, String> {
    if task.wait > 0 {
        task.wait -= 1;
        if task.wait > 0 {
            return Ok(false);
        }
        // wait reached zero: resume on this tick (wait n = resume n ticks later)
    }
    // fuel: a hostile card can slow the frame, not hang it (language.md §8)
    let mut fuel: u32 = 100_000;
    loop {
        fuel -= 1;
        if fuel == 0 {
            return Err("control-layer fuel exhausted this tick".into());
        }
        // Find the next form to run from the top of the stack.
        let Some(top) = task.stack.last_mut() else {
            return Ok(true); // task complete
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
                    // wait BETWEEN iterations (not after the last: checked above)
                    *started = false;
                    task.wait = *every;
                    // fallthrough: pause now, resume into the body next time
                    // (started=false means body comes next)
                    return Ok(false);
                }
                // enter body for iteration i
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
                    // fell off the end without recur: loop completes
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
        let v = evaluate(&form, &env, ctx)?;
        match v {
            Val::Action(a) => {
                if run_action(&a, task, tick, bullets, events, new_tasks)? {
                    return Ok(false); // blocked on wait
                }
            }
            _ => { /* non-action value in body position: discard */ }
        }
    }
}

/// Execute an evaluated action. Returns true if the task blocked.
fn run_action(
    a: &ActionV,
    task: &mut Task,
    tick: u64,
    bullets: &mut Vec<Bullet>,
    events: &mut Vec<(u64, String)>,
    new_tasks: &mut Vec<Task>,
) -> Result<bool, String> {
    match a {
        ActionV::Nothing => Ok(false),
        ActionV::Wait { ticks } => {
            task.wait = *ticks;
            Ok(*ticks > 0)
        }
        ActionV::Event { channel } => {
            events.push((tick, channel.to_string()));
            Ok(false)
        }
        ActionV::Spawn { dyns, styles } => {
            let amb = ambient(&task.stack);
            for (d, s) in dyns.iter().zip(styles.iter()) {
                let motion = if amb == Pose::IDENTITY {
                    d.clone()
                } else {
                    Rc::new(DynNode::Frame(Rc::new(DynNode::Const(amb)), d.clone()))
                };
                bullets.push(Bullet { motion, birth: tick, style: s.clone(), alive: true });
            }
            Ok(false)
        }
        ActionV::Seq { items, env } => {
            task.stack.push(TF::Seq { items: items.clone(), idx: 0, env: env.clone() });
            Ok(false)
        }
        ActionV::InFrame { frame, inner } => {
            task.stack.push(TF::Frame(*frame));
            run_action(inner, task, tick, bullets, events, new_tasks)
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
            // unwind to nearest Loop frame, rebind, restart
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
            // prototype: forked child inherits the ambient frame as a snapshot
            let amb = ambient(&task.stack);
            let mut stack = Vec::new();
            if amb != Pose::IDENTITY {
                stack.push(TF::Frame(amb));
            }
            let mut child = Task { stack, wait: 0 };
            run_action(inner, &mut child, tick, bullets, events, new_tasks)?;
            new_tasks.push(child);
            Ok(false)
        }
        ActionV::Par(kids) => {
            // prototype: all children become tasks; completion tracking later
            for k in kids {
                let amb = ambient(&task.stack);
                let mut stack = Vec::new();
                if amb != Pose::IDENTITY {
                    stack.push(TF::Frame(amb));
                }
                let mut child = Task { stack, wait: 0 };
                run_action(k, &mut child, tick, bullets, events, new_tasks)?;
                new_tasks.push(child);
            }
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BoWaP Version A, verbatim from translations/130_bowap.edn (code only).
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
        // run 1 second = 120 ticks; volleys at tick 0, 8, ..., 112 → 15 volleys
        for _ in 0..120 {
            sim.step().unwrap();
        }
        assert_eq!(sim.bullets.len(), 15 * 5, "15 volleys × 5 arms");

        // first volley, arm 0: base angle 0.4°, τ = 1s, speed 4, anchored at (0,2)
        let b = &sim.bullets[0];
        assert_eq!(b.birth, 0);
        assert_eq!(b.style.family, "gem");
        assert_eq!(b.style.color, "yellow");
        assert_eq!(b.style.variant, "w");
        let p = dyn_pose(&b.motion, 1.0);
        let ang = (0.4f64).to_radians();
        assert!((p.x - 4.0 * ang.cos()).abs() < 1e-9, "x: {}", p.x);
        assert!((p.y - (2.0 + 4.0 * ang.sin())).abs() < 1e-9, "y: {}", p.y);

        // arm colors cycle the 5-palette
        assert_eq!(sim.bullets[1].style.color, "orange");
        assert_eq!(sim.bullets[4].style.color, "purple");

        // second volley (i=1): base = 0.2*2*3 = 1.2°
        let b5 = &sim.bullets[5];
        assert_eq!(b5.birth, 8);
        let p5 = dyn_pose(&b5.motion, 0.0);
        assert!((p5.x - 0.0).abs() < 1e-9 && (p5.y - 2.0).abs() < 1e-9);
        let heading = dyn_pose(&b5.motion, 1.0);
        let ang5 = (1.2f64).to_radians();
        assert!((heading.x - 4.0 * ang5.cos()).abs() < 1e-9);
    }

    #[test]
    fn bowap_fold_version_matches() {
        // Version B (loop/recur fold) from the same translation file.
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
        assert_eq!(sa.bullets.len(), sb.bullets.len());
        // A and B are the same pattern: F3's telescoping claim, checked numerically
        for (a, b) in sa.bullets.iter().zip(sb.bullets.iter()) {
            assert_eq!(a.birth, b.birth);
            let (pa, pb) = (dyn_pose(&a.motion, 0.7), dyn_pose(&b.motion, 0.7));
            assert!(
                (pa.x - pb.x).abs() < 1e-6 && (pa.y - pb.y).abs() < 1e-6,
                "A/B diverged: {:?} vs {:?}",
                pa,
                pb
            );
        }
    }
}
