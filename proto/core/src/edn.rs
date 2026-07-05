//! EDN reader for the pattern language (language.md §11).
//!
//! Emits canonical trees: reader shorthands desugar at read time —
//! `c[x y]` → `(cart x y)`, `p[r θ]` → `(polar r θ)`, `m"…"` → the parsed
//! s-expr form (the math macro is parse-only; the canonical tree is what
//! this reader returns).

use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Num(f64),
    Str(Rc<str>),
    Sym(Rc<str>),
    Kw(Rc<str>),
    Bool(bool),
    List(Rc<[Form]>),
    Vector(Rc<[Form]>),
    Map(Rc<[(Form, Form)]>), // insertion order preserved (maps are option/meta records)
}

impl Form {
    pub fn list(items: Vec<Form>) -> Form {
        Form::List(items.into())
    }
    pub fn sym(s: &str) -> Form {
        Form::Sym(s.into())
    }
    pub fn kw(s: &str) -> Form {
        Form::Kw(s.into())
    }
    pub fn call(head: &str, args: Vec<Form>) -> Form {
        let mut v = vec![Form::sym(head)];
        v.extend(args);
        Form::list(v)
    }
}

impl fmt::Display for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Form::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }
            Form::Str(s) => write!(f, "{:?}", s),
            Form::Sym(s) => write!(f, "{}", s),
            Form::Kw(s) => write!(f, ":{}", s),
            Form::Bool(b) => write!(f, "{}", b),
            Form::List(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, ")")
            }
            Form::Vector(xs) => {
                write!(f, "[")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, "]")
            }
            Form::Map(kvs) => {
                write!(f, "{{")?;
                for (i, (k, v)) in kvs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{} {}", k, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

#[derive(Debug)]
pub struct ReadError {
    pub msg: String,
    pub pos: usize,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "read error at byte {}: {}", self.pos, self.msg)
    }
}

pub struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

pub fn read_all(src: &str) -> Result<Vec<Form>, ReadError> {
    let mut r = Reader { src: src.as_bytes(), pos: 0 };
    let mut out = Vec::new();
    loop {
        r.skip_ws();
        if r.at_end() {
            return Ok(out);
        }
        out.push(r.read_form()?);
    }
}

pub fn read_one(src: &str) -> Result<Form, ReadError> {
    let forms = read_all(src)?;
    match forms.len() {
        1 => Ok(forms.into_iter().next().unwrap()),
        n => Err(ReadError { msg: format!("expected one form, got {}", n), pos: 0 }),
    }
}

impl<'a> Reader<'a> {
    fn at_end(&self) -> bool {
        self.pos >= self.src.len()
    }
    fn peek(&self) -> u8 {
        if self.at_end() { 0 } else { self.src[self.pos] }
    }
    fn peek2(&self) -> u8 {
        if self.pos + 1 >= self.src.len() { 0 } else { self.src[self.pos + 1] }
    }
    fn bump(&mut self) -> u8 {
        let c = self.peek();
        self.pos += 1;
        c
    }
    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ReadError> {
        Err(ReadError { msg: msg.into(), pos: self.pos })
    }

    fn skip_ws(&mut self) {
        loop {
            while !self.at_end() && matches!(self.peek(), b' ' | b'\t' | b'\n' | b'\r' | b',') {
                self.pos += 1;
            }
            if self.peek() == b';' {
                while !self.at_end() && self.peek() != b'\n' {
                    self.pos += 1;
                }
            } else {
                return;
            }
        }
    }

