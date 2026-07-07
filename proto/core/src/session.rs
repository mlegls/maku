//! A scrubbable session: the sim as a deterministic fold over TWO tapes
//! (design.md §11) — the input tape (one `Inputs` per tick) and the command
//! tape (program changes: add/swap, stamped with the tick they landed on).
//! Any tick is reachable as nearest-snapshot + re-step, and program changes
//! re-apply during the re-step, so scrubbing back across an (add ...) or
//! (swap ...) boundary reproduces the layered/swapped timeline exactly.
//!
//! Replay invariant: a command recorded at tick T applies in `advance()`
//! BEFORE stepping T→T+1, and the snapshot at tick X is taken on arrival at
//! X (before any commands at X apply) — so each command applies exactly once
//! per timeline pass, live or replayed.

use crate::edn::read_all;
use crate::sim::{Inputs, Sim};

/// One snapshot per second of sim time (default cadence).
pub const SNAP_EVERY: u64 = 120;
/// Snapshot count that triggers thinning of the older half.
pub const MAX_SNAPS: usize = 240;

#[derive(Clone)]
enum ProgCmd {
    Add(String),
    Swap(String),
    ResizeEntities(usize),
}

pub struct Session {
    pub sim: Option<Sim>,
    /// tape[t] stepped the sim t → t+1.
    pub tape: Vec<Inputs>,
    /// Periodic snapshots, ascending by tick.
    pub snaps: Vec<(u64, Sim)>,
    cmds: Vec<(u64, ProgCmd)>,
    /// Live inputs used when advancing past the recorded tape.
    pub last_inputs: Inputs,
    /// Host policy: form source (e.g. a player-rig defpattern) layered into
    /// every fresh timeline as an (add ...) at tick 0 — it rides the command
    /// tape, so card + tapes fully determine a replay (no hidden host state).
    pub rig: Option<String>,
    /// Snapshot cadence in ticks; 0 disables snapshotting beyond the
    /// start-tick baseline (long soak runs; scrub-back replays from it).
    pub snap_every: u64,
    /// Thinning threshold: past this count the OLDER half drops every other
    /// snapshot — logarithmic density, so recent scrubbing stays
    /// fine-grained while memory stays bounded; distant seeks just re-step
    /// more ticks from a sparser base.
    pub max_snaps: usize,
}

impl Default for Session {
    fn default() -> Session {
        Session {
            sim: None,
            tape: Vec::new(),
            snaps: Vec::new(),
            cmds: Vec::new(),
            last_inputs: Inputs::default(),
            rig: None,
            snap_every: SNAP_EVERY,
            max_snaps: MAX_SNAPS,
        }
    }
}

impl Session {
    pub fn tick(&self) -> Option<u64> {
        self.sim.as_ref().map(|s| s.tick())
    }

    /// Ticks where program changes landed (for timeline markers).
    pub fn cmd_ticks(&self) -> Vec<u64> {
        self.cmds.iter().map(|(t, _)| *t).collect()
    }

    /// Begin a fresh timeline around `sim` (drops all history). The host
    /// rig, if any, is recorded as a command at the start tick.
    pub fn start(&mut self, sim: Sim) {
        self.tape.clear();
        self.snaps.clear();
        self.cmds.clear();
        if let Some(r) = &self.rig {
            self.cmds.push((sim.tick(), ProgCmd::Add(r.clone())));
        }
        self.snaps.push((sim.tick(), sim.clone()));
        self.sim = Some(sim);
    }

    /// Stop and drop history.
    pub fn stop(&mut self) {
        self.sim = None;
        self.tape.clear();
        self.snaps.clear();
        self.cmds.clear();
    }

