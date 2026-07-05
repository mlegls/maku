//! World data: bullets, colliders, triggers, events, counters.

use super::*;
use crate::edn::Form;
use std::rc::Rc;

// World: bullets + events. The control layer's mutable half.

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub family: String,
    pub color: String,
    pub variant: String,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Point,
    Laser {
        shape: Option<Rc<DynNode>>,
        warn: f64,
        active: f64,
        u_max: f64,
        u_max_sig: Option<(Form, Env)>,
        resolution: f64,
    },
}

/// A signal-valued meta tag sampled at render time (e.g. :hue).
#[derive(Debug, Clone)]
pub struct MetaSig {
    pub form: Form,
    pub env: Env,
    pub idx: usize, // element index for array-valued tag signals
}

#[derive(Clone)]
pub struct Bullet {
    pub id: u64,
    /// Gameplay team tag (F20: derived channels like $nearest-enemy are
    /// queries over tagged entities). None = hostile fire; :player = player
    /// fire (hits :enemy entities); :enemy = a target with hp.
    pub team: Option<Rc<str>>,
    pub kind: Kind,
    pub motion: Rc<DynNode>,
    pub birth: u64,
    pub style: Style,
    pub alive: bool,
    pub state: MotionState,
    pub scanned: bool,
    pub hue: Option<MetaSig>,
    /// Collider set — archetype data, Rc-shared across a spawn's elements.
    pub colliders: Rc<[Collider]>,
    /// User-defined numeric columns (§9's sidecar, inline for the
    /// prototype). hp is not special — it's just the first custom column.
    pub cols: Vec<(Rc<str>, f64)>,
    /// Standing edge-triggers over own columns — archetype data. Death is
    /// not special: :hp n synthesizes (col hp ≤ 0 → cull + event :died).
    pub triggers: Rc<[TriggerRule]>,
    /// Damage on contact (:damage meta): a number, a DMK player() map whose
    /// :hit is taken, or a PURE FUNCTION (fn [self other] num) evaluated at
    /// contact — contacts are rare, so interpreting there is free.
    pub damage: Val,
    /// Grazes count once per bullet.
    pub grazed: bool,
    /// Last tick's position (collision pass) — contact velocity is the
    /// finite difference, uniform across Closed and Scanned motion.
    pub prev_pos: Option<(f64, f64)>,
}

impl Bullet {
    pub fn col_get(&self, name: &str) -> Option<f64> {
        self.cols.iter().find(|(k, _)| &**k == name).map(|(_, v)| *v)
    }

    pub fn col_set(&mut self, name: &Rc<str>, v: f64) {
        match self.cols.iter_mut().find(|(k, _)| k == name) {
            Some((_, slot)) => *slot = v,
            None => self.cols.push((name.clone(), v)),
        }
    }
}

/// A standing rule over an entity's own columns: when `col ≤ leq` first
/// becomes true (edge-triggered; the latch is itself a column, so it
/// snapshots and scrubs), emit the event and optionally cull. The same
/// mechanism covers death, HP-gated boss phases, enrage thresholds, lives.
#[derive(Clone, Debug)]
pub struct TriggerRule {
    /// Event name; also keys the latch column.
    pub name: Rc<str>,
    /// Precomputed latch column key.
    pub latch: Rc<str>,
    pub col: Rc<str>,
    pub leq: f64,
    pub cull: bool,
}

impl TriggerRule {
    pub fn new(name: &str, col: &str, leq: f64, cull: bool) -> TriggerRule {
        TriggerRule {
            name: name.into(),
            latch: format!("{}#fired", name).into(),
            col: col.into(),
            leq,
            cull,
        }
    }
}

/// Collision layers. The engine's interaction matrix pairs them:
/// damage × player-hurtbox → hit, graze × player-hurtbox → graze,
/// shot × hurt → damage resolution. The player hurtbox is implicit
/// (an engine-side entity at $player).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    Damage,     // hostile fire
    Graze,      // graze ring, conventionally on the bullet
    Shot,       // player fire
    Hurt,       // enemy hurtbox
    PlayerHurt, // the player entity's hurtbox (host-mounted)
}

/// One collider: a circle in the owner's frame. Lasers derive capsule
/// chains from their sampled curve at collision time instead.
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub layer: Layer,
    pub r: f64,
}

