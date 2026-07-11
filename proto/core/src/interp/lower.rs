use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug)]
pub struct NumProgram {
    pub ops: Vec<NumOp>,
    pub n_regs: usize,
    /// Capture-slot count: `Input { slot }` ops read a per-entity capture
    /// vector (rand draws today; spawn-env values later). Slot numbering is
    /// shared across the programs of one dyn node (a and b index one
    /// vector), so a program may not use every slot below `n_inputs`.
    pub n_inputs: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum NumOp {
    Const { dst: u16, v: f64 },
    /// Per-entity capture-vector read (`(%capture slot)` marker forms).
    Input { dst: u16, slot: u16 },
    T { dst: u16 },
    U { dst: u16 },
    PosX { dst: u16 },
    PosY { dst: u16 },
    Add { dst: u16, a: u16, b: u16 },
    Sub { dst: u16, a: u16, b: u16 },
    Mul { dst: u16, a: u16, b: u16 },
    Div { dst: u16, a: u16, b: u16 },
    Eq { dst: u16, a: u16, b: u16 },
    Lt { dst: u16, a: u16, b: u16 },
    Gt { dst: u16, a: u16, b: u16 },
    Lte { dst: u16, a: u16, b: u16 },
    Gte { dst: u16, a: u16, b: u16 },
    Neg { dst: u16, x: u16 },
    Not { dst: u16, x: u16 },
    Abs { dst: u16, x: u16 },
    Floor { dst: u16, x: u16 },
    Ceil { dst: u16, x: u16 },
    Round { dst: u16, x: u16 },
    Sin { dst: u16, x: u16 },
    Cos { dst: u16, x: u16 },
    Sqrt { dst: u16, x: u16 },
    Pow { dst: u16, a: u16, b: u16 },
    Min { dst: u16, a: u16, b: u16 },
    Max { dst: u16, a: u16, b: u16 },
    Mod { dst: u16, a: u16, b: u16 },
    Quot { dst: u16, a: u16, b: u16 },
    Sine { dst: u16, period: u16, amp: u16, x: u16 },
    Lerp { dst: u16, a: u16, b: u16, ctrl: u16, v1: u16, v2: u16 },
    Lerp3 { dst: u16, a1: u16, b1: u16, a2: u16, b2: u16, ctrl: u16, v1: u16, v2: u16, v3: u16 },
    Ease { dst: u16, kind: EaseKind, x: u16 },
    LerpSmooth { dst: u16, kind: EaseKind, a: u16, b: u16, ctrl: u16, v1: u16, v2: u16 },
    Lssht { dst: u16, c: u16, pv: u16, f1: u16, f2: u16 },
}

#[derive(Clone, Copy, Debug)]
pub enum EaseKind {
    InSine,
    OutSine,
    InOutSine,
}

struct Builder<'a> {
    ops: Vec<NumOp>,
    next: u16,
    defs: &'a HashMap<String, Form>,
    inline_depth: usize,
    n_inputs: usize,
    /// Slot mode (input-slot lowering): numeric env captures become Input
    /// slots at `base + index` instead of folding to Const, so nodes that
    /// differ only in captured values lower to ONE interned program and
    /// batch as lanes. None = classic Const folding (render/dyn-col paths).
    env_slots: Option<EnvSlots<'a>>,
}

struct EnvSlots<'a> {
    /// Capture-slot names in slot order, shared across a node's programs.
    names: &'a mut Vec<std::rc::Rc<str>>,
    /// First env slot id (rand marker sites occupy 0..base).
    base: usize,
}

const MAX_INLINE_DEPTH: usize = 32;

#[derive(Clone, Copy)]
enum LowerScope<'a> {
    Current,
    Def { params: &'a HashMap<String, u16> },
}

