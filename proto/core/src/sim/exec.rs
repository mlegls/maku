use super::*;

#[derive(Clone)]
pub(super) enum TF {
    Seq { items: Rc<[Form]>, idx: usize, env: Env },
    /// (until pred ...) scope marker: while on the stack, the predicate
    /// cancels this task; forks under it inherit it as a task guard.
    Guard { pred: Form, env: Env },
    Loop {
        names: Vec<Rc<str>>,
        body: Rc<[Form]>,
        env: Env,
        cur: Vec<Val>,
        idx: usize,
    },
    Frame(FrameSpec),
    /// A running `states` machine: the trampoline over ordered states.
    /// stage: 0 = enter cur (arm the goto guard, push the body), 1 = body
    /// exited (completed/cancelled) → bump generation, 2 = route (goto
    /// target or state order) and loop to 0. Core `finally` cleanups run
    /// before the bump when a state body is wrapped by phases sugar.
    States {
        clauses: Rc<[StateClause]>,
        env: Env,
        /// goto-request cell (world-counter id, so it snapshots/replays).
        cell: u64,
        /// state-generation cell: bumped at every state EXIT, so guards
        /// (and the movesets forked under them) die when their state ends —
        /// however it ends — even after the request cell is cleared.
        gen_cell: u64,
        cur: usize,
        stage: u8,
    },
    /// Core `(finally body cleanup...)`: idx None while body is active,
    /// Some(k) while cleanup is being replayed after normal completion or
    /// protected cancellation.
    Finally { cleanup: Rc<[Form]>, env: Env, idx: Option<usize> },
}

#[derive(Clone)]
pub(super) struct Task {
    stack: Vec<TF>,
    wait: u64,
    wait_pred: Option<(Form, Env)>,
    /// Cancellation guards inherited from enclosing (until ...) scopes at
    /// fork time — structured cancellation: when a guard fires, this task
    /// and every task forked under the same scope die together.
    guards: Vec<(Form, Env)>,
}

pub(super) fn new_task(stack: Vec<TF>) -> Task {
    Task { stack, wait: 0, wait_pred: None, guards: Vec::new() }
}


fn ambient(stack: &[TF], world: &World, sig: &SigEnv) -> Pose {
    let mut p = Pose::IDENTITY;
    for tf in stack {
        if let TF::Frame(fs) = tf {
            match fs {
                FrameSpec::World => p = Pose::IDENTITY, // escape the caller anchor
                FrameSpec::Const(fp) => p = p.compose(fp),
                FrameSpec::Node(node) => p = p.compose(&resolve_node_pose(node, world, sig)),
                FrameSpec::Entity(h) => {
                    if let Some(i) = world.find(*h) {
                        if let Ok(ep) = crate::interp::entity_pose_at(i, world, sig) {
                            p = p.compose(&ep);
                        }
                    }
                }
            }
        }
    }
    p
}

fn resolve_node_pose(node: &Rc<DynNode>, world: &World, sig: &SigEnv) -> Pose {
    let key = Rc::as_ptr(node) as usize;
    for (i, _) in world.entities.iter().enumerate() {
        let carries_node = world
            .entities
            .motion_schema(i)
            .is_some_and(|schema| schema.node_ids.contains_key(&key));
        if world.entities.is_alive(i) && carries_node {
            let tau = world.entity_motion_tau(i, world.tick);
            let state = MotionState::default();
            let readers = crate::interp::entity_motion_readers(i, world);
            if let Ok(p) = dyn_node_pose_u_in(
                node,
                tau,
                0.0,
                MotionEvalCtx::with_tick_rate(&state, sig, &readers, world.tick_rate()),
            ) {
                return p;
            }
        }
    }
    // no carrier yet (or stateless node): evaluate with empty state at t=0
    dyn_node_pose(node, 0.0, &MotionState::default(), sig).unwrap_or(Pose::IDENTITY)
}

/// Bump a `states` machine's generation cell (state exit: everything
/// guarded on the old generation cancels).
fn bump_gen(gen_cell: u64, ctx: &mut Ctx) {
    let mut cells = ctx.sig.cells.borrow_mut();
    let g = match cells.get(&gen_cell) {
        Some((_, Val::Num(n))) => *n,
        _ => 0.0,
    };
    cells.insert(gen_cell, ("#state-gen".to_string(), Val::Num(g + 1.0)));
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

fn protected_finally(tf: &TF) -> Option<TF> {
    match tf {
        TF::Finally { cleanup, env, idx } => Some(TF::Finally {
            cleanup: cleanup.clone(),
            env: env.clone(),
            idx: Some(idx.unwrap_or(0)),
        }),
        _ => None,
    }
}

fn protected_finally_frames<'a>(frames: impl Iterator<Item = &'a TF>) -> Vec<TF> {
    frames.filter_map(protected_finally).collect()
}

