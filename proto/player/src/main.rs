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

use danmaku_core::host::Instance;
use danmaku_core::interp::{Val, TICK_RATE};
use danmaku_core::sim::{Inputs, RenderItem};
use macroquad::prelude::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver};

const PORT: u16 = 7777;
const PIXELS_PER_UNIT: f32 = 55.0;
const TIMELINE_H: f32 = 30.0;

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
fn timeline_ui(app: &mut App, mx: f32, my: f32) {
    let Some(tl) = app.inst.timeline() else { return };
    let cur = tl.tick;
    let h = screen_height();
    let w = screen_width();
    let cy = h - TIMELINE_H / 2.0;
    let total = tl.tape_len.max(1) as f32;

    // play/pause button
    let (bx, br) = (22.0, 9.0);
    let over_btn = (mx - bx).abs() < 14.0 && (my - cy).abs() < 14.0;
    let btn_col = if over_btn { WHITE } else { GRAY };
    if app.inst.paused() {
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
        app.inst.toggle_pause();
        return;
    }

    // slider track
    let (x0, x1) = (44.0, w - 96.0);
    let frac = (cur as f32 / total).clamp(0.0, 1.0);
    let hx = x0 + frac * (x1 - x0);
    draw_line(x0, cy, x1, cy, 3.0, Color::new(1.0, 1.0, 1.0, 0.15));
    draw_line(x0, cy, hx, cy, 3.0, Color::new(0.4, 0.7, 1.0, 0.8));
    // snapshot notches
    for st in &tl.snap_ticks {
        let sx = x0 + (*st as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 4.0, sx, cy + 4.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.2));
    }
    // command-tape markers: where adds/swaps landed
    for ct in &tl.cmd_ticks {
        let sx = x0 + (*ct as f32 / total).clamp(0.0, 1.0) * (x1 - x0);
        draw_line(sx, cy - 6.0, sx, cy + 6.0, 2.0, Color::new(1.0, 0.7, 0.3, 0.8));
    }
    draw_circle(hx, cy, 6.0, if app.dragging { WHITE } else { LIGHTGRAY });
    draw_text(
        &format!("{} / {}", cur, tl.tape_len),
        x1 + 10.0,
        cy + 5.0,
        16.0,
        GRAY,
    );

    // drag to scrub
    let over_track = mx >= x0 - 8.0 && mx <= x1 + 8.0 && (my - cy).abs() < 12.0;
    if over_track && is_mouse_button_pressed(MouseButton::Left) {
        app.dragging = true;
    }
    if !is_mouse_button_down(MouseButton::Left) {
        app.dragging = false;
    }
    if app.dragging {
        let f = ((mx - x0) / (x1 - x0)).clamp(0.0, 1.0);
        let target = (f * tl.tape_len as f32).round() as u64;
        if target != cur {
            app.inst.seek(target);
        }
    }
}

/// Host-local per-frame state around the core Instance.
struct App {
    inst: Instance,
    accum: f64,
    dragging: bool, // scrubbing via the timeline slider
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

    // this host's player contract: layer the stock rig card
    // (swap in your own live: <localleader>es a rig defpattern)
    let rig = format!("{}\n(player-rig)", include_str!("../../../cards/player-rig.dmk"));
    let mut app = App { inst: Instance::new(Some(rig)), accum: 0.0, dragging: false };
    if card_path.is_empty() {
        app.inst.set_status(format!("no card — listening on 127.0.0.1:{}", PORT));
    } else {
        app.inst.boot(card_path, pattern);
    }
    let commands = serve(PORT);

