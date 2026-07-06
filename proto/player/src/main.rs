//! The debug player: a sim+render SERVER (sclang/scsynth split).
//!
//! Usage: maku-player [card.dmk [pattern-name]]
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

use maku_core::host::Instance;
use maku_core::interp::{Val, TICK_RATE};
use maku_core::sim::{Inputs, RenderItem};
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

/// Style color with hue shift, as a macroquad Color (core::host palette).
fn styled(style: &maku_core::interp::Style, hue: f64) -> Color {
    let (r, g, b) = maku_core::host::style_rgb_hued(style, hue);
    Color::new(r, g, b, 1.0)
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
mod bindings;
use bindings::Bindings;

struct App {
    inst: Instance,
    accum: f64,
    dragging: bool,   // scrubbing via the timeline slider
    binds: Bindings,  // key→channel bindings + constant channels (B panel)
}

fn window_conf() -> Conf {
    Conf {
        window_title: "maku-player".into(),
        window_width: 900,
        window_height: 960,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // usage: maku-player [card.dmk [pattern-name]] — with no card, start
    // empty and wait for (load ...) / (run ...) from clients
    let mut args = std::env::args().skip(1);
    let card_path = args.next().unwrap_or_default();
    let pattern = args.next();

    // this host's player contract: layer the stock rig card
    // (swap in your own live: <localleader>es a rig defpattern)
    let rig = format!(
        "{}\n(player-rig)",
        maku_core::edn::stdlib("player-rig").unwrap()
    );
    let mut app = App {
        inst: Instance::new(Some(rig)),
        accum: 0.0,
        dragging: false,
        binds: Bindings::defaults(),
    };
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
        let panel_open = app.binds.wants_keys();
        if !panel_open && is_key_pressed(KeyCode::R) {
            app.inst.reload_restart();
        }
        if !panel_open && is_key_pressed(KeyCode::C) {
            app.inst.clear();
        }
        if !panel_open && is_key_pressed(KeyCode::Space) {
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
            if !panel_open && is_key_pressed(*key) {
                app.inst.select(i);
            }
        }
        // difficulty: T/Y/U/I quick-set the $rank constant (easy..lunatic)
        if !panel_open {
            for (key, r) in [
                (KeyCode::T, 0.7),
                (KeyCode::Y, 1.0),
                (KeyCode::U, 1.4),
                (KeyCode::I, 2.0),
            ] {
                if is_key_pressed(key) {
                    app.binds.set_const("rank", r);
                }
            }
        }
        // Tab / Shift-Tab step through ALL patterns (hotkeys stop at 9)
        if !panel_open && is_key_pressed(KeyCode::Tab) {
            let names = app.inst.patterns().to_vec();
            if !names.is_empty() {
                let cur = app.inst.current_pattern().unwrap_or_default();
                let at = names.iter().position(|n| *n == cur);
                let next = if is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift) {
                    at.map(|i| (i + names.len() - 1) % names.len()).unwrap_or(names.len() - 1)
                } else {
                    at.map(|i| (i + 1) % names.len()).unwrap_or(0)
                };
                app.inst.select(next);
            }
        }
        if !panel_open && is_key_pressed(KeyCode::Escape) {
            break;
        }

        // mock player rides the mouse (design.md §11: sandbox mock player)
        let (cx, cy) = (screen_width() / 2.0, screen_height() / 2.0 + 100.0);
        let (mx, my) = mouse_position();
        let mouse_world = (
            ((mx - cx) / PIXELS_PER_UNIT) as f64,
            ((cy - my) / PIXELS_PER_UNIT) as f64,
        );
        // raw input side of the host contract: the BINDING TABLE (B panel)
        // maps keys to channels — axes for movement, buttons with
        // tap/hold/toggle modes, plus constant channels ($rank lives
        // there). The mouse remains the mock $player / $nearest-enemy.
        // Replays are unaffected: the tape records channel VALUES.
        let mut inputs = Inputs::classic(mouse_world, mouse_world);
        if !panel_open {
            app.binds.poll_edges();
        }
        app.binds.inject(&mut inputs, app.inst.paused());

        // fixed-timestep sim (design.md §4: variable dt never reaches the sim)
        if !app.inst.paused() {
            app.accum += get_frame_time() as f64;
            let dt = 1.0 / TICK_RATE;
            while app.accum >= dt {
                app.accum -= dt;
                app.inst.advance(inputs.clone());
                // taps are high for exactly one stepped tick
                app.binds.consume_taps(&mut inputs);
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
                    RenderItem::Dot { x, y, style, hue, scale, alpha, .. } => {
                        let (sx, sy) = to_screen(x, y);
                        let r = maku_core::host::dot_radius(&style.family)
                            * PIXELS_PER_UNIT
                            * scale as f32;
                        let a = alpha.clamp(0.0, 1.0) as f32;
                        let mut col = styled(&style, hue);
                        col.a *= a;
                        draw_circle(sx, sy, r, col);
                        draw_circle_lines(sx, sy, r, 1.5, Color::new(1.0, 1.0, 1.0, 0.35 * a));
                    }
                    RenderItem::Polyline { pts, style, active, hue, alpha } => {
                        let a = alpha.clamp(0.0, 1.0) as f32;
                        let mut col = styled(&style, hue);
                        col.a *= a;
                        let (w, col) = if active {
                            (6.0, col)
                        } else {
                            (1.5, Color::new(col.r, col.g, col.b, 0.45 * a))
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
        // pattern menu (above the timeline strip): every entry is a
        // click target — hotkeys 1-9 only reach the first nine
        let current = app.inst.current_pattern().unwrap_or_default();
        let n_patterns = app.inst.patterns().len();
        let names: Vec<String> = app.inst.patterns().to_vec();
        let mut clicked: Option<usize> = None;
        for (i, name) in names.iter().enumerate() {
            let sel = *name == current;
            let y = screen_height() - TIMELINE_H - 14.0 * (n_patterns - i) as f32;
            let label = if i < 9 {
                format!("{} {}", i + 1, name)
            } else {
                format!("  {}", name)
            };
            let w = measure_text(&label, None, 18, 1.0).width;
            let hover = mx >= 12.0 && mx <= 12.0 + w && my >= y - 14.0 && my <= y + 4.0;
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                clicked = Some(i);
            }
            draw_text(
                &label,
                12.0,
                y,
                18.0,
                if sel {
                    WHITE
                } else if hover {
                    YELLOW
                } else {
                    GRAY
                },
            );
        }
        if let Some(i) = clicked {
            app.inst.select(i);
        }
        draw_text(
            &format!("rank {:.1} (T/Y/U/I)  [B]indings", app.binds.get_const("rank").unwrap_or(1.0)),
            screen_width() - 230.0,
            24.0,
            18.0,
            GRAY,
        );
        timeline_ui(&mut app, mx, my);
        app.binds.ui();
        next_frame().await;
    }
}