    fn read_form(&mut self) -> Result<Form, ReadError> {
        self.skip_ws();
        match self.peek() {
            0 => self.err("unexpected end of input"),
            b'(' => self.read_seq(b')').map(Form::List),
            b'[' => self.read_seq(b']').map(Form::Vector),
            b'{' => self.read_map(),
            b'"' => self.read_string().map(|s| Form::Str(s.into())),
            b':' => {
                self.bump();
                let s = self.read_symbol_chars()?;
                Ok(Form::Kw(s.into()))
            }
            b'$' => {
                self.bump();
                let s = self.read_symbol_chars()?;
                Ok(desugar_dotted(format!("${}", s)))
            }
            b'c' | b'p' if self.peek2() == b'[' => {
                let head = if self.bump() == b'c' { "cart" } else { "polar" };
                let items = self.read_seq(b']')?;
                let mut v = vec![Form::sym(head)];
                v.extend(items.iter().cloned());
                Ok(Form::list(v))
            }
            b'`' => {
                // quasiquote: a form template for macros (§11)
                self.bump();
                let f = self.read_form()?;
                Ok(Form::list(vec![Form::sym("quasiquote"), f]))
            }
            b'\'' => {
                self.bump();
                let f = self.read_form()?;
                Ok(Form::list(vec![Form::sym("quote"), f]))
            }
            b'~' => {
                self.bump();
                if self.peek() == b'@' {
                    self.bump();
                    let f = self.read_form()?;
                    Ok(Form::list(vec![Form::sym("unquote-splicing"), f]))
                } else {
                    let f = self.read_form()?;
                    Ok(Form::list(vec![Form::sym("unquote"), f]))
                }
            }
            b'm' if self.peek2() == b'"' => {
                self.bump();
                let s = self.read_string()?;
                let start = self.pos;
                math::parse(&s).map_err(|msg| ReadError { msg: format!("in m-string: {}", msg), pos: start })
            }
            c if c == b'-' && self.peek2().is_ascii_digit() => self.read_number(),
            c if c.is_ascii_digit() => self.read_number(),
            _ => {
                let s = self.read_symbol_chars()?;
                Ok(match s.as_str() {
                    "true" => Form::Bool(true),
                    "false" => Form::Bool(false),
                    _ => desugar_dotted(s),
                })
            }
        }
    }

    fn read_seq(&mut self, close: u8) -> Result<Rc<[Form]>, ReadError> {
        self.bump(); // opener
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.at_end() {
                return self.err(format!("unclosed sequence, expected '{}'", close as char));
            }
            if self.peek() == close {
                self.bump();
                return Ok(items.into());
            }
            items.push(self.read_form()?);
        }
    }

    fn read_map(&mut self) -> Result<Form, ReadError> {
        let items = self.read_seq(b'}')?;
        if items.len() % 2 != 0 {
            return self.err("map literal with odd number of forms");
        }
        let kvs: Vec<(Form, Form)> =
            items.chunks(2).map(|c| (c[0].clone(), c[1].clone())).collect();
        Ok(Form::Map(kvs.into()))
    }

    fn read_string(&mut self) -> Result<String, ReadError> {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.bump() {
                0 => return self.err("unclosed string"),
                b'"' => return Ok(s),
                b'\\' => match self.bump() {
                    b'n' => s.push('\n'),
                    b't' => s.push('\t'),
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    c => return self.err(format!("bad escape '\\{}'", c as char)),
                },
                c => s.push(c as char),
            }
        }
    }

    fn read_number(&mut self) -> Result<Form, ReadError> {
        let start = self.pos;
        if self.peek() == b'-' {
            self.bump();
        }
        while self.peek().is_ascii_digit() {
            self.bump();
        }
        if self.peek() == b'.' && self.peek2().is_ascii_digit() {
            self.bump();
            while self.peek().is_ascii_digit() {
                self.bump();
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        match text.parse::<f64>() {
            Ok(n) => Ok(Form::Num(n)),
            Err(_) => self.err(format!("bad number '{}'", text)),
        }
    }

    fn read_symbol_chars(&mut self) -> Result<String, ReadError> {
        let start = self.pos;
        while !self.at_end() {
            let c = self.peek();
            // '&' is a symbol char solely for the `& rest` param marker
            if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'*' | b'+' | b'/' | b'<' | b'>' | b'=' | b'!' | b'?' | b'.' | b'&') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return self.err(format!("unexpected character '{}'", self.peek() as char));
        }
        Ok(std::str::from_utf8(&self.src[start..self.pos]).unwrap().to_string())
    }
}

/// The math macro: parse-only infix (language.md §11). Emits the same
/// canonical trees as hand-written s-exprs.
pub mod math {
    use super::Form;

    #[derive(Debug, PartialEq, Clone)]
    enum Tok {
        Num(f64),
        Ident(String),
        Op(&'static str),
        LParen,
        RParen,
        LBracket,
        RBracket,
        Comma,
        Dot,
        CoordCart,  // c[
        CoordPolar, // p[
    }

