//! The host facade: everything a frontend needs, nothing else.
//!
//! `Instance` owns card management (load/expand/menu/selection), the wire
//! command dispatch, and read accessors for rendering/UI — the host-generic
//! half of the sclang/scsynth split. A frontend (macroquad, JS, Godot…) is
//! then only: input devices → `Inputs`, a fixed-timestep loop calling
//! `advance`, a renderer over `render()`/`recent_events()`/channels, and
//! whatever transport feeds `command_line`. The session (tapes, snapshots,
//! scrubbing) sits underneath and stays reachable for advanced hosts.

use crate::edn::{expand_card, expand_card_with, read_all, Form};
use crate::interp::{load_card, Event, Style, Val};
use crate::session::Session;
use crate::sim::{Inputs, RenderItem, Sim};

pub struct Instance {
    card_path: String,
    card_src: String,
    pattern: Option<String>,
    patterns: Vec<String>,
    /// The scrubbable timeline; public for advanced hosts (timeline UIs).
    pub session: Session,
    paused: bool,
    status: String,
    /// Virtual filesystem for hosts without one (wasm): path → card text.
    /// When set, loads and import expansion read from here, not the fs.
    pub vfs: Option<std::collections::HashMap<String, String>>,
}

/// Timeline info for scrub UIs.
pub struct Timeline {
    pub tick: u64,
    pub tape_len: usize,
    pub snap_ticks: Vec<u64>,
    pub cmd_ticks: Vec<u64>,
}

impl Instance {
    /// `rig`: form source layered into every fresh timeline (the host's
    /// player contract — e.g. the stock rig card + an invocation).
    pub fn new(rig: Option<String>) -> Instance {
        let mut session = Session::default();
        session.rig = rig;
        Instance {
            card_path: String::new(),
            card_src: String::new(),
            pattern: None,
            patterns: Vec::new(),
            session,
            paused: false,
            status: String::new(),
            vfs: None,
        }
    }

    // -- card management ------------------------------------------------

