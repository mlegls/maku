//! The debug player: a sim+render SERVER (sclang/scsynth split).
//!
//! Usage: danmaku-player <card.dmk> [pattern-name]
//!
//! The CLI card argument is the degenerate client. A TCP listener on
//! 127.0.0.1:7777 accepts newline-delimited EDN commands — the wire format
//! is the card format — so editor clients (vim plugin) are thin
//! send-form-to-socket shims:
//!
//!   (load "path/to/card.dmk")        reload from disk
//!   (load "path" "pattern-name")     ... selecting a pattern
//!   (pattern "name")                 switch pattern in the current card
//!   (restart)                        re-run the current pattern
//!   (pause) (resume)

use danmaku_core::edn::{read_all, Form};
use danmaku_core::interp::{load_card, TICK_RATE};
use danmaku_core::sim::{Inputs, RenderItem, Sim};
use macroquad::prelude::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};

const PORT: u16 = 7777;
const PIXELS_PER_UNIT: f32 = 55.0;

struct Player {
    card_path: String,
    card_src: String,
    pattern: Option<String>,
    patterns: Vec<String>,
    sim: Option<Sim>,
    paused: bool,
    accum: f64,
    status: String,
}

impl Player {
    fn reload_from_disk(&mut self) {
        match std::fs::read_to_string(&self.card_path) {
            Ok(src) => {
                self.card_src = src;
                self.restart();
            }
            Err(e) => self.status = format!("read {}: {}", self.card_path, e),
        }
    }

    fn restart(&mut self) {
        // card menu: every defpattern in the file, in order
        self.patterns = read_all(&self.card_src)
            .ok()
            .and_then(|forms| load_card(&forms).ok())
            .map(|c| c.order)
            .unwrap_or_default();
        if let Some(p) = &self.pattern {
            if !self.patterns.iter().any(|n| n == p) {
                self.pattern = None; // stale selection after reload
            }
        }
        match Sim::load(&self.card_src, self.pattern.as_deref()) {
            Ok(sim) => {
                self.sim = Some(sim);
                let name = self
                    .pattern
                    .clone()
                    .or_else(|| self.patterns.first().cloned())
                    .unwrap_or_default();
                self.status = format!("{} [{}]", self.card_path, name);
            }
            Err(e) => {
                self.sim = None;
                self.status = format!("load error: {}", e);
            }
        }
    }

    fn select(&mut self, idx: usize) {
        if let Some(name) = self.patterns.get(idx) {
            self.pattern = Some(name.clone());
            self.restart();
        }
    }

    fn command(&mut self, form: &Form) {
        let Form::List(items) = form else {
            self.status = "bad command (expected list)".into();
            return;
        };
        let head = match items.first() {
            Some(Form::Sym(s)) => s.to_string(),
            _ => {
                self.status = "bad command head".into();
                return;
            }
        };
        let arg_str = |i: usize| -> Option<String> {
            match items.get(i) {
                Some(Form::Str(s)) => Some(s.to_string()),
                Some(Form::Sym(s)) => Some(s.to_string()),
                _ => None,
            }
        };
        match head.as_str() {
            "load" => {
                if let Some(p) = arg_str(1) {
                    self.card_path = p;
                }
                if let Some(n) = arg_str(2) {
                    self.pattern = Some(n);
                } else {
                    self.pattern = None;
                }
                self.reload_from_disk();
            }
            "pattern" => {
                self.pattern = arg_str(1);
                self.restart();
            }
            "restart" => self.restart(),
            "pause" => self.paused = true,
            "resume" => self.paused = false,
            _ => self.status = format!("unknown command '{}'", head),
        }
    }
}

/// Forward raw command lines; Forms are Rc-based (interpreter-local), so
/// parsing happens on the sim thread.
fn serve(port: u16) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("server: bind {}: {}", port, e);
                return;
            }
        };
        eprintln!("server: listening on 127.0.0.1:{}", port);
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        let _ = tx.send(line);
                    }
                }
            });
        }
    });
    rx
}

/// Rotate a color's hue by `deg` (cheap RGB-space rotation).
fn hue_shift(c: Color, deg: f64) -> Color {
    if deg.abs() < 1e-9 {
        return c;
    }
    let (h, s, l) = rgb_to_hsl(c);
    hsl_to_rgb(((h + deg as f32).rem_euclid(360.0), s, l), c.a)
}