impl Builder<'_> {
    fn reg(&mut self) -> Option<u16> {
        let r = self.next;
        self.next = self.next.checked_add(1)?;
        Some(r)
    }

    fn push(&mut self, op: NumOp) -> Option<u16> {
        let dst = op_dst(op);
        self.ops.push(op);
        Some(dst)
    }

    fn lower(&mut self, form: &Form, env: &Env, scope: LowerScope<'_>) -> Option<u16> {
        match form {
            Form::Num(v) => {
                let dst = self.reg()?;
                self.push(NumOp::Const { dst, v: *v })
            }
            Form::Bool(b) => {
                let dst = self.reg()?;
                self.push(NumOp::Const { dst, v: if *b { 1.0 } else { 0.0 } })
            }
            Form::Sym(s) => self.lower_sym(s, env, scope),
            Form::List(items) => self.lower_list(items, env, scope),
            _ => None,
        }
    }

    fn lower_sym(&mut self, s: &str, env: &Env, scope: LowerScope<'_>) -> Option<u16> {
        match scope {
            LowerScope::Current => match s {
                "t" => {
                    if env.lookup("t").is_some() {
                        return None;
                    }
                    let dst = self.reg()?;
                    self.push(NumOp::T { dst })
                }
                "u" => {
                    if env.lookup("u").is_some() {
                        return None;
                    }
                    let dst = self.reg()?;
                    self.push(NumOp::U { dst })
                }
                "inf" => {
                    let dst = self.reg()?;
                    self.push(NumOp::Const { dst, v: f64::INFINITY })
                }
                "phi" => {
                    let dst = self.reg()?;
                    self.push(NumOp::Const { dst, v: 1.618_033_988_749_895 })
                }
                name if name.starts_with('$') => None,
                name => match env.lookup(name) {
                    Some(Val::Num(v)) => match &mut self.env_slots {
                        Some(slots) => {
                            let idx = match slots.names.iter().position(|n| &**n == name) {
                                Some(i) => i,
                                None => {
                                    slots.names.push(name.into());
                                    slots.names.len() - 1
                                }
                            };
                            let slot = (slots.base + idx) as u16;
                            self.n_inputs = self.n_inputs.max(slot as usize + 1);
                            let dst = self.reg()?;
                            self.push(NumOp::Input { dst, slot })
                        }
                        None => {
                            let dst = self.reg()?;
                            self.push(NumOp::Const { dst, v })
                        }
                    },
                    None => self.lower_bare_def(name, env),
                    _ => None,
                },
            },
            LowerScope::Def { params } => {
                if let Some(r) = params.get(s) {
                    return Some(*r);
                }
                match s {
                    "t" => {
                        if env.lookup("t").is_some() {
                            return None;
                        }
                        let dst = self.reg()?;
                        self.push(NumOp::T { dst })
                    }
                    "u" => {
                        if env.lookup("u").is_some() {
                            return None;
                        }
                        let dst = self.reg()?;
                        self.push(NumOp::U { dst })
                    }
                    "inf" => {
                        let dst = self.reg()?;
                        self.push(NumOp::Const { dst, v: f64::INFINITY })
                    }
                    "phi" => {
                        let dst = self.reg()?;
                        self.push(NumOp::Const { dst, v: 1.618_033_988_749_895 })
                    }
                    name if name.starts_with('$') => None,
                    name => self.lower_bare_def(name, env),
                }
            }
        }
    }

    fn lower_list(&mut self, items: &[Form], env: &Env, scope: LowerScope<'_>) -> Option<u16> {
        if let Some(Form::Kw(field)) = items.first() {
            return self.lower_kw_access(field, items, env, scope);
        }
        let Some(Form::Sym(head)) = items.first() else {
            return None;
        };
        let name = &**head;
        // `(%capture i)`: a rand site rewritten to a capture slot at spawn
        // extraction (spawn.rs). `%` heads are unbindable, so no shadow check.
        if name == "%capture" {
            let (2, Some(Form::Num(slot))) = (items.len(), items.get(1)) else {
                return None;
            };
            let slot = *slot as u16;
            self.n_inputs = self.n_inputs.max(slot as usize + 1);
            let dst = self.reg()?;
            return self.push(NumOp::Input { dst, slot });
        }
        if special_or_channel_head(name) {
            return None;
        }
        match scope {
            LowerScope::Current => {
                if env.lookup(name).is_some() {
                    return None;
                }
                if self.defs.contains_key(name) {
                    return self.lower_def_call(name, &items[1..], env, scope);
                }
            }
            LowerScope::Def { params } => {
                if params.contains_key(name) {
                    return None;
                }
                if self.defs.contains_key(name) {
                    return self.lower_def_call(name, &items[1..], env, scope);
                }
            }
        }
        if name == "lerpsmooth" && items.len() == 7 {
            let kind = self.ease_kind_arg(&items[1], env, scope)?;
            let args = items[2..]
                .iter()
                .map(|f| self.lower(f, env, scope))
                .collect::<Option<Vec<_>>>()?;
            let dst = self.reg()?;
            return self.push(NumOp::LerpSmooth {
                dst,
                kind,
                a: args[0],
                b: args[1],
                ctrl: args[2],
                v1: args[3],
                v2: args[4],
            });
        }

        let args = items[1..]
            .iter()
            .map(|f| self.lower(f, env, scope))
            .collect::<Option<Vec<_>>>()?;
        self.lower_call(name, &args)
    }

    /// Keyword-head application `(:field base)`: `pos` component reads
    /// become PosX/PosY ops; access chains rooted at an env-captured
    /// Map/Pose fold to Const at lower time (the program cache lives on
    /// the node next to its captured env, so captures are stable for the
    /// program's lifetime). Def scope bails: def bodies evaluate in a
    /// fresh env, so neither `pos` nor captures are visible there (F12).
    fn lower_kw_access(&mut self, field: &str, items: &[Form], env: &Env, scope: LowerScope<'_>) -> Option<u16> {
        if items.len() != 2 || !matches!(scope, LowerScope::Current) {
            return None;
        }
        if let Form::Sym(base) = &items[1] {
            // slot-bound pos: only when not shadowed by a capture (a
            // captured `pos` wins at non-pos eval sites, so it's ambiguous)
            if &**base == "pos" && env.lookup("pos").is_none() {
                let dst = self.reg()?;
                return match field {
                    "x" => self.push(NumOp::PosX { dst }),
                    "y" => self.push(NumOp::PosY { dst }),
                    _ => None,
                };
            }
        }
        match kw_get_val(field, &fold_access_val(&items[1], env)?)? {
            Val::Num(v) => {
                let dst = self.reg()?;
                self.push(NumOp::Const { dst, v })
            }
            _ => None,
        }
    }

    /// Static easing argument for lerpsmooth: a bare easing-builtin name
    /// (not shadowed by env or defs) or a captured Val::Builtin. Mirrors
    /// the interpreter's resolution order per scope.
    fn ease_kind_arg(&self, form: &Form, env: &Env, scope: LowerScope<'_>) -> Option<EaseKind> {
        let Form::Sym(s) = form else {
            return None;
        };
        let name = &**s;
        match scope {
            LowerScope::Current => match env.lookup(name) {
                Some(Val::Builtin(nm)) => ease_kind(&nm),
                Some(_) => None,
                None if self.defs.contains_key(name) => None,
                None => ease_kind(name),
            },
            LowerScope::Def { params } => {
                if params.contains_key(name) || self.defs.contains_key(name) {
                    return None;
                }
                ease_kind(name)
            }
        }
    }

    fn lower_bare_def(&mut self, name: &str, env: &Env) -> Option<u16> {
        let def = self.defs.get(name)?.clone();
        if literal_fn_parts(&def).is_some() {
            return None;
        }
        self.with_inline_depth(|this| {
            let params = HashMap::new();
            this.lower(&def, env, LowerScope::Def { params: &params })
        })
    }

    fn lower_def_call(
        &mut self,
        name: &str,
        args: &[Form],
        env: &Env,
        scope: LowerScope<'_>,
    ) -> Option<u16> {
        let def = self.defs.get(name)?.clone();
        let (params, body) = literal_fn_parts(&def)?;
        if params.len() != args.len() {
            return None;
        }
        let arg_regs = args
            .iter()
            .map(|f| self.lower(f, env, scope))
            .collect::<Option<Vec<_>>>()?;
        let mut param_regs = HashMap::new();
        for (param, reg) in params.iter().zip(arg_regs) {
            let Form::Sym(param) = param else {
                return None;
            };
            if &**param == "&" {
                return None;
            }
            param_regs.insert(param.to_string(), reg);
        }
        self.with_inline_depth(|this| {
            this.lower(body, env, LowerScope::Def { params: &param_regs })
        })
    }

    fn with_inline_depth<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        if self.inline_depth >= MAX_INLINE_DEPTH {
            return None;
        }
        self.inline_depth += 1;
        let out = f(self);
        self.inline_depth -= 1;
        out
    }

    fn lower_call(&mut self, name: &str, args: &[u16]) -> Option<u16> {
        match name {
            "+" => self.lower_fold(args, 0.0, |dst, a, b| NumOp::Add { dst, a, b }),
            "*" => self.lower_fold(args, 1.0, |dst, a, b| NumOp::Mul { dst, a, b }),
            "-" => self.lower_fold(args, 0.0, |dst, a, b| NumOp::Sub { dst, a, b }),
            "/" => self.lower_fold(args, 1.0, |dst, a, b| NumOp::Div { dst, a, b }),
            "=" | "<" | ">" | "<=" | ">=" | "min" | "max" | "mod" | "pow" | "quot" if args.len() == 2 => {
                let dst = self.reg()?;
                let (a, b) = (args[0], args[1]);
                let op = match name {
                    "=" => NumOp::Eq { dst, a, b },
                    "<" => NumOp::Lt { dst, a, b },
                    ">" => NumOp::Gt { dst, a, b },
                    "<=" => NumOp::Lte { dst, a, b },
                    ">=" => NumOp::Gte { dst, a, b },
                    "min" => NumOp::Min { dst, a, b },
                    "max" => NumOp::Max { dst, a, b },
                    "mod" => NumOp::Mod { dst, a, b },
                    "pow" => NumOp::Pow { dst, a, b },
                    "quot" => NumOp::Quot { dst, a, b },
                    _ => unreachable!(),
                };
                self.push(op)
            }
            "abs" | "floor" | "ceil" | "round" | "sqrt" | "not" | "sin" | "cos" | "einsine" | "eoutsine" | "eiosine"
                if args.len() == 1 =>
            {
                let dst = self.reg()?;
                let x = args[0];
                let op = match name {
                    "abs" => NumOp::Abs { dst, x },
                    "floor" => NumOp::Floor { dst, x },
                    "ceil" => NumOp::Ceil { dst, x },
                    "round" => NumOp::Round { dst, x },
                    "sqrt" => NumOp::Sqrt { dst, x },
                    "not" => NumOp::Not { dst, x },
                    "sin" => NumOp::Sin { dst, x },
                    "cos" => NumOp::Cos { dst, x },
                    "einsine" => NumOp::Ease { dst, kind: EaseKind::InSine, x },
                    "eoutsine" => NumOp::Ease { dst, kind: EaseKind::OutSine, x },
                    "eiosine" => NumOp::Ease { dst, kind: EaseKind::InOutSine, x },
                    _ => unreachable!(),
                };
                self.push(op)
            }
            "sine" if args.len() == 3 => {
                let dst = self.reg()?;
                self.push(NumOp::Sine { dst, period: args[0], amp: args[1], x: args[2] })
            }
            "lerp" if args.len() == 5 => {
                let dst = self.reg()?;
                self.push(NumOp::Lerp { dst, a: args[0], b: args[1], ctrl: args[2], v1: args[3], v2: args[4] })
            }
            "lerp3" if args.len() == 8 => {
                let dst = self.reg()?;
                self.push(NumOp::Lerp3 {
                    dst,
                    a1: args[0],
                    b1: args[1],
                    a2: args[2],
                    b2: args[3],
                    ctrl: args[4],
                    v1: args[5],
                    v2: args[6],
                    v3: args[7],
                })
            }
            "lssht" if args.len() == 4 => {
                let dst = self.reg()?;
                self.push(NumOp::Lssht { dst, c: args[0], pv: args[1], f1: args[2], f2: args[3] })
            }
            _ => None,
        }
    }

    fn lower_fold(
        &mut self,
        args: &[u16],
        init: f64,
        make: fn(u16, u16, u16) -> NumOp,
    ) -> Option<u16> {
        let mut iter = args.iter().copied();
        let mut acc = if let Some(first) = iter.next() {
            first
        } else {
            let dst = self.reg()?;
            return self.push(NumOp::Const { dst, v: init });
        };
        if args.len() == 1 {
            let init_reg = self.reg()?;
            self.push(NumOp::Const { dst: init_reg, v: init })?;
            let dst = self.reg()?;
            return self.push(make(dst, init_reg, acc));
        }
        for arg in iter {
            let dst = self.reg()?;
            acc = self.push(make(dst, acc, arg))?;
        }
        Some(acc)
    }
}

