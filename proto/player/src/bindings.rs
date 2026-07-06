//! Key→channel bindings: the host contract as editable data. Buttons
//! (hold/tap/toggle), axes (a key pair yielding -1/0/+1), and constants
//! (channels set directly). Bindings live host-side; the tape records the
//! resulting channel VALUES, so replays are unaffected by how they were
//! produced. Axis pairs sharing a stem (`foo-x`/`foo-y`) are vector-
//! normalized, matching the classic movement contract.

use maku::sim::Inputs;
use macroquad::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Hold,
    Tap,
    Toggle,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Hold => "hold",
            Mode::Tap => "tap",
            Mode::Toggle => "toggle",
        }
    }
    fn next(self) -> Mode {
        match self {
            Mode::Hold => Mode::Tap,
            Mode::Tap => Mode::Toggle,
            Mode::Toggle => Mode::Hold,
        }
    }
}

pub enum Kind {
    Button(Mode),
    /// The row's `key` is the POSITIVE direction; this is the negative.
    Axis(KeyCode),
}

pub struct Binding {
    pub key: KeyCode,
    pub kind: Kind,
    pub channel: String,
    latch: bool,    // toggle state
    tap_armed: bool // tap: high for exactly one stepped tick
}

impl Binding {
    fn button(key: KeyCode, mode: Mode, channel: &str) -> Binding {
        Binding { key, kind: Kind::Button(mode), channel: channel.into(), latch: false, tap_armed: false }
    }
    fn axis(neg: KeyCode, pos: KeyCode, channel: &str) -> Binding {
        Binding { key: pos, kind: Kind::Axis(neg), channel: channel.into(), latch: false, tap_armed: false }
    }
}

enum Capture {
    Key(usize),    // row: main key
    NegKey(usize), // row: axis negative key
}

enum Edit {
    Channel(usize, String),
    ConstName(usize, String),
    ConstVal(usize, String),
}

pub struct Bindings {
    pub rows: Vec<Binding>,
    pub consts: Vec<(String, f64)>,
    pub open: bool,
    capture: Option<Capture>,
    edit: Option<Edit>,
}

impl Bindings {
    /// The classic sandbox contract, as data.
    pub fn defaults() -> Bindings {
        Bindings {
            rows: vec![
                Binding::axis(KeyCode::A, KeyCode::D, "move-x"),
                Binding::axis(KeyCode::S, KeyCode::W, "move-y"),
                Binding::axis(KeyCode::Left, KeyCode::Right, "move-x"),
                Binding::axis(KeyCode::Down, KeyCode::Up, "move-y"),
                Binding::axis(KeyCode::A, KeyCode::D, "p1-move-x"),
                Binding::axis(KeyCode::S, KeyCode::W, "p1-move-y"),
                Binding::axis(KeyCode::Left, KeyCode::Right, "p2-move-x"),
                Binding::axis(KeyCode::Down, KeyCode::Up, "p2-move-y"),
                Binding::button(KeyCode::LeftShift, Mode::Hold, "focus-firing"),
                Binding::button(KeyCode::X, Mode::Hold, "bomb"),
            ],
            consts: vec![("rank".into(), 1.0)],
            open: false,
            capture: None,
            edit: None,
        }
    }

    pub fn set_const(&mut self, name: &str, v: f64) {
        match self.consts.iter_mut().find(|(n, _)| n == name) {
            Some((_, slot)) => *slot = v,
            None => self.consts.push((name.into(), v)),
        }
    }

