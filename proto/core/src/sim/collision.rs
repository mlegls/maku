use super::*;
use super::slots::{eval_collider_slot, materialize_collider_defs_into, materialize_direct_colliders};

/// Per-pass classification of an all-Circle direct projector — the plain
/// bullet: per slot a layer plus a radius source with EntityCol reads
/// already resolved to store slots. Materializing a row against a plan is
/// exactly the direct path's arithmetic (radius eval in slot order, then
/// `radius * scale` around the sampled center) minus the projector walk,
/// closure, and per-read slot-cache scan.
#[derive(Clone)]
pub(super) struct FastColliderPlan {
    pub(super) slots: Vec<(Symbol, FastRadius)>,
}

#[derive(Clone, Copy)]
pub(super) enum FastRadius {
    Const(f64),
    Field(FieldSlots),
}

/// All projector values are circles with statically-readable radii: Circle
/// specs with Const/EntityCol numbers, or Stable circle slots with Const
/// dyn radii (the graze-ring case). Anything else — capsule chains,
/// Expr/AxisSel radii, Callable/Cond — takes the general path.
fn classify_fast_circles(projector: &ColliderProjector, world: &World) -> Option<FastColliderPlan> {
    let mut slots = Vec::with_capacity(projector.projectors.len());
    for value in projector.projectors.iter() {
        match &value.expr {
            ColliderProjectorExpr::Circle(spec) => {
                let radius = match &spec.radius {
                    ProjectorNum::Const(n) => FastRadius::Const(*n),
                    ProjectorNum::EntityCol(col) => FastRadius::Field(world.field_slots(col)),
                    ProjectorNum::Expr(_) => return None,
                };
                slots.push((spec.layer, radius));
            }
            ColliderProjectorExpr::Stable(stable) => {
                for dc in stable.iter() {
                    let slot = dc.slot();
                    match &slot.shape {
                        ColliderSlotShape::Circle { radius } => match radius.repr() {
                            NumDynRepr::Const(n) => slots.push((slot.layer, FastRadius::Const(*n))),
                            _ => return None,
                        },
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(FastColliderPlan { slots })
}

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
    slot_cache: &mut Vec<(*const u8, FieldSlots)>,
    plans: &mut crate::fxhash::FxHashMap<usize, Option<Rc<FastColliderPlan>>>,
    tick_rate: f64,
) -> Result<(), String> {
    let (trace, traced): (&[Pose], bool) = match row {
        Some(row) => (world.entities.trace_samples(row), world.entities.is_traced(row)),
        None => (&[], false),
    };
    // Direct projectors (the plain-bullet case: Stable slots plus circles/
    // capsule chains with Const/EntityCol numbers) evaluate by reference —
    // no per-tick DynCollider round-trip through the defs vec.
    if projector.is_direct() {
        return materialize_direct_colliders(
            dyn_figure, projector, tau, sig, scale, pose, world, row, trace, traced, out, slot_cache,
            tick_rate,
        )
        .map_err(|e| format!("colliders: {}", e));
    }
    let state = MotionState::default();
    defs.clear();
    materialize_collider_defs_into(projector, tau, &state, sig, e_view, ctx_view, world, row, defs, tick_rate)
        .map_err(|e| format!("colliders: {}", e))?;
    // the defs pass held &mut World: cached slot resolutions may be stale
    slot_cache.clear();
    plans.clear();
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
            let p = if let Some(p) = self.fast_pos_pose(i, tau, &sig) {
                p
            } else {
                let dyn_figure = self
                    .world
                    .entities
                    .dyn_figure(i)
                    .ok_or_else(|| format!("colliders: missing dyn figure for row {i}"))?;
                let readers = self.motion_readers(i);
                let state = MotionState::default();
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
            // fast path: all-Circle direct projector on an untraced point
            // figure — plan memoized per projector address for the pass
            {
                let projector = self
                    .world
                    .entities
                    .collider_projector(i)
                    .ok_or_else(|| format!("colliders: missing projector for row {i}"))?;
                let key = Rc::as_ptr(&projector.projectors) as *const u8 as usize;
                let world = &self.world;
                let plan = self
                    .collider_scratch
                    .plans
                    .entry(key)
                    .or_insert_with(|| classify_fast_circles(projector, world).map(Rc::new))
                    .clone();
                let untraced_point = !self.world.entities.is_traced(i)
                    && self
                        .world
                        .entities
                        .dyn_figure(i)
                        .is_some_and(|fig| matches!(fig.repr(), FigureDynRepr::Pose(_)));
                if let (Some(plan), true) = (plan, untraced_point) {
                    let start = self.collider_scratch.begin_row();
                    for (layer, radius) in plan.slots.iter() {
                        let r = match radius {
                            FastRadius::Const(n) => *n,
                            FastRadius::Field(slots) => entity_field_at_slots(i, *slots, &self.world)
                                .num()
                                .map_err(|e| format!("colliders: {}", e))?,
                        };
                        self.collider_scratch.rows.push(ColliderData::Circle {
                            layer: *layer,
                            center: (p.x, p.y),
                            radius: r * scale,
                        });
                    }
                    self.collider_scratch.finish_row(start);
                    continue;
                }
            }
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
                &mut self.collider_scratch.field_slots,
                &mut self.collider_scratch.plans,
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
            &mut self.collider_scratch.rows,
            &mut self.collider_scratch.ranges,
            eligible,
        );
        if let Some(f) = probe {
            crate::interp::profile::close("phase:collide-index", f);
        }
        Ok(())
    }
}