fn op_dst(op: NumOp) -> u16 {
    match op {
        NumOp::Const { dst, .. }
        | NumOp::Input { dst, .. }
        | NumOp::T { dst }
        | NumOp::U { dst }
        | NumOp::PosX { dst }
        | NumOp::PosY { dst }
        | NumOp::Add { dst, .. }
        | NumOp::Sub { dst, .. }
        | NumOp::Mul { dst, .. }
        | NumOp::Div { dst, .. }
        | NumOp::Eq { dst, .. }
        | NumOp::Lt { dst, .. }
        | NumOp::Gt { dst, .. }
        | NumOp::Lte { dst, .. }
        | NumOp::Gte { dst, .. }
        | NumOp::Neg { dst, .. }
        | NumOp::Not { dst, .. }
        | NumOp::Abs { dst, .. }
        | NumOp::Floor { dst, .. }
        | NumOp::Ceil { dst, .. }
        | NumOp::Round { dst, .. }
        | NumOp::Sin { dst, .. }
        | NumOp::Cos { dst, .. }
        | NumOp::Sqrt { dst, .. }
        | NumOp::Pow { dst, .. }
        | NumOp::Min { dst, .. }
        | NumOp::Max { dst, .. }
        | NumOp::Mod { dst, .. }
        | NumOp::Quot { dst, .. }
        | NumOp::Sine { dst, .. }
        | NumOp::Lerp { dst, .. }
        | NumOp::Lerp3 { dst, .. }
        | NumOp::Ease { dst, .. }
        | NumOp::LerpSmooth { dst, .. }
        | NumOp::Lssht { dst, .. } => dst,
    }
}

fn literal_fn_parts(form: &Form) -> Option<(&[Form], &Form)> {
    let Form::List(items) = form else {
        return None;
    };
    match (&items[..]).split_first()? {
        (Form::Sym(head), rest) if &**head == "fn" && rest.len() == 2 => {
            let Form::Vector(params) = &rest[0] else {
                return None;
            };
            if params.iter().any(|p| !matches!(p, Form::Sym(s) if &**s != "&")) {
                return None;
            }
            Some((params, &rest[1]))
        }
        _ => None,
    }
}

/// Resolve an access-chain base to a lower-time-stable value: an
/// env-captured binding, or a nested keyword access on one. `t`/`u`/`pos`
/// are excluded — the eval site rebinds them, so a capture never wins.
fn fold_access_val(form: &Form, env: &Env) -> Option<Val> {
    match form {
        Form::Sym(s) if !matches!(&**s, "t" | "u" | "pos") => env.lookup(s),
        Form::List(items) if items.len() == 2 => {
            let Form::Kw(field) = &items[0] else {
                return None;
            };
            kw_get_val(field, &fold_access_val(&items[1], env)?)
        }
        _ => None,
    }
}

/// One keyword read, restricted to the cases whose interpreter semantics
/// need no ctx/world (mod.rs keyword application): pose components and
/// plain map lookup. Missing keys and every other value kind bail.
fn kw_get_val(field: &str, v: &Val) -> Option<Val> {
    match (field, v) {
        ("x", Val::Pose(p)) => Some(Val::Num(p.x)),
        ("y", Val::Pose(p)) => Some(Val::Num(p.y)),
        ("th", Val::Pose(p)) => Some(Val::Num(p.angle_or(0.0))),
        (_, Val::Map(_)) => super::spawn::map_get(v, field),
        _ => None,
    }
}

