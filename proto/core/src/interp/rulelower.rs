use super::engine::RenderKey;
use super::world::FieldSlots;
use super::{evaluate, row_predicate, Ctx, Env, World};
use crate::edn::Form;
use crate::sim::kernel::FilterPlan;
use std::rc::Rc;

pub(crate) struct CompiledTickForm {
    pub filter: FilterPlan,
    pub action: CompiledTickAction,
}

pub(crate) enum CompiledTickAction {
    Render(CompiledRender),
    /// The map body is exactly `(cull e)` over the row param: scan, then
    /// cull matched rows in row order (the interpreted phase order —
    /// entities-where completes before any cull applies).
    Cull,
}

pub(crate) struct CompiledRender {
    pub needs_pose: bool,
    pub fields: Vec<(Rc<str>, RenderKey, RowVal)>,
    /// Memoized batch schema: rebuilt only when observed column kinds
    /// change (stable once dynamic-kind fields settle), so hosts can key
    /// layouts on `Rc::ptr_eq`.
    pub schema: std::cell::RefCell<Option<Rc<crate::model::RenderSchema>>>,
}

pub(crate) enum RowVal {
    Num(f64),
    Kw(Rc<str>),
    PoseX,
    PoseY,
    PoseTheta,
    Field(Rc<str>),
    FieldOr(Rc<str>, Box<RowVal>),
}

/// A RowVal with its field names resolved against the world's symbol table
/// AND store slots, once per rule pass instead of once per row — rows read
/// their columns by index. All-`None` slots are a name that was never
/// interned anywhere — it can name neither a sym field nor a numeric
/// column, so the read is `Nothing` (mirroring `entity_field_at`).
pub(crate) enum ResolvedRowVal {
    Num(f64),
    Kw(Rc<str>),
    PoseX,
    PoseY,
    PoseTheta,
    Field(FieldSlots),
    FieldOr(FieldSlots, Box<ResolvedRowVal>),
}

impl RowVal {
    pub(crate) fn resolve(&self, world: &World) -> ResolvedRowVal {
        match self {
            RowVal::Num(n) => ResolvedRowVal::Num(*n),
            RowVal::Kw(k) => ResolvedRowVal::Kw(k.clone()),
            RowVal::PoseX => ResolvedRowVal::PoseX,
            RowVal::PoseY => ResolvedRowVal::PoseY,
            RowVal::PoseTheta => ResolvedRowVal::PoseTheta,
            RowVal::Field(name) => ResolvedRowVal::Field(world.field_slots(name)),
            RowVal::FieldOr(name, default) => ResolvedRowVal::FieldOr(
                world.field_slots(name),
                Box::new(default.resolve(world)),
            ),
        }
    }
}

const HEADS: [&str; 6] = ["map", "entities-where", "emit", "let", "%value-or", "fn"];

fn unshadowed(name: &str, env: &Env, ctx: &Ctx) -> bool {
    env.lookup(name).is_none() && !ctx.sig.defs.contains_key(name)
}

fn sym(form: &Form) -> Option<&str> {
    match form { Form::Sym(s) => Some(s), _ => None }
}

fn call<'a>(form: &'a Form, head: &str, env: &Env, ctx: &Ctx) -> Option<&'a [Form]> {
    let Form::List(items) = form else { return None };
    if sym(items.first()?)? != head || !unshadowed(head, env, ctx) { return None; }
    Some(&items[1..])
}

fn access(form: &Form, subject: &str) -> Option<Rc<str>> {
    let Form::List(items) = form else { return None };
    let [Form::Kw(field), Form::Sym(target)] = &items[..] else { return None };
    (target.as_ref() == subject).then(|| field.clone())
}