    /// Advance one tick: apply program commands recorded at this tick, step
    /// with taped inputs (recording live inputs past the tape end), snapshot
    /// periodically.
    pub fn advance(&mut self, card_src: &str) -> Result<(), String> {
        let Some(sim) = &mut self.sim else { return Ok(()) };
        let t = sim.tick();
        for (ct, cmd) in &self.cmds {
            if *ct == t {
                match cmd {
                    ProgCmd::Add(src) => sim.add_forms(card_src, src)?,
                    ProgCmd::Swap(src) => sim.swap_forms(card_src, src)?,
                    ProgCmd::ResizeEntities(max) => sim.resize_entity_capacity(*max)?,
                }
            }
        }
        let ti = t as usize;
        let inputs = if ti < self.tape.len() {
            self.tape[ti].clone()
        } else {
            self.last_inputs.clone()
        };
        if ti >= self.tape.len() {
            self.tape.push(inputs.clone());
        }
        sim.step_with(&inputs)?;
        let now = sim.tick();
        if self.snap_every > 0
            && now % self.snap_every == 0
            && self.snaps.last().map(|(t, _)| *t) != Some(now)
        {
            self.snaps.push((now, sim.clone()));
            if self.snaps.len() > self.max_snaps.max(4) {
                // thin the older half (keep the baseline at index 0)
                let half = self.snaps.len() / 2;
                let mut idx = 0usize;
                self.snaps.retain(|_| {
                    let k = idx;
                    idx += 1;
                    k == 0 || k >= half || k % 2 == 0
                });
            }
        }
        Ok(())
    }

    /// Scrub to an absolute tick. Backward = restore the nearest snapshot ≤
    /// target and re-step both tapes; forward extends the input tape.
    pub fn seek(&mut self, card_src: &str, target: u64) -> Result<(), String> {
        if self.sim.is_none() {
            return Err("nothing running".into());
        }
        let cur = self.tick().unwrap();
        if target < cur {
            let base = self
                .snaps
                .iter()
                .rev()
                .find(|(t, _)| *t <= target)
                .map(|(_, s)| s.clone())
                .ok_or("no snapshot history")?;
            self.sim = Some(base);
            // the event log is shared; drop events this timeline hasn't
            // emitted yet (re-stepping re-emits them deterministically)
            self.sim.as_mut().unwrap().rewind_events();
        }
        while self.tick().unwrap() < target {
            self.advance(card_src)?;
        }
        Ok(())
    }

    /// Branch the timeline at the current tick: drop future inputs,
    /// snapshots, and program commands (commands AT the current tick
    /// survive — they haven't applied yet in this pass).
    pub fn truncate_future(&mut self) {
        if let Some(t) = self.tick() {
            self.tape.truncate(t as usize);
            self.snaps.retain(|(st, _)| *st <= t);
            self.cmds.retain(|(ct, _)| *ct <= t);
        }
    }

    /// Record a layer at the current tick; it applies on the next advance
    /// (live or replayed). Parse errors surface immediately.
    pub fn record_add(&mut self, src: String) -> Result<(), String> {
        read_all(&src).map_err(|e| e.to_string())?;
        let t = self.tick().ok_or("nothing running")?;
        self.cmds.push((t, ProgCmd::Add(src)));
        Ok(())
    }

    /// Record a generational hot-swap at the current tick.
    pub fn record_swap(&mut self, src: String) -> Result<(), String> {
        read_all(&src).map_err(|e| e.to_string())?;
        let t = self.tick().ok_or("nothing running")?;
        self.cmds.push((t, ProgCmd::Swap(src)));
        Ok(())
    }

    /// Record an explicit host-side entity capacity change at the current tick.
    pub fn record_resize_entities(&mut self, max_entities: usize) -> Result<(), String> {
        if let Some(sim) = &self.sim {
            let mut probe = sim.clone();
            probe.resize_entity_capacity(max_entities)?;
        } else {
            return Err("nothing running".into());
        }
        let t = self.tick().ok_or("nothing running")?;
        self.cmds.push((t, ProgCmd::ResizeEntities(max_entities)));
        Ok(())
    }