    fn lex(src: &str) -> Result<Vec<Tok>, String> {
        let b = src.as_bytes();
        let mut i = 0;
        let mut out = Vec::new();
        while i < b.len() {
            let c = b[i];
            match c {
                b' ' | b'\t' | b'\n' | b',' if c == b',' => {
                    out.push(Tok::Comma);
                    i += 1;
                }
                b' ' | b'\t' | b'\n' => i += 1,
                b'(' => {
                    out.push(Tok::LParen);
                    i += 1;
                }
                b')' => {
                    out.push(Tok::RParen);
                    i += 1;
                }
                b'[' => {
                    out.push(Tok::LBracket);
                    i += 1;
                }
                b']' => {
                    out.push(Tok::RBracket);
                    i += 1;
                }
                b'+' => {
                    out.push(Tok::Op("+"));
                    i += 1;
                }
                b'-' => {
                    out.push(Tok::Op("-"));
                    i += 1;
                }
                b'*' => {
                    out.push(Tok::Op("*"));
                    i += 1;
                }
                b'/' => {
                    out.push(Tok::Op("/"));
                    i += 1;
                }
                b'^' => {
                    out.push(Tok::Op("^"));
                    i += 1;
                }
                b'%' => {
                    out.push(Tok::Op("%"));
                    i += 1;
                }
                b'<' | b'>' => {
                    if i + 1 < b.len() && b[i + 1] == b'=' {
                        out.push(Tok::Op(if c == b'<' { "<=" } else { ">=" }));
                        i += 2;
                    } else {
                        out.push(Tok::Op(if c == b'<' { "<" } else { ">" }));
                        i += 1;
                    }
                }
                b'=' if i + 1 < b.len() && b[i + 1] == b'=' => {
                    out.push(Tok::Op("=="));
                    i += 2;
                }
                _ if c.is_ascii_digit()
                    || (c == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) =>
                {
                    let start = i;
                    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                        i += 1;
                    }
                    let text = &src[start..i];
                    out.push(Tok::Num(text.parse().map_err(|_| format!("bad number '{}'", text))?));
                }
                b'.' => {
                    out.push(Tok::Dot);
                    i += 1;
                }
                b'$' => {
                    if i + 1 < b.len() && b[i + 1] == b'(' {
                        return Err("$(...) splice not yet implemented in math".into());
                    }
                    let start = i + 1;
                    i += 1;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'-')) {
                        i += 1;
                    }
                    out.push(Tok::Ident(format!("${}", &src[start..i])));
                }
                _ if c.is_ascii_alphabetic() || c == b'_' || c == b':' => {
                    let start = i;
                    while i < b.len()
                        && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'-' | b':' | b'!' | b'?'))
                    {
                        // '-' only counts as part of an identifier if followed by a letter
                        // (so `a-b` lexes as ident minus ident is impossible in math mode;
                        // kebab identifiers must be referenced via $() -- math idents are
                        // alnum/underscore)
                        if b[i] == b'-' {
                            break;
                        }
                        i += 1;
                    }
                    // coordinate literal heads
                    if i < b.len() && b[i] == b'[' && (&src[start..i] == "c" || &src[start..i] == "p") {
                        out.push(if &src[start..i] == "c" { Tok::CoordCart } else { Tok::CoordPolar });
                        i += 1; // consume '['
                    } else {
                        out.push(Tok::Ident(src[start..i].to_string()));
                    }
                }
                _ => return Err(format!("unexpected character '{}' in math", c as char)),
            }
        }
        Ok(out)
    }

    struct P {
        toks: Vec<Tok>,
        i: usize,
    }

    pub fn parse(src: &str) -> Result<Form, String> {
        let mut p = P { toks: lex(src)?, i: 0 };
        let e = p.expr(0)?;
        if p.i != p.toks.len() {
            return Err(format!("trailing tokens in math expression at {}", p.i));
        }
        Ok(e)
    }

    fn bp(op: &str) -> (u8, u8) {
        match op {
            "==" | "<" | ">" | "<=" | ">=" => (1, 2),
            "+" | "-" => (3, 4),
            "*" | "/" | "%" => (5, 6),
            "^" => (8, 7), // right-assoc
            _ => (0, 0),
        }
    }

    impl P {
        fn peek(&self) -> Option<&Tok> {
            self.toks.get(self.i)
        }
        fn bump(&mut self) -> Option<Tok> {
            let t = self.toks.get(self.i).cloned();
            if t.is_some() {
                self.i += 1;
            }
            t
        }

        fn expr(&mut self, min_bp: u8) -> Result<Form, String> {
            let mut lhs = self.postfix()?;
            loop {
                let op = match self.peek() {
                    Some(Tok::Op(o)) => *o,
                    _ => break,
                };
                let (lbp, rbp) = bp(op);
                if lbp < min_bp || lbp == 0 {
                    break;
                }
                self.bump();
                let rhs = self.expr(rbp)?;
                let head = match op {
                    "==" => "=",
                    "^" => "pow",
                    "%" => "mod",
                    o => o,
                };
                lhs = Form::call(head, vec![lhs, rhs]);
            }
            Ok(lhs)
        }

        /// atom followed by any chain of postfix accessors, all introduced
        /// by `.`: `.field` reads, `.[index]` gathers -- `nth(bs, 0).pos.y`,
        /// `xs.[0 1]`, `xs.[iota(3)]`. Bare `[` stays a literal (arrays,
        /// c[...]/p[...] coords), so there is no ident-bracket ambiguity.
        /// Desugars to keyword application / (nth ...) -- cyclic nth
        /// broadcasts, so an array index selects many.
        fn postfix(&mut self) -> Result<Form, String> {
            let mut e = self.atom()?;
            while let Some(Tok::Dot) = self.peek() {
                self.bump();
                match self.bump() {
                    Some(Tok::Ident(field)) => {
                        e = Form::list(vec![Form::Kw(field.into()), e]);
                    }
                    Some(Tok::LBracket) => {
                        let items = self.seq_until(Tok::RBracket)?;
                        let idx = match items.len() {
                            1 => items.into_iter().next().unwrap(),
                            _ => Form::Vector(items.into()),
                        };
                        e = Form::call("nth", vec![e, idx]);
                    }
                    t => return Err(format!("expected field or [index] after '.', got {:?}", t)),
                }
            }
            Ok(e)
        }

        fn atom(&mut self) -> Result<Form, String> {
            match self.bump() {
                Some(Tok::Num(n)) => Ok(Form::Num(n)),
                Some(Tok::Op("-")) => {
                    let e = self.expr(7)?; // tighter than * /
                    Ok(match e {
                        Form::Num(n) => Form::Num(-n),
                        e => Form::call("-", vec![Form::Num(0.0), e]),
                    })
                }
                Some(Tok::LParen) => {
                    let e = self.expr(0)?;
                    match self.bump() {
                        Some(Tok::RParen) => Ok(e),
                        _ => Err("expected ')'".into()),
                    }
                }
                Some(Tok::LBracket) => {
                    let items = self.seq_until(Tok::RBracket)?;
                    Ok(Form::Vector(items.into()))
                }
                Some(Tok::CoordCart) => {
                    let items = self.seq_until(Tok::RBracket)?;
                    let mut v = vec![Form::sym("cart")];
                    v.extend(items);
                    Ok(Form::list(v))
                }
                Some(Tok::CoordPolar) => {
                    let items = self.seq_until(Tok::RBracket)?;
                    let mut v = vec![Form::sym("polar")];
                    v.extend(items);
                    Ok(Form::list(v))
                }
                Some(Tok::Ident(name)) => {
                    if name.starts_with(':') {
                        return Ok(Form::Kw(name[1..].into()));
                    }
                    if self.peek() == Some(&Tok::LParen) {
                        self.bump();
                        let args = self.seq_until(Tok::RParen)?;
                        let mut v = vec![Form::Sym(name.into())];
                        v.extend(args);
                        Ok(Form::list(v))
                    } else {
                        Ok(Form::Sym(name.into()))
                    }
                }
                t => Err(format!("unexpected token in math: {:?}", t)),
            }
        }

        /// comma- (or juxtaposition-) separated expressions until `close`.
        fn seq_until(&mut self, close: Tok) -> Result<Vec<Form>, String> {
            let mut items = Vec::new();
            loop {
                if self.peek() == Some(&close) {
                    self.bump();
                    return Ok(items);
                }
                if self.peek().is_none() {
                    return Err(format!("unclosed sequence, expected {:?}", close));
                }
                items.push(self.expr(0)?);
                if self.peek() == Some(&Tok::Comma) {
                    self.bump();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(src: &str) -> String {
        read_one(src).unwrap().to_string()
    }

    #[test]
    fn basic_forms() {
        assert_eq!(rt("(spawn x {:a 1})"), "(spawn x {:a 1})");
        assert_eq!(rt("[1 2 3]"), "[1 2 3]");
        assert_eq!(rt(":family"), ":family");
        assert_eq!(rt("-4.5"), "-4.5");
        assert_eq!(rt("\"gem\""), "\"gem\"");
    }

    #[test]
    fn comments_and_commas() {
        assert_eq!(rt("(a, b ; comment\n c)"), "(a b c)");
    }

    #[test]
    fn coord_literals() {
        assert_eq!(rt("c[0 2]"), "(cart 0 2)");
        assert_eq!(rt("p[4 90]"), "(polar 4 90)");
        assert_eq!(rt("c[(lerp 0.4 0.8 t 12 2) 0]"), "(cart (lerp 0.4 0.8 t 12 2) 0)");
    }

    #[test]
    fn math_macro_canonical() {
        // m"" is parse-only: canonical tree == hand-written s-exprs
        assert_eq!(rt("m\"0.2*(i+1)*(i+2)\""), "(* (* 0.2 (+ i 1)) (+ i 2))");
        assert_eq!(rt("m\"120*vol + shot*40/7\""), "(+ (* 120 vol) (/ (* shot 40) 7))");
        assert_eq!(rt("m\"2 + 0.3*shot\""), "(+ 2 (* 0.3 shot))");
    }

    #[test]
    fn math_calls_and_arrays() {
        assert_eq!(rt("m\"60*iota(6) + 120*t\""), "(+ (* 60 (iota 6)) (* 120 t))");
        assert_eq!(
            rt("m\"sine(2.1, 18, t + 1.1*u)\""),
            "(sine 2.1 18 (+ t (* 1.1 u)))"
        );
        assert_eq!(rt("m\"[0 120 240] + 80*t\""), "(+ [0 120 240] (* 80 t))");
        assert_eq!(rt("m\"-14*u\""), "(* -14 u)");
        assert_eq!(rt("m\"240 * ud * live(mode)\""), "(* (* 240 ud) (live mode))");
    }

    #[test]
    fn math_precedence() {
        assert_eq!(rt("m\"1 + 2 * 3\""), "(+ 1 (* 2 3))");
        assert_eq!(rt("m\"(1 + 2) * 3\""), "(* (+ 1 2) 3)");
        assert_eq!(rt("m\"a == 1\""), "(= a 1)");
        assert_eq!(rt("m\"2^3^2\""), "(pow 2 (pow 3 2))");
    }

    #[test]
    fn whole_pattern_reads() {
        let src = r#"
(defpattern bowap [speed 4.0 arms 5 period (ticks 8)]
  ((pose c[0 2])
    (dotimes [i inf :every period]
      (spawn ((rot m"0.2*(i+1)*(i+2)")
               (circle arms (linear c[speed 0])))
             {:style {:family :gem :variant :w
                      :color [:yellow :orange :red :pink :purple]}}))))
"#;
        let form = read_one(src).unwrap();
        let printed = form.to_string();
        assert!(printed.starts_with("(defpattern bowap"));
        assert!(printed.contains("(rot (* (* 0.2 (+ i 1)) (+ i 2)))"));
        assert!(printed.contains("{:family :gem :variant :w"));
    }
}

/// Accessor sugar: a symbol containing dots reads as a field chain —
/// `b.pos.y` desugars in the READER to `(:y (:pos b))`, so the canonical
/// tree is ordinary keyword application (card transformations never see
/// dots). Malformed chains (empty segments) stay plain symbols.
fn desugar_dotted(s: String) -> Form {
    if !s.contains('.') {
        return Form::Sym(s.into());
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Form::Sym(s.into());
    }
    let mut form = Form::Sym(parts[0].into());
    for field in &parts[1..] {
        form = Form::list(vec![Form::Kw((*field).into()), form]);
    }
    form
}

// ---------------------------------------------------------------------------
// imports: (import "relative/path.dmk") on its own line splices that card's
// text at this position (recursively, include-once). Textual include with
// dedup: the importing file's own later defs shadow imported ones, matching
// ordinary def ordering. Expansion happens at file-load time, so the wire
// card source stays self-contained and run/add/swap need no path context.

use std::collections::HashSet;
use std::path::Path;

/// Library imports: a BARE name — no '/' and no ".dmk" — is a stdlib
/// import, `(import "touhou")`. It canonicalizes to `@lib/<name>.dmk`
/// (the include-once key) and resolves from STDLIB below — never from a
/// reader — so every host, native or wasm, sees the same library.
/// Path imports stay relative to the importing file.
const LIB_PREFIX: &str = "@lib/";

/// The standard library, inlined at COMPILE time: authored as separate
/// files under cards/lib/, shipped inside the engine artifact (users
/// import it; they don't edit it). One entry per library card.
const STDLIB: &[(&str, &str)] = &[
    ("@lib/prelude.dmk", include_str!("../../../cards/lib/prelude.dmk")),
    ("@lib/touhou.dmk", include_str!("../../../cards/lib/touhou.dmk")),
    ("@lib/player-rig.dmk", include_str!("../../../cards/lib/player-rig.dmk")),
];

/// The prelude is AUTOIMPORTED: every top-level expansion prepends it
/// unless the expanded source already carries the sentinel (its first
/// line), which keeps re-expansion of an already-expanded source — and
/// an explicit (import "prelude") — idempotent. Definition order doesn't
/// matter (macros/defs resolve at evaluation), so prepending after the
/// fact is sound.
const PRELUDE_KEY: &str = "@lib/prelude.dmk";
const PRELUDE_SENTINEL: &str = ";;@prelude";

fn with_prelude(
    expanded: String,
    read: &dyn Fn(&str) -> Result<String, String>,
    visited: &mut HashSet<String>,
) -> Result<String, String> {
    if expanded.contains(PRELUDE_SENTINEL) {
        return Ok(expanded);
    }
    let mut out = expand_inner(PRELUDE_KEY, read, visited)?;
    out.push_str(&expanded);
    Ok(out)
}

/// A stdlib card's source by bare name ("touhou") — hosts use this to
/// build rig strings without carrying card files around.
pub fn stdlib(name: &str) -> Option<&'static str> {
    let key = format!("{}{}.dmk", LIB_PREFIX, name);
    STDLIB.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
}

/// The native reader: plain filesystem (the stdlib never reaches it).
fn fs_reader(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {}", p, e))
}

/// Read a card file from the filesystem, splicing (import "...") lines.
pub fn expand_card(path: &Path) -> Result<String, String> {
    let key = path.to_string_lossy().to_string();
    expand_card_with(&key, &fs_reader)
}

/// Expand imports in an in-memory source (tests, REPL, rig strings):
/// bare imports hit the library; relative paths resolve against the
/// process cwd. The prelude is prepended unless already present.
pub fn expand_src(src: &str) -> Result<String, String> {
    let mut visited = HashSet::new();
    let expanded = expand_lines(src, "", &fs_reader, &mut visited)?;
    with_prelude(expanded, &fs_reader, &mut visited)
}

/// Import expansion over an abstract reader (filesystem natively; a fetched
/// file map on wasm). Paths are /-separated strings, lexically normalized.
pub fn expand_card_with(
    path: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut visited = HashSet::new();
    let expanded = expand_inner(&normalize(path), read, &mut visited)?;
    with_prelude(expanded, read, &mut visited)
}

fn expand_inner(
    path: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
    visited: &mut HashSet<String>,
) -> Result<String, String> {
    if !visited.insert(path.to_string()) {
        return Ok(String::new()); // include-once
    }
    // @lib/ resolves from the embedded stdlib, bypassing the reader —
    // identical on every host, present in every distribution
    let src = if path.starts_with(LIB_PREFIX) {
        STDLIB
            .iter()
            .find(|(k, _)| *k == path)
            .map(|(_, s)| s.to_string())
            .ok_or_else(|| format!("import {}: no such library", path))?
    } else {
        read(path).map_err(|e| format!("import {}", e))?
    };
    let base = match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    };
    expand_lines(&src, base, read, visited)
}

fn expand_lines(
    src: &str,
    base: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
    visited: &mut HashSet<String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        match import_target(line) {
            Some(rel) => {
                let target = if !rel.contains('/') && !rel.ends_with(".dmk") {
                    format!("{}{}.dmk", LIB_PREFIX, rel) // library import
                } else if base.is_empty() {
                    rel.to_string()
                } else {
                    format!("{}/{}", base, rel)
                };
                out.push_str(&expand_inner(&normalize(&target), read, visited)?);
                out.push('\n');
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// Lexical path normalization (resolve "." and ".."); no filesystem access,
/// so include-once dedup works identically on native and wasm.
fn normalize(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let absolute = p.starts_with('/');
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.last().map(|l| *l != "..").unwrap_or(false) {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}

/// `(import "path")` alone on a line (comments after are fine).
fn import_target(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix("(import")?;
    let rest = rest.trim_start().strip_prefix('"')?;
    let (path, rest) = rest.split_once('"')?;
    let rest = rest.trim_start().strip_prefix(')')?;
    let rest = rest.trim_start();
    (rest.is_empty() || rest.starts_with(';')).then_some(path)
}
