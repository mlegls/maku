use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug)]
pub struct NumProgram {
    pub ops: Vec<NumOp>,
    pub n_regs: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum NumOp {
    Const { dst: u16, v: f64 },
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
                    Some(Val::Num(v)) => {
                        let dst = self.reg()?;
                        self.push(NumOp::Const { dst, v })
                    }
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
        let Some(Form::Sym(head)) = items.first() else {
            return None;
        };
        let name = &**head;
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
        if matches!(scope, LowerScope::Current) && items.len() == 3 && matches!(name, ":x" | ":y") {
            let Form::Sym(sym) = &items[2] else {
                return None;
            };
            if &**sym != "pos" {
                return None;
            }
            let dst = self.reg()?;
            return if name == ":x" {
                self.push(NumOp::PosX { dst })
            } else {
                self.push(NumOp::PosY { dst })
            };
        }

        let args = items[1..]
            .iter()
            .map(|f| self.lower(f, env, scope))
            .collect::<Option<Vec<_>>>()?;
        self.lower_call(name, &args)
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

fn special_or_channel_head(name: &str) -> bool {
    matches!(name, "t" | "u" | "inf" | "phi") || name.starts_with('$')
}

pub fn lower_num_form(form: &Form, env: &Env, defs: &HashMap<String, Form>) -> Option<NumProgram> {
    // Def inlining needs no cell-scope guard: signal evaluation skips bare
    // cell reads (Ctx.signal_scope — signals read cells via (live name)
    // only), so a def name can never be shadowed by a cell at runtime.
    let mut b = Builder { ops: Vec::new(), next: 0, defs, inline_depth: 0 };
    b.lower(form, env, LowerScope::Current)?;
    Some(NumProgram { ops: b.ops, n_regs: b.next as usize })
}

pub fn program_uses_pos(prog: &NumProgram) -> bool {
    prog.ops.iter().any(|op| matches!(op, NumOp::PosX { .. } | NumOp::PosY { .. }))
}

thread_local! {
    static REGS: RefCell<Vec<f64>> = RefCell::new(Vec::new());
}

pub fn run_num_program(prog: &NumProgram, t: f64, u: f64, pos: Option<(f64, f64)>) -> f64 {
    REGS.with(|regs| {
        let mut regs = regs.borrow_mut();
        run(prog, t, u, pos, &mut regs)
    })
}

pub fn run(prog: &NumProgram, t: f64, u: f64, pos: Option<(f64, f64)>, regs: &mut Vec<f64>) -> f64 {
    regs.clear();
    regs.resize(prog.n_regs, 0.0);
    for op in &prog.ops {
        match *op {
            NumOp::Const { dst, v } => regs[dst as usize] = v,
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
    fn defn_arity_mismatch_bails() {
        let defs = defs(&[("half", "(fn [x] (/ x 2))")]);
        assert!(lower_num_form(&read("(half t 1)"), &Env::empty(), &defs).is_none());
    }
}
