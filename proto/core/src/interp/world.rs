//! World data: entities, colliders, triggers, events, contact rules.

use super::*;
use crate::edn::Form;
use std::collections::HashMap;
use std::rc::Rc;

// World: entities + events. The control layer's mutable half.

pub const DEFAULT_ENTITY_CAPACITY: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

pub type ColName = Symbol;
pub type FieldName = Symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Num,
    Sym,
    Handle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NumFieldId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymFieldId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HandleFieldId(pub u32);

/// Bridge layout for typed entity field matrices. Values still live on
/// entities for now; this owns the load-time schema/slot ids that will back
/// world-owned SoA matrices.
#[derive(Clone, Debug, Default)]
pub struct WorldFields {
    pub num_slots: HashMap<FieldName, usize>,
    pub num_names: Vec<FieldName>,
    pub num_values: Vec<Vec<Option<f64>>>,
    pub sym_slots: HashMap<FieldName, usize>,
    pub sym_names: Vec<FieldName>,
    pub handle_names: Vec<FieldName>,
}

#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    by_name: HashMap<Rc<str>, Symbol>,
    names: Vec<Rc<str>>,
}

impl SymbolTable {
    pub fn intern(&mut self, name: impl AsRef<str>) -> Symbol {
        let name = name.as_ref();
        if let Some(sym) = self.by_name.get(name) {
            return *sym;
        }
        let sym = Symbol(self.names.len() as u32);
        let name: Rc<str> = name.into();
        self.names.push(name.clone());
        self.by_name.insert(name, sym);
        sym
    }

    pub fn lookup(&self, name: &str) -> Option<Symbol> {
        self.by_name.get(name).copied()
    }

    pub fn resolve(&self, sym: Symbol) -> Option<&str> {
        self.names.get(sym.0 as usize).map(|s| s.as_ref())
    }
}

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
    Circle { layer: Symbol, center: (f64, f64), radius: f64 },
    CapsuleChain { layer: Symbol, points: Vec<(f64, f64)>, radius: f64 },
}