    /// Replace the program and replay the input tape through the NEW code up
    /// to the current tick — the pause/rewind/edit/re-run loop. The command
    /// tape restarts (old program mutations don't apply to the new program);
    /// the input tape is kept. Returns the tick replayed to.
    pub fn rerun(&mut self, card_src: &str, form_src: &str) -> Result<u64, String> {
        let cur = self.tick().unwrap_or(0);
        let sim = Sim::load_forms(card_src, form_src)?;
        self.snaps.clear();
        self.cmds.clear();
        if let Some(r) = &self.rig {
            self.cmds.push((0, ProgCmd::Add(r.clone())));
        }
        self.snaps.push((0, sim.clone()));
        self.sim = Some(sim);
        let replay_to = cur.min(self.tape.len() as u64);
        while self.tick().unwrap() < replay_to {
            self.advance(card_src)?;
        }
        Ok(replay_to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = r#"
(defpattern a [] (dotimes [i inf :every (ticks 60)]
  (spawn (circle 2 (linear c[1 0])) {:style {:family :x}})))
(defpattern b [] (seq (wait (ticks 30))
  (spawn (circle 3 (linear c[1 0])) {:style {:family :y}})))
"#;

    fn count(s: &Session, fam: &str) -> usize {
        let world = &s.sim.as_ref().unwrap().world;
        world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                world.entities.is_alive(*i)
                    && world
                        .entities
                        .render_projector(*i)
                        .is_some_and(|projector| projector.style.family == fam)
            })
            .count()
    }