/// Default collider sets by team — archetype data (built once per spawn).
pub fn default_colliders(team: Option<&str>, family: &str, hitbox: Option<f64>) -> Vec<Collider> {
    let style_r = match family {
        "lstar" | "gglcircle" => 0.20,
        "gem" | "star" => 0.09,
        _ => 0.12,
    };
    match team {
        None => vec![
            Collider { layer: Layer::Damage, r: hitbox.unwrap_or(style_r) },
            Collider { layer: Layer::Graze, r: 0.35 },
        ],
        Some("player") => vec![Collider { layer: Layer::Shot, r: hitbox.unwrap_or(style_r) }],
        Some("enemy") => vec![Collider { layer: Layer::Hurt, r: hitbox.unwrap_or(0.3) }],
        Some(_) => vec![],
    }
}

#[derive(Clone)]
pub struct World {
    pub tick: u64,
    pub next_id: u64,
    pub bullets: Vec<Bullet>,
    /// The event log is SHARED across snapshots (Rc): the log is monotonic,
    /// so a snapshot needs only `cursor` — restore truncates the shared
    /// tail and re-stepping re-emits deterministically. Snapshots carry
    /// zero event data.
    pub log: Rc<std::cell::RefCell<EventLog>>,
    /// Global index one past the last event THIS timeline emitted.
    pub cursor: u64,
    pub rng: u64,
    pub boss: Pose,
    pub boss_anim: Option<BossAnim>,
    /// Column-expose rules from spawn meta :expose {:col :channel}:
    /// channel := that entity's column while alive, else 0. Registered at
    /// spawn, persists past the entity (death reads as 0, so hp gates fire).
    pub exposes: Vec<(Rc<str>, u64, Rc<str>)>,
    /// Gameplay counters — part of World so they snapshot/scrub with it.
    pub graze: u64,
    pub player_hits: u64,
}

/// A gameplay event: emitted by collision or by the (event :name) action.
/// Names are interned (Rc<str>) — a card emits a handful of distinct names.
#[derive(Clone, Debug)]
pub struct Event {
    pub tick: u64,
    pub name: Rc<str>,
    pub pos: Option<(f64, f64)>,
}

/// Append-only event log with a global index origin: entries[i] has global
/// index base + i. The front may be pruned (display history only — restores
/// truncate the TAIL, never read the pruned front).
#[derive(Default)]
pub struct EventLog {
    pub base: u64,
    pub entries: std::collections::VecDeque<Event>,
}

impl EventLog {
    fn tip(&self) -> u64 {
        self.base + self.entries.len() as u64
    }

    /// Drop everything at or after the cursor (a timeline restore).
    pub fn truncate_to(&mut self, cursor: u64) {
        while self.tip() > cursor {
            self.entries.pop_back();
        }
    }

    /// Bound the retained window (front prune; amortized by the caller).
    pub fn prune(&mut self, keep_from_tick: u64) {
        while self
            .entries
            .front()
            .map(|e| e.tick < keep_from_tick)
            .unwrap_or(false)
        {
            self.entries.pop_front();
            self.base += 1;
        }
    }
}

impl World {
    /// Emit an event. Invariant: only the sim at the shared log's tip may
    /// append; a clone stepped in parallel (diverged timeline) detects the
    /// mismatch and copy-on-writes its own fresh log.
    pub fn push_event(&mut self, ev: Event) {
        if self.log.borrow().tip() != self.cursor {
            self.log = Rc::new(std::cell::RefCell::new(EventLog {
                base: self.cursor,
                entries: std::collections::VecDeque::new(),
            }));
        }
        self.log.borrow_mut().entries.push_back(ev);
        self.cursor += 1;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BossAnim {
    pub from: Pose,
    pub to: (f64, f64),
    pub start: u64,
    pub dur: u64,
}

impl Default for World {
    fn default() -> Self {
        World {
            tick: 0,
            next_id: 0,
            bullets: Vec::new(),
            log: Rc::new(std::cell::RefCell::new(EventLog::default())),
            cursor: 0,
            rng: 0x9e37_79b9_7f4a_7c15,
            boss: Pose::IDENTITY,
            boss_anim: None,
            exposes: Vec::new(),
            graze: 0,
            player_hits: 0,
        }
    }
}

impl World {
    /// Deterministic splitmix64-ish stream (counter-based enough for the
    /// prototype: same run order → same stream → replays agree).
    pub fn next_rand(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn find(&self, id: u64) -> Option<usize> {
        self.bullets.iter().position(|b| b.id == id && b.alive)
    }
}