fn ease_kind(name: &str) -> Option<EaseKind> {
    match name {
        "einsine" => Some(EaseKind::InSine),
        "eoutsine" => Some(EaseKind::OutSine),
        "eiosine" => Some(EaseKind::InOutSine),
        _ => None,
    }
}

fn special_or_channel_head(name: &str) -> bool {
    matches!(name, "t" | "u" | "inf" | "phi") || name.starts_with('$')
}

pub fn lower_num_form(form: &Form, env: &Env, defs: &HashMap<String, Form>) -> Option<NumProgram> {
    // Def inlining needs no cell-scope guard: signal evaluation skips bare
    // cell reads (Ctx.signal_scope — signals read cells via (live name)
    // only), so a def name can never be shadowed by a cell at runtime.
    let mut b = Builder { ops: Vec::new(), next: 0, defs, inline_depth: 0, n_inputs: 0, env_slots: None };
    b.lower(form, env, LowerScope::Current)?;
    Some(NumProgram { ops: b.ops, n_regs: b.next as usize, n_inputs: b.n_inputs })
}

/// Slot-mode lowering: numeric env captures become Input slots (numbered
/// `base + index` into `names`, deduped by name and SHARED across a node's
/// programs — pass the same `names` vec for a and b). Rand markers keep
/// their extraction slot ids below `base`.
pub fn lower_num_form_slotted(
    form: &Form,
    env: &Env,
    defs: &HashMap<String, Form>,
    names: &mut Vec<std::rc::Rc<str>>,
    base: usize,
) -> Option<NumProgram> {
    let mut b = Builder {
        ops: Vec::new(),
        next: 0,
        defs,
        inline_depth: 0,
        n_inputs: 0,
        env_slots: Some(EnvSlots { names, base }),
    };
    b.lower(form, env, LowerScope::Current)?;
    Some(NumProgram { ops: b.ops, n_regs: b.next as usize, n_inputs: b.n_inputs })
}

pub fn program_uses_pos(prog: &NumProgram) -> bool {
    prog.ops.iter().any(|op| matches!(op, NumOp::PosX { .. } | NumOp::PosY { .. }))
}

/// Structural interning: programs with identical op streams share one Rc,
/// so spawn sites (and repeated constructions at one site) that lower to
/// the same shape fuse into one vel-batch group — per-entity/per-site data
/// arrives through Input slots, never through the program body. The cache
/// key is an exact encoding of (ops, n_regs, n_inputs); f64s compare by
/// bits (a NaN-const program simply never unifies).
pub fn intern_program(prog: NumProgram) -> Rc<NumProgram> {
    thread_local! {
        static CACHE: RefCell<HashMap<Vec<u64>, Rc<NumProgram>>> = RefCell::new(HashMap::new());
    }
    let key = program_key(&prog);
    CACHE.with(|c| c.borrow_mut().entry(key).or_insert_with(|| Rc::new(prog)).clone())
}

fn program_key(prog: &NumProgram) -> Vec<u64> {
    let mut k = Vec::with_capacity(prog.ops.len() * 2 + 2);
    k.push(prog.n_regs as u64);
    k.push(prog.n_inputs as u64);
    let reg2 = |a: u16, b: u16| ((a as u64) << 16) | b as u64;
    let reg3 = |a: u16, b: u16, c: u16| ((a as u64) << 32) | ((b as u64) << 16) | c as u64;
    for op in &prog.ops {
        // one discriminant word, then operand words; dst is derivable from
        // op order for most ops but encoded anyway — exactness over bytes
        match *op {
            NumOp::Const { dst, v } => {
                k.push(1 << 32 | dst as u64);
                k.push(v.to_bits());
            }
            NumOp::Input { dst, slot } => k.push(2 << 32 | reg2(dst, slot)),
            NumOp::T { dst } => k.push(3 << 32 | dst as u64),
            NumOp::U { dst } => k.push(4 << 32 | dst as u64),
            NumOp::PosX { dst } => k.push(5 << 32 | dst as u64),
            NumOp::PosY { dst } => k.push(6 << 32 | dst as u64),
            NumOp::Add { dst, a, b } => k.push(7 << 32 | reg3(dst, a, b)),
            NumOp::Sub { dst, a, b } => k.push(8 << 32 | reg3(dst, a, b)),
            NumOp::Mul { dst, a, b } => k.push(9 << 32 | reg3(dst, a, b)),
            NumOp::Div { dst, a, b } => k.push(10 << 32 | reg3(dst, a, b)),
            NumOp::Eq { dst, a, b } => k.push(11 << 32 | reg3(dst, a, b)),
            NumOp::Lt { dst, a, b } => k.push(12 << 32 | reg3(dst, a, b)),
            NumOp::Gt { dst, a, b } => k.push(13 << 32 | reg3(dst, a, b)),
            NumOp::Lte { dst, a, b } => k.push(14 << 32 | reg3(dst, a, b)),
            NumOp::Gte { dst, a, b } => k.push(15 << 32 | reg3(dst, a, b)),
            NumOp::Neg { dst, x } => k.push(16 << 32 | reg2(dst, x)),
            NumOp::Not { dst, x } => k.push(17 << 32 | reg2(dst, x)),
            NumOp::Abs { dst, x } => k.push(18 << 32 | reg2(dst, x)),
            NumOp::Floor { dst, x } => k.push(19 << 32 | reg2(dst, x)),
            NumOp::Ceil { dst, x } => k.push(20 << 32 | reg2(dst, x)),
            NumOp::Round { dst, x } => k.push(21 << 32 | reg2(dst, x)),
            NumOp::Sin { dst, x } => k.push(22 << 32 | reg2(dst, x)),
            NumOp::Cos { dst, x } => k.push(23 << 32 | reg2(dst, x)),
            NumOp::Sqrt { dst, x } => k.push(24 << 32 | reg2(dst, x)),
            NumOp::Pow { dst, a, b } => k.push(25 << 32 | reg3(dst, a, b)),
            NumOp::Min { dst, a, b } => k.push(26 << 32 | reg3(dst, a, b)),
            NumOp::Max { dst, a, b } => k.push(27 << 32 | reg3(dst, a, b)),
            NumOp::Mod { dst, a, b } => k.push(28 << 32 | reg3(dst, a, b)),
            NumOp::Quot { dst, a, b } => k.push(29 << 32 | reg3(dst, a, b)),
            NumOp::Sine { dst, period, amp, x } => {
                k.push(30 << 32 | reg2(dst, period));
                k.push(reg2(amp, x));
            }
            NumOp::Lerp { dst, a, b, ctrl, v1, v2 } => {
                k.push(31 << 32 | reg3(dst, a, b));
                k.push(reg3(ctrl, v1, v2));
            }
            NumOp::Lerp3 { dst, a1, b1, a2, b2, ctrl, v1, v2, v3 } => {
                k.push(32 << 32 | reg3(dst, a1, b1));
                k.push(reg3(a2, b2, ctrl));
                k.push(reg3(v1, v2, v3));
            }
            NumOp::Ease { dst, kind, x } => k.push((33 + kind as u64) << 32 | reg2(dst, x)),
            NumOp::LerpSmooth { dst, kind, a, b, ctrl, v1, v2 } => {
                k.push((36 + kind as u64) << 32 | reg3(dst, a, b));
                k.push(reg3(ctrl, v1, v2));
            }
            NumOp::Lssht { dst, c, pv, f1, f2 } => {
                k.push(39 << 32 | reg3(dst, c, pv));
                k.push(reg2(f1, f2));
            }
        }
    }
    k
}

