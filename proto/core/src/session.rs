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

/// One snapshot per second of sim time.
pub const SNAP_EVERY: u64 = 120;

#[derive(Clone)]
enum ProgCmd {
    Add(String),
    Swap(String),
}

#[derive(Default)]
pub struct Session {
    pub sim: Option<Sim>,
    /// tape[t] stepped the sim t → t+1.
    pub tape: Vec<Inputs>,
    /// Periodic snapshots, ascending by tick.
    pub snaps: Vec<(u64, Sim)>,
    cmds: Vec<(u64, ProgCmd)>,
    /// Live inputs used when advancing past the recorded tape.
    pub last_inputs: Inputs,
}

impl Session {
    pub fn tick(&self) -> Option<u64> {
        self.sim.as_ref().map(|s| s.tick())
    }

    /// Ticks where program changes landed (for timeline markers).
    pub fn cmd_ticks(&self) -> Vec<u64> {
        self.cmds.iter().map(|(t, _)| *t).collect()
    }

    /// Begin a fresh timeline around `sim` (drops all history).
    pub fn start(&mut self, sim: Sim) {
        self.tape.clear();
        self.snaps.clear();
        self.cmds.clear();
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
                }
            }
        }
        let ti = t as usize;
        let inputs = if ti < self.tape.len() { self.tape[ti] } else { self.last_inputs };
        if ti >= self.tape.len() {
            self.tape.push(inputs);
        }
        sim.step_with(&inputs)?;
        let now = sim.tick();
        if now % SNAP_EVERY == 0 && self.snaps.last().map(|(t, _)| *t) != Some(now) {
            self.snaps.push((now, sim.clone()));
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

    /// Replace the program and replay the input tape through the NEW code up
    /// to the current tick — the pause/rewind/edit/re-run loop. The command
    /// tape restarts (old program mutations don't apply to the new program);
    /// the input tape is kept. Returns the tick replayed to.
    pub fn rerun(&mut self, card_src: &str, form_src: &str) -> Result<u64, String> {
        let cur = self.tick().unwrap_or(0);
        let sim = Sim::load_forms(card_src, form_src)?;
        self.snaps.clear();
        self.cmds.clear();
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
        s.sim
            .as_ref()
            .unwrap()
            .world
            .bullets
            .iter()
            .filter(|b| b.style.family == fam)
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
            .bullets
            .iter()
            .filter(|b| b.style.family == "y")
            .map(|b| b.birth)
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
            .bullets
            .iter()
            .filter(|b| b.style.family == "y")
            .map(|b| b.birth)
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
        // swap kills a's control tree at tick 70; old bullets keep flying
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
}
