//! World data: entities, colliders, triggers, events, contact rules.

use super::*;
use crate::edn::Form;
use std::collections::HashMap;
use std::rc::Rc;

// World: entities + events. The control layer's mutable half.

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub family: String,
    pub color: String,
    pub variant: String,
}

#[derive(Debug, Clone)]
pub struct SlotActivity {
    pub warn: f64,
    pub active: f64,
    /// Signal-valued active-domain fraction, clamped to 0..1.
    pub hot_frac_sig: Option<DynNum>,
}

#[derive(Debug, Clone)]
pub struct CapsuleChainSlot {
    /// Sampling used by this collision projection.
    /// Abstract parametric figures do not own sampling.
    pub sample_set: SampleSet,
    /// Signal-valued override for the upper range bound (:u-max varLength).
    pub u_max_sig: Option<DynNum>,
    /// Width multiplier for the capsule-chain half-width.
    pub width: f64,
    pub activity: SlotActivity,
}

#[derive(Debug, Clone)]
pub struct CurveRenderSlot {
    /// Sampling used by this render projection.
    /// Abstract parametric figures do not own sampling.
    pub sample_set: SampleSet,
    /// Signal-valued override for the upper range bound (:u-max varLength).
    pub u_max_sig: Option<DynNum>,
    /// Width multiplier for the current rendered stroke. The host still
    /// controls final appearance.
    pub width: f64,
    pub activity: SlotActivity,
}

