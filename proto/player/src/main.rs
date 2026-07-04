//! The debug player: a sim+render SERVER (sclang/scsynth split).
//!
//! Usage: danmaku-player [card.dmk [pattern-name]]
//! With no card argument the player starts empty and waits for clients.
//!
//! The CLI card argument is the degenerate client. A TCP listener on
//! 127.0.0.1:7777 accepts newline-delimited EDN commands — the wire format
//! is the card format — so editor clients (vim plugin) are thin
//! send-form-to-socket shims:
//!
//!   (run <forms...>)                 run forms as an anonymous pattern
//!                                     (current card's defs in scope; the
//!                                     input tape replays through new code)
//!   (swap <forms...>)                generational hot-swap: in-flight
//!                                     bullets keep their old code
//!   (add <forms...>)                 layer onto the running sim; the added
//!                                     pattern's clocks anchor at this tick
//!   (load "path/to/card.dmk")        reload from disk (does NOT play)
//!   (load "path" "pattern-name")     reload and play the named pattern
//!   (pattern "name")                 switch pattern in the current card
//!   (restart)                        re-run the current pattern
//!   (clear)                          stop the running pattern
//!   (seek N) (step ±N)               scrub the timeline (pauses). The sim
//!                                    is a deterministic fold over two tapes
//!                                    (inputs + program commands); backward =
//!                                    snapshot + re-step, and add/swap
//!                                    boundaries replay at their ticks
//!   (snapshots N)                    snapshot cadence in ticks; 0 = off
//!                                    (old snapshots auto-thin regardless)
//!   (pause) (resume)

use danmaku_core::edn::{read_all, Form};
use danmaku_core::interp::Val;
use danmaku_core::interp::{load_card, TICK_RATE};
use danmaku_core::session::Session;
use danmaku_core::sim::{Inputs, RenderItem, Sim};
use macroquad::prelude::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};

const PORT: u16 = 7777;
const PIXELS_PER_UNIT: f32 = 55.0;
const TIMELINE_H: f32 = 30.0;

struct Player {
    card_path: String,
    card_src: String,
    pattern: Option<String>,
    patterns: Vec<String>,
    /// The scrubbable timeline (core::session): input tape + command tape
    /// + snapshots; the sim lives inside.
    session: Session,
    paused: bool,
    accum: f64,
    status: String,
    dragging: bool, // scrubbing via the timeline slider
}

impl Player {
    /// Read the card from disk and refresh the pattern menu. Does NOT play.
    fn reload_from_disk(&mut self) -> bool {
        if self.card_path.is_empty() {
            self.status = "no card loaded — send (load \"path\") or (run …)".into();
            return false;
        }
        match std::fs::read_to_string(&self.card_path) {
            Ok(src) => {
                self.card_src = src;
                self.refresh_menu();
                self.status = format!(
                    "{} loaded ({} pattern{})",
                    self.card_path,
                    self.patterns.len(),
                    if self.patterns.len() == 1 { "" } else { "s" }
                );
                true
            }
            Err(e) => {
                self.status = format!("read {}: {}", self.card_path, e);
                false
            }
        }
    }

    fn refresh_menu(&mut self) {
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
    }

    /// (Re-)instantiate and run the selected (or first) pattern.
    fn restart(&mut self) {
        self.refresh_menu();
        match Sim::load(&self.card_src, self.pattern.as_deref()) {
            Ok(sim) => {
                self.session.start(sim);
                let name = self
                    .pattern
                    .clone()
                    .or_else(|| self.patterns.first().cloned())
                    .unwrap_or_default();
                self.status = format!("{} [{}]", self.card_path, name);
            }
            Err(e) => {
                self.session.stop();
                self.status = format!("load error: {}", e);
            }
        }
    }

    /// Scrub to an absolute tick (pauses).
    fn seek(&mut self, target: u64) {
        self.paused = true;
        match self.session.seek(&self.card_src, target) {
            Ok(()) => self.status = format!("scrub @ tick {}", target),
            Err(e) => self.status = format!("seek: {}", e),
        }
    }

