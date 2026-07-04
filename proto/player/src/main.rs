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
//!   (load "path/to/card.dmk")        reload from disk (does NOT play)
//!   (load "path" "pattern-name")     reload and play the named pattern
//!   (pattern "name")                 switch pattern in the current card
//!   (restart)                        re-run the current pattern
//!   (clear)                          stop the running pattern
//!   (seek N) (step ±N)               scrub the timeline (pauses; the sim is
//!                                    a deterministic fold over the input
//!                                    tape — backward = snapshot + re-step)
//!   (pause) (resume)

use danmaku_core::edn::{read_all, Form};
use danmaku_core::interp::{load_card, TICK_RATE};
use danmaku_core::interp::Val;
use danmaku_core::sim::{Inputs, RenderItem, Sim};
use macroquad::prelude::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};

const PORT: u16 = 7777;
const PIXELS_PER_UNIT: f32 = 55.0;
const SNAP_EVERY: u64 = 120; // one snapshot per second of sim time
const TIMELINE_H: f32 = 30.0;

struct Player {
    card_path: String,
    card_src: String,
    pattern: Option<String>,
    patterns: Vec<String>,
    sim: Option<Sim>,
    paused: bool,
    accum: f64,
    status: String,
    /// Scrubbing (design.md §11): the sim is a deterministic fold over the
    /// input tape, so any tick = nearest snapshot + re-step.
    tape: Vec<Inputs>,          // tape[t] stepped the sim t → t+1
    snaps: Vec<(u64, Sim)>,     // periodic snapshots, ascending
    last_inputs: Inputs,
    dragging: bool,             // scrubbing via the timeline slider
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
                self.sim = Some(sim);
                self.reset_history();
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

    /// Reset scrub history around a fresh sim (call after any (re)start).
    fn reset_history(&mut self) {
        self.tape.clear();
        self.snaps.clear();
        if let Some(sim) = &self.sim {
            self.snaps.push((sim.tick(), sim.clone()));
        }
    }

    /// Advance the sim one tick, recording tape + periodic snapshots. Replays
    /// from the tape when stepping over already-recorded ticks.
    fn advance(&mut self, live: Inputs) -> Result<(), String> {
        let Some(sim) = &mut self.sim else { return Ok(()) };
        let t = sim.tick() as usize;
        let inputs = if t < self.tape.len() { self.tape[t] } else { live };
        if t >= self.tape.len() {
            self.tape.push(inputs);
        }
        sim.step_with(&inputs)?;
        let now = sim.tick();
        if now % SNAP_EVERY == 0 && self.snaps.last().map(|(t, _)| *t) != Some(now) {
            self.snaps.push((now, sim.clone()));
        }
        Ok(())
    }

    /// Scrub to an absolute tick (pauses). Backward = restore nearest
    /// snapshot and re-step the tape; forward extends the tape if needed.
    fn seek(&mut self, target: u64) {
        if self.sim.is_none() {
            self.status = "seek: nothing running".into();
            return;
        }
        self.paused = true;
        let cur = self.sim.as_ref().unwrap().tick();
        if target < cur {
            let (base_tick, base) = match self
                .snaps
                .iter()
                .rev()
                .find(|(t, _)| *t <= target)
            {
                Some((t, s)) => (*t, s.clone()),
                None => {
                    self.status = "seek: no snapshot history".into();
                    return;
                }
            };
            let _ = base_tick;
            self.sim = Some(base);
        }
        let live = self.last_inputs;
        while self.sim.as_ref().unwrap().tick() < target {
            if let Err(e) = self.advance(live) {
                self.status = format!("seek error: {}", e);
                return;
            }
        }
        self.status = format!("scrub @ tick {}", target);
    }

    /// Resuming after a scrub-back branches the timeline: drop the future.
    fn truncate_future(&mut self) {
        if let Some(sim) = &self.sim {
            let t = sim.tick();
            self.tape.truncate(t as usize);
            self.snaps.retain(|(st, _)| *st <= t);
        }
    }

    /// Stop the running pattern; keep the card loaded.
    fn clear(&mut self) {
        self.sim = None;
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
                let cur = self.sim.as_ref().map(|s| s.tick()).unwrap_or(0);
                match Sim::load_forms(&self.card_src, &src) {
                    Ok(sim) => {
                        // keep the input tape: replay the recorded timeline
                        // through the NEW code up to the current tick — the
                        // pause/rewind/edit/re-run loop (design.md §11)
                        self.sim = Some(sim);
                        self.snaps.clear();
                        self.snaps.push((0, self.sim.as_ref().unwrap().clone()));
                        let replay_to = cur.min(self.tape.len() as u64);
                        while self.sim.as_ref().unwrap().tick() < replay_to {
                            let live = self.last_inputs;
                            if let Err(e) = self.advance(live) {
                                self.status = format!("run replay error: {}", e);
                                break;
                            }
                        }
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
            "swap" => {
                // generational hot-swap: keep the world, replace the program
                let src = items[1..]
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                match &mut self.sim {
                    Some(sim) => match sim.swap_forms(&self.card_src, &src) {
                        Ok(()) => {
                            let preview: String = src.chars().take(40).collect();
                            self.status = format!("swap {}…", preview);
                        }
                        Err(e) => self.status = format!("swap error: {}", e),
                    },
                    None => self.status = "swap: nothing running (use run)".into(),
                }
            }
            "seek" => {
                if let Some(Form::Num(n)) = items.get(1) {
                    self.seek((*n).max(0.0) as u64);
                }
            }
            "step" => {
                let n = match items.get(1) {
                    Some(Form::Num(n)) => *n,
                    _ => 1.0,
                };
                if let Some(sim) = &self.sim {
                    let cur = sim.tick() as f64;
                    self.seek((cur + n).max(0.0) as u64);
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
                self.truncate_future();
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
    let Some(cur) = player.sim.as_ref().map(|s| s.tick()) else { return };
    let h = screen_height();
    let w = screen_width();
    let cy = h - TIMELINE_H / 2.0;
    let total = player.tape.len().max(1) as f32;

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
            player.truncate_future();
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
    for (st, _) in &player.snaps {
        let sx = x0 + (*st as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 4.0, sx, cy + 4.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.2));
    }
    draw_circle(hx, cy, 6.0, if player.dragging { WHITE } else { LIGHTGRAY });
    draw_text(
        &format!("{} / {}", cur, player.tape.len()),
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
        let target = (f * player.tape.len() as f32).round() as u64;
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
        sim: None,
        paused: false,
        accum: 0.0,
        status: String::new(),
        tape: Vec::new(),
        snaps: Vec::new(),
        last_inputs: Inputs::default(),
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
                player.truncate_future(); // resuming after scrub-back branches
            }
        }
        // scrub hotkeys (auto-pause): left/right ±1 tick, down/up ∓30
        if let Some(cur) = player.sim.as_ref().map(|s| s.tick()) {
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
        player.last_inputs = inputs;

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !player.paused {
            player.accum += get_frame_time() as f64;
            let dt = 1.0 / TICK_RATE;
            while player.accum >= dt {
                player.accum -= dt;
                if player.sim.is_some() {
                    if let Err(e) = player.advance(inputs) {
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