    /// Read the card from disk (expanding imports) and refresh the pattern
    /// menu. Does NOT play.
    pub fn reload_from_disk(&mut self) -> bool {
        if self.card_path.is_empty() {
            self.status = "no card loaded — send (load \"path\") or (run …)".into();
            return false;
        }
        let expanded = match &self.vfs {
            Some(files) => expand_card_with(&self.card_path, &|p| {
                files.get(p).cloned().ok_or_else(|| format!("{}: not in vfs", p))
            }),
            None => expand_card(std::path::Path::new(&self.card_path)),
        };
        match expanded {
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

    /// (Re-)instantiate and run the selected (or first) pattern on a fresh
    /// timeline.
    pub fn restart(&mut self) {
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

    /// Reload the card from disk and restart (the "r" hotkey).
    pub fn reload_restart(&mut self) {
        if self.reload_from_disk() {
            self.restart();
        }
    }

    /// Boot from a CLI-style card argument (auto-plays: explicit intent).
    pub fn boot(&mut self, card_path: String, pattern: Option<String>) {
        self.card_path = card_path;
        self.pattern = pattern;
        if !self.card_path.is_empty() && self.reload_from_disk() {
            self.restart();
        }
    }

    /// Stop the running pattern; keep the card loaded.
    pub fn clear(&mut self) {
        self.session.stop();
        self.status = if self.card_src.is_empty() {
            "cleared".into()
        } else {
            format!("cleared — {} still loaded", self.card_path)
        };
    }

    /// Select a pattern from the menu by index and play it.
    pub fn select(&mut self, idx: usize) {
        if let Some(name) = self.patterns.get(idx) {
            self.pattern = Some(name.clone());
            self.restart();
        }
    }

    // -- stepping / timeline ---------------------------------------------

    /// Advance one tick with these inputs. Sim errors set status and pause.
    pub fn advance(&mut self, inputs: Inputs) {
        self.session.last_inputs = inputs;
        if self.session.sim.is_some() {
            if let Err(e) = self.session.advance(&self.card_src) {
                self.status = format!("sim error: {}", e);
                self.paused = true;
            }
        }
    }

    /// Scrub to an absolute tick (pauses).
    pub fn seek(&mut self, target: u64) {
        self.paused = true;
        match self.session.seek(&self.card_src, target) {
            Ok(()) => self.status = format!("scrub @ tick {}", target),
            Err(e) => self.status = format!("seek: {}", e),
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Resuming after a rewind branches the timeline (truncates the future).
    pub fn set_paused(&mut self, p: bool) {
        self.paused = p;
        if !p {
            self.session.truncate_future();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.set_paused(!self.paused);
    }

    // -- wire commands ----------------------------------------------------

    /// Parse a wire line (one or more forms) and dispatch each.
    pub fn command_line(&mut self, line: &str) {
        match read_all(line) {
            Ok(forms) => {
                for f in &forms {
                    self.command(f);
                }
            }
            Err(e) => self.status = format!("command parse: {}", e),
        }
    }

    /// Dispatch one wire command form (see docs/player.md for the protocol).
    pub fn command(&mut self, form: &Form) {
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
        let forms_src = |items: &[Form]| {
            items[1..].iter().map(|f| f.to_string()).collect::<Vec<_>>().join(" ")
        };
        match head.as_str() {
            "run" => {
                // replace the program; the input tape replays through the
                // NEW code (the pause/rewind/edit/re-run loop)
                let src = forms_src(items);
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
                // layer at the current tick, on the command tape
                let src = forms_src(items);
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
                // generational hot-swap, on the command tape
                let src = forms_src(items);
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
            "step" => {
                let n = match items.get(1) {
                    Some(Form::Num(n)) => *n,
                    _ => 1.0,
                };
                if let Some(cur) = self.session.tick() {
                    self.seek((cur as f64 + n).max(0.0) as u64);
                }
            }
            "snapshots" => {
                if let Some(Form::Num(n)) = items.get(1) {
                    self.session.snap_every = (*n).max(0.0) as u64;
                    self.status = if self.session.snap_every == 0 {
                        "snapshots off (scrub-back replays from tick 0)".into()
                    } else {
                        format!("snapshots every {} ticks", self.session.snap_every)
                    };
                }
            }
            "load" => {
                // (load "path") = load only; (load "path" "pattern") = play
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
            "resume" => self.set_paused(false),
            _ => self.status = format!("unknown command '{}'", head),
        }
    }

    // -- reads --------------------------------------------------------------

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    pub fn running(&self) -> bool {
        self.session.sim.is_some()
    }

    pub fn tick(&self) -> Option<u64> {
        self.session.tick()
    }

    pub fn render(&self) -> Vec<RenderItem> {
        self.session.sim.as_ref().map(|s| s.render()).unwrap_or_default()
    }

    pub fn channel(&self, name: &str) -> Option<Val> {
        self.session.sim.as_ref().and_then(|s| s.channel_val(name))
    }

    /// Debug/tooling: the pattern-scoped control cells (not game contract).
    pub fn cells(&self) -> Vec<(String, Val)> {
        self.session.sim.as_ref().map(|s| s.cells_snapshot()).unwrap_or_default()
    }

    /// Events no older than `max_age` ticks, newest first — for stateless
    /// effect flashes (they replay under scrubbing).
    pub fn recent_events(&self, max_age: u64) -> Vec<Event> {
        let Some(sim) = &self.session.sim else { return Vec::new() };
        let now = sim.tick();
        sim.with_events(|events| {
            events
                .iter()
                .rev()
                .take_while(|e| now.saturating_sub(e.tick) <= max_age)
                .cloned()
                .collect()
        })
    }

    /// World positions of alive entities carrying a column — the same
    /// tagged-entity query shape derived channels use (`:pilot`, `:boss`,
    /// or any card-declared marker). Position is the collision-pass sample
    /// (current tick).
    pub fn positions(&self, col: &str) -> Vec<(f64, f64)> {
        let Some(sim) = &self.session.sim else { return Vec::new() };
        sim.world
            .bullets
            .iter()
            .filter(|b| b.alive && b.col_get(col).is_some())
            .filter_map(|b| b.prev_pos)
            .collect()
    }

    pub fn entity_count(&self) -> usize {
        self.session.sim.as_ref().map(|s| s.world.bullets.len()).unwrap_or(0)
    }

    pub fn graze(&self) -> u64 {
        self.session.sim.as_ref().map(|s| s.channel_u64("graze")).unwrap_or(0)
    }

    pub fn player_hits(&self) -> u64 {
        self.session.sim.as_ref().map(|s| s.channel_u64("hits")).unwrap_or(0)
    }

    /// Post-hit invulnerability active on any player entity (marker
    /// flicker). Iframes are a per-entity column.
    pub fn iframes_active(&self) -> bool {
        self.session
            .sim
            .as_ref()
            .map(|s| {
                let t = s.tick() as f64;
                s.world.bullets.iter().any(|b| {
                    b.alive
                        && b.team.as_deref() == Some("player-body")
                        && b.col_get("iframe-until").map(|u| u > t).unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// The selected (or default first) pattern name.
    pub fn current_pattern(&self) -> Option<String> {
        self.pattern.clone().or_else(|| self.patterns.first().cloned())
    }

    pub fn timeline(&self) -> Option<Timeline> {
        let tick = self.session.tick()?;
        Some(Timeline {
            tick,
            tape_len: self.session.tape.len(),
            snap_ticks: self.session.snaps.iter().map(|(t, _)| *t).collect(),
            cmd_ticks: self.session.cmd_ticks(),
        })
    }
}

// -- shared render helpers (any host) ----------------------------------------

/// Stock palette for style colors, as sRGB bytes.
pub fn style_rgb(color: &str) -> (u8, u8, u8) {
    match color {
        "red" => (0xff, 0x4d, 0x5e),
        "orange" => (0xff, 0x9d, 0x3c),
        "yellow" => (0xff, 0xe0, 0x66),
        "green" => (0x66, 0xe0, 0x85),
        "teal" => (0x4d, 0xd8, 0xd0),
        "blue" => (0x5c, 0x9d, 0xff),
        "purple" => (0xb2, 0x7d, 0xff),
        "pink" => (0xff, 0x85, 0xc2),
        "black" => (0x60, 0x60, 0x70),
        "blueteal" => (0x4d, 0xbc, 0xe8),
        _ => (0xff, 0xff, 0xff),
    }
}

/// Style color with a hue-shift (degrees) applied, as linear 0–1 RGB.
pub fn style_rgb_hued(style: &Style, hue_deg: f64) -> (f32, f32, f32) {
    let (r, g, b) = style_rgb(&style.color);
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    if hue_deg.abs() < 1e-9 {
        return (r, g, b);
    }
    let (h, s, l) = rgb_to_hsl(r, g, b);
    hsl_to_rgb((h + hue_deg as f32).rem_euclid(360.0), s, l)
}

/// Display radius per family, in world units (render-contract default).
pub fn dot_radius(family: &str) -> f32 {
    match family {
        "lstar" | "gglcircle" => 10.0 / 55.0,
        "gem" | "star" => 5.0 / 55.0,
        _ => 6.0 / 55.0,
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
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

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
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
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frontend's whole life, headless: boot a card through the facade,
    /// drive it with inputs, read everything a renderer needs, scrub, and
    /// hot-eval through the wire dispatch. If this passes, a new frontend
    /// only owes input devices + drawing + transport.
    #[test]
    fn facade_drives_a_frontend() {
        let rig = format!(
            "{}\n(player-rig)",
            crate::edn::stdlib("player-rig").unwrap()
        );
        let mut inst = Instance::new(Some(rig));
        inst.boot("../../cards/translations/130_bowap.maku".into(), None);
        assert!(inst.running());
        assert!(inst.patterns().len() >= 2, "menu populated");

        let inputs = Inputs::default();
        for _ in 0..240 {
            inst.advance(inputs.clone());
        }
        assert_eq!(inst.tick(), Some(240));
        assert!(!inst.render().is_empty(), "bullets to draw");
        assert!(inst.channel("player").is_some());

        // wire dispatch: hot-eval an anonymous pattern (tape replays)
        inst.command_line("(run (spawn (circle 4 (linear c[1 0]))))");
        inst.advance(inputs.clone());
        assert!(inst.entity_count() >= 4);

        // scrub through the facade
        inst.command_line("(seek 100)");
        assert!(inst.paused());
        assert_eq!(inst.tick(), Some(100));
        let tl = inst.timeline().unwrap();
        assert!(tl.tape_len >= 240, "tape preserved across the rerun+seek");

        inst.command_line("(clear)");
        assert!(!inst.running());
    }
}