thread_local! {
    static REGS: RefCell<Vec<f64>> = RefCell::new(Vec::new());
}

/// Cap-free convenience (tests, cap-free call sites).
#[cfg_attr(not(test), allow(dead_code))]
pub fn run_num_program(prog: &NumProgram, t: f64, u: f64, pos: Option<(f64, f64)>) -> f64 {
    run_num_program_caps(prog, t, u, pos, &[])
}

pub fn run_num_program_caps(prog: &NumProgram, t: f64, u: f64, pos: Option<(f64, f64)>, caps: &[f64]) -> f64 {
    REGS.with(|regs| {
        let mut regs = regs.borrow_mut();
        run(prog, t, u, pos, caps, &mut regs)
    })
}

pub fn run(prog: &NumProgram, t: f64, u: f64, pos: Option<(f64, f64)>, caps: &[f64], regs: &mut Vec<f64>) -> f64 {
    debug_assert!(caps.len() >= prog.n_inputs);
    regs.clear();
    regs.resize(prog.n_regs, 0.0);
    for op in &prog.ops {
        match *op {
            NumOp::Const { dst, v } => regs[dst as usize] = v,
            NumOp::Input { dst, slot } => regs[dst as usize] = caps[slot as usize],
            NumOp::T { dst } => regs[dst as usize] = t,
            NumOp::U { dst } => regs[dst as usize] = u,
            NumOp::PosX { dst } => regs[dst as usize] = pos.map(|p| p.0).unwrap_or(0.0),
            NumOp::PosY { dst } => regs[dst as usize] = pos.map(|p| p.1).unwrap_or(0.0),
            NumOp::Add { dst, a, b } => regs[dst as usize] = regs[a as usize] + regs[b as usize],
            NumOp::Sub { dst, a, b } => regs[dst as usize] = regs[a as usize] - regs[b as usize],
            NumOp::Mul { dst, a, b } => regs[dst as usize] = regs[a as usize] * regs[b as usize],
            NumOp::Div { dst, a, b } => regs[dst as usize] = regs[a as usize] / regs[b as usize],
            NumOp::Eq { dst, a, b } => regs[dst as usize] = mask_num((regs[a as usize] - regs[b as usize]).abs() < 1e-9),
            NumOp::Lt { dst, a, b } => regs[dst as usize] = mask_num(regs[a as usize] < regs[b as usize]),
            NumOp::Gt { dst, a, b } => regs[dst as usize] = mask_num(regs[a as usize] > regs[b as usize]),
            NumOp::Lte { dst, a, b } => regs[dst as usize] = mask_num(regs[a as usize] <= regs[b as usize]),
            NumOp::Gte { dst, a, b } => regs[dst as usize] = mask_num(regs[a as usize] >= regs[b as usize]),
            NumOp::Neg { dst, x } => regs[dst as usize] = -regs[x as usize],
            NumOp::Not { dst, x } => regs[dst as usize] = mask_num(regs[x as usize] == 0.0),
            NumOp::Abs { dst, x } => regs[dst as usize] = regs[x as usize].abs(),
            NumOp::Floor { dst, x } => regs[dst as usize] = regs[x as usize].floor(),
            NumOp::Ceil { dst, x } => regs[dst as usize] = regs[x as usize].ceil(),
            NumOp::Round { dst, x } => regs[dst as usize] = regs[x as usize].round(),
            NumOp::Sin { dst, x } => regs[dst as usize] = regs[x as usize].to_radians().sin(),
            NumOp::Cos { dst, x } => regs[dst as usize] = regs[x as usize].to_radians().cos(),
            NumOp::Sqrt { dst, x } => regs[dst as usize] = regs[x as usize].sqrt(),
            NumOp::Pow { dst, a, b } => regs[dst as usize] = regs[a as usize].powf(regs[b as usize]),
            NumOp::Min { dst, a, b } => regs[dst as usize] = regs[a as usize].min(regs[b as usize]),
            NumOp::Max { dst, a, b } => regs[dst as usize] = regs[a as usize].max(regs[b as usize]),
            NumOp::Mod { dst, a, b } => regs[dst as usize] = regs[a as usize].rem_euclid(regs[b as usize]),
            NumOp::Quot { dst, a, b } => regs[dst as usize] = (regs[a as usize] / regs[b as usize]).trunc(),
            NumOp::Sine { dst, period, amp, x } => {
                regs[dst as usize] = regs[amp as usize] * (std::f64::consts::TAU * regs[x as usize] / regs[period as usize]).sin();
            }
            NumOp::Lerp { dst, a, b, ctrl, v1, v2 } => {
                let r = ((regs[ctrl as usize] - regs[a as usize]) / (regs[b as usize] - regs[a as usize])).clamp(0.0, 1.0);
                regs[dst as usize] = regs[v1 as usize] + r * (regs[v2 as usize] - regs[v1 as usize]);
            }
            NumOp::Lerp3 { dst, a1, b1, a2, b2, ctrl, v1, v2, v3 } => {
                let out = if regs[ctrl as usize] < regs[a2 as usize] {
                    let r = ((regs[ctrl as usize] - regs[a1 as usize]) / (regs[b1 as usize] - regs[a1 as usize])).clamp(0.0, 1.0);
                    regs[v1 as usize] + r * (regs[v2 as usize] - regs[v1 as usize])
                } else {
                    let r = ((regs[ctrl as usize] - regs[a2 as usize]) / (regs[b2 as usize] - regs[a2 as usize])).clamp(0.0, 1.0);
                    regs[v2 as usize] + r * (regs[v3 as usize] - regs[v2 as usize])
                };
                regs[dst as usize] = out;
            }
            NumOp::Ease { dst, kind, x } => regs[dst as usize] = ease_num(kind, regs[x as usize]),
            NumOp::LerpSmooth { dst, kind, a, b, ctrl, v1, v2 } => {
                let r = ((regs[ctrl as usize] - regs[a as usize]) / (regs[b as usize] - regs[a as usize])).clamp(0.0, 1.0);
                regs[dst as usize] = regs[v1 as usize] + ease_num(kind, r) * (regs[v2 as usize] - regs[v1 as usize]);
            }
            NumOp::Lssht { dst, c, pv, f1, f2 } => {
                let c = regs[c as usize];
                let pv = regs[pv as usize];
                let _w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
                let m = (c * regs[f1 as usize]).exp() + (c * regs[f2 as usize]).exp();
                regs[dst as usize] = m.ln() / c;
            }
        }
    }
    prog.ops.last().map(|op| regs[op_dst(*op) as usize]).unwrap_or(0.0)
}

