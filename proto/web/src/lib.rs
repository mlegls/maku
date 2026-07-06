//! wasm bindings over core::host::Instance — the browser is just another
//! host: input devices → Inputs, a fixed-timestep loop, a canvas renderer
//! over flat f32 buffers, and command_line as the transport. Cards arrive
//! through the vfs (fetched by the page; import expansion runs in core).

use maku_core::host::{dot_radius, style_rgb_hued, Instance};
use maku_core::interp::Val;
use maku_core::sim::{Inputs, RenderItem};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = stdlibSource)]
pub fn stdlib_source(name: &str) -> Option<String> {
    maku_core::edn::stdlib(name).map(str::to_owned)
}

#[wasm_bindgen]
pub struct Danmaku {
    inst: Instance,
    pending: Inputs,
}

#[wasm_bindgen]
impl Danmaku {
    #[wasm_bindgen(constructor)]
    pub fn new(rig: Option<String>) -> Danmaku {
        Danmaku { inst: Instance::new(rig), pending: Inputs::default() }
    }

    /// Register a card file in the virtual filesystem (path → text).
    pub fn add_file(&mut self, path: String, text: String) {
        self.inst
            .vfs
            .get_or_insert_with(Default::default)
            .insert(path, text);
    }

    pub fn boot(&mut self, path: String, pattern: Option<String>) {
        self.inst.boot(path, pattern);
    }

    /// Wire protocol (docs/player.md): run/swap/add/load/pattern/restart/
    /// clear/seek/step/snapshots/pause/resume.
    pub fn command(&mut self, line: &str) {
        self.inst.command_line(line);
    }

    /// Set a numeric input channel for subsequent steps ($move-x,
    /// $p2-move-x, $focus-firing, $bomb — an open vocabulary, by name).
    pub fn input_num(&mut self, name: &str, v: f64) {
        self.pending.set_num(name, v);
    }

    /// Set a Vec2 input channel ($player mock, $nearest-enemy mock, …).
    pub fn input_vec2(&mut self, name: &str, x: f64, y: f64) {
        self.pending.set_vec2(name, x, y);
    }

