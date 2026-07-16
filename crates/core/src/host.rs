//! The host facade: everything a frontend needs, nothing else.
//!
//! `Instance` owns card management (load/expand/menu/selection), the wire
//! command dispatch, and read accessors for rendering/UI Ã¢ÂÂ the host-generic
//! half of the sclang/scsynth split. A frontend (macroquad, JS, GodotÃ¢ÂÂ¦) is
//! then only: input devices Ã¢ÂÂ `Inputs`, a fixed-timestep loop calling
//! `advance`, a renderer over `render()`/`recent_events()`/channels, and
//! whatever transport feeds `command_line`. The session (tapes, snapshots,
//! scrubbing) sits underneath and stays reachable for advanced hosts.

use crate::edn::{expand_card, expand_card_with, read_all, Form};
use crate::interp::{load_card, Val, DEFAULT_TICK_RATE};
use crate::model::RenderRow;
use crate::session::Session;
use crate::sim::Sim;

pub use crate::interp::Event;
pub use crate::sim::Inputs;

pub struct Instance {
    card_path: String,
    card_src: String,
    pattern: Option<String>,
    patterns: Vec<String>,
    /// The scrubbable timeline is owned behind this facade.
    session: Session,
    paused: bool,
    status: String,
    /// Virtual filesystem for hosts without one (wasm): path â card text.
    /// When set, loads and import expansion read from here, not the fs.
    vfs: Option<std::collections::HashMap<String, String>>,
    /// Channels this host provides (bindings, mocks). When set, loads
    /// verify the card's (from-host ...) manifest against it â a missing
    /// channel fails the load before tick 0 (specs/load-time-schema).
    host_channels: Option<Vec<String>>,
    /// Render kinds this host understands. None keeps permissive legacy loading.
    render_kinds: Option<Vec<String>>,
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
    /// player contract Ã¢ÂÂ e.g. the stock rig card + an invocation).
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
            host_channels: None,
            render_kinds: None,
        }
    }

    // -- card management ------------------------------------------------

    /// Add or replace one source in the virtual filesystem used by wasm and
    /// other hosts without native filesystem access.
    pub fn add_file(&mut self, path: impl Into<String>, source: impl Into<String>) {
        self.vfs.get_or_insert_with(Default::default).insert(path.into(), source.into());
    }

    /// Declare channel names supplied by this host for load-time verification.
    pub fn set_host_channels<I, S>(&mut self, channels: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.host_channels = Some(channels.into_iter().map(Into::into).collect());
    }

    /// Declare render kinds understood by the selected renderer.
    pub fn set_render_kinds<I, S>(&mut self, kinds: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.render_kinds = Some(kinds.into_iter().map(Into::into).collect());
    }

    /// Read the card from disk (expanding imports) and refresh the pattern
    /// menu. Does NOT play.
    pub fn reload_from_disk(&mut self) -> bool {
        if self.card_path.is_empty() {
            self.status = "no card loaded Ã¢ÂÂ send (load \"path\") or (run Ã¢ÂÂ¦)".into();
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
            Ok(mut sim) => {
                if let Some(provided) = &self.host_channels {
                    let provided: Vec<&str> = provided.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = sim.verify_host_channels(&provided) {
                        self.session.stop();
                        self.status = format!("load error: {} (add a binding row â press B)", e);
                        return;
                    }
                }
                if let Some(supported) = &self.render_kinds {
                    let supported: Vec<&str> = supported.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = sim.verify_render_kinds(&supported) {
                        self.session.stop();
                        self.status = format!("load error: {}", e);
                        return;
                    }
                }
                let lints = sim.load_warnings().join("; ");
                self.session.start(sim);
                let name = self
                    .pattern
                    .clone()
                    .or_else(|| self.patterns.first().cloned())
                    .unwrap_or_default();
                self.status = if lints.is_empty() {
                    format!("{} [{}]", self.card_path, name)
                } else {
                    format!("{} [{}] — lint: {}", self.card_path, name, lints)
                };
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
            format!("cleared Ã¢ÂÂ {} still loaded", self.card_path)
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

    /// Dispatch one parsed wire command form.
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
                            format!("run {}Ã¢ÂÂ¦ (replayed to tick {})", preview, replay_to)
                        } else {
                            format!("run {}Ã¢ÂÂ¦", preview)
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
                        Ok(()) => self.status = format!("add {}Ã¢ÂÂ¦", preview),
                        Err(e) => self.status = format!("add error: {}", e),
                    }
                } else {
                    match self.session.rerun(&self.card_src, &src) {
                        Ok(_) => self.status = format!("add (started fresh) {}Ã¢ÂÂ¦", preview),
                        Err(e) => self.status = format!("add error: {}", e),
                    }
                }
            }
            "swap" => {
                // generational hot-swap, on the command tape
                let src = forms_src(items);
                let preview: String = src.chars().take(40).collect();
                match self.session.record_swap(src) {
                    Ok(()) => self.status = format!("swap {}Ã¢ÂÂ¦", preview),
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
            "resize-entities" => {
                if let Some(Form::Num(n)) = items.get(1) {
                    let max_entities = (*n).max(0.0) as usize;
                    match self.session.record_resize_entities(max_entities) {
                        Ok(()) => self.status = format!("entity capacity -> {}", max_entities),
                        Err(e) => self.status = format!("resize-entities error: {}", e),
                    }
                } else {
                    self.status = "resize-entities: expected numeric capacity".into();
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

    /// The running sim's tick rate in Hz (the default rate when no card
    /// runs) Ã¢ÂÂ hosts pace their fixed-timestep loop against this.
    pub fn tick_rate(&self) -> f64 {
        self.session.sim.as_ref().map(|s| s.world.tick_rate()).unwrap_or(DEFAULT_TICK_RATE)
    }

    pub fn render(&mut self) -> Vec<RenderRow> {
        self.session.sim.as_mut().map(|s| s.render()).unwrap_or_default()
    }

    /// The frame form of `render`: compiled passes stay column batches.
    pub fn render_frame(&mut self) -> Vec<crate::model::RenderItem> {
        self.session.sim.as_mut().map(|s| s.render_frame()).unwrap_or_default()
    }

    /// Stable load-time schema identity for an optional renderer host.
    pub fn declared_render_schema(
        &self,
        kind: &str,
    ) -> Option<std::rc::Rc<crate::model::RenderSchema>> {
        self.session.sim.as_ref().and_then(|sim| sim.declared_render_schema(kind))
    }

    /// Numeric channel value, if the channel currently holds a number.
    pub fn channel_num(&self, name: &str) -> Option<f64> {
        match self.session.sim.as_ref()?.channel_val(name)? {
            Val::Num(value) => Some(value),
            _ => None,
        }
    }

    /// Point-valued channel as `(x, y)`, if present.
    pub fn channel_point(&self, name: &str) -> Option<(f64, f64)> {
        match self.session.sim.as_ref()?.channel_val(name)? {
            Val::Pose(value) => Some((value.x, value.y)),
            _ => None,
        }
    }

    /// Unstable raw channel value for internal tooling.
    #[doc(hidden)]
    pub fn channel(&self, name: &str) -> Option<Val> {
        self.session.sim.as_ref().and_then(|s| s.channel_val(name))
    }

    /// Debug/tooling: the pattern-scoped control cells (not game contract).
    #[doc(hidden)]
    pub fn cells(&self) -> Vec<(String, Val)> {
        self.session.sim.as_ref().map(|s| s.cells_snapshot()).unwrap_or_default()
    }

    /// Events no older than `max_age` ticks, newest first Ã¢ÂÂ for stateless
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

    /// World positions of alive entities carrying a column Ã¢ÂÂ the same
    /// tagged-entity query shape derived channels use (`:pilot`, `:boss`,
    /// or any card-declared marker). Position is the collision-pass sample
    /// (current tick).
    pub fn positions(&self, col: &str) -> Vec<(f64, f64)> {
        let Some(sim) = &self.session.sim else { return Vec::new() };
        sim.world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                sim.world.entities.is_alive(*i) && sim.world.col_get_at(*i, col).is_some()
            })
            .filter_map(|(i, _)| {
                sim.world.entities.latest_sampled_pose(i, sim.world.tick).map(|p| (p.x, p.y))
            })
            .collect()
    }

    pub fn entity_count(&self) -> usize {
        self.session
            .sim
            .as_ref()
            .map(|s| {
                s.world.entities.iter().enumerate().filter(|(i, _)| s.world.entities.is_alive(*i)).count()
            })
            .unwrap_or(0)
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
                s.world.entities.iter().enumerate().any(|(i, _)| {
                    s.world.entities.is_alive(i)
                        && s.world.sym_field_matches_at(i, "team", "player-body")
                        && s.world.col_get_at(i, "iframe-until").map(|u| u > t).unwrap_or(false)
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
            "(defpattern __host-player-rig [] (player-rig))\n{}",
            crate::edn::stdlib("touhou").unwrap()
        );
        let card = "../../cards/translations/130_bowap.maku";
        if !std::path::Path::new(card).exists() {
            return; // repository-level integration fixture is not in the crate archive
        }
        let mut inst = Instance::new(Some(rig));
        inst.boot(card.into(), None);
        assert!(inst.running(), "{}", inst.status());
        assert!(inst.patterns().len() >= 2, "menu populated");

        let mut inputs = Inputs::classic((0.0, -4.0), (0.0, 3.0));
        inputs.set_num("move-x", 0.0);
        inputs.set_num("move-y", 0.0);
        inputs.set_num("focus-firing", 0.0);
        inputs.set_num("bomb", 0.0);
        for _ in 0..240 {
            inst.advance(inputs.clone());
        }
        assert_eq!(inst.tick(), Some(240), "{}", inst.status);
        assert!(!inst.render().is_empty(), "entities to draw");
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