fn rgb_to_hsl(c: Color) -> (f32, f32, f32) {
    let (r, g, b) = (c.r, c.g, c.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn hsl_to_rgb((h, s, l): (f32, f32, f32), a: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::new(r + m, g + m, b + m, a)
}

fn style_color(color: &str) -> Color {
    match color {
        "red" => Color::from_rgba(0xff, 0x4d, 0x5e, 0xff),
        "orange" => Color::from_rgba(0xff, 0x9d, 0x3c, 0xff),
        "yellow" => Color::from_rgba(0xff, 0xe0, 0x66, 0xff),
        "green" => Color::from_rgba(0x66, 0xe0, 0x85, 0xff),
        "teal" => Color::from_rgba(0x4d, 0xd8, 0xd0, 0xff),
        "blue" => Color::from_rgba(0x5c, 0x9d, 0xff, 0xff),
        "purple" => Color::from_rgba(0xb2, 0x7d, 0xff, 0xff),
        "pink" => Color::from_rgba(0xff, 0x85, 0xc2, 0xff),
        "black" => Color::from_rgba(0x60, 0x60, 0x70, 0xff),
        "blueteal" => Color::from_rgba(0x4d, 0xbc, 0xe8, 0xff),
        _ => WHITE,
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "danmaku-player".into(),
        window_width: 900,
        window_height: 960,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut args = std::env::args().skip(1);
    let card_path = args.next().unwrap_or_else(|| {
        eprintln!("usage: danmaku-player <card.dmk> [pattern-name]");
        std::process::exit(2);
    });
    let pattern = args.next();

    let mut player = Player {
        card_path,
        card_src: String::new(),
        pattern,
        patterns: Vec::new(),
        sim: None,
        paused: false,
        accum: 0.0,
        status: String::new(),
    };
    player.reload_from_disk();
    let commands = serve(PORT);

    loop {
        // server commands
        while let Ok(line) = commands.try_recv() {
            match read_all(&line) {
                Ok(forms) => {
                    for f in &forms {
                        player.command(f);
                    }
                }
                Err(e) => player.status = format!("command parse: {}", e),
            }
        }
        // hotkeys: r = restart from disk, space = pause, esc = quit
        if is_key_pressed(KeyCode::R) {
            player.reload_from_disk();
        }
        if is_key_pressed(KeyCode::Space) {
            player.paused = !player.paused;
        }
        // pattern menu: 1-9 select a defpattern from the card
        for (i, key) in [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
            KeyCode::Key8,
            KeyCode::Key9,
        ]
        .iter()
        .enumerate()
        {
            if is_key_pressed(*key) {
                player.select(i);
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        // mock player rides the mouse (design.md §11: sandbox mock player)
        let (cx, cy) = (screen_width() / 2.0, screen_height() / 2.0 + 100.0);
        let (mx, my) = mouse_position();
        let mouse_world = (
            ((mx - cx) / PIXELS_PER_UNIT) as f64,
            ((cy - my) / PIXELS_PER_UNIT) as f64,
        );
        // the mouse is the mock player for boss patterns and the mock
        // nearest-enemy for player-side patterns
        let inputs = Inputs { player: mouse_world, nearest_enemy: mouse_world };

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !player.paused {
            player.accum += get_frame_time() as f64;
            let dt = 1.0 / TICK_RATE;
            while player.accum >= dt {
                player.accum -= dt;
                if let Some(sim) = &mut player.sim {
                    if let Err(e) = sim.step_with(&inputs) {
                        player.status = format!("sim error: {}", e);
                        player.paused = true;
                        break;
                    }
                }
            }
        }

        clear_background(Color::from_rgba(0x12, 0x12, 0x1a, 0xff));
        // player marker
        draw_circle_lines(mx, my, 8.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.8));
        draw_circle(mx, my, 2.5, WHITE);
        if let Some(sim) = &player.sim {
            let to_screen =
                |x: f64, y: f64| (cx + x as f32 * PIXELS_PER_UNIT, cy - y as f32 * PIXELS_PER_UNIT);
            for item in sim.render() {
                match item {
                    RenderItem::Dot { x, y, style, hue, .. } => {
                        let (sx, sy) = to_screen(x, y);
                        let r = match style.family.as_str() {
                            "lstar" | "gglcircle" => 10.0,
                            "gem" | "star" => 5.0,
                            _ => 6.0,
                        };
                        let col = hue_shift(style_color(&style.color), hue);
                        draw_circle(sx, sy, r, col);
                        draw_circle_lines(sx, sy, r, 1.5, Color::new(1.0, 1.0, 1.0, 0.35));
                    }
                    RenderItem::Polyline { pts, style, active, hue } => {
                        let col = hue_shift(style_color(&style.color), hue);
                        let (w, col) = if active {
                            (6.0, col)
                        } else {
                            (1.5, Color::new(col.r, col.g, col.b, 0.45))
                        };
                        for seg in pts.windows(2) {
                            let (ax, ay) = to_screen(seg[0].0, seg[0].1);
                            let (bx, by) = to_screen(seg[1].0, seg[1].1);
                            draw_line(ax, ay, bx, by, w, col);
                        }
                    }
                }
            }
            draw_text(
                &format!(
                    "{}  tick {}  entities {}  {}",
                    player.status,
                    sim.tick(),
                    sim.world.bullets.len(),
                    if player.paused { "[paused]" } else { "" }
                ),
                12.0,
                24.0,
                22.0,
                GRAY,
            );
        } else {
            draw_text(&player.status, 12.0, 24.0, 22.0, RED);
        }
        // pattern menu
        let current = player
            .pattern
            .clone()
            .or_else(|| player.patterns.first().cloned())
            .unwrap_or_default();
        for (i, name) in player.patterns.iter().enumerate() {
            let sel = *name == current;
            draw_text(
                &format!("{} {}", i + 1, name),
                12.0,
                screen_height() - 14.0 * (player.patterns.len() - i) as f32,
                18.0,
                if sel { WHITE } else { GRAY },
            );
        }
        next_frame().await;
    }
}
