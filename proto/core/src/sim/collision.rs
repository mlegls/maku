use super::*;
use super::slots::{eval_collider_slot, materialize_collider_defs_into};

fn materialize_colliders_into(
    dyn_figure: &DynFigure,
    projector: &ColliderProjector,
    tau: f64,
    sig: &SigEnv,
    e_view: Option<&Val>,
    ctx_view: Option<&Val>,
    scale: f64,
    pose: Pose,
    world: &mut World,
    row: Option<usize>,
    defs: &mut Vec<DynCollider>,
    out: &mut Vec<ColliderData>,
    tick_rate: f64,
) -> Result<(), String> {
    let (trace, traced): (&[Pose], bool) = match row {
        Some(row) => (world.entities.trace_samples(row), world.entities.is_traced(row)),
        None => (&[], false),
    };
    // All-Stable projectors (the plain-bullet case) evaluate their slots
    // by reference — no per-tick clone round-trip through the defs vec.
    if projector
        .projectors
        .iter()
        .all(|value| matches!(value.expr, ColliderProjectorExpr::Stable(_)))
    {
        for value in projector.projectors.iter() {
            let ColliderProjectorExpr::Stable(slots) = &value.expr else { unreachable!() };
            out.extend(slots.iter().map(|slot| {
                eval_collider_slot(dyn_figure, slot, tau, sig, scale, pose, trace, traced, tick_rate)
            }));
        }
        return Ok(());
    }
    let state = MotionState::new();
    defs.clear();
    materialize_collider_defs_into(projector, tau, &state, sig, e_view, ctx_view, world, row, defs, tick_rate)
        .map_err(|e| format!("colliders: {}", e))?;
    // re-borrow: the defs pass may have grown the symbol table
    let (trace, traced): (&[Pose], bool) = match row {
        Some(row) => (world.entities.trace_samples(row), world.entities.is_traced(row)),
        None => (&[], false),
    };
    out.extend(
        defs.drain(..)
            .map(|slot| eval_collider_slot(dyn_figure, &slot, tau, sig, scale, pose, trace, traced, tick_rate)),
    );
    Ok(())
}

impl Sim {
    /// Collision pass: materialize collider rows and record current-tick
    /// collision facts for `(collisions :a :b)` domain queries.
    pub(super) fn collide(&mut self, _inputs: &Inputs) -> Result<(), String> {
        let sig = self.ctx.sig.clone();
        let tick = self.world.tick;

        // phase 0: materialized collider data + contact velocities
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        let n = self.world.entities.len();
        let mut pos: Vec<Option<(f64, f64)>> = Vec::with_capacity(n);
        self.collider_scratch.clear_for_entities(n);
        // interned once: the per-entity read below must not re-hash the name
        let scale_sym = self.world.symbols.lookup("scale");
        // :scale multiplies collider radii (a scaled sprite scales its
        // hitbox); sampled once per bullet per tick, 1.0 when absent
        for i in 0..self.world.entities.len() {
            if !self.world.entities.is_alive(i) {
                self.world.entities.set_sampled_pose(i, tick, None);
                pos.push(None);
                self.collider_scratch.push_empty();
                continue;
            }
            let tau = self.world.entity_motion_tau(i, tick);
            let p = {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(i)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?;
                let readers = self.motion_readers(i);
                let state = MotionState::new();
                // pos_only: the sampled-pose cache and colliders read x/y
                // (velocity-from-samples, exit snapshots re-derive heading)
                dyn_figure_pose_in(
                    dyn_figure,
                    tau,
                    MotionEvalCtx::with_tick_rate(&state, &sig, &readers, self.world.tick_rate())
                        .pos_only(),
                )?
            };
            self.world.entities.set_sampled_pose(i, tick, Some(p));
            pos.push(Some((p.x, p.y)));
            let scale = scale_sym
                .and_then(|sym| self.world.col_get_sym_at(i, sym))
                .unwrap_or(1.0);
            let dyn_figure = self
                .world
                .entities
                .dyn_figure(i)
                .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?
                .clone();
            let tick_rate = self.world.tick_rate();
            let collider_projector = self
                .world
                .entities
                .collider_projector(i)
                .ok_or_else(|| format!("colliders: missing projector for row {i}"))?
                .clone();
            let start = self.collider_scratch.begin_row();
            let (e_view, ctx_view) = if collider_projector.needs_views() {
                (Some(entity_view(i, &self.world, &sig)?), Some(Val::Map(std::rc::Rc::new(vec![
                    (Val::Kw("age".into()), Val::Num(tau)),
                    (Val::Kw("t".into()), Val::Num(tau)),
                    (Val::Kw("tick".into()), Val::Num(tick as f64)),
                ]))))
            } else {
                (None, None)
            };
            materialize_colliders_into(
                &dyn_figure,
                &collider_projector,
                tau,
                &sig,
                e_view.as_ref(),
                ctx_view.as_ref(),
                scale,
                p,
                &mut self.world,
                Some(i),
                &mut self.collider_scratch.defs,
                &mut self.collider_scratch.rows,
                tick_rate,
            )?;
            self.collider_scratch.finish_row(start);
        }

        if let Some(f) = probe {
            crate::interp::profile::close("phase:collide-mat", f);
        }
        let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
        let eligible = (0..n)
            .map(|i| self.world.entities.is_alive(i) && pos[i].is_some())
            .collect::<Vec<_>>();
        self.world.collision_index.capture(
            std::mem::take(&mut self.collider_scratch.rows),
            std::mem::take(&mut self.collider_scratch.ranges),
            eligible,
        );
        if let Some(f) = probe {
            crate::interp::profile::close("phase:collide-index", f);
        }
        Ok(())
    }
}
