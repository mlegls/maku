//! The debug player: a sim+render SERVER (sclang/scsynth split).
//!
//! Usage: danmaku-player <card.edn> [pattern-name]
//!
//! The CLI card argument is the degenerate client. A TCP listener on
//! 127.0.0.1:7777 accepts newline-delimited EDN commands — the wire format
//! is the card format — so editor clients (vim plugin) are thin
//! send-form-to-socket shims:
//!
//!   (load "path/to/card.edn")        reload from disk
//!   (load "path" "pattern-name")     ... selecting a pattern
//!   (pattern "name")                 switch pattern in the current card
//!   (restart)                        re-run the current pattern
//!   (pause) (resume)

use danmaku_core::edn::{read_all, Form};
use danmaku_core::interp::TICK_RATE;
use danmaku_core::sim::{Inputs, Sim};
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
        match Sim::load(&self.card_src, self.pattern.as_deref()) {
            Ok(sim) => {
                self.sim = Some(sim);
                self.status = format!(
                    "{} [{}]",
                    self.card_path,
                    self.pattern.as_deref().unwrap_or("first pattern")
                );
            }
            Err(e) => {
                self.sim = None;
                self.status = format!("load error: {}", e);
            }
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
        eprintln!("usage: danmaku-player <card.edn> [pattern-name]");
        std::process::exit(2);
    });
    let pattern = args.next();

    let mut player = Player {
        card_path,
        card_src: String::new(),
        pattern,
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
        if is_key_pressed(KeyCode::Escape) {
            break;
        }

        // mock player rides the mouse (design.md §11: sandbox mock player)
        let (cx, cy) = (screen_width() / 2.0, screen_height() / 2.0 + 100.0);
        let (mx, my) = mouse_position();
        let inputs = Inputs {
            player: (
                ((mx - cx) / PIXELS_PER_UNIT) as f64,
                ((cy - my) / PIXELS_PER_UNIT) as f64,
            ),
        };

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
            for b in sim.render() {
                let sx = cx + b.x as f32 * PIXELS_PER_UNIT;
                let sy = cy - b.y as f32 * PIXELS_PER_UNIT; // world y-up
                let r = match b.style.family.as_str() {
                    "lstar" | "gglcircle" => 10.0,
                    "gem" | "star" => 5.0,
                    _ => 6.0,
                };
                let col = style_color(&b.style.color);
                draw_circle(sx, sy, r, col);
                draw_circle_lines(sx, sy, r, 1.5, Color::new(1.0, 1.0, 1.0, 0.35));
            }
            draw_text(
                &format!(
                    "{}  tick {}  bullets {}  {}",
                    player.status,
                    sim.tick,
                    sim.bullets.len(),
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
        next_frame().await;
    }
}