    pub fn get_const(&self, name: &str) -> Option<f64> {
        self.consts.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    /// True while the panel should swallow the app's other hotkeys.
    pub fn wants_keys(&self) -> bool {
        self.open
    }

    /// Per-frame: arm taps, flip toggles (edge-triggered on key press).
    pub fn poll_edges(&mut self) {
        if self.open {
            return; // panel open: keys belong to the panel
        }
        for b in &mut self.rows {
            if let Kind::Button(mode) = &b.kind {
                if is_key_pressed(b.key) {
                    match mode {
                        Mode::Tap => b.tap_armed = true,
                        Mode::Toggle => b.latch = !b.latch,
                        Mode::Hold => {}
                    }
                }
            }
        }
    }

    /// Write all bound channels into `inputs`. `paused` suppresses arrow
    /// keys (they belong to scrubbing while paused).
    pub fn inject(&self, inputs: &mut Inputs, paused: bool) {
        let arrow = |k: KeyCode| {
            matches!(k, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down)
        };
        let down = |k: KeyCode| -> bool {
            if paused && arrow(k) {
                false
            } else {
                is_key_down(k)
            }
        };
        // sum contributions per channel
        let mut acc: Vec<(String, f64)> = Vec::new();
        let push = |ch: &str, v: f64, acc: &mut Vec<(String, f64)>| {
            match acc.iter_mut().find(|(n, _)| n == ch) {
                Some((_, slot)) => *slot += v,
                None => acc.push((ch.into(), v)),
            }
        };
        for b in &self.rows {
            let v = match &b.kind {
                Kind::Button(Mode::Hold) => down(b.key) as i8 as f64,
                Kind::Button(Mode::Tap) => b.tap_armed as i8 as f64,
                Kind::Button(Mode::Toggle) => b.latch as i8 as f64,
                Kind::Axis(neg) => (down(b.key) as i8 - down(*neg) as i8) as f64,
            };
            push(&b.channel, v, &mut acc);
        }
        // clamp sums; vector-normalize -x/-y stem pairs
        for (_, v) in acc.iter_mut() {
            *v = v.clamp(-1.0, 1.0);
        }
        let stems: Vec<String> = acc
            .iter()
            .filter_map(|(n, _)| n.strip_suffix("-x").map(|s| s.to_string()))
            .collect();
        for stem in stems {
            let xn = format!("{}-x", stem);
            let yn = format!("{}-y", stem);
            let x = acc.iter().find(|(n, _)| *n == xn).map(|(_, v)| *v).unwrap_or(0.0);
            let y = acc.iter().find(|(n, _)| *n == yn).map(|(_, v)| *v);
            if let Some(y) = y {
                let m = (x * x + y * y).sqrt();
                if m > 1.0 {
                    for (n, v) in acc.iter_mut() {
                        if *n == xn {
                            *v = x / m;
                        } else if *n == yn {
                            *v = y / m;
                        }
                    }
                }
            }
        }
        for (n, v) in acc {
            inputs.set_num(&n, v);
        }
        for (n, v) in &self.consts {
            inputs.set_num(n, *v);
        }
    }

    /// After the first stepped tick of a frame: taps have fired.
    pub fn consume_taps(&mut self, inputs: &mut Inputs) {
        for b in &mut self.rows {
            if b.tap_armed {
                b.tap_armed = false;
                inputs.set_num(&b.channel, 0.0);
            }
        }
    }

    /// Draw the panel and handle its mouse/keyboard. Call every frame;
    /// no-op unless open (except the B toggle).
    pub fn ui(&mut self) {
        if is_key_pressed(KeyCode::B) && self.edit.is_none() && self.capture.is_none() {
            self.open = !self.open;
        }
        if !self.open {
            return;
        }
        if is_key_pressed(KeyCode::Escape) {
            self.capture = None;
            self.edit = None;
            self.open = false;
            return;
        }

        // key capture: next key press lands in the captured slot
        if let Some(cap) = &self.capture {
            if let Some(k) = get_last_key_pressed() {
                if k != KeyCode::Escape {
                    match cap {
                        Capture::Key(i) => self.rows[*i].key = k,
                        Capture::NegKey(i) => {
                            if let Kind::Axis(neg) = &mut self.rows[*i].kind {
                                *neg = k;
                            }
                        }
                    }
                }
                self.capture = None;
            }
        }

        // text editing: chars into the buffer, Enter commits
        if let Some(edit) = &mut self.edit {
            while let Some(c) = get_char_pressed() {
                let buf = match edit {
                    Edit::Channel(_, b) | Edit::ConstName(_, b) | Edit::ConstVal(_, b) => b,
                };
                if c == '\u{8}' {
                    buf.pop();
                } else if !c.is_control() {
                    buf.push(c);
                }
            }
            if is_key_pressed(KeyCode::Enter) {
                match self.edit.take().unwrap() {
                    Edit::Channel(i, b) => self.rows[i].channel = b.trim_start_matches('$').into(),
                    Edit::ConstName(i, b) => self.consts[i].0 = b.trim_start_matches('$').into(),
                    Edit::ConstVal(i, b) => {
                        if let Ok(v) = b.parse::<f64>() {
                            self.consts[i].1 = v;
                        }
                    }
                }
            }
        }

        let (mx, my) = mouse_position();
        let click = is_mouse_button_pressed(MouseButton::Left);
        let px = 60.0;
        let py = 40.0;
        let pw = screen_width() - 120.0;
        let row_h = 20.0;
        let ph = 90.0 + row_h * (self.rows.len() + self.consts.len()) as f32;
        draw_rectangle(px, py, pw, ph, Color::from_rgba(0x10, 0x10, 0x18, 0xf0));
        draw_rectangle_lines(px, py, pw, ph, 1.0, GRAY);
        draw_text("bindings  (B to close; click a cell to edit)", px + 10.0, py + 22.0, 20.0, WHITE);

        let hover = |x: f32, y: f32, w: f32| mx >= x && mx <= x + w && my >= y - 14.0 && my <= y + 4.0;
        let cell = |txt: &str, x: f32, y: f32, w: f32, active: bool| {
            let h = hover(x, y, w);
            draw_text(txt, x, y, 18.0, if active { YELLOW } else if h { WHITE } else { LIGHTGRAY });
            h
        };

        let mut y = py + 50.0;
        let mut kill: Option<usize> = None;
        for i in 0..self.rows.len() {
            let capturing_key = matches!(self.capture, Some(Capture::Key(j)) if j == i);
            let capturing_neg = matches!(self.capture, Some(Capture::NegKey(j)) if j == i);
            let editing = matches!(&self.edit, Some(Edit::Channel(j, _)) if *j == i);

            // channel cell
            let ch_txt = if editing {
                if let Some(Edit::Channel(_, b)) = &self.edit {
                    format!("${}_", b)
                } else {
                    unreachable!()
                }
            } else {
                format!("${}", self.rows[i].channel)
            };
            if cell(&ch_txt, px + 12.0, y, 150.0, editing) && click {
                self.edit = Some(Edit::Channel(i, self.rows[i].channel.clone()));
            }
            // key cell(s) + mode
            match &self.rows[i].kind {
                Kind::Button(mode) => {
                    let ktxt = if capturing_key { "press a key…".into() } else { format!("[{:?}]", self.rows[i].key) };
                    if cell(&ktxt, px + 180.0, y, 130.0, capturing_key) && click {
                        self.capture = Some(Capture::Key(i));
                    }
                    let m = *mode;
                    if cell(m.label(), px + 330.0, y, 60.0, false) && click {
                        self.rows[i].kind = Kind::Button(m.next());
                    }
                }
                Kind::Axis(neg) => {
                    let ntxt = if capturing_neg { "press a key…".into() } else { format!("[{:?}]", neg) };
                    let ptxt = if capturing_key { "press a key…".into() } else { format!("[{:?}]", self.rows[i].key) };
                    if cell(&format!("-{}", ntxt), px + 180.0, y, 100.0, capturing_neg) && click {
                        self.capture = Some(Capture::NegKey(i));
                    }
                    if cell(&format!("+{}", ptxt), px + 290.0, y, 100.0, capturing_key) && click {
                        self.capture = Some(Capture::Key(i));
                    }
                    cell("axis", px + 400.0, y, 40.0, false);
                }
            }
            if cell("[x]", px + pw - 40.0, y, 30.0, false) && click {
                kill = Some(i);
            }
            y += row_h;
        }
        if let Some(i) = kill {
            self.rows.remove(i);
        }

        // add-row buttons
        if cell("+ button", px + 12.0, y, 80.0, false) && click {
            self.rows.push(Binding::button(KeyCode::Space, Mode::Hold, "chan"));
        }
        if cell("+ axis", px + 110.0, y, 70.0, false) && click {
            self.rows.push(Binding::axis(KeyCode::Comma, KeyCode::Period, "chan"));
        }
        y += row_h + 8.0;

        draw_text("constants", px + 12.0, y, 18.0, WHITE);
        y += row_h;
        let mut kill_c: Option<usize> = None;
        for i in 0..self.consts.len() {
            let editing_n = matches!(&self.edit, Some(Edit::ConstName(j, _)) if *j == i);
            let editing_v = matches!(&self.edit, Some(Edit::ConstVal(j, _)) if *j == i);
            let ntxt = if editing_n {
                if let Some(Edit::ConstName(_, b)) = &self.edit { format!("${}_", b) } else { unreachable!() }
            } else {
                format!("${}", self.consts[i].0)
            };
            if cell(&ntxt, px + 12.0, y, 150.0, editing_n) && click {
                self.edit = Some(Edit::ConstName(i, self.consts[i].0.clone()));
            }
            let vtxt = if editing_v {
                if let Some(Edit::ConstVal(_, b)) = &self.edit { format!("{}_", b) } else { unreachable!() }
            } else {
                format!("{}", self.consts[i].1)
            };
            if cell(&vtxt, px + 180.0, y, 80.0, editing_v) && click {
                self.edit = Some(Edit::ConstVal(i, format!("{}", self.consts[i].1)));
            }
            if cell("-", px + 280.0, y, 20.0, false) && click {
                self.consts[i].1 -= 0.1;
            }
            if cell("+", px + 310.0, y, 20.0, false) && click {
                self.consts[i].1 += 0.1;
            }
            if cell("[x]", px + pw - 40.0, y, 30.0, false) && click {
                kill_c = Some(i);
            }
            y += row_h;
        }
        if let Some(i) = kill_c {
            self.consts.remove(i);
        }
        if cell("+ const", px + 12.0, y, 80.0, false) && click {
            self.consts.push(("chan".into(), 0.0));
        }
    }
}
