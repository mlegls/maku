//! Engine-facing special forms.
//!
//! These are not pure builtins: they need `World`, `Ctx`, entity handles,
//! rows, or action construction. Keeping them out of `builtins/` makes the
//! low-level engine API surface easier to audit separately from math,
//! language, array, and geometry intrinsics.

use super::*;

const NAMES: &[&str] = &[
    "matches",
    "manip",
    "remat",
    "change-col",
    "cull",
    "pos",
    "on-curve",
    "count-entities",
    "sum-entities",
    "entities-where",
    "collisions",
    "curve-samples",
    "emit",
    "entity-col",
    "nearest-entity",
    "export",
    "bind-channel!",
];

pub(crate) fn is_special(name: &str) -> bool {
    NAMES.contains(&name)
}

pub(crate) fn special(
    name: &str,
    items: &[Form],
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Option<Val>, String> {
    let val = match name {
        "matches" => sf_matches(items, env)?,
        "manip" => {
            let target = evaluate(&items[1], env, ctx, world)?;
            let callback = evaluate(&items[2], env, ctx, world)?;
            if is_query_value(&target) {
                Val::Action(Rc::new(ActionV::Manipulate {
                    targets: Vec::new(),
                    query: Some(target),
                    callback,
                }))
            } else {
                let mut targets = Vec::new();
                collect_handles(&target, &mut targets)?;
                Val::Action(Rc::new(ActionV::Manipulate {
                    targets,
                    query: None,
                    callback,
                }))
            }
        }
        "remat" => {
            let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                return Err("remat: expected bullet handle".into());
            };
            let spec = parse_remat_spec_form(&items[2], env, ctx, world)?;
            Val::Action(Rc::new(ActionV::Remat { target: id, spec }))
        }
        "change-col" => {
            let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                return Err("change-col: expected bullet handle".into());
            };
            let Val::Kw(col) = evaluate(&items[2], env, ctx, world)? else {
                return Err("change-col: expected keyword column name".into());
            };
            let f = evaluate(&items[3], env, ctx, world)?;
            if !matches!(f, Val::Fn { .. } | Val::Builtin(_)) {
                return Err(format!("change-col: expected function, got {:?}", f));
            }
            Val::Action(Rc::new(ActionV::ChangeCol {
                target: id,
                col: world.intern_col(col.as_ref()),
                f,
            }))
        }
        "cull" => {
            if items.len() == 1 {
                Val::Action(Rc::new(ActionV::CullHostile))
            } else {
                let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                    return Err("cull: expected bullet handle".into());
                };
                Val::Action(Rc::new(ActionV::Cull { target: id }))
            }
        }
        "pos" => {
            let target = evaluate(&items[1], env, ctx, world)?;
            entity_field_value(target, "pos", world, &ctx.sig)?
        }
        "on-curve" => {
            let Val::Handle(id) = evaluate(&items[1], env, ctx, world)? else {
                return Err("on-curve: expected curve entity handle".into());
            };
            let u = evaluate(&items[2], env, ctx, world)?.num()?;
            let Some(i) = world.find(id) else {
                return Ok(Some(Val::Pose(Pose::IDENTITY)));
            };
            let dyn_figure = world
                .entities
                .dyn_figure(i)
                .ok_or_else(|| format!("on-curve: missing dyn figure for row {i}"))?;
            let Some(curve) = dyn_figure.curve() else {
                return Err("on-curve: not a curve figure".into());
            };
            let tau = world.entity_motion_tau(i, world.tick);
            let readers = entity_motion_readers(i, world);
            let state = MotionState::default();
            let mctx = MotionEvalCtx::with_tick_rate(&state, &ctx.sig, &readers, world.tick_rate());
            let anchor = dyn_figure_pose_in(dyn_figure, tau, mctx)?;
            let at = |uu: f64| -> Result<Pose, String> {
                let local = eval_curve_pose_with_tick_rate(&curve.eval, tau, uu, &state, &ctx.sig, world.tick_rate())?;
                Ok(anchor.compose(&local))
            };
            let p0 = at(u)?;
            let p1 = at(u + 0.01)?;
            let th = (p1.y - p0.y).atan2(p1.x - p0.x).to_degrees();
            Val::Pose(Pose::oriented(p0.x, p0.y, th))
        }
        "count-entities" => {
            let q = evaluate(&items[1], env, ctx, world)?;
            let idxs = resolve_query(&q, ctx, world)?;
            Val::Num(idxs.len() as f64)
        }
        "sum-entities" => {
            let q = evaluate(&items[1], env, ctx, world)?;
            let Val::Kw(col) = evaluate(&items[2], env, ctx, world)? else {
                return Err("sum-entities: expected a keyword column".into());
            };
            let idxs = resolve_query(&q, ctx, world)?;
            let mut total = 0.0;
            for i in idxs {
                total += world.col_get_at(i, &col).unwrap_or(0.0);
            }
            Val::Num(total)
        }
        "entities-where" => {
            let q = evaluate(&items[1], env, ctx, world)?;
            let idxs = resolve_query(&q, ctx, world)?;
            Val::EntitySet(idxs.into())
        }
        "collisions" => {
            let Val::Kw(a) = evaluate(&items[1], env, ctx, world)? else {
                return Err("collisions: expected first layer keyword".into());
            };
            let Val::Kw(b) = evaluate(&items[2], env, ctx, world)? else {
                return Err("collisions: expected second layer keyword".into());
            };
            let a = world.symbols.intern(a.as_ref());
            let b = world.symbols.intern(b.as_ref());
            let pairs = world.collision_index.query(a, b);
            Val::CollisionSet(pairs)
        }
        "curve-samples" => {
            let entity = curve_samples_entity(evaluate(&items[1], env, ctx, world)?)?;
            let (u_max, resolution) = match items.get(2) {
                Some(form) => curve_samples_options(evaluate(form, env, ctx, world)?)?,
                None => (10.0, 0.1),
            };
            Val::CurveSamples(Rc::new(CurveSamples { entity, u_max, resolution }))
        }
        "emit" => {
            if items.len() != 3 {
                return Err("emit: expected (emit :render|:events row-map)".into());
            }
            let Val::Kw(stream) = evaluate(&items[1], env, ctx, world)? else {
                return Err("emit: expected stream keyword (:render or :events)".into());
            };
            match stream.as_ref() {
                "render" => {
                    let row = match render_row_from_literal_map(&items[2], env, ctx, world)? {
                        Some(row) => row,
                        None => {
                            let row = evaluate(&items[2], env, ctx, world)?;
                            render_row_from_value(row, world, &ctx.sig)?
                        }
                    };
                    Val::Action(Rc::new(ActionV::Render { row: Rc::new(row) }))
                }
                "events" => {
                    let row = evaluate(&items[2], env, ctx, world)?;
                    let (name, pos) = event_row_from_value(row, world)?;
                    Val::Action(Rc::new(ActionV::Event { name, pos }))
                }
                other => return Err(format!("emit: unknown stream :{} (known: :render, :events)", other)),
            }
        }
        "entity-col" => {
            let target = evaluate(&items[1], env, ctx, world)?;
            let Val::Kw(col) = evaluate(&items[2], env, ctx, world)? else {
                return Err("entity-col: expected a keyword column".into());
            };
            entity_col_value(target, &col, world)?
        }
        "nearest-entity" => {
            let q = evaluate(&items[1], env, ctx, world)?;
            let (tx, ty) = match evaluate(&items[2], env, ctx, world)? {
                Val::Pose(p) => (p.x, p.y),
                v => return Err(format!("nearest-entity: expected a point, got {:?}", v)),
            };
            let idxs = resolve_query(&q, ctx, world)?;
            let sig = ctx.sig.clone();
            let mut best: Option<(f64, (f64, f64))> = None;
            for i in idxs {
                let Some(dyn_figure) = world.entities.dyn_figure(i) else { continue };
                let tau = world.entity_motion_tau(i, world.tick);
                let readers = entity_motion_readers(i, world);
                let state = MotionState::default();
                let Ok(p) = dyn_figure_pose_in(
                    dyn_figure,
                    tau,
                    MotionEvalCtx::with_tick_rate(&state, &sig, &readers, world.tick_rate())
                        .pos_only(),
                ) else {
                    continue;
                };
                let d2 = (p.x - tx).powi(2) + (p.y - ty).powi(2);
                if best.map(|(bd, _)| d2 < bd).unwrap_or(true) {
                    best = Some((d2, (p.x, p.y)));
                }
            }
            match best {
                Some((_, (x, y))) => Val::Pose(Pose::point(x, y)),
                None => Val::Nothing,
            }
        }
        "export" => {
            let Form::Sym(name) = &items[1] else {
                return Err("export: expected a cell name".into());
            };
            let scope = cell_scope(env).ok_or("export: no cell scope")?;
            Val::Action(Rc::new(ActionV::Export { scope, name: name.clone() }))
        }
        "bind-channel!" => {
            let Some(Form::Sym(n)) = items.get(1) else {
                return Err("bind-channel!: expected a $channel name".into());
            };
            let Some(name) = n.strip_prefix('$') else {
                return Err("bind-channel!: name must start with $".into());
            };
            let Some(expr) = items.get(2) else {
                return Err(format!("bind-channel! ${}: expected an expression", name));
            };
            Val::Action(Rc::new(ActionV::BindChannel {
                name: Rc::from(name),
                expr: expr.clone(),
                env: env.clone(),
            }))
        }
        _ => return Ok(None),
    };
    Ok(Some(val))
}