    loop {
        // server commands
        while let Ok(line) = commands.try_recv() {
            app.inst.command_line(&line);
        }
        // hotkeys: r = restart from disk, c = clear, space = pause, esc = quit
        if is_key_pressed(KeyCode::R) {
            app.inst.reload_restart();
        }
        if is_key_pressed(KeyCode::C) {
            app.inst.clear();
        }
        if is_key_pressed(KeyCode::Space) {
            app.inst.toggle_pause();
        }
        // scrub hotkeys — only while paused (live arrows belong to movement)
        if app.inst.paused() {
            if let Some(cur) = app.inst.tick() {
                if is_key_pressed(KeyCode::Right) {
                    app.inst.seek(cur + 1);
                }
                if is_key_pressed(KeyCode::Left) {
                    app.inst.seek(cur.saturating_sub(1));
                }
                if is_key_pressed(KeyCode::Up) {
                    app.inst.seek(cur + 30);
                }
                if is_key_pressed(KeyCode::Down) {
                    app.inst.seek(cur.saturating_sub(30));
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
                app.inst.select(i);
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
            + if !app.inst.paused() {
                (key(KeyCode::Right) as i8 - key(KeyCode::Left) as i8) as f64
            } else {
                0.0
            };
        let ay = (key(KeyCode::W) as i8 - key(KeyCode::S) as i8) as f64
            + if !app.inst.paused() {
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

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !app.inst.paused() {
            app.accum += get_frame_time() as f64;
            let dt = 1.0 / TICK_RATE;
            while app.accum >= dt {
                app.accum -= dt;
                app.inst.advance(inputs);
                if app.inst.paused() {
                    break; // sim error pauses
                }
            }
        }

        clear_background(Color::from_rgba(0x12, 0x12, 0x1a, 0xff));
        // player marker at the $player channel (derived from a piloted rig,
        // or the mouse): true hitbox dot + graze ring
        let (pmx, pmy) = match app.inst.channel("player") {
            Some(Val::Vec2 { x, y }) => {
                (cx + x as f32 * PIXELS_PER_UNIT, cy - y as f32 * PIXELS_PER_UNIT)
            }
            _ => (mx, my),
        };
        draw_circle_lines(pmx, pmy, 0.35 * PIXELS_PER_UNIT, 1.0, Color::new(1.0, 1.0, 1.0, 0.25));
        draw_circle_lines(pmx, pmy, 8.0, 2.0, Color::new(1.0, 1.0, 1.0, 0.8));
        draw_circle(pmx, pmy, 0.06 * PIXELS_PER_UNIT, WHITE);
        if app.inst.running() {
            let to_screen =
                |x: f64, y: f64| (cx + x as f32 * PIXELS_PER_UNIT, cy - y as f32 * PIXELS_PER_UNIT);
            for item in app.inst.render() {
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
            // event flashes: expanding rings read straight from the event
            // log (stateless — rewind and they replay with the timeline)
            let now = app.inst.tick().unwrap_or(0);
            for ev in app.inst.recent_events(24) {
                let k = now.saturating_sub(ev.tick) as f32 / 24.0;
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
            // post-hit iframes: flash the player marker
            if app.inst.iframes_active() && (now / 6) % 2 == 0 {
                draw_circle_lines(pmx, pmy, 14.0, 2.0, Color::new(1.0, 0.3, 0.3, 0.8));
            }

            // scrub indicators: where the position channels ARE at this tick
            // (while paused they diverge from the live mouse)
            if app.inst.paused() {
                for (name, col) in
                    [("player", Color::new(1.0, 1.0, 1.0, 0.9)), ("nearest-enemy", ORANGE)]
                {
                    if let Some(Val::Vec2 { x, y }) = app.inst.channel(name) {
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
                    app.inst.status(),
                    now,
                    app.inst.entity_count(),
                    app.inst.graze(),
                    app.inst.player_hits(),
                    match app.inst.channel("lives") {
                        Some(Val::Num(n)) => format!("{}", n),
                        _ => "-".into(),
                    },
                    if app.inst.paused() { "[paused]" } else { "" }
                ),
                12.0,
                24.0,
                22.0,
                GRAY,
            );
        } else {
            draw_text(app.inst.status(), 12.0, 24.0, 22.0, RED);
        }
        // pattern menu (above the timeline strip)
        let current = app.inst.current_pattern().unwrap_or_default();
        let n_patterns = app.inst.patterns().len();
        for (i, name) in app.inst.patterns().iter().enumerate() {
            let sel = *name == current;
            draw_text(
                &format!("{} {}", i + 1, name),
                12.0,
                screen_height() - TIMELINE_H - 14.0 * (n_patterns - i) as f32,
                18.0,
                if sel { WHITE } else { GRAY },
            );
        }
        timeline_ui(&mut app, mx, my);
        next_frame().await;
    }
}