/// Lane-batched program run over per-lane (t, pos) inputs (u is one shared
/// scalar): one op decode per op for the whole batch instead of per entity.
/// Each lane computes exactly the ops `run` would execute with that lane's
/// inputs, in the same order, so a lane's result is bit-identical to the
/// scalar run. The builder allocates a fresh destination register for every
/// op, so `dst` is strictly greater than any operand register and
/// `split_at_mut` separates the write lanes from the read lanes.
///
/// Registers live at `regs[r * n + lane]`. Appends the n results to `out`
/// (0.0 per lane for an empty program, matching `run`). `caps` holds each
/// lane's capture vector at stride `prog.n_inputs` (empty when the program
/// takes no inputs).
pub fn run_lanes(
    prog: &NumProgram,
    u: f64,
    tau: &[f64],
    pos: &[[f64; 2]],
    caps: &[f64],
    regs: &mut Vec<f64>,
    out: &mut Vec<f64>,
) {
    let n = tau.len();
    debug_assert_eq!(pos.len(), n);
    let stride = prog.n_inputs;
    debug_assert!(stride == 0 || caps.len() >= stride * n);
    regs.clear();
    regs.resize(prog.n_regs * n, 0.0);
    for op in &prog.ops {
        let dst = op_dst(*op) as usize;
        let (src, d) = regs.split_at_mut(dst * n);
        let d = &mut d[..n];
        let at = |r: u16, l: usize| src[r as usize * n + l];
        match *op {
            NumOp::Const { v, .. } => d.fill(v),
            NumOp::Input { slot, .. } => {
                for l in 0..n {
                    d[l] = caps[l * stride + slot as usize];
                }
            }
            NumOp::T { .. } => d.copy_from_slice(tau),
            NumOp::U { .. } => d.fill(u),
            NumOp::PosX { .. } => {
                for l in 0..n {
                    d[l] = pos[l][0];
                }
            }
            NumOp::PosY { .. } => {
                for l in 0..n {
                    d[l] = pos[l][1];
                }
            }
            NumOp::Add { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l) + at(b, l);
                }
            }
            NumOp::Sub { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l) - at(b, l);
                }
            }
            NumOp::Mul { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l) * at(b, l);
                }
            }
            NumOp::Div { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l) / at(b, l);
                }
            }
            NumOp::Eq { a, b, .. } => {
                for l in 0..n {
                    d[l] = mask_num((at(a, l) - at(b, l)).abs() < 1e-9);
                }
            }
            NumOp::Lt { a, b, .. } => {
                for l in 0..n {
                    d[l] = mask_num(at(a, l) < at(b, l));
                }
            }
            NumOp::Gt { a, b, .. } => {
                for l in 0..n {
                    d[l] = mask_num(at(a, l) > at(b, l));
                }
            }
            NumOp::Lte { a, b, .. } => {
                for l in 0..n {
                    d[l] = mask_num(at(a, l) <= at(b, l));
                }
            }
            NumOp::Gte { a, b, .. } => {
                for l in 0..n {
                    d[l] = mask_num(at(a, l) >= at(b, l));
                }
            }
            NumOp::Neg { x, .. } => {
                for l in 0..n {
                    d[l] = -at(x, l);
                }
            }
            NumOp::Not { x, .. } => {
                for l in 0..n {
                    d[l] = mask_num(at(x, l) == 0.0);
                }
            }
            NumOp::Abs { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).abs();
                }
            }
            NumOp::Floor { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).floor();
                }
            }
            NumOp::Ceil { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).ceil();
                }
            }
            NumOp::Round { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).round();
                }
            }
            NumOp::Sin { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).to_radians().sin();
                }
            }
            NumOp::Cos { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).to_radians().cos();
                }
            }
            NumOp::Sqrt { x, .. } => {
                for l in 0..n {
                    d[l] = at(x, l).sqrt();
                }
            }
            NumOp::Pow { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l).powf(at(b, l));
                }
            }
            NumOp::Min { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l).min(at(b, l));
                }
            }
            NumOp::Max { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l).max(at(b, l));
                }
            }
            NumOp::Mod { a, b, .. } => {
                for l in 0..n {
                    d[l] = at(a, l).rem_euclid(at(b, l));
                }
            }
            NumOp::Quot { a, b, .. } => {
                for l in 0..n {
                    d[l] = (at(a, l) / at(b, l)).trunc();
                }
            }
            NumOp::Sine { period, amp, x, .. } => {
                for l in 0..n {
                    d[l] = at(amp, l) * (std::f64::consts::TAU * at(x, l) / at(period, l)).sin();
                }
            }
            NumOp::Lerp { a, b, ctrl, v1, v2, .. } => {
                for l in 0..n {
                    let r = ((at(ctrl, l) - at(a, l)) / (at(b, l) - at(a, l))).clamp(0.0, 1.0);
                    d[l] = at(v1, l) + r * (at(v2, l) - at(v1, l));
                }
            }
            NumOp::Lerp3 { a1, b1, a2, b2, ctrl, v1, v2, v3, .. } => {
                for l in 0..n {
                    d[l] = if at(ctrl, l) < at(a2, l) {
                        let r = ((at(ctrl, l) - at(a1, l)) / (at(b1, l) - at(a1, l))).clamp(0.0, 1.0);
                        at(v1, l) + r * (at(v2, l) - at(v1, l))
                    } else {
                        let r = ((at(ctrl, l) - at(a2, l)) / (at(b2, l) - at(a2, l))).clamp(0.0, 1.0);
                        at(v2, l) + r * (at(v3, l) - at(v2, l))
                    };
                }
            }
            NumOp::Ease { kind, x, .. } => {
                for l in 0..n {
                    d[l] = ease_num(kind, at(x, l));
                }
            }
            NumOp::LerpSmooth { kind, a, b, ctrl, v1, v2, .. } => {
                for l in 0..n {
                    let r = ((at(ctrl, l) - at(a, l)) / (at(b, l) - at(a, l))).clamp(0.0, 1.0);
                    d[l] = at(v1, l) + ease_num(kind, r) * (at(v2, l) - at(v1, l));
                }
            }
            NumOp::Lssht { c, pv, f1, f2, .. } => {
                for l in 0..n {
                    let c = at(c, l);
                    let pv = at(pv, l);
                    let _w = 1.0 / (1.0 + (c.abs() * 4.0 * (pv - pv)).exp());
                    let m = (c * at(f1, l)).exp() + (c * at(f2, l)).exp();
                    d[l] = m.ln() / c;
                }
            }
        }
    }
    match prog.ops.last() {
        Some(op) => {
            let base = op_dst(*op) as usize * n;
            out.extend_from_slice(&regs[base..base + n]);
        }
        None => out.extend(std::iter::repeat(0.0).take(n)),
    }
}