    /// Advance up to `n` ticks with the pending inputs (host accumulates
    /// frame time; 120 ticks = 1 s).
    pub fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.inst.paused() {
                break;
            }
            self.inst.advance(self.pending.clone());
        }
    }

    pub fn paused(&self) -> bool {
        self.inst.paused()
    }

    pub fn toggle_pause(&mut self) {
        self.inst.toggle_pause();
    }

    pub fn seek(&mut self, tick: f64) {
        self.inst.seek(tick.max(0.0) as u64);
    }

    pub fn select(&mut self, idx: usize) {
        self.inst.select(idx);
    }

    pub fn restart(&mut self) {
        self.inst.reload_restart();
    }

    // -- reads (flat buffers for the renderer) ---------------------------

    pub fn tick(&self) -> f64 {
        self.inst.tick().unwrap_or(0) as f64
    }

    pub fn status(&self) -> String {
        self.inst.status().to_string()
    }

    pub fn running(&self) -> bool {
        self.inst.running()
    }

    pub fn entity_count(&self) -> usize {
        self.inst.entity_count()
    }

    pub fn graze(&self) -> f64 {
        self.inst.graze() as f64
    }

    pub fn hits(&self) -> f64 {
        self.inst.player_hits() as f64
    }

    /// Lives column via the $lives channel; -1 when absent.
    pub fn lives(&self) -> f64 {
        match self.inst.channel("lives") {
            Some(Val::Num(n)) => n,
            _ => -1.0,
        }
    }

    pub fn iframes(&self) -> bool {
        self.inst.iframes_active()
    }

    /// Newline-joined pattern menu.
    pub fn patterns(&self) -> String {
        self.inst.patterns().join("\n")
    }

    pub fn current_pattern(&self) -> String {
        self.inst.current_pattern().unwrap_or_default()
    }

    /// [x, y] of a Vec2-valued channel ($player, $boss, …), or empty.
    pub fn channel_vec(&self, name: &str) -> Vec<f32> {
        match self.inst.channel(name) {
            Some(Val::Vec2 { x, y }) => vec![x as f32, y as f32],
            _ => Vec::new(),
        }
    }

    /// Numeric channel ($lives, $boss-hp, $graze, …); NaN when absent.
    pub fn channel_num(&self, name: &str) -> f64 {
        match self.inst.channel(name) {
            Some(Val::Num(n)) => n,
            _ => f64::NAN,
        }
    }

    /// [x, y]* of alive entities carrying a column (:pilot, :boss, or any
    /// card-declared marker) — generic tagged-entity positions.
    pub fn positions(&self, col: &str) -> Vec<f32> {
        self.inst
            .positions(col)
            .into_iter()
            .flat_map(|(x, y)| [x as f32, y as f32])
            .collect()
    }

    /// Debug: pattern-scoped control cells as "name=value" lines (an
    /// inspector view — cells are not part of the host game contract).
    pub fn cells(&self) -> String {
        self.inst
            .cells()
            .into_iter()
            .map(|(k, v)| format!("{}={:?}", k, v))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// [x, y] of the $player channel, or empty (sugar for channel_vec).
    pub fn player_pos(&self) -> Vec<f32> {
        self.channel_vec("player")
    }

    /// Point bullets: [x, y, radius_world, r, g, b, a]* (colors 0–1, hue
    /// pre-applied — every host renders identical colors; :scale is
    /// pre-applied to the radius, :opacity arrives as a).
    pub fn dots(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for item in self.inst.render() {
            if let RenderItem::Dot { x, y, style, hue, scale, alpha, .. } = item {
                let (r, g, b) = style_rgb_hued(&style, hue);
                out.extend_from_slice(&[
                    x as f32,
                    y as f32,
                    dot_radius(&style.family) * scale as f32,
                    r,
                    g,
                    b,
                    alpha.clamp(0.0, 1.0) as f32,
                ]);
            }
        }
        out
    }

    /// Lasers/pathers: [active, r, g, b, a, n, x1, y1, … xn, yn]* repeated.
    pub fn beams(&self) -> Vec<f32> {
        let mut out = Vec::new();
        for item in self.inst.render() {
            if let RenderItem::Polyline { pts, style, active, hue, alpha } = item {
                let (r, g, b) = style_rgb_hued(&style, hue);
                out.extend_from_slice(&[
                    if active { 1.0 } else { 0.0 },
                    r,
                    g,
                    b,
                    alpha.clamp(0.0, 1.0) as f32,
                    pts.len() as f32,
                ]);
                for (x, y) in pts {
                    out.extend_from_slice(&[x as f32, y as f32]);
                }
            }
        }
        out
    }

    /// Recent positioned events for effect flashes: [code, age_ticks, x, y]*
    /// (codes: 0 graze, 1 player-hit, 2 enemy-hit, 3 died). Stateless — they
    /// replay under scrubbing.
    pub fn flashes(&self, max_age: f64) -> Vec<f32> {
        let now = self.inst.tick().unwrap_or(0);
        let mut out = Vec::new();
        for ev in self.inst.recent_events(max_age as u64) {
            let code = match &*ev.name {
                "graze" => 0.0,
                "player-hit" => 1.0,
                "enemy-hit" => 2.0,
                "died" => 3.0,
                _ => continue,
            };
            if let Some((x, y)) = ev.pos {
                out.extend_from_slice(&[
                    code,
                    now.saturating_sub(ev.tick) as f32,
                    x as f32,
                    y as f32,
                ]);
            }
        }
        out
    }

    /// [tick, tape_len] — timeline extent for the scrub slider.
    pub fn timeline(&self) -> Vec<f32> {
        match self.inst.timeline() {
            Some(tl) => vec![tl.tick as f32, tl.tape_len as f32],
            None => Vec::new(),
        }
    }

    /// Command-tape ticks (orange markers on the slider).
    pub fn cmd_ticks(&self) -> Vec<f32> {
        self.inst
            .timeline()
            .map(|tl| tl.cmd_ticks.iter().map(|t| *t as f32).collect())
            .unwrap_or_default()
    }
}