fn parse_remat_spec(v: Val, world: &mut World) -> Result<RematSpec, String> {
    match v {
        Val::Map(kvs) => {
            if kvs.is_empty() {
                return Err("remat: expected non-empty spec map".into());
            }
            let mut spec = RematSpec { motion: None, fields: Vec::new() };
            for (k, v) in kvs.iter() {
                let Val::Kw(name) = k else {
                    return Err("remat: spec map keys must be keywords".into());
                };
                if name.as_ref() == "motion" {
                    validate_remat_motion(v)?;
                    spec.motion = Some(v.clone());
                } else {
                    validate_remat_field_value(v)?;
                    spec.fields.push((world.intern_col(name.as_ref()), v.clone()));
                }
            }
            Ok(spec)
        }
        other => {
            validate_remat_motion(&other)?;
            Ok(RematSpec { motion: Some(other), fields: Vec::new() })
        }
    }
}

fn parse_remat_spec_form(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<RematSpec, String> {
    let Form::Map(kvs) = form else {
        return parse_remat_spec(evaluate(form, env, ctx, world)?, world);
    };
    if kvs.is_empty() {
        return Err("remat: expected non-empty spec map".into());
    }
    let mut spec = RematSpec { motion: None, fields: Vec::new() };
    for (k, v) in kvs.iter() {
        let Val::Kw(name) = evaluate(k, env, ctx, world)? else {
            return Err("remat: spec map keys must be keywords".into());
        };
        let value = evaluate(v, env, ctx, world)?;
        if name.as_ref() == "motion" {
            validate_remat_motion(&value)?;
            spec.motion = Some(value);
        } else {
            validate_remat_field_value(&value)?;
            spec.fields.push((world.intern_col(name.as_ref()), value));
        }
    }
    Ok(spec)
}

fn validate_remat_motion(v: &Val) -> Result<(), String> {
    if matches!(v, Val::Fn { .. } | Val::Builtin(_)) {
        Ok(())
    } else {
        as_dyn_pose(v.clone()).map(|_| ())
    }
}

fn validate_remat_field_value(v: &Val) -> Result<(), String> {
    if matches!(v, Val::Num(_) | Val::Kw(_) | Val::Fn { .. } | Val::Builtin(_)) {
        Ok(())
    } else {
        Err(format!("remat: field value must be number, keyword, or function, got {:?}", v))
    }
}

fn curve_samples_entity(v: Val) -> Result<EntityRef, String> {
    match v {
        Val::Handle(id) => Ok(id),
        Val::EntityView(id) => Ok(id),
        Val::Map(kvs) => kvs
            .iter()
            .find_map(|(k, v)| match (k, v) {
                (Val::Kw(k), Val::Handle(id)) if &**k == "handle" => Some(*id),
                _ => None,
            })
            .ok_or_else(|| "curve-samples: expected entity handle or entity view".to_string()),
        other => Err(format!("curve-samples: expected entity handle or entity view, got {:?}", other)),
    }
}

fn curve_samples_options(v: Val) -> Result<(f64, f64), String> {
    let Val::Map(kvs) = v else {
        return Err("curve-samples: options must be a map".into());
    };
    let mut u_max = 10.0;
    let mut resolution = 0.1;
    for (k, v) in kvs.iter() {
        let Val::Kw(key) = k else {
            return Err("curve-samples: option keys must be keywords".into());
        };
        match key.as_ref() {
            "u-max" => u_max = v.num().map_err(|_| "curve-samples: :u-max must be a static number".to_string())?,
            "resolution" => {
                resolution = v.num().map_err(|_| "curve-samples: :resolution must be a static number".to_string())?
            }
            "warn" | "fill" | "fraction" | "frac" => {
                return Err(format!(
                    "curve-samples: :{} is lifecycle policy; put lifecycle logic in rule code over entity fields",
                    key
                ));
            }
            other => return Err(format!("curve-samples: unknown option :{}", other)),
        }
    }
    Ok((u_max, resolution))
}

#[derive(Default)]
pub(crate) struct RenderRowFields {
    shape: Option<Val>,
    x: Option<Val>,
    y: Option<Val>,
    theta: Option<Val>,
    facing: Option<Val>,
    scale: Option<Val>,
    alpha: Option<Val>,
    opacity: Option<Val>,
    hue: Option<Val>,
    points: Option<Val>,
    pts: Option<Val>,
    active: Option<Val>,
    extras: Vec<(Rc<str>, Val)>,
}

/// A render map key's slot, resolved from its name. Compiled tick rules
/// resolve keys once at lowering so the per-row push skips the name match.
#[derive(Clone, Copy)]
pub(crate) enum RenderKey {
    Shape,
    X,
    Y,
    Theta,
    Facing,
    Scale,
    Alpha,
    Opacity,
    Hue,
    Points,
    Pts,
    Active,
    Extra,
}

impl RenderKey {
    pub(crate) fn from_name(name: &str) -> RenderKey {
        match name {
            "shape" => RenderKey::Shape,
            "x" => RenderKey::X,
            "y" => RenderKey::Y,
            "theta" => RenderKey::Theta,
            "facing" => RenderKey::Facing,
            "scale" => RenderKey::Scale,
            "alpha" => RenderKey::Alpha,
            "opacity" => RenderKey::Opacity,
            "hue" => RenderKey::Hue,
            "points" => RenderKey::Points,
            "pts" => RenderKey::Pts,
            "active" => RenderKey::Active,
            _ => RenderKey::Extra,
        }
    }
}

impl RenderRowFields {
    pub(crate) fn push_kw(&mut self, key: Rc<str>, value: Val) {
        self.push_slot(RenderKey::from_name(&key), &key, value);
    }

    /// `key` is only cloned for extras; named slots drop it.
    pub(crate) fn push_slot(&mut self, slot: RenderKey, key: &Rc<str>, value: Val) {
        match slot {
            RenderKey::Shape => set_first(&mut self.shape, value),
            RenderKey::X => set_first(&mut self.x, value),
            RenderKey::Y => set_first(&mut self.y, value),
            RenderKey::Theta => set_first(&mut self.theta, value),
            RenderKey::Facing => set_first(&mut self.facing, value),
            RenderKey::Scale => set_first(&mut self.scale, value),
            RenderKey::Alpha => set_first(&mut self.alpha, value),
            RenderKey::Opacity => set_first(&mut self.opacity, value),
            RenderKey::Hue => set_first(&mut self.hue, value),
            RenderKey::Points => set_first(&mut self.points, value),
            RenderKey::Pts => set_first(&mut self.pts, value),
            RenderKey::Active => set_first(&mut self.active, value),
            RenderKey::Extra => self.extras.push((key.clone(), value)),
        }
    }

    pub(crate) fn finish(self, world: &mut World, sig: &SigEnv) -> Result<RenderRow, String> {
        self.finish_checked(world, sig, None)
    }

    /// `checked` memoizes (key, kind) pairs already accepted by
    /// `render_field_check` within one compiled-rule pass: the schema only
    /// accretes and no other rule runs between the pass's rows, so a pair
    /// that passed once cannot conflict later in the same pass.
    pub(crate) fn finish_checked(
        self,
        world: &mut World,
        sig: &SigEnv,
        checked: Option<&mut Vec<(Rc<str>, RenderFieldKind)>>,
    ) -> Result<RenderRow, String> {
        let shape_value = self.shape.ok_or("render: missing :shape")?;
        let shape = match &shape_value {
            Val::Kw(k) => k.clone(),
            Val::Arr(items) => match items.first() {
                Some(Val::Kw(k)) => k.clone(),
                _ => return Err("render: :shape vector must start with a keyword".into()),
            },
            Val::CurveSamples(_) => "polyline".into(),
            _ => return Err("render: missing keyword :shape".into()),
        };
        let data = match &*shape {
            "point" | "dot" => RenderData::Point {
                x: self.x.map(|v| v.num()).transpose()?.unwrap_or(0.0),
                y: self.y.map(|v| v.num()).transpose()?.unwrap_or(0.0),
                theta: self.theta.or(self.facing).map(|v| v.num()).transpose()?.unwrap_or(0.0),
                scale: self.scale.map(|v| v.num()).transpose()?.unwrap_or(1.0),
                alpha: self.alpha.or(self.opacity).map(|v| v.num()).transpose()?.unwrap_or(1.0),
                hue: self.hue.map(|v| v.num()).transpose()?.unwrap_or(0.0),
            },
            "polyline" => {
                let points = match &shape_value {
                    Val::CurveSamples(samples) => sample_curve_shape(samples, world, sig)?,
                    _ => match self.points.or(self.pts) {
                        Some(Val::Arr(items)) => items
                            .iter()
                            .cloned()
                            .map(render_point_xy)
                            .collect::<Result<Vec<_>, _>>()?,
                        Some(v) => return Err(format!("render: :points must be an array, got {:?}", v)),
                        None => return Err("render: polyline missing :points".into()),
                    },
                };
                let active = self.active.map(|v| v.num()).transpose()?.unwrap_or(1.0) != 0.0;
                RenderData::Polyline { points, active }
            }
            other => return Err(format!("render: unsupported shape :{}", other)),
        };
        let mut row = RenderRow::plain(data);
        let mut checked = checked;
        let mut check = |world: &mut World, key: &Rc<str>, kind: RenderFieldKind| {
            if let Some(memo) = checked.as_deref_mut() {
                if memo.iter().any(|(k, seen)| *seen == kind && k == key) {
                    return Ok(());
                }
                world.render_field_check(key, kind)?;
                memo.push((key.clone(), kind));
                return Ok(());
            }
            world.render_field_check(key, kind)
        };
        for (key, v) in self.extras {
            match v {
                Val::Num(n) => {
                    check(world, &key, RenderFieldKind::Num)?;
                    row.nums.push((key, n));
                }
                Val::Kw(sym) => {
                    check(world, &key, RenderFieldKind::Sym)?;
                    row.syms.push((key, sym));
                }
                Val::Nothing => {}
                _ => return Err(format!("render: field :{key} must be a number or keyword")),
            }
        }
        Ok(row)
    }
}

fn set_first(slot: &mut Option<Val>, value: Val) {
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn render_row_from_value(v: Val, world: &mut World, sig: &SigEnv) -> Result<RenderRow, String> {
    let Val::Map(kvs) = v else {
        return Err("render: expected row map".into());
    };
    let mut fields = RenderRowFields::default();
    for (k, v) in kvs.iter() {
        if let Val::Kw(key) = k {
            fields.push_kw(key.clone(), v.clone());
        }
    }
    fields.finish(world, sig)
}

fn render_row_from_literal_map(
    form: &Form,
    env: &Env,
    ctx: &mut Ctx,
    world: &mut World,
) -> Result<Option<RenderRow>, String> {
    let Form::Map(kvs) = form else {
        return Ok(None);
    };
    if !kvs.iter().all(|(k, _)| matches!(k, Form::Kw(_))) {
        return Ok(None);
    }
    let mut fields = RenderRowFields::default();
    for (k, v) in kvs.iter() {
        let Form::Kw(key) = k else { unreachable!() };
        let value = evaluate(v, env, ctx, world)?;
        fields.push_kw(key.clone(), value);
    }
    fields.finish(world, &ctx.sig).map(Some)
}

fn event_row_from_value(v: Val, world: &mut World) -> Result<(Symbol, Option<(f64, f64)>), String> {
    let Val::Map(kvs) = v else {
        return Err("emit :events: expected row map".into());
    };
    let mut name = None;
    let mut pos = None;
    for (k, v) in kvs.iter() {
        let Val::Kw(key) = k else {
            return Err("emit :events: row keys must be keywords".into());
        };
        match key.as_ref() {
            "name" => match v {
                Val::Kw(n) => name = Some(world.symbols.intern(n.as_ref())),
                other => return Err(format!("emit :events: :name must be a keyword, got {:?}", other)),
            },
            "pos" => match v {
                Val::Pose(p) => pos = Some((p.x, p.y)),
                Val::Nothing => pos = None,
                _ => pos = None,
            },
            // Event row schemas are intentionally closed until the manifest pass
            // introduces declared/open host-facing event streams.
            other => return Err(format!("emit :events: unknown field :{}", other)),
        }
    }
    let name = name.ok_or("emit :events: missing :name")?;
    Ok((name, pos))
}

fn sample_curve_shape(samples: &CurveSamples, world: &World, sig: &SigEnv) -> Result<Vec<(f64, f64)>, String> {
    let Some(i) = world.find(samples.entity) else {
        return Err("render: curve-samples entity is not live".into());
    };
    let dyn_figure = world
        .entities
        .dyn_figure(i)
        .ok_or_else(|| format!("render: curve-samples missing dyn figure for row {i}"))?;
    let tau = world.entity_motion_tau(i, world.tick);
    let state = MotionState::default();
    let Figure::Curve(curve) = eval_dyn_with_tick_rate(dyn_figure, tau, &state, sig, world.tick_rate())
        .map_err(|err| format!("render: curve-samples could not sample curve: {err}"))?
    else {
        return Err("render: curve-samples entity is not a live curve".into());
    };
    let min = match &curve.spec.domain {
        CurveDomain::Range { min, .. } => *min,
        CurveDomain::Values(vals) => *vals.first().ok_or("render: curve-samples empty domain")?,
    };
    let max = match &curve.spec.domain {
        CurveDomain::Range { max, .. } => {
            if samples.u_max.is_finite() { samples.u_max } else { *max }
        }
        CurveDomain::Values(vals) => *vals.last().ok_or("render: curve-samples empty domain")?,
    };
    let us: Vec<f64> = match &curve.spec.domain {
        CurveDomain::Values(vals) => vals.iter().copied().filter(|u| *u <= samples.u_max).collect(),
        CurveDomain::Range { .. } => {
            let span = (max - min).abs().max(0.01);
            let steps = ((span / samples.resolution).ceil() as usize).clamp(2, 400);
            (0..=steps).map(|k| min + (max - min) * k as f64 / steps as f64).collect()
        }
    };
    let mut pts = Vec::with_capacity(us.len());
    for u in us {
        let local = eval_curve_pose_with_tick_rate(&curve.spec.eval, tau, u, &state, sig, world.tick_rate())
            .map_err(|err| format!("render: curve-samples could not evaluate curve: {err}"))?;
        let w = curve.frame.compose(&local);
        pts.push((w.x, w.y));
    }
    Ok(pts)
}

fn render_point_xy(v: Val) -> Result<(f64, f64), String> {
    match v {
        Val::Pose(p) => Ok((p.x, p.y)),
        Val::Arr(items) if items.len() >= 2 => Ok((items[0].num()?, items[1].num()?)),
        Val::Map(kvs) => {
            let get = |name: &str| {
                kvs.iter().find_map(|(k, v)| match k {
                    Val::Kw(kw) if &**kw == name => Some(v.clone()),
                    _ => None,
                })
            };
            Ok((
                get("x").ok_or("render: point missing :x")?.num()?,
                get("y").ok_or("render: point missing :y")?.num()?,
            ))
        }
        other => Err(format!("render: unsupported point {:?}", other)),
    }
}