    /// Stop the running pattern; keep the card loaded.
    fn clear(&mut self) {
        self.session.stop();
        self.status = if self.card_src.is_empty() {
            format!("cleared — listening on 127.0.0.1:{}", PORT)
        } else {
            format!("cleared — {} still loaded", self.card_path)
        };
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
            "run" => {
                // re-serialize the parsed forms (Display prints canonical)
                let src = items[1..]
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                // replace the program; the input tape replays through the
                // NEW code (the pause/rewind/edit/re-run loop, design.md §11)
                match self.session.rerun(&self.card_src, &src) {
                    Ok(replay_to) => {
                        let preview: String = src.chars().take(40).collect();
                        self.status = if replay_to > 0 {
                            format!("run {}… (replayed to tick {})", preview, replay_to)
                        } else {
                            format!("run {}…", preview)
                        };
                    }
                    Err(e) => self.status = format!("run error: {}", e),
                }
            }
            "add" => {
                // layer at the current tick, recorded on the command tape
                // (scrub-safe); falls back to run when nothing is running
                let src = items[1..]
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let preview: String = src.chars().take(40).collect();
                if self.session.sim.is_some() {
                    match self.session.record_add(src) {
                        Ok(()) => self.status = format!("add {}…", preview),
                        Err(e) => self.status = format!("add error: {}", e),
                    }
                } else {
                    match self.session.rerun(&self.card_src, &src) {
                        Ok(_) => self.status = format!("add (started fresh) {}…", preview),
                        Err(e) => self.status = format!("add error: {}", e),
                    }
                }
            }
            "swap" => {
                // generational hot-swap, recorded on the command tape
                let src = items[1..]
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let preview: String = src.chars().take(40).collect();
                match self.session.record_swap(src) {
                    Ok(()) => self.status = format!("swap {}…", preview),
                    Err(e) => self.status = format!("swap error: {}", e),
                }
            }
            "seek" => {
                if let Some(Form::Num(n)) = items.get(1) {
                    self.seek((*n).max(0.0) as u64);
                }
            }
            "snapshots" => {
                // (snapshots n): cadence in ticks; 0 disables (soak runs)
                if let Some(Form::Num(n)) = items.get(1) {
                    self.session.snap_every = (*n).max(0.0) as u64;
                    self.status = if self.session.snap_every == 0 {
                        "snapshots off (scrub-back replays from tick 0)".into()
                    } else {
                        format!("snapshots every {} ticks", self.session.snap_every)
                    };
                }
            }
            "step" => {
                let n = match items.get(1) {
                    Some(Form::Num(n)) => *n,
                    _ => 1.0,
                };
                if let Some(cur) = self.session.tick() {
                    self.seek((cur as f64 + n).max(0.0) as u64);
                }
            }
            "load" => {
                // (load "path") = load only; (load "path" "pattern") = play it
                if let Some(p) = arg_str(1) {
                    self.card_path = p;
                }
                let play = arg_str(2).is_some();
                if play {
                    self.pattern = arg_str(2);
                }
                if self.reload_from_disk() && play {
                    self.restart();
                }
            }
            "pattern" => {
                self.pattern = arg_str(1);
                self.restart();
            }
            "restart" => self.restart(),
            "clear" => self.clear(),
            "pause" => self.paused = true,
            "resume" => {
                self.paused = false;
                self.session.truncate_future();
            }
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

/// Bottom strip: play/pause button + timeline slider over the recorded tape.
/// Dragging the handle scrubs (auto-pauses); clicking play resumes, which
/// branches the timeline (truncates the future) like every other resume.
fn timeline_ui(player: &mut Player, mx: f32, my: f32) {
    let Some(cur) = player.session.tick() else { return };
    let h = screen_height();
    let w = screen_width();
    let cy = h - TIMELINE_H / 2.0;
    let total = player.session.tape.len().max(1) as f32;

    // play/pause button
    let (bx, br) = (22.0, 9.0);
    let over_btn = (mx - bx).abs() < 14.0 && (my - cy).abs() < 14.0;
    let btn_col = if over_btn { WHITE } else { GRAY };
    if player.paused {
        draw_triangle(
            Vec2::new(bx - br * 0.6, cy - br),
            Vec2::new(bx - br * 0.6, cy + br),
            Vec2::new(bx + br, cy),
            btn_col,
        );
    } else {
        draw_rectangle(bx - br * 0.7, cy - br, br * 0.55, br * 2.0, btn_col);
        draw_rectangle(bx + br * 0.15, cy - br, br * 0.55, br * 2.0, btn_col);
    }
    if over_btn && is_mouse_button_pressed(MouseButton::Left) {
        player.paused = !player.paused;
        if !player.paused {
            player.session.truncate_future();
        }
        return;
    }

    // slider track
    let (x0, x1) = (44.0, w - 96.0);
    let frac = (cur as f32 / total).clamp(0.0, 1.0);
    let hx = x0 + frac * (x1 - x0);
    draw_line(x0, cy, x1, cy, 3.0, Color::new(1.0, 1.0, 1.0, 0.15));
    draw_line(x0, cy, hx, cy, 3.0, Color::new(0.4, 0.7, 1.0, 0.8));
    // snapshot notches
    for (st, _) in &player.session.snaps {
        let sx = x0 + (*st as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 4.0, sx, cy + 4.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.2));
    }
    // command-tape markers: where adds/swaps landed
    for ct in player.session.cmd_ticks() {
        let sx = x0 + (ct as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 6.0, sx, cy + 6.0, 2.0, Color::new(1.0, 0.7, 0.3, 0.8));
    }
    draw_circle(hx, cy, 6.0, if player.dragging { WHITE } else { LIGHTGRAY });
    draw_text(
        &format!("{} / {}", cur, player.session.tape.len()),
        x1 + 10.0,
        cy + 5.0,
        16.0,
        GRAY,
    );

    // drag to scrub
    let over_track = mx >= x0 - 8.0 && mx <= x1 + 8.0 && (my - cy).abs() < 12.0;
    if over_track && is_mouse_button_pressed(MouseButton::Left) {
        player.dragging = true;
    }
    if !is_mouse_button_down(MouseButton::Left) {
        player.dragging = false;
    }
    if player.dragging {
        let f = ((mx - x0) / (x1 - x0)).clamp(0.0, 1.0);
        let target = (f * player.session.tape.len() as f32).round() as u64;
        if target != cur {
            player.seek(target);
        }
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
    // usage: danmaku-player [card.dmk [pattern-name]] — with no card, start
    // empty and wait for (load ...) / (run ...) from clients
    let mut args = std::env::args().skip(1);
    let card_path = args.next().unwrap_or_default();
    let pattern = args.next();

    let mut player = Player {
        card_path,
        card_src: String::new(),
        pattern,
        patterns: Vec::new(),
        session: {
            let mut s = Session::default();
            // this host's player contract: layer the stock rig card
            // (swap in your own live: <localleader>es a rig defpattern)
            s.rig = Some(format!(
                "{}\n(player-rig)",
                include_str!("../../../cards/player-rig.dmk")
            ));
            s
        },
        paused: false,
        accum: 0.0,
        status: String::new(),
        dragging: false,
    };
    if player.card_path.is_empty() {
        player.status = format!("no card — listening on 127.0.0.1:{}", PORT);
    } else if player.reload_from_disk() {
        // CLI card argument is explicit intent to watch it: auto-play
        player.restart();
    }
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
        // hotkeys: r = restart from disk, c = clear, space = pause, esc = quit
        if is_key_pressed(KeyCode::R) && player.reload_from_disk() {
            player.restart();
        }
        if is_key_pressed(KeyCode::C) {
            player.clear();
        }
        if is_key_pressed(KeyCode::Space) {
            player.paused = !player.paused;
            if !player.paused {
                player.session.truncate_future(); // resume after rewind = branch
            }
        }
        // scrub hotkeys — only while paused (live arrows belong to movement)
        if player.paused {
            if let Some(cur) = player.session.tick() {
                if is_key_pressed(KeyCode::Right) {
                    player.seek(cur + 1);
                }
                if is_key_pressed(KeyCode::Left) {
                    player.seek(cur.saturating_sub(1));
                }
                if is_key_pressed(KeyCode::Up) {
                    player.seek(cur + 30);
                }
                if is_key_pressed(KeyCode::Down) {
                    player.seek(cur.saturating_sub(30));
                }
            }
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
        // raw input side of the host contract: WASD/arrows -> axes,
        // Shift -> focus, X -> bomb. The mouse remains the mock $player
        // for mouse-rig cards and the mock nearest-enemy fallback.
        let key = |k: KeyCode| is_key_down(k);
        let ax = (key(KeyCode::D) as i8 - key(KeyCode::A) as i8) as f64
            + if !player.paused {
                (key(KeyCode::Right) as i8 - key(KeyCode::Left) as i8) as f64
            } else {
                0.0
            };
        let ay = (key(KeyCode::W) as i8 - key(KeyCode::S) as i8) as f64
            + if !player.paused {
                (key(KeyCode::Up) as i8 - key(KeyCode::Down) as i8) as f64
            } else {
                0.0
            };
        let mag = (ax * ax + ay * ay).sqrt();
        let axes = if mag > 1.0 { (ax / mag, ay / mag) } else { (ax, ay) };
        let inputs = Inputs {
            player: mouse_world,
            nearest_enemy: mouse_world,
            axes,
            focus: key(KeyCode::LeftShift) || key(KeyCode::RightShift),
            bomb: key(KeyCode::X),
        };
        player.session.last_inputs = inputs;

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !player.paused {
            player.accum += get_frame_time() as f64;
            let dt = 1.0 / TICK_RATE;
            while player.accum >= dt {
                player.accum -= dt;
                if player.session.sim.is_some() {
                    if let Err(e) = player.session.advance(&player.card_src) {
                        player.status = format!("sim error: {}", e);
                        player.paused = true;
                        break;
                    }
                }
            }
        }

        clear_background(Color::from_rgba(0x12, 0x12, 0x1a, 0xff));
        // player marker at the $player channel (derived from a piloted rig,
        // or the mouse): true hitbox dot + graze ring
        let (pmx, pmy) = player
            .session
            .sim
            .as_ref()
            .and_then(|s| match s.channel_val("player") {
                Some(Val::Vec2 { x, y }) => Some((
                    cx + x as f32 * PIXELS_PER_UNIT,
                    cy - y as f32 * PIXELS_PER_UNIT,
                )),
                _ => None,
            })
            .unwrap_or((mx, my));
        draw_circle_lines(pmx, pmy, 0.35 * PIXELS_PER_UNIT, 1.0, Color::new(1.0, 1.0, 1.0, 0.25));
        draw_circle_lines(pmx, pmy, 8.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.8));
        draw_circle(pmx, pmy, 0.06 * PIXELS_PER_UNIT, WHITE);
        if let Some(sim) = &player.session.sim {
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
            // event flashes: expanding rings read straight from world.events
            // (stateless — rewind and they replay with the timeline)
            let now = sim.tick();
            sim.with_events(|events| {
                for ev in events.iter().rev().take(64) {
                    let age = now.saturating_sub(ev.tick);
                    if age > 24 {
                        continue;
                    }
                    let k = age as f32 / 24.0;
                    let (col, r0) = match &*ev.name {
                        "graze" => (Color::new(0.6, 0.9, 1.0, 0.7 * (1.0 - k)), 6.0),
                        "player-hit" => (Color::new(1.0, 0.25, 0.3, 0.9 * (1.0 - k)), 12.0),
                        "enemy-hit" => (Color::new(1.0, 0.8, 0.3, 0.5 * (1.0 - k)), 8.0),
                        "died" => (Color::new(1.0, 0.6, 0.2, 0.8 * (1.0 - k)), 12.0),
                        _ => continue,
                    };
                    if let Some((ex, ey)) = ev.pos {
                        let (sx, sy) = to_screen(ex, ey);
                        draw_circle_lines(sx, sy, r0 + k * 26.0, 2.0, col);
                    }
                }
            });
            // post-hit iframes: flash the player marker
            if sim.world.iframe_until > now && (now / 6) % 2 == 0 {
                draw_circle_lines(mx, my, 14.0, 2.0, Color::new(1.0, 0.3, 0.3, 0.8));
            }

            // scrub indicators: where the position channels ARE at this tick
            // (while paused they diverge from the live mouse)
            if player.paused {
                for (name, col) in
                    [("player", Color::new(1.0, 1.0, 1.0, 0.9)), ("nearest-enemy", ORANGE)]
                {
                    if let Some(Val::Vec2 { x, y }) = sim.channel_val(name) {
                        let (sx, sy) = to_screen(x, y);
                        draw_line(sx - 10.0, sy, sx + 10.0, sy, 1.5, col);
                        draw_line(sx, sy - 10.0, sx, sy + 10.0, 1.5, col);
                        draw_circle_lines(sx, sy, 6.0, 1.5, col);
                        draw_text(&format!("${}", name), sx + 12.0, sy - 8.0, 16.0, col);
                    }
                }
            }
            draw_text(
                &format!(
                    "{}  tick {}  entities {}  graze {}  hits {}  lives {}  {}",
                    player.status,
                    sim.tick(),
                    sim.world.bullets.len(),
                    sim.world.graze,
                    sim.world.player_hits,
                    match sim.channel_val("lives") {
                        Some(Val::Num(n)) => format!("{}", n),
                        _ => "-".into(),
                    },
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
        // pattern menu (above the timeline strip)
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
                screen_height() - TIMELINE_H - 14.0 * (player.patterns.len() - i) as f32,
                18.0,
                if sel { WHITE } else { GRAY },
            );
        }
        timeline_ui(&mut player, mx, my);
        next_frame().await;
    }
}