fn row_val(form: &Form, entity: &str, pose: Option<&str>, env: &Env, ctx: &Ctx) -> Option<RowVal> {
    match form {
        Form::Num(n) => Some(RowVal::Num(*n)),
        Form::Kw(k) => Some(RowVal::Kw(k.clone())),
        _ => {
            if let Some(pose) = pose {
                if let Some(field) = access(form, pose) {
                    return match field.as_ref() {
                        "x" => Some(RowVal::PoseX),
                        "y" => Some(RowVal::PoseY),
                        "th" => Some(RowVal::PoseTheta),
                        _ => None,
                    };
                }
            }
            if let Some(field) = access(form, entity) {
                return (!matches!(field.as_ref(), "pos" | "vel" | "t" | "tick" | "handle" | "kind"))
                    .then(|| RowVal::Field(field));
            }
            let args = call(form, "%value-or", env, ctx)?;
            let [value, default] = args else { return None };
            let RowVal::Field(field) = row_val(value, entity, pose, env, ctx)? else { return None };
            let default = row_val(default, entity, pose, env, ctx)?;
            if !matches!(default, RowVal::Num(_) | RowVal::Kw(_) | RowVal::PoseX | RowVal::PoseY | RowVal::PoseTheta) {
                return None;
            }
            Some(RowVal::FieldOr(field, Box::new(default)))
        }
    }
}

fn is_pose(rv: &RowVal) -> bool {
    match rv {
        RowVal::PoseX | RowVal::PoseY | RowVal::PoseTheta => true,
        RowVal::FieldOr(_, default) => is_pose(default),
        _ => false,
    }
}

pub(crate) fn lower_tick_form(form: &Form, env: &Env, ctx: &mut Ctx, world: &mut World) -> Option<CompiledTickForm> {
    let args = call(form, "map", env, ctx)?;
    let [fnform, query] = args else { return None };
    let query_args = call(query, "entities-where", env, ctx)?;
    let [predform] = query_args else { return None };
    let pred_args = call(predform, "fn", env, ctx)?;
    if pred_args.len() < 2 || !matches!(&pred_args[0], Form::Vector(_)) { return None; }
    let filter = row_predicate(&evaluate(predform, env, ctx, world).ok()?, ctx)?.lower(world)?;

    let fn_args = call(fnform, "fn", env, ctx)?;
    let [Form::Vector(params), body] = fn_args else { return None };
    let [Form::Sym(entity)] = &params[..] else { return None };
    if matches!(entity.as_ref(), "&" | "*" | "=") || HEADS.contains(&entity.as_ref()) { return None; }

    // the cull-rule shape: body is exactly (cull e); anything else about
    // the body (extra args, a shadowing param named cull) bails the form
    if let Some(args) = call(body, "cull", env, ctx) {
        return (entity.as_ref() != "cull"
            && matches!(args, [Form::Sym(target)] if target == entity))
            .then(|| CompiledTickForm { filter, action: CompiledTickAction::Cull });
    }

    let (pose, emit_form) = if let Some(let_args) = call(body, "let", env, ctx) {
        let [Form::Vector(bindings), emit] = let_args else { return None };
        let [Form::Sym(pose), value] = &bindings[..] else { return None };
        if pose == entity || HEADS.contains(&pose.as_ref())
            || access(value, entity.as_ref()).as_deref() != Some("pos") { return None; }
        (Some(pose.as_ref()), emit)
    } else {
        (None, body)
    };
    let emit_args = call(emit_form, "emit", env, ctx)?;
    let [Form::Kw(channel), Form::Map(kvs)] = emit_args else { return None };
    if channel.as_ref() != "render" { return None; }
    let mut fields = Vec::with_capacity(kvs.len());
    let mut has_shape = false;
    for (key, value) in kvs.iter() {
        let Form::Kw(key) = key else { return None };
        if key.as_ref() == "shape" {
            if !matches!(value, Form::Kw(shape) if matches!(shape.as_ref(), "point" | "dot")) {
                return None;
            }
            has_shape = true;
        }
        fields.push((key.clone(), RenderKey::from_name(key), row_val(value, entity, pose, env, ctx)?));
    }
    if !has_shape { return None; }
    let needs_pose = fields.iter().any(|(_, _, value)| is_pose(value));
    Some(CompiledTickForm {
        filter,
        action: CompiledTickAction::Render(CompiledRender {
            needs_pose,
            fields,
            schema: std::cell::RefCell::new(None),
        }),
    })
}