impl ColliderData {
    pub fn layer(&self) -> Option<Symbol> {
        match self {
            ColliderData::None => None,
            ColliderData::Circle { layer, .. } | ColliderData::CapsuleChain { layer, .. } => {
                Some(*layer)
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

/// Source-level collider/render metadata carried as generic dyn-like data.
/// Typed projection happens at the collision/render boundary after this data
/// is realized for the current tick.
pub type ColliderSpecList = DynLike;
pub type RenderSpecList = DynLike;
/// Bridge representation of a collider projector: source-level spec lists
/// that lower against the entity's current figure into realized collider rows.
#[derive(Clone, Debug)]
pub struct ColliderProjector {
    pub specs: Rc<[ColliderSpecList]>,
}
/// Bridge representation of a render projector: source-level spec lists
/// that lower against the entity's current figure into realized render rows.
#[derive(Clone, Debug)]
pub struct RenderProjector {
    pub specs: Rc<[RenderSpecList]>,
    /// Compatibility host style. This belongs to the current default renderer,
    /// not to entity semantics.
    pub style: Style,
    /// Compatibility render/collider modifier signals from legacy meta tags.
    pub sigs: RenderSigs,
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
    pub dyn_figure: DynFigure,
    pub cache_policy: EntityCachePolicy,
    pub state: MotionState,
    pub scanned: bool,
    /// Collider projector — archetype data, Rc-shared across a spawn's
    /// elements. Layers are opaque core routing keys; specs evaluate each
    /// tick against the current figure into collision rows or nothing.
    pub collider_projector: ColliderProjector,
    /// Render projector — archetype data, Rc-shared across a spawn's elements.
    pub render_projector: RenderProjector,
    /// Bridge symbol fields in a per-entity dense slot vector. Target
    /// storage is a world-owned symbol field matrix; `:team` lives here for
    /// compatibility.
    pub sym_fields: Vec<Option<Symbol>>,
    /// Standing edge-triggers over own columns — archetype data. Death is
    /// not special: :hp n synthesizes (col hp ≤ 0 → cull + event :died).
    pub triggers: Rc<[TriggerRule]>,
    /// Traced curve samples, capped at the remembrance window. Only valid
    /// pose samples are stored; before the trace fills, the domain is
    /// shorter and indexed from entity-local sample 0. Facing is part of the
    /// sample data; finite-difference facing is only a possible
    /// helper/default, not the core representation.
    /// Interpolation over these samples should be an explicit higher-level
    /// curve function, not implicit core behavior.
    pub trail: Vec<Pose>,
}

pub struct EntityStore {
    rows: Vec<Entity>,
    generation: Vec<u32>,
    alive: Vec<bool>,
    freed_at: Vec<Option<u64>>,
    birth: Vec<u64>,
    sampled_pose: [Vec<Option<Pose>>; 2],
    motion_schema: Vec<Rc<MotionStateSchema>>,
    max: usize,
    free: Vec<usize>,
}

impl EntityStore {
    pub fn with_capacity(max: usize) -> EntityStore {
        EntityStore {
            rows: Vec::with_capacity(max),
            generation: Vec::with_capacity(max),
            alive: Vec::with_capacity(max),
            freed_at: Vec::with_capacity(max),
            birth: Vec::with_capacity(max),
            sampled_pose: [Vec::with_capacity(max), Vec::with_capacity(max)],
            motion_schema: Vec::with_capacity(max),
            max,
            free: Vec::new(),
        }
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn is_alive(&self, row: usize) -> bool {
        self.alive.get(row).copied().unwrap_or(false)
    }

    pub fn generation(&self, row: usize) -> Option<u32> {
        self.generation.get(row).copied()
    }

    pub fn birth(&self, row: usize) -> Option<u64> {
        self.birth.get(row).copied()
    }

    pub fn tau(&self, row: usize, tick: u64) -> f64 {
        let birth = self.birth[row];
        tick.saturating_sub(birth) as f64 / TICK_RATE
    }

    pub fn reset_birth(&mut self, row: usize, tick: u64) {
        if let Some(birth) = self.birth.get_mut(row) {
            *birth = tick;
        }
    }

    pub fn motion_schema(&self, row: usize) -> Option<&MotionStateSchema> {
        self.motion_schema.get(row).map(|schema| schema.as_ref())
    }

    pub fn set_motion_schema(&mut self, row: usize, schema: Rc<MotionStateSchema>) {
        if let Some(slot) = self.motion_schema.get_mut(row) {
            *slot = schema;
        }
    }

    fn pose_slot(tick: u64) -> usize {
        (tick as usize) & 1
    }

    pub fn sampled_pose(&self, row: usize, tick: u64) -> Option<Pose> {
        self.sampled_pose[Self::pose_slot(tick)].get(row).copied().flatten()
    }

    pub fn previous_sampled_pose(&self, row: usize, tick: u64) -> Option<Pose> {
        self.sampled_pose[1 - Self::pose_slot(tick)].get(row).copied().flatten()
    }

    pub fn sampled_pos(&self, row: usize, tick: u64) -> Option<(f64, f64)> {
        self.sampled_pose(row, tick).map(|p| (p.x, p.y))
    }

    pub fn velocity_from_samples(&self, row: usize, tick: u64) -> (f64, f64) {
        match (self.sampled_pose(row, tick), self.previous_sampled_pose(row, tick)) {
            (Some(p), Some(prev)) => ((p.x - prev.x) * TICK_RATE, (p.y - prev.y) * TICK_RATE),
            _ => (0.0, 0.0),
        }
    }

    pub fn set_sampled_pose(&mut self, row: usize, tick: u64, pose: Option<Pose>) {
        let slot = Self::pose_slot(tick);
        if self.sampled_pose[slot].len() <= row {
            self.sampled_pose[slot].resize(row + 1, None);
        }
        self.sampled_pose[slot][row] = pose;
    }

    pub fn clear_sampled_poses(&mut self, row: usize) {
        for poses in &mut self.sampled_pose {
            if let Some(pose) = poses.get_mut(row) {
                *pose = None;
            }
        }
    }

    pub fn entity_ref(&self, row: usize) -> EntityRef {
        EntityRef { row, generation: self.generation[row] }
    }

    pub fn find(&self, handle: EntityRef) -> Option<usize> {
        self.rows
            .get(handle.row)
            .filter(|_| self.is_alive(handle.row) && self.generation[handle.row] == handle.generation)
            .map(|_| handle.row)
    }

    pub fn reusable_free_row(&self, tick: u64) -> Option<(usize, usize)> {
        self.free
            .iter()
            .position(|&i| self.freed_at[i].is_some_and(|t| t < tick))
            .map(|slot| (slot, self.free[slot]))
    }

    pub fn reuse_free_row(
        &mut self,
        slot: usize,
        entity: Entity,
        birth: u64,
        motion_schema: Rc<MotionStateSchema>,
    ) -> usize {
        let i = self.free.swap_remove(slot);
        self.generation[i] = self.generation[i].wrapping_add(1);
        self.alive[i] = true;
        self.freed_at[i] = None;
        self.birth[i] = birth;
        self.motion_schema[i] = motion_schema;
        self.clear_sampled_poses(i);
        self.rows[i] = entity;
        i
    }

    pub fn push_row(
        &mut self,
        entity: Entity,
        birth: u64,
        motion_schema: Rc<MotionStateSchema>,
    ) -> Result<usize, String> {
        if self.rows.len() >= self.max {
            return Err(format!("spawn: entity capacity {} exhausted", self.max));
        }
        let i = self.rows.len();
        self.rows.push(entity);
        self.generation.push(0);
        self.alive.push(true);
        self.freed_at.push(None);
        self.birth.push(birth);
        self.sampled_pose[0].push(None);
        self.sampled_pose[1].push(None);
        self.motion_schema.push(motion_schema);
        Ok(i)
    }

    pub fn cull(&mut self, row: usize, tick: u64) {
        if row < self.rows.len() && self.alive[row] {
            self.alive[row] = false;
            self.freed_at[row] = Some(tick);
            self.free.push(row);
        }
    }
}

impl Clone for EntityStore {
    fn clone(&self) -> EntityStore {
        let mut rows = Vec::with_capacity(self.max);
        rows.extend(self.rows.iter().cloned());
        EntityStore {
            rows,
            generation: self.generation.clone(),
            alive: self.alive.clone(),
            freed_at: self.freed_at.clone(),
            birth: self.birth.clone(),
            sampled_pose: self.sampled_pose.clone(),
            motion_schema: self.motion_schema.clone(),
            max: self.max,
            free: self.free.clone(),
        }
    }
}

impl std::ops::Deref for EntityStore {
    type Target = Vec<Entity>;

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl std::ops::DerefMut for EntityStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rows
    }
}

impl<'a> IntoIterator for &'a EntityStore {
    type Item = &'a Entity;
    type IntoIter = std::slice::Iter<'a, Entity>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

impl<'a> IntoIterator for &'a mut EntityStore {
    type Item = &'a mut Entity;
    type IntoIter = std::slice::IterMut<'a, Entity>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter_mut()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityRef {
    pub row: usize,
    pub generation: u32,
}

/// A standing rule over an entity's own columns: when `col <= leq` first
/// becomes true (edge-triggered; the latch is itself a column, so it
/// snapshots and scrubs), emit the event and optionally cull. The same
/// mechanism covers death, HP-gated boss phases, enrage thresholds, lives.
#[derive(Clone, Debug)]
pub struct TriggerRule {
    /// Event name; also keys the latch column.
    pub name: Symbol,
    /// Precomputed latch column key.
    pub latch: ColName,
    pub col: ColName,
    pub leq: f64,
    pub cull: bool,
}

impl TriggerRule {
    pub fn new(name: Symbol, latch: ColName, col: ColName, leq: f64, cull: bool) -> TriggerRule {
        TriggerRule {
            name,
            latch,
            col,
            leq,
            cull,
        }
    }
}

/// A collider slot: universal collision-routing metadata plus a
/// shape-specific interpretation of the entity's current figure.
#[derive(Clone, Debug)]
pub struct ColliderSlot {
    pub layer: Symbol,
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

    pub fn collider_circle(layer: Symbol, radius: DynNum) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::Circle { radius },
        })
    }

    pub fn collider_circle_const(layer: Symbol, radius: f64) -> DynCollider {
        DynCollider::collider_circle(layer, DynNum::num(radius))
    }

    pub fn collider_capsule_chain(
        layer: Symbol,
        radius: DynNum,
        slot: CapsuleChainSlot,
    ) -> DynCollider {
        DynCollider::collider(ColliderSlot {
            layer,
            shape: ColliderSlotShape::CapsuleChain { radius, slot },
        })
    }

    pub fn collider_capsule_chain_const(
        layer: Symbol,
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

pub(crate) fn curve_capsule_slots(
    slots: impl IntoIterator<Item = DynCollider>,
    curve_slot: &CapsuleChainSlot,
) -> Vec<DynCollider> {
    slots
        .into_iter()
        .map(|collider| {
            let slot = collider.slot();
            match &slot.shape {
                ColliderSlotShape::Circle { radius } => DynCollider::collider_capsule_chain(
                    slot.layer.clone(),
                    radius.clone(),
                    curve_slot.clone(),
                ),
                ColliderSlotShape::CapsuleChain { .. } => collider,
            }
        })
        .collect()
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
    pub a: Symbol,
    pub b: Symbol,
    /// Column name latched to 1.0 on the A entity after the callback fires.
    pub once: Option<ColName>,
    /// (side, col, op, rhs): skip the pair when `side.col op rhs` holds.
    pub skip_if: Option<SkipIf>,
    pub callback: Val,
}

#[derive(Clone)]
pub struct SkipIf {
    pub on_b: bool,          // :a or :b
    pub col: ColName,
    pub gt: bool,            // :gt or :lt (missing col reads 0.0)
    pub rhs: SkipRhs,
}

#[derive(Clone)]
pub enum SkipRhs { Tick, Num(f64) }

pub struct World {
    pub tick: u64,
    pub next_id: u64,
    pub entities: EntityStore,
    /// The event log is SHARED across snapshots (Rc): the log is monotonic,
    /// so a snapshot needs only `cursor` — restore truncates the shared
    /// tail and re-stepping re-emits deterministically. Snapshots carry
    /// zero event data.
    pub log: Rc<std::cell::RefCell<EventLog>>,
    /// Global index one past the last event THIS timeline emitted.
    pub cursor: u64,
    pub rng: u64,
    /// Typed finite user-field schema/layout. Intrinsic entity storage lives
    /// in `entities`; user-addressable numeric values live here.
    pub fields: WorldFields,
    pub symbols: SymbolTable,
    /// Column-expose rules from spawn meta :expose {$channel :col}:
    /// channel := that entity's column while alive, else 0. Registered at
    /// spawn, persists past the entity (death reads as 0, so hp gates fire).
    pub exposes: Vec<(Rc<str>, EntityRef, ColName)>,
    /// Card-defined contact rules, registered by defcontact. World data so
    /// hot-swaps and timeline restore carry the same collision semantics.
    pub contacts: Vec<ContactRule>,
}

impl Clone for World {
    fn clone(&self) -> World {
        World {
            tick: self.tick,
            next_id: self.next_id,
            entities: self.entities.clone(),
            log: self.log.clone(),
            cursor: self.cursor,
            rng: self.rng,
            fields: self.fields.clone(),
            symbols: self.symbols.clone(),
            exposes: self.exposes.clone(),
            contacts: self.contacts.clone(),
        }
    }
}

/// A host-facing gameplay event: emitted by collision or by `(event :name)`.
#[derive(Clone, Debug)]
pub struct Event {
    pub tick: u64,
    pub name: Rc<str>,
    pub pos: Option<(f64, f64)>,
}

/// Internal event log entry. Names are interned symbols; host/test APIs resolve
/// them at the boundary.
#[derive(Clone, Debug)]
pub struct StoredEvent {
    pub tick: u64,
    pub name: Symbol,
    pub pos: Option<(f64, f64)>,
}

/// Append-only event log with a global index origin: entries[i] has global
/// index base + i. The front may be pruned (display history only — restores
/// truncate the TAIL, never read the pruned front).
#[derive(Default)]
pub struct EventLog {
    pub base: u64,
    pub entries: std::collections::VecDeque<StoredEvent>,
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
    pub fn push_event(&mut self, ev: StoredEvent) {
        if self.log.borrow().tip() != self.cursor {
            self.log = Rc::new(std::cell::RefCell::new(EventLog {
                base: self.cursor,
                entries: std::collections::VecDeque::new(),
            }));
        }
        self.log.borrow_mut().entries.push_back(ev);
        self.cursor += 1;
    }

    pub fn resolve_event(&self, ev: &StoredEvent) -> Event {
        Event {
            tick: ev.tick,
            name: self.symbols.resolve(ev.name).unwrap_or("<unknown>").into(),
            pos: ev.pos,
        }
    }
}

impl Default for World {
    fn default() -> Self {
        World::with_entity_capacity(DEFAULT_ENTITY_CAPACITY)
    }
}

impl World {
    pub fn with_entity_capacity(max_entities: usize) -> World {
        World {
            tick: 0,
            next_id: 0,
            entities: EntityStore::with_capacity(max_entities),
            log: Rc::new(std::cell::RefCell::new(EventLog::default())),
            cursor: 0,
            rng: 0x9e37_79b9_7f4a_7c15,
            fields: WorldFields::default(),
            symbols: SymbolTable::default(),
            exposes: Vec::new(),
            contacts: Vec::new(),
        }
    }
}

impl World {
    pub fn resize_entity_capacity(&mut self, max_entities: usize) -> Result<(), String> {
        let live_past_new = self
            .entities
            .iter()
            .enumerate()
            .any(|(i, _)| i >= max_entities && self.entities.is_alive(i));
        if live_past_new {
            return Err(format!(
                "resize-entities: cannot shrink to {}; live rows would be dropped",
                max_entities
            ));
        }
        if max_entities < self.entities.len() {
            self.entities.rows.truncate(max_entities);
            self.entities.generation.truncate(max_entities);
            self.entities.alive.truncate(max_entities);
            self.entities.freed_at.truncate(max_entities);
            self.entities.birth.truncate(max_entities);
            self.entities.sampled_pose[0].truncate(max_entities);
            self.entities.sampled_pose[1].truncate(max_entities);
            self.entities.motion_schema.truncate(max_entities);
            self.entities.free.retain(|i| *i < max_entities);
        }
        for values in &mut self.fields.num_values {
            if max_entities < values.len() {
                values.truncate(max_entities);
            }
            if values.capacity() < max_entities {
                values.reserve_exact(max_entities - values.capacity());
            }
        }
        self.entities.max = max_entities;
        if self.entities.rows.capacity() < max_entities {
            self.entities.rows.reserve_exact(max_entities - self.entities.rows.capacity());
        }
        if self.entities.generation.capacity() < max_entities {
            self.entities.generation.reserve_exact(max_entities - self.entities.generation.capacity());
        }
        if self.entities.alive.capacity() < max_entities {
            self.entities.alive.reserve_exact(max_entities - self.entities.alive.capacity());
        }
        if self.entities.freed_at.capacity() < max_entities {
            self.entities.freed_at.reserve_exact(max_entities - self.entities.freed_at.capacity());
        }
        if self.entities.birth.capacity() < max_entities {
            self.entities.birth.reserve_exact(max_entities - self.entities.birth.capacity());
        }
        for poses in &mut self.entities.sampled_pose {
            if poses.capacity() < max_entities {
                poses.reserve_exact(max_entities - poses.capacity());
            }
        }
        if self.entities.motion_schema.capacity() < max_entities {
            self.entities.motion_schema.reserve_exact(max_entities - self.entities.motion_schema.capacity());
        }
        Ok(())
    }

    pub fn install_entity(&mut self, entity: Entity) -> Result<usize, String> {
        let motion_schema = Rc::new(collect_motion_state_schema(&entity.dyn_figure));
        if let Some((slot, i)) = self.entities.reusable_free_row(self.tick) {
            self.clear_num_fields_at(i);
            Ok(self.entities.reuse_free_row(slot, entity, self.tick, motion_schema))
        } else {
            self.entities.push_row(entity, self.tick, motion_schema)
        }
    }

    pub fn cull_at(&mut self, i: usize) {
        self.entities.cull(i, self.tick);
    }

    pub fn entity_ref(&self, row: usize) -> EntityRef {
        self.entities.entity_ref(row)
    }

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

    pub fn find(&self, handle: EntityRef) -> Option<usize> {
        self.entities.find(handle)
    }

    pub fn col_slot(&self, name: ColName) -> Option<usize> {
        self.fields.num_slots.get(&name).copied()
    }

    pub fn intern_col(&mut self, name: impl AsRef<str>) -> ColName {
        self.symbols.intern(name)
    }

    pub fn intern_col_slot(&mut self, name: ColName) -> usize {
        if let Some(slot) = self.fields.num_slots.get(&name).copied() {
            return slot;
        }
        let slot = self.fields.num_names.len();
        self.fields.num_names.push(name);
        self.fields.num_values.push(Vec::new());
        self.fields.num_slots.insert(name, slot);
        slot
    }

    pub fn col_get_sym_at(&self, bullet_idx: usize, name: ColName) -> Option<f64> {
        let slot = self.col_slot(name)?;
        self.entities.get(bullet_idx)?;
        self.fields.num_values.get(slot)?.get(bullet_idx).copied().flatten()
    }

    pub fn col_get_at(&self, bullet_idx: usize, name: &str) -> Option<f64> {
        let sym = self.symbols.lookup(name)?;
        self.col_get_sym_at(bullet_idx, sym)
    }

    pub fn col_set_sym_at(&mut self, bullet_idx: usize, name: ColName, v: f64) {
        let slot = self.intern_col_slot(name);
        if self.entities.get(bullet_idx).is_none() {
            return;
        }
        let values = &mut self.fields.num_values[slot];
        if values.len() <= bullet_idx {
            values.resize(bullet_idx + 1, None);
        }
        values[bullet_idx] = Some(v);
    }

    pub fn col_set_at(&mut self, bullet_idx: usize, name: &Rc<str>, v: f64) {
        let sym = self.intern_col(name.as_ref());
        self.col_set_sym_at(bullet_idx, sym, v);
    }

    pub fn cols_for_view(&self, row: usize) -> Vec<(Rc<str>, f64)> {
        self.fields
            .num_values
            .iter()
            .enumerate()
            .filter_map(|(slot, values)| {
                let v = values.get(row).copied().flatten()?;
                let name = self.symbols.resolve(*self.fields.num_names.get(slot)?)?;
                Some((name.into(), v))
            })
            .collect()
    }

    fn clear_num_fields_at(&mut self, row: usize) {
        for values in &mut self.fields.num_values {
            if let Some(value) = values.get_mut(row) {
                *value = None;
            }
        }
    }

    pub fn field_sym(&mut self, name: impl AsRef<str>) -> FieldName {
        self.symbols.intern(name)
    }

    pub fn sym_field_slot(&self, field: FieldName) -> Option<usize> {
        self.fields.sym_slots.get(&field).copied()
    }

    pub fn intern_sym_field_slot(&mut self, field: FieldName) -> usize {
        if let Some(slot) = self.fields.sym_slots.get(&field).copied() {
            return slot;
        }
        let slot = self.fields.sym_names.len();
        self.fields.sym_names.push(field);
        self.fields.sym_slots.insert(field, slot);
        slot
    }

    pub fn sym_field_value(&self, entity: &Entity, field: FieldName) -> Option<Symbol> {
        let slot = self.sym_field_slot(field)?;
        entity.sym_fields.get(slot).copied().flatten()
    }

    pub fn sym_field_value_at(&self, i: usize, field: FieldName) -> Option<Symbol> {
        self.entities.get(i).and_then(|entity| self.sym_field_value(entity, field))
    }

    pub fn sym_field_resolved<'a>(&'a self, entity: &'a Entity, field: &str) -> Option<&'a str> {
        let field = self.symbols.lookup(field)?;
        let value = self.sym_field_value(entity, field)?;
        self.symbols.resolve(value)
    }

    pub fn sym_field_resolved_at(&self, i: usize, field: &str) -> Option<&str> {
        let field = self.symbols.lookup(field)?;
        let value = self.sym_field_value_at(i, field)?;
        self.symbols.resolve(value)
    }

    pub fn sym_field_matches(&self, entity: &Entity, field: &str, value: &str) -> bool {
        let Some(field) = self.symbols.lookup(field) else { return false };
        let Some(value) = self.symbols.lookup(value) else { return false };
        self.sym_field_value(entity, field) == Some(value)
    }

    pub fn sym_field_missing(&self, entity: &Entity, field: &str) -> bool {
        self.symbols
            .lookup(field)
            .is_none_or(|field| self.sym_field_value(entity, field).is_none())
    }
}