pub(super) fn step_task(
    task: &mut Task,
    ctx: &mut Ctx,
    world: &mut World,
    new_tasks: &mut Vec<Task>,
) -> Result<bool, String> {
    // structured cancellation: guards inherited at fork time kill the whole
    // task (the fork lives entirely inside the cancelled scope)…
    for (pred, env) in task.guards.clone() {
        if truthy_pub(&evaluate(&pred, &env, ctx, world)?) {
            let cleanups = protected_finally_frames(task.stack.iter());
            if cleanups.is_empty() {
                return Ok(true);
            }
            task.stack = cleanups;
            task.guards.clear();
            task.wait = 0;
            task.wait_pred = None;
            break;
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
        let cleanups = protected_finally_frames(task.stack[i..].iter());
        task.stack.truncate(i);
        task.stack.extend(cleanups);
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
            TF::Finally { cleanup, env, idx } => match idx {
                None => {
                    *idx = Some(0);
                    continue;
                }
                Some(k) if *k >= cleanup.len() => {
                    task.stack.pop();
                    continue;
                }
                Some(k) => {
                    let f = cleanup[*k].clone();
                    *k += 1;
                    Some((f, env.clone()))
                }
            },
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
            TF::States { clauses, env, cell, gen_cell, cur, stage } => {
                match *stage {
                    // enter the current state: clear any stale goto request,
                    // arm the guard (goto requested OR generation moved on)
                    // over the body scope — forks under it inherit it —
                    // and push the body
                    0 => {
                        if *cur >= clauses.len() {
                            // fell off the end: complete — bump the
                            // generation so the last state's forks die too
                            bump_gen(*gen_cell, ctx);
                            task.stack.pop();
                            continue;
                        }
                        let c = clauses[*cur].clone();
                        let (cell, gen_cell) = (*cell, *gen_cell);
                        let benv = env.bind("#state-cell".into(), Val::Num(cell as f64));
                        *stage = 1;
                        let g = {
                            let mut cells = ctx.sig.cells.borrow_mut();
                            cells.insert(cell, ("#goto".to_string(), Val::Nothing));
                            match cells.get(&gen_cell) {
                                Some((_, Val::Num(n))) => *n,
                                _ => 0.0,
                            }
                        };
                        let pred = Form::list(vec![
                            Form::sym("state-end?"),
                            Form::Num(cell as f64),
                            Form::Num(gen_cell as f64),
                            Form::Num(g),
                        ]);
                        task.stack.push(TF::Guard { pred, env: benv.clone() });
                        task.stack.push(TF::Seq { items: c.body.clone(), idx: 0, env: benv });
                        continue;
                    }
                    // body exited (completed or goto'd): core `finally`
                    // cleanups have already run if the body had them. Now
                    // bump the generation so forks under the state die.
                    // This means cleanup precedes the generation bump; for
                    // instant cleanup it is still within the same tick.
                    1 => {
                        let gen_cell = *gen_cell;
                        *stage = 2;
                        bump_gen(gen_cell, ctx);
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
        | ActionV::Remat { .. }
        | ActionV::Render { .. }
        | ActionV::ChangeCol { .. }
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
        ActionV::SetStream { .. }
        | ActionV::BindStream { .. }
        | ActionV::ExportStream { .. } => {
            exec_instant(a, ctx, world)?;
            Ok(false)
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
        ActionV::Finally { body, cleanup, env } => {
            task.stack.push(TF::Finally {
                cleanup: cleanup.clone(),
                env: env.clone(),
                idx: None,
            });
            task.stack.push(TF::Seq {
                items: vec![body.clone()].into(),
                idx: 0,
                env: env.clone(),
            });
            Ok(false)
        }
        ActionV::Race { arms, env } => {
            let cell = world.next_id;
            world.next_id += 1;
            ctx.sig.cells.borrow_mut().insert(cell, ("#race".to_string(), Val::Nothing));
            let done = Form::list(vec![Form::sym("race-done?"), Form::Num(cell as f64)]);
            for arm in arms.iter() {
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
                child.guards.push((done.clone(), env.clone()));
                let won = Form::list(vec![Form::sym("race-won!"), Form::Num(cell as f64)]);
                child.stack.push(TF::Seq {
                    items: vec![arm.clone(), won].into(),
                    idx: 0,
                    env: env.clone(),
                });
                new_tasks.push(child);
            }
            task.wait_pred = Some((done, env.clone()));
            Ok(true)
        }
        ActionV::States { clauses, env } => {
            // allocate the goto-request and generation cells from the world
            // counter (ids are deterministic, so they snapshot and replay)
            let cell = world.next_id;
            let gen_cell = world.next_id + 1;
            world.next_id += 2;
            {
                let mut cells = ctx.sig.cells.borrow_mut();
                cells.insert(cell, ("#goto".to_string(), Val::Nothing));
                cells.insert(gen_cell, ("#state-gen".to_string(), Val::Num(0.0)));
            }
            task.stack.push(TF::States {
                clauses: clauses.clone(),
                env: env.clone(),
                cell,
                gen_cell,
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
                .any(|tf| matches!(tf, TF::States { cell: c, .. } if c == cell));
            if owns {
                let mut cleanups = Vec::new();
                while let Some(tf) = task.stack.last() {
                    if matches!(tf, TF::States { cell: c, .. } if c == cell) {
                        break;
                    }
                    if let Some(cleanup) = task.stack.pop().and_then(|tf| protected_finally(&tf)) {
                        cleanups.push(cleanup);
                    }
                }
                cleanups.reverse();
                task.stack.extend(cleanups);
            }
            Ok(false)
        }
        ActionV::CallPattern { params, body, args } => {
            let mut env = Env::empty();
            for (i, (pname, default)) in params.iter().enumerate() {
                let mut v = match args.get(i) {
                    Some(v) => v.clone(),
                    None => evaluate(default, &env, ctx, world)?,
                };
                // sigiled param with a defaulted (or plain-value) argument:
                // a fresh private stream initialized to it
                if pname.starts_with('$') && !matches!(v, Val::Stream(_)) {
                    let id = world.next_id;
                    world.next_id += 1;
                    ctx.sig.cells.borrow_mut().insert(id, (pname[1..].to_string(), v));
                    v = Val::Stream(id);
                }
                env = env.bind(pname.clone(), v);
            }
            task.stack.push(TF::Seq { items: body.clone(), idx: 0, env });
            Ok(false)
        }
    }
}