#[derive(Debug, Clone)]
pub struct TracePolicy {
    /// Optional retained history in seconds. None means tracing disabled.
    /// Shortening this only drops older samples, making the trace
    /// indistinguishable from one produced by a younger entity.
    pub window: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct EntityCachePolicy {
    pub trace: Option<TracePolicy>,
}

#[derive(Clone, Debug)]
pub enum ColliderData {
    None,
    Circle { layer: Rc<str>, center: (f64, f64), radius: f64 },
    CapsuleChain { layer: Rc<str>, points: Vec<(f64, f64)>, radius: f64 },
}

impl ColliderData {
    pub fn layer(&self) -> Option<&str> {
        match self {
            ColliderData::None => None,
            ColliderData::Circle { layer, .. } | ColliderData::CapsuleChain { layer, .. } => {
                Some(layer.as_ref())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum RenderData {
    None,
    Polyline { points: Vec<(f64, f64)>, active: bool },
}

#[derive(Clone, Debug)]
pub enum ColliderDynRepr {
    Slot(ColliderSlot),
}

#[derive(Clone, Debug)]
pub enum RenderDynRepr {
    Polyline(CurveRenderSlot),
}

impl DynKind for ColliderData {
    type Repr = ColliderDynRepr;
}

impl DynKind for RenderData {
    type Repr = RenderDynRepr;
}

pub type DynCollider = Dyn<ColliderData>;
pub type DynRender = Dyn<RenderData>;

/// Semantic `Dyn<List<Collider>>` boundary. The current interpreter lowers
/// only stable-arity lists; later variants can carry dynamic whole-list
/// expressions without changing Entity's shape again.
#[derive(Clone, Debug)]
pub enum DynColliderList {
    Stable(Rc<[DynCollider]>),
}

impl DynColliderList {
    pub fn stable(slots: Rc<[DynCollider]>) -> DynColliderList {
        DynColliderList::Stable(slots)
    }

    pub fn empty() -> DynColliderList {
        DynColliderList::Stable(Vec::new().into())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, DynCollider> {
        match self {
            DynColliderList::Stable(slots) => slots.iter(),
        }
    }

    pub fn first(&self) -> Option<&DynCollider> {
        match self {
            DynColliderList::Stable(slots) => slots.first(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            DynColliderList::Stable(slots) => slots.len(),
        }
    }

    pub fn to_vec(&self) -> Vec<DynCollider> {
        self.iter().cloned().collect()
    }
}

/// Semantic `Dyn<List<Render>>` boundary, currently lowered to stable slots.
#[derive(Clone, Debug)]
pub enum DynRenderList {
    Stable(Rc<[DynRender]>),
}

impl DynRenderList {
    pub fn stable(slots: Rc<[DynRender]>) -> DynRenderList {
        DynRenderList::Stable(slots)
    }

    pub fn empty() -> DynRenderList {
        DynRenderList::Stable(Vec::new().into())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, DynRender> {
        match self {
            DynRenderList::Stable(slots) => slots.iter(),
        }
    }

    pub fn first(&self) -> Option<&DynRender> {
        match self {
            DynRenderList::Stable(slots) => slots.first(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            DynRenderList::Stable(slots) => slots.len(),
        }
    }
}

/// A signal-valued meta tag sampled at render time (e.g. :hue).
#[derive(Debug, Clone)]
pub struct MetaSig {
    pub form: Form,
    pub env: Env,
    pub idx: usize, // element index for array-valued tag signals
}

/// The render-affecting signal tags (§7): each is an optional signal over
/// entity-local t, sampled at render time (scale also at collision time —
/// a scaled sprite scales its colliders). DMK's simple-bullet modifiers
/// (scale/dir/opacity), dissolved into meta tags like :hue.
#[derive(Debug, Clone, Default)]
pub struct RenderSigs {
    pub hue: Option<MetaSig>,
    /// Sprite + collider size multiplier (default 1).
    pub scale: Option<MetaSig>,
    /// Sprite rotation in degrees, overriding the motion direction.
    pub facing: Option<MetaSig>,
    /// Alpha multiplier (default 1).
    pub opacity: Option<MetaSig>,
}

#[derive(Clone)]
pub struct Entity {
    pub id: u64,
    /// Gameplay team tag (F20: derived channels like $nearest-enemy are
    /// queries over tagged entities). Collision ignores this; layer tags and
    /// contact rules define interactions.
    pub team: Option<Rc<str>>,
    pub dyn_figure: DynFigure,
    pub cache_policy: EntityCachePolicy,
    pub birth: u64,
    pub style: Style,
    pub alive: bool,
    pub state: MotionState,
    pub scanned: bool,
    pub sigs: RenderSigs,
    /// Collider slots — archetype data, Rc-shared across a spawn's
    /// elements. Layers are opaque core routing keys; slots evaluate each
    /// tick into collision data or nothing.
    pub colliders: DynColliderList,
    /// Render slots — archetype data, Rc-shared across a spawn's elements.
    pub renderers: DynRenderList,
    /// User-defined numeric columns in World's dense column layout. hp is
    /// not special — it is just another named source column assigned to a
    /// slot by the world.
    pub cols: Vec<Option<f64>>,
    /// Standing edge-triggers over own columns — archetype data. Death is
    /// not special: :hp n synthesizes (col hp ≤ 0 → cull + event :died).
    pub triggers: Rc<[TriggerRule]>,
    /// Damage on contact (:damage meta): a number, a DMK player() map whose
    /// :hit is taken, or a PURE FUNCTION (fn [self other] num) evaluated at
    /// contact — contacts are rare, so interpreting there is free.
    pub damage: Val,
    /// Last tick's position (collision pass) — contact velocity is the
    /// finite difference, uniform across Closed and Scanned motion.
    pub prev_pos: Option<(f64, f64)>,
    /// Traced curve samples, capped at the remembrance window. Only valid
    /// pose samples are stored; before the trace fills, the domain is
    /// shorter and indexed from entity-local sample 0. Facing is part of the
    /// sample data; finite-difference facing is only a possible
    /// helper/default, not the core representation.
    /// Interpolation over these samples should be an explicit higher-level
    /// curve function, not implicit core behavior.
    pub trail: Vec<Pose>,
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

/// A collider slot: universal collision-routing metadata plus a
/// shape-specific interpretation of the entity's current figure.
#[derive(Clone, Debug)]
pub struct ColliderSlot {
    pub layer: Rc<str>,
    pub shape: ColliderSlotShape,
}

#[derive(Clone, Debug)]
pub enum ColliderSlotShape {
    Circle { radius: DynNum },
    CapsuleChain { radius: DynNum, slot: CapsuleChainSlot },
}

impl Dyn<ColliderData> {
    pub fn collider(slot: ColliderSlot) -> DynCollider {
        Dyn { repr: ColliderDynRepr::Slot(slot) }
    }

    pub fn collider_circle(layer: Rc<str>, radius: DynNum) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::Circle { radius },
        })
    }

    pub fn collider_circle_const(layer: Rc<str>, radius: f64) -> DynCollider {
        DynCollider::collider_circle(layer, DynNum::num(radius))
    }

    pub fn collider_capsule_chain(
        layer: Rc<str>,
        radius: DynNum,
        slot: CapsuleChainSlot,
    ) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::CapsuleChain { radius, slot },
        })
    }

    pub fn collider_capsule_chain_const(
        layer: Rc<str>,
        radius: f64,
        slot: CapsuleChainSlot,
    ) -> DynCollider {
        DynCollider::collider_capsule_chain(layer, DynNum::num(radius), slot)
    }

    pub fn repr(&self) -> &ColliderDynRepr {
        &self.repr
    }

    pub fn slot(&self) -> &ColliderSlot {
        match &self.repr {
            ColliderDynRepr::Slot(slot) => slot,
        }
    }

    pub fn capsule_chain(&self) -> Option<(&ColliderSlot, &CapsuleChainSlot, &DynNum)> {
        let slot = self.slot();
        match &slot.shape {
            ColliderSlotShape::CapsuleChain { radius, slot: shape } => Some((slot, shape, radius)),
            ColliderSlotShape::Circle { .. } => None,
        }
    }
}

impl Dyn<RenderData> {
    pub fn render_polyline(slot: CurveRenderSlot) -> DynRender {
        Dyn { repr: RenderDynRepr::Polyline(slot) }
    }

    pub fn repr(&self) -> &RenderDynRepr {
        &self.repr
    }

    pub fn polyline(&self) -> &CurveRenderSlot {
        match &self.repr {
            RenderDynRepr::Polyline(r) => r,
        }
    }
}

/// A collision rule: when an entity with an `a`-layer collider overlaps an
/// entity with a `b`-layer collider, run the callback with both handles.
/// Prefilters are rule DATA, evaluated engine-side per pair (no interpreter
/// on the hot path): `once` latches a column on the A entity (fires once per
/// A-entity ever), `skip_if` compares a column against a threshold.
#[derive(Clone)]
pub struct ContactRule {
    pub a: Rc<str>,
    pub b: Rc<str>,
    /// Column name latched to 1.0 on the A entity after the callback fires.
    pub once: Option<Rc<str>>,
    /// (side, col, op, rhs): skip the pair when `side.col op rhs` holds.
    pub skip_if: Option<SkipIf>,
    pub callback: Val,
}

#[derive(Clone)]
pub struct SkipIf {
    pub on_b: bool,          // :a or :b
    pub col: Rc<str>,
    pub gt: bool,            // :gt or :lt (missing col reads 0.0)
    pub rhs: SkipRhs,
}

#[derive(Clone)]
pub enum SkipRhs { Tick, Num(f64) }

#[derive(Clone)]
pub struct World {
    pub tick: u64,
    pub next_id: u64,
    pub entities: Vec<Entity>,
    /// The event log is SHARED across snapshots (Rc): the log is monotonic,
    /// so a snapshot needs only `cursor` — restore truncates the shared
    /// tail and re-stepping re-emits deterministically. Snapshots carry
    /// zero event data.
    pub log: Rc<std::cell::RefCell<EventLog>>,
    /// Global index one past the last event THIS timeline emitted.
    pub cursor: u64,
    pub rng: u64,
    /// Source column name → dense numeric slot. Entities store only slot
    /// values; names live once in the world layout.
    pub col_slots: HashMap<Rc<str>, usize>,
    pub col_names: Vec<Rc<str>>,
    /// Column-expose rules from spawn meta :expose {$channel :col}:
    /// channel := that entity's column while alive, else 0. Registered at
    /// spawn, persists past the entity (death reads as 0, so hp gates fire).
    pub exposes: Vec<(Rc<str>, u64, Rc<str>)>,
    /// Card-defined contact rules, registered by defcontact. World data so
    /// hot-swaps and timeline restore carry the same collision semantics.
    pub contacts: Vec<ContactRule>,
}

/// A gameplay event: emitted by collision or by the `(event :name)` action.
/// `name` is a keyword symbol. `Rc<str>` is the bridge representation until
/// keywords/events/layers/styles share one small-int symbol table; hosts
/// convert the symbol back to their string/name boundary representation.
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

impl Default for World {
    fn default() -> Self {
        World {
            tick: 0,
            next_id: 0,
            entities: Vec::new(),
            log: Rc::new(std::cell::RefCell::new(EventLog::default())),
            cursor: 0,
            rng: 0x9e37_79b9_7f4a_7c15,
            col_slots: HashMap::new(),
            col_names: Vec::new(),
            exposes: Vec::new(),
            contacts: Vec::new(),
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
        self.entities.iter().position(|b| b.id == id && b.alive)
    }

    pub fn col_slot(&self, name: &str) -> Option<usize> {
        self.col_slots.get(name).copied()
    }

    pub fn intern_col(&mut self, name: &Rc<str>) -> usize {
        if let Some(slot) = self.col_slots.get(name).copied() {
            return slot;
        }
        let slot = self.col_names.len();
        self.col_names.push(name.clone());
        self.col_slots.insert(name.clone(), slot);
        slot
    }

    pub fn col_get_at(&self, bullet_idx: usize, name: &str) -> Option<f64> {
        let slot = self.col_slot(name)?;
        self.entities.get(bullet_idx)?.cols.get(slot).copied().flatten()
    }

    pub fn col_set_at(&mut self, bullet_idx: usize, name: &Rc<str>, v: f64) {
        let slot = self.intern_col(name);
        let Some(b) = self.entities.get_mut(bullet_idx) else { return };
        if b.cols.len() <= slot {
            b.cols.resize(slot + 1, None);
        }
        b.cols[slot] = Some(v);
    }

    pub fn cols_for_view(&self, b: &Entity) -> Vec<(Rc<str>, f64)> {
        b.cols
            .iter()
            .enumerate()
            .filter_map(|(slot, v)| {
                let v = (*v)?;
                let name = self.col_names.get(slot)?;
                Some((name.clone(), v))
            })
            .collect()
    }
}