fn mask_num(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn ease_num(kind: EaseKind, r: f64) -> f64 {
    use std::f64::consts::FRAC_PI_2;
    let r = r.clamp(0.0, 1.0);
    match kind {
        EaseKind::InSine => 1.0 - (r * FRAC_PI_2).cos(),
        EaseKind::OutSine => (r * FRAC_PI_2).sin(),
        EaseKind::InOutSine => 0.5 - 0.5 * (r * std::f64::consts::PI).cos(),
    }
}

pub fn oracle_enabled() -> bool {
    #[cfg(test)]
    {
        if ORACLE_TEST_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
    }
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("MAKU_LOWER_ORACLE").as_deref() == Ok("1"))
}

#[cfg(test)]
static ORACLE_TEST_OVERRIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn set_oracle_for_tests(enabled: bool) {
    ORACLE_TEST_OVERRIDE.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(src: &str) -> Form {
        crate::edn::read_one(src).unwrap()
    }

    fn no_defs() -> HashMap<String, Form> {
        HashMap::new()
    }

    fn defs(pairs: &[(&str, &str)]) -> HashMap<String, Form> {
        pairs
            .iter()
            .map(|(name, form)| ((*name).to_string(), read(form)))
            .collect()
    }

    fn eval_num(src: &str, env: &Env, defs: &HashMap<String, Form>, t: f64, u: f64) -> f64 {
        let mut ctx = Ctx::default();
        ctx.sig.defs = Rc::new(defs.clone());
        let eval_env = env.bind("t".into(), Val::Num(t)).bind("u".into(), Val::Num(u));
        evaluate(&read(src), &eval_env, &mut ctx, &mut World::for_eval(60.0))
            .unwrap()
            .num()
            .unwrap()
    }

    #[test]
    fn lowers_basic_t_arithmetic() {
        let prog = lower_num_form(&read("(* 2 t)"), &Env::empty(), &no_defs()).unwrap();
        assert_eq!(run_num_program(&prog, 3.0, 0.0, None), 6.0);
    }

    #[test]
    fn lowers_numeric_capture() {
        let env = Env::empty().bind("speed".into(), Val::Num(4.0));
        let prog = lower_num_form(&read("(* speed t)"), &env, &no_defs()).unwrap();
        assert_eq!(run_num_program(&prog, 2.5, 0.0, None), 10.0);
    }

    #[test]
    fn bails_on_unlowerable_forms() {
        let env = Env::empty();
        assert!(lower_num_form(&read("(user-fn t)"), &env, &no_defs()).is_none());
        assert!(lower_num_form(&read("(slew t 1)"), &env, &no_defs()).is_none());

        let t_env = Env::empty().bind("t".into(), Val::Num(9.0));
        assert!(lower_num_form(&read("t"), &t_env, &no_defs()).is_none());

        let bad_env = Env::empty().bind("target".into(), Val::Pose(Pose::point(1.0, 2.0)));
        assert!(lower_num_form(&read("target"), &bad_env, &no_defs()).is_none());

        // a card defn named after a builtin shadows it at eval time
        let mut defs = HashMap::new();
        defs.insert("sin".to_string(), read("(fn [x] x)"));
        let prog = lower_num_form(&read("(sin t)"), &Env::empty(), &defs).unwrap();
        assert_eq!(run_num_program(&prog, 45.0, 0.0, None), 45.0);
        assert!(lower_num_form(&read("(* 2 t)"), &Env::empty(), &defs).is_some());

        defs.insert("$ch".to_string(), read("(fn [x] x)"));
        defs.insert("inf".to_string(), read("(fn [x] x)"));
        assert!(lower_num_form(&read("($ch t)"), &Env::empty(), &defs).is_none());
        assert!(lower_num_form(&read("(inf t)"), &Env::empty(), &defs).is_none());
    }

    #[test]
    fn inlines_defn_helper_call() {
        let defs = defs(&[("half", "(fn [x] (/ x 2))")]);
        let prog = lower_num_form(&read("(half t)"), &Env::empty(), &defs).unwrap();
        let got = run_num_program(&prog, 9.0, 0.0, None);
        let want = eval_num("(half t)", &Env::empty(), &defs, 9.0, 0.0);
        assert_eq!(got, want);
    }

    #[test]
    fn inlines_bare_numeric_def() {
        let defs = defs(&[("speed2", "3")]);
        let prog = lower_num_form(&read("(* speed2 t)"), &Env::empty(), &defs).unwrap();
        assert_eq!(run_num_program(&prog, 4.0, 0.0, None), 12.0);
    }

    #[test]
    fn inlines_def_chain_and_bails_on_cycle() {
        let chain_defs = defs(&[("base", "3"), ("speed2", "(* base 2)")]);
        let prog = lower_num_form(&read("(* speed2 t)"), &Env::empty(), &chain_defs).unwrap();
        assert_eq!(run_num_program(&prog, 4.0, 0.0, None), 24.0);

        let cyclic = defs(&[("a", "b"), ("b", "a")]);
        assert!(lower_num_form(&read("a"), &Env::empty(), &cyclic).is_none());
    }

    #[test]
    fn def_scope_does_not_see_pos_or_caller_env() {
        let pos_defs = defs(&[("xpos", "(:x pos)")]);
        assert!(lower_num_form(&read("xpos"), &Env::empty(), &pos_defs).is_none());

        let env = Env::empty().bind("speed".into(), Val::Num(4.0));
        let caller_defs = defs(&[("uses-speed", "(* speed 2)")]);
        assert!(lower_num_form(&read("uses-speed"), &env, &caller_defs).is_none());
    }

    #[test]
    fn cell_scope_does_not_disable_def_inlining() {
        // signals read cells only via (live name) (Ctx.signal_scope), so a
        // captured cell scope cannot shadow defs during signal evaluation
        let env = Env::empty().bind(CELLS_KEY.into(), fresh_cell_scope());
        let defs = defs(&[("speed2", "3")]);
        let prog = lower_num_form(&read("(* speed2 t)"), &env, &defs).unwrap();
        assert_eq!(run_num_program(&prog, 2.0, 0.0, None), 6.0);
    }

    #[test]
    fn env_shadowed_def_still_wins() {
        let env = Env::empty().bind("speed2".into(), Val::Num(5.0));
        let defs = defs(&[("speed2", "3")]);
        let prog = lower_num_form(&read("(* speed2 t)"), &env, &defs).unwrap();
        assert_eq!(run_num_program(&prog, 4.0, 0.0, None), 20.0);
    }

    #[test]
    fn lowers_lerpsmooth_with_static_easing() {
        for ez in ["einsine", "eoutsine", "eiosine"] {
            let src = format!("(lerpsmooth {} 0 4 t 0 480)", ez);
            let prog = lower_num_form(&read(&src), &Env::empty(), &no_defs()).unwrap();
            for t in [-1.0, 0.0, 1.3, 4.0, 9.0] {
                let got = run_num_program(&prog, t, 0.0, None);
                let want = eval_num(&src, &Env::empty(), &no_defs(), t, 0.0);
                assert!((got - want).abs() <= 1e-12, "{} t={}: {} vs {}", ez, t, got, want);
            }
        }
    }

    #[test]
    fn lerpsmooth_easing_resolution() {
        // captured Val::Builtin under another name folds
        let env = Env::empty().bind("ez".into(), Val::Builtin("eoutsine".into()));
        let prog = lower_num_form(&read("(lerpsmooth ez 0 1 t 0 10)"), &env, &no_defs()).unwrap();
        let want = eval_num("(lerpsmooth ez 0 1 t 0 10)", &env, &no_defs(), 0.5, 0.0);
        assert_eq!(run_num_program(&prog, 0.5, 0.0, None), want);

        // def-shadowed easing name bails; non-builtin capture bails;
        // non-sym easing arg bails
        let shadow = defs(&[("eoutsine", "(fn [x] x)")]);
        assert!(lower_num_form(&read("(lerpsmooth eoutsine 0 1 t 0 10)"), &Env::empty(), &shadow).is_none());
        let bad = Env::empty().bind("ez".into(), Val::Num(1.0));
        assert!(lower_num_form(&read("(lerpsmooth ez 0 1 t 0 10)"), &bad, &no_defs()).is_none());
        assert!(lower_num_form(&read("(lerpsmooth (pick-ease) 0 1 t 0 10)"), &Env::empty(), &no_defs()).is_none());
    }

    #[test]
    fn lowers_pos_component_reads() {
        let prog = lower_num_form(&read("(+ (:x pos) (:y pos))"), &Env::empty(), &no_defs()).unwrap();
        assert!(program_uses_pos(&prog));
        assert_eq!(run_num_program(&prog, 0.0, 0.0, Some((3.0, 4.0))), 7.0);

        // captured pos shadows the slot at non-pos eval sites: ambiguous, bail
        let shadowed = Env::empty().bind("pos".into(), Val::Pose(Pose::point(1.0, 2.0)));
        assert!(lower_num_form(&read("(:x pos)"), &shadowed, &no_defs()).is_none());
        // no theta op
        assert!(lower_num_form(&read("(:th pos)"), &Env::empty(), &no_defs()).is_none());
    }

    #[test]
    fn folds_captured_keyword_reads() {
        let exit = Val::Map(Rc::new(vec![(
            Val::Kw("vel".into()),
            Val::Pose(Pose::point(1.5, -2.0)),
        )]));
        let env = Env::empty()
            .bind("exit".into(), exit)
            .bind("delta".into(), Val::Pose(Pose::point(6.0, 8.0)));

        let prog = lower_num_form(&read("(* (:x (:vel exit)) t)"), &env, &no_defs()).unwrap();
        assert_eq!(run_num_program(&prog, 2.0, 0.0, None), 3.0);
        let prog = lower_num_form(&read("(:y delta)"), &env, &no_defs()).unwrap();
        assert_eq!(run_num_program(&prog, 0.0, 0.0, None), 8.0);
        // pointless pose has no theta: angle_or(0.0), same as the interpreter
        let prog = lower_num_form(&read("(:th delta)"), &env, &no_defs()).unwrap();
        assert_eq!(
            run_num_program(&prog, 0.0, 0.0, None),
            eval_num("(:th delta)", &env, &no_defs(), 0.0, 0.0)
        );

        // missing key, unbound base, non-num terminal: bail
        assert!(lower_num_form(&read("(:speed exit)"), &env, &no_defs()).is_none());
        assert!(lower_num_form(&read("(:x (:vel nothere))"), &env, &no_defs()).is_none());
        assert!(lower_num_form(&read("(:vel exit)"), &env, &no_defs()).is_none());
    }

    #[test]
    fn lanes_match_scalar_run() {
        // every op class the lowering can emit, exercised across lanes with
        // distinct (t, pos) inputs — the batch runner must be bit-identical
        // to the scalar runner per lane
        let srcs = [
            "(+ (* 2 t) (- (:x pos) (/ (:y pos) 3)))",
            "(min (max t 2) (mod t 7))",
            "(sine 12.94 2 t)",
            "(lerp 0.3 1.4 t 0 2.6)",
            "(lerp3 0 1 1 2 t 0 5 9)",
            "(lerpsmooth eiosine 0 4 t 0 480)",
            "(if (< t 3) (sqrt (abs t)) (pow t 0.5))",
            "(quot (floor (* t 3)) (ceil (+ t 0.1)))",
            "(einsine (mod t 1))",
            "(sin (* 10 t))",
            "(round (cos t))",
        ];
        for src in srcs {
            let Some(prog) = lower_num_form(&read(src), &Env::empty(), &no_defs()) else {
                continue;
            };
            let tau: Vec<f64> = (0..17).map(|i| i as f64 * 0.37 - 2.0).collect();
            let pos: Vec<[f64; 2]> = (0..17).map(|i| [i as f64 * 1.3, 5.0 - i as f64]).collect();
            let mut regs = Vec::new();
            let mut out = Vec::new();
            run_lanes(&prog, 0.25, &tau, &pos, &[], &mut regs, &mut out);
            for l in 0..tau.len() {
                let want = run_num_program(&prog, tau[l], 0.25, Some((pos[l][0], pos[l][1])));
                let got = out[l];
                assert!(
                    got == want || (got.is_nan() && want.is_nan()),
                    "{src} lane {l}: batch {got} vs scalar {want}"
                );
            }
        }
    }

    #[test]
    fn defn_arity_mismatch_bails() {
        let defs = defs(&[("half", "(fn [x] (/ x 2))")]);
        assert!(lower_num_form(&read("(half t 1)"), &Env::empty(), &defs).is_none());
    }
}