    /// The command tape makes program changes scrub-safe: rewind across an
    /// (add ...) boundary and re-step — the layer reappears at its tick.
    #[test]
    fn scrub_across_add_boundary() {
        let mut sess = Session::default();
        sess.start(Sim::load(CARD, Some("a")).unwrap());
        for _ in 0..100 {
            sess.advance(CARD).unwrap();
        }
        sess.record_add("(b)".into()).unwrap(); // lands at tick 100
        for _ in 0..100 {
            sess.advance(CARD).unwrap();
        }
        // live timeline at 200: b fired at 130
        assert_eq!(count(&sess, "y"), 3);
        let x_before = count(&sess, "x");
        let y_births: Vec<u64> = sess
            .sim
            .as_ref()
            .unwrap()
            .world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                sess.sim
                    .as_ref()
                    .unwrap()
                    .world
                    .entities
                    .render_projector(*i)
                    .is_some_and(|projector| projector.style.family == "y")
            })
            .filter_map(|(i, _)| sess.sim.as_ref().unwrap().world.entities.birth(i))
            .collect();

        // rewind to before the add: the layer is gone
        sess.seek(CARD, 50).unwrap();
        assert_eq!(count(&sess, "y"), 0);
        assert_eq!(count(&sess, "x"), 2);

        // forward across the boundary: the add re-applies at tick 100
        sess.seek(CARD, 200).unwrap();
        assert_eq!(count(&sess, "y"), 3, "layer survives the round trip");
        assert_eq!(count(&sess, "x"), x_before);
        let y_births_after: Vec<u64> = sess
            .sim
            .as_ref()
            .unwrap()
            .world
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                sess.sim
                    .as_ref()
                    .unwrap()
                    .world
                    .entities
                    .render_projector(*i)
                    .is_some_and(|projector| projector.style.family == "y")
            })
            .filter_map(|(i, _)| sess.sim.as_ref().unwrap().world.entities.birth(i))
            .collect();
        assert_eq!(y_births, y_births_after, "identical birth ticks (130)");
        assert_eq!(y_births_after[0], 130);
    }

    /// Swaps replay too: rewind past a swap, step forward, and the program
    /// change happens again at its recorded tick.
    #[test]
    fn scrub_across_swap_boundary() {
        let mut sess = Session::default();
        sess.start(Sim::load(CARD, Some("a")).unwrap());
        for _ in 0..70 {
            sess.advance(CARD).unwrap();
        }
        // swap kills a's control tree at tick 70; old entities keep flying
        sess.record_swap("(seq (wait (ticks 10)) (spawn (circle 5 (linear c[2 0])) {:style {:family :z}}))".into())
            .unwrap();
        for _ in 0..60 {
            sess.advance(CARD).unwrap();
        }
        // at 130: a's volleys at 0 and 60 (4 x), NO volley at 120 (swapped
        // out), z fired at 80
        assert_eq!((count(&sess, "x"), count(&sess, "z")), (4, 5));

        sess.seek(CARD, 30).unwrap();
        assert_eq!((count(&sess, "x"), count(&sess, "z")), (2, 0));
        sess.seek(CARD, 130).unwrap();
        assert_eq!((count(&sess, "x"), count(&sess, "z")), (4, 5), "swap replayed at tick 70");
    }

    /// The event log is shared: snapshots hold a cursor, not a copy; a
    /// restore truncates the shared tail and re-stepping re-emits.
    #[test]
    fn event_log_shared_and_cursored() {
        const ECARD: &str = r#"
(defpattern e [] (dotimes [i inf :every (ticks 10)] (event :ping)))
"#;
        let mut sess = Session::default();
        sess.snap_every = 50;
        sess.start(Sim::load(ECARD, Some("e")).unwrap());
        for _ in 0..200 {
            sess.advance(ECARD).unwrap();
        }
        // every snapshot shares ONE log allocation with the live sim
        let live = sess.sim.as_ref().unwrap().world.log.clone();
        for (_, snap) in &sess.snaps {
            assert!(std::rc::Rc::ptr_eq(&live, &snap.world.log));
        }
        let n_at_200 = sess.sim.as_ref().unwrap().events_vec().len();
        assert_eq!(n_at_200, 20);
        // rewind: the shared tail is truncated to the restored cursor...
        sess.seek(ECARD, 55).unwrap();
        assert_eq!(sess.sim.as_ref().unwrap().events_vec().len(), 6);
        // ...and scrubbing forward re-emits the identical suffix
        sess.seek(ECARD, 200).unwrap();
        assert_eq!(sess.sim.as_ref().unwrap().events_vec().len(), n_at_200);
    }

    /// Thinning keeps the snapshot set bounded with the baseline intact,
    /// and scrubbing still works against the sparser old history.
    #[test]
    fn snapshot_thinning() {
        let mut sess = Session::default();
        sess.snap_every = 1;
        sess.max_snaps = 16;
        sess.start(Sim::load(CARD, Some("a")).unwrap());
        for _ in 0..300 {
            sess.advance(CARD).unwrap();
        }
        assert!(sess.snaps.len() <= 17, "bounded: {}", sess.snaps.len());
        assert_eq!(sess.snaps[0].0, 0, "baseline survives thinning");
        sess.seek(CARD, 61).unwrap(); // volley at 60 must replay exactly
        assert_eq!(count(&sess, "x"), 4);
        sess.seek(CARD, 300).unwrap();
        assert_eq!(count(&sess, "x"), 10);
    }

    /// snap_every = 0: no snapshots beyond the baseline; scrub-back
    /// replays from tick 0.
    #[test]
    fn snapshots_disabled() {
        let mut sess = Session::default();
        sess.snap_every = 0;
        sess.start(Sim::load(CARD, Some("a")).unwrap());
        for _ in 0..250 {
            sess.advance(CARD).unwrap();
        }
        assert_eq!(sess.snaps.len(), 1, "baseline only");
        sess.seek(CARD, 61).unwrap();
        assert_eq!(count(&sess, "x"), 4, "seek replays from the baseline");
    }

    /// Resuming after a rewind branches: future commands past the branch
    /// point are dropped along with the input tape.
    #[test]
    fn branch_drops_future_commands() {
        let mut sess = Session::default();
        sess.start(Sim::load(CARD, Some("a")).unwrap());
        for _ in 0..100 {
            sess.advance(CARD).unwrap();
        }
        sess.record_add("(b)".into()).unwrap();
        for _ in 0..50 {
            sess.advance(CARD).unwrap();
        }
        sess.seek(CARD, 50).unwrap();
        sess.truncate_future(); // resume here: the add at 100 is dropped
        for _ in 0..150 {
            sess.advance(CARD).unwrap();
        }
        assert_eq!(count(&sess, "y"), 0, "dropped layer stays dropped");
    }

    #[test]
    fn resize_entities_replays() {
        const TWO: &str = r#"
(defpattern p [] (spawn (circle 2 (still)) {:style {:family :x}}))
"#;
        let mut sim = Sim::load(TWO, Some("p")).unwrap();
        sim.resize_entity_capacity(1).unwrap();
        let mut sess = Session::default();
        sess.start(sim);
        sess.record_resize_entities(2).unwrap();
        sess.advance(TWO).unwrap();
        assert_eq!(count(&sess, "x"), 2);
        sess.seek(TWO, 0).unwrap();
        assert_eq!(count(&sess, "x"), 0);
        sess.advance(TWO).unwrap();
        assert_eq!(count(&sess, "x"), 2, "resize command replayed at tick 0");
    }
}
