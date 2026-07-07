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
/// world-owned SoA matrices.
#[derive(Clone, Debug, Default)]
pub struct WorldFields {
    pub num_slots: HashMap<FieldName, usize>,
    pub num_names: Vec<FieldName>,
    pub num_values: Vec<Vec<Option<f64>>>,
    pub sym_slots: HashMap<FieldName, usize>,
    pub sym_names: Vec<FieldName>,
    pub sym_values: Vec<Vec<Option<Symbol>>>,
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
    /// Collider projector — archetype data, Rc-shared across a spawn's
    /// elements. Layers are opaque core routing keys; specs evaluate each
    /// tick against the current figure into collision rows or nothing.
    pub collider_projector: ColliderProjector,
    /// Render projector — archetype data, Rc-shared across a spawn's elements.
    pub render_projector: RenderProjector,
}

pub struct EntityStore {
    rows: Vec<Entity>,
    generation: Vec<u32>,
    alive: Vec<bool>,
    freed_at: Vec<Option<u64>>,
    birth: Vec<u64>,
    scanned: Vec<bool>,
    cache_policy: Vec<EntityCachePolicy>,
    triggers: Vec<Rc<[TriggerRule]>>,
    sampled_pose: [Vec<Option<Pose>>; 2],
    trace_cache: TraceCache,
    motion_schema: Vec<Rc<MotionStateSchema>>,
    state_n2: Vec<Vec<[f64; 2]>>,
    state_dyn: Vec<Vec<Option<DynPose>>>,
    max: usize,
    free: Vec<usize>,
}

#[derive(Clone)]
struct TraceCache {
    stride: usize,
    samples: Vec<Pose>,
    len: Vec<usize>,
}

impl TraceCache {
    fn with_capacity(max_rows: usize) -> TraceCache {
        TraceCache {
            stride: 0,
            samples: Vec::new(),
            len: Vec::with_capacity(max_rows),
        }
    }

    fn rows(&self) -> usize {
        self.len.len()
    }

    fn ensure_rows(&mut self, rows: usize) {
        if self.len.len() < rows {
            self.len.resize(rows, 0);
            self.samples.resize(rows * self.stride, Pose::IDENTITY);
        }
    }

    fn reserve_rows(&mut self, max_rows: usize) {
        if self.len.capacity() < max_rows {
            self.len.reserve_exact(max_rows - self.len.capacity());
        }
        let wanted = max_rows.saturating_mul(self.stride);
        if self.samples.capacity() < wanted {
            self.samples.reserve_exact(wanted - self.samples.capacity());
        }
    }

    fn truncate_rows(&mut self, rows: usize) {
        self.len.truncate(rows);
        self.samples.truncate(rows * self.stride);
    }

    fn push_row(&mut self) {
        self.len.push(0);
        if self.stride > 0 {
            self.samples.resize(self.samples.len() + self.stride, Pose::IDENTITY);
        }
    }

    fn grow_stride(&mut self, stride: usize) {
        if stride <= self.stride {
            return;
        }
        let rows = self.rows();
        let mut next = vec![Pose::IDENTITY; rows * stride];
        for row in 0..rows {
            let old_start = row * self.stride;
            let new_start = row * stride;
            let len = self.len[row].min(stride);
            if len > 0 {
                next[new_start..new_start + len].copy_from_slice(&self.samples[old_start..old_start + len]);
                self.len[row] = len;
            }
        }
        self.stride = stride;
        self.samples = next;
    }

    fn samples(&self, row: usize) -> &[Pose] {
        let Some(len) = self.len.get(row).copied() else { return &[] };
        let start = row * self.stride;
        &self.samples[start..start + len]
    }

    fn push(&mut self, row: usize, pose: Pose, cap: usize) {
        if cap == 0 {
            self.clear(row);
            return;
        }
        if cap > self.stride {
            self.grow_stride(cap);
        }
        self.ensure_rows(row + 1);
        let start = row * self.stride;
        let mut len = self.len[row];
        if len >= cap {
            let keep = cap - 1;
            if keep > 0 {
                self.samples.copy_within(start + len - keep..start + len, start);
            }
            len = keep;
        }
        self.samples[start + len] = pose;
        self.len[row] = len + 1;
    }

    fn clear(&mut self, row: usize) {
        if let Some(len) = self.len.get_mut(row) {
            *len = 0;
        }
    }
}

fn remap_motion_state_key(schema: &MotionStateSchema, key: MotionStateKey) -> MotionStateKey {
    match key {
        MotionStateKey::NodePtr(ptr) => schema
            .node_ids
            .get(&ptr)
            .copied()
            .map(MotionStateKey::Node)
            .unwrap_or(key),
        MotionStateKey::LazyStagePtr { base } => schema
            .node_ids
            .get(&base)
            .copied()
            .map(|base| MotionStateKey::LazyStage { base })
            .unwrap_or(key),
        other => other,
    }
}

impl EntityStore {
    pub fn with_capacity(max: usize) -> EntityStore {
        EntityStore {
            rows: Vec::with_capacity(max),
            generation: Vec::with_capacity(max),
            alive: Vec::with_capacity(max),
            freed_at: Vec::with_capacity(max),
            birth: Vec::with_capacity(max),
            scanned: Vec::with_capacity(max),
            cache_policy: Vec::with_capacity(max),
            triggers: Vec::with_capacity(max),
            sampled_pose: [Vec::with_capacity(max), Vec::with_capacity(max)],
            trace_cache: TraceCache::with_capacity(max),
            motion_schema: Vec::with_capacity(max),
            state_n2: Vec::new(),
            state_dyn: Vec::new(),
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

    pub fn is_scanned(&self, row: usize) -> bool {
        self.scanned.get(row).copied().unwrap_or(false)
    }

    pub fn set_scanned(&mut self, row: usize, scanned: bool) {
        if let Some(slot) = self.scanned.get_mut(row) {
            *slot = scanned;
        }
    }

    pub fn is_traced(&self, row: usize) -> bool {
        self.trace_window(row).is_some()
    }

    pub fn trace_window(&self, row: usize) -> Option<f64> {
        self.cache_policy.get(row)?.trace.as_ref()?.window
    }

    pub fn triggers(&self, row: usize) -> Rc<[TriggerRule]> {
        self.triggers.get(row).cloned().unwrap_or_else(|| Rc::from([]))
    }

    pub fn motion_schema(&self, row: usize) -> Option<&MotionStateSchema> {
        self.motion_schema.get(row).map(|schema| schema.as_ref())
    }

    pub fn set_motion_schema(&mut self, row: usize, schema: Rc<MotionStateSchema>) {
        if let Some(slot) = self.motion_schema.get_mut(row) {
            *slot = schema;
        }
        self.reset_motion_state(row);
    }

    pub fn extend_motion_schema_for_lazy_stage(
        &mut self,
        row: usize,
        key: MotionStateKey,
        dyn_pose: &DynPose,
    ) -> Result<Vec<(usize, MotionNodeId)>, String> {
        if !matches!(key, MotionStateKey::LazyStage { .. } | MotionStateKey::LazyStagePtr { .. }) {
            return Err(format!(
                "motion schema extension is only supported for lazy stages, got {key:?}"
            ));
        }
        let Some(current) = self.motion_schema.get(row).cloned() else { return Ok(Vec::new()) };
        let old_nodes = current.node_ptrs.len();
        let old_n2 = current.n2_keys.len();
        let old_dyn = current.dyn_keys.len();
        let mut next = (*current).clone();
        collect_pose_state(dyn_pose, &mut next);
        if next.n2_keys.len() == old_n2 && next.dyn_keys.len() == old_dyn {
            return Ok(Vec::new());
        }
        let new_nodes = next.node_ptrs[old_nodes..]
            .iter()
            .filter_map(|ptr| next.node_ids.get(ptr).copied().map(|id| (*ptr, id)))
            .collect::<Vec<_>>();
        if let Some(slot) = self.motion_schema.get_mut(row) {
            *slot = Rc::new(next.clone());
        }
        self.ensure_motion_state_shape(&next);
        for slot in old_n2..next.n2_keys.len() {
            if let Some(cell) = self.state_n2.get_mut(slot).and_then(|col| col.get_mut(row)) {
                *cell = [0.0, 0.0];
            }
        }
        for slot in old_dyn..next.dyn_keys.len() {
            if let Some(cell) = self.state_dyn.get_mut(slot).and_then(|col| col.get_mut(row)) {
                *cell = None;
            }
        }
        Ok(new_nodes)
    }

    pub fn state_n2(&self, row: usize, key: MotionStateKey) -> Option<[f64; 2]> {
        let schema = self.motion_schema(row)?;
        let key = remap_motion_state_key(schema, key);
        let slot = schema.n2_slots.get(&key)?.0 as usize;
        self.state_n2.get(slot)?.get(row).copied()
    }

    pub fn state_n2_snapshot(&self, row: usize) -> HashMap<MotionStateKey, [f64; 2]> {
        let Some(schema) = self.motion_schema(row) else { return HashMap::new() };
        let mut out = HashMap::with_capacity(schema.n2_keys.len());
        for key in schema.n2_keys.iter().copied() {
            if let Some(value) = self.state_n2(row, key) {
                out.insert(key, value);
            }
        }
        out
    }

    pub fn set_state_n2(&mut self, row: usize, key: MotionStateKey, value: [f64; 2]) -> bool {
        let Some(slot) = self
            .motion_schema(row)
            .and_then(|schema| schema.n2_slots.get(&remap_motion_state_key(schema, key)).copied())
            .map(|slot| slot.0 as usize)
        else {
            return false;
        };
        let Some(col) = self.state_n2.get_mut(slot) else { return false };
        let Some(cell) = col.get_mut(row) else { return false };
        *cell = value;
        true
    }

    pub fn state_dyn(&self, row: usize, key: MotionStateKey) -> Option<DynPose> {
        let schema = self.motion_schema(row)?;
        let key = remap_motion_state_key(schema, key);
        let slot = schema.dyn_slots.get(&key)?.0 as usize;
        self.state_dyn.get(slot)?.get(row)?.clone()
    }

    pub fn state_dyn_snapshot(&self, row: usize) -> HashMap<MotionStateKey, DynPose> {
        let Some(schema) = self.motion_schema(row) else { return HashMap::new() };
        let mut out = HashMap::with_capacity(schema.dyn_keys.len());
        for key in schema.dyn_keys.iter().copied() {
            if let Some(value) = self.state_dyn(row, key) {
                out.insert(key, value);
            }
        }
        out
    }

    pub fn set_state_dyn(&mut self, row: usize, key: MotionStateKey, value: DynPose) -> bool {
        let Some(slot) = self
            .motion_schema(row)
            .and_then(|schema| schema.dyn_slots.get(&remap_motion_state_key(schema, key)).copied())
            .map(|slot| slot.0 as usize)
        else {
            return false;
        };
        let Some(col) = self.state_dyn.get_mut(slot) else { return false };
        let Some(cell) = col.get_mut(row) else { return false };
        *cell = Some(value);
        true
    }

    fn ensure_motion_state_shape(&mut self, schema: &MotionStateSchema) {
        let rows = self.rows.len();
        while self.state_n2.len() < schema.n2_keys.len() {
            self.state_n2.push(vec![[0.0, 0.0]; rows]);
        }
        while self.state_dyn.len() < schema.dyn_keys.len() {
            self.state_dyn.push(vec![None; rows]);
        }
        for col in &mut self.state_n2 {
            if col.len() < rows {
                col.resize(rows, [0.0, 0.0]);
            }
        }
        for col in &mut self.state_dyn {
            if col.len() < rows {
                col.resize(rows, None);
            }
        }
    }

    pub fn reset_motion_state(&mut self, row: usize) {
        let Some(schema) = self.motion_schema.get(row).cloned() else { return };
        self.ensure_motion_state_shape(&schema);
        for slot in 0..schema.n2_keys.len() {
            if let Some(cell) = self.state_n2.get_mut(slot).and_then(|col| col.get_mut(row)) {
                *cell = [0.0, 0.0];
            }
        }
        for slot in 0..schema.dyn_keys.len() {
            if let Some(cell) = self.state_dyn.get_mut(slot).and_then(|col| col.get_mut(row)) {
                *cell = None;
            }
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

    pub fn trace_samples(&self, row: usize) -> &[Pose] {
        self.trace_cache.samples(row)
    }

    pub fn push_trace_sample(&mut self, row: usize, pose: Pose, cap: usize) {
        self.trace_cache.push(row, pose, cap);
    }

    pub fn clear_trace(&mut self, row: usize) {
        self.trace_cache.clear(row);
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
        scanned: bool,
        cache_policy: EntityCachePolicy,
        triggers: Rc<[TriggerRule]>,
        motion_schema: Rc<MotionStateSchema>,
    ) -> usize {
        let i = self.free.swap_remove(slot);
        self.generation[i] = self.generation[i].wrapping_add(1);
        self.alive[i] = true;
        self.freed_at[i] = None;
        self.birth[i] = birth;
        self.scanned[i] = scanned;
        self.cache_policy[i] = cache_policy;
        self.triggers[i] = triggers;
        self.motion_schema[i] = motion_schema;
        self.reset_motion_state(i);
        self.clear_sampled_poses(i);
        self.clear_trace(i);
        self.rows[i] = entity;
        i
    }

    pub fn push_row(
        &mut self,
        entity: Entity,
        birth: u64,
        scanned: bool,
        cache_policy: EntityCachePolicy,
        triggers: Rc<[TriggerRule]>,
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
        self.scanned.push(scanned);
        self.cache_policy.push(cache_policy);
        self.triggers.push(triggers);
        self.sampled_pose[0].push(None);
        self.sampled_pose[1].push(None);
        self.trace_cache.push_row();
        self.motion_schema.push(motion_schema);
        for col in &mut self.state_n2 {
            col.push([0.0, 0.0]);
        }
        for col in &mut self.state_dyn {
            col.push(None);
        }
        self.reset_motion_state(i);
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
            scanned: self.scanned.clone(),
            cache_policy: self.cache_policy.clone(),
            triggers: self.triggers.clone(),
            sampled_pose: self.sampled_pose.clone(),
            trace_cache: self.trace_cache.clone(),
            motion_schema: self.motion_schema.clone(),
            state_n2: self.state_n2.clone(),
            state_dyn: self.state_dyn.clone(),
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
            self.entities.scanned.truncate(max_entities);
            self.entities.cache_policy.truncate(max_entities);
            self.entities.triggers.truncate(max_entities);
            self.entities.sampled_pose[0].truncate(max_entities);
            self.entities.sampled_pose[1].truncate(max_entities);
            self.entities.trace_cache.truncate_rows(max_entities);
            self.entities.motion_schema.truncate(max_entities);
            for col in &mut self.entities.state_n2 {
                col.truncate(max_entities);
            }
            for col in &mut self.entities.state_dyn {
                col.truncate(max_entities);
            }
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
        for values in &mut self.fields.sym_values {
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
        if self.entities.scanned.capacity() < max_entities {
            self.entities.scanned.reserve_exact(max_entities - self.entities.scanned.capacity());
        }
        if self.entities.cache_policy.capacity() < max_entities {
            self.entities.cache_policy.reserve_exact(max_entities - self.entities.cache_policy.capacity());
        }
        if self.entities.triggers.capacity() < max_entities {
            self.entities.triggers.reserve_exact(max_entities - self.entities.triggers.capacity());
        }
        for poses in &mut self.entities.sampled_pose {
            if poses.capacity() < max_entities {
                poses.reserve_exact(max_entities - poses.capacity());
            }
        }
        self.entities.trace_cache.reserve_rows(max_entities);
        if self.entities.motion_schema.capacity() < max_entities {
            self.entities.motion_schema.reserve_exact(max_entities - self.entities.motion_schema.capacity());
        }
        for col in &mut self.entities.state_n2 {
            if col.capacity() < max_entities {
                col.reserve_exact(max_entities - col.capacity());
            }
        }
        for col in &mut self.entities.state_dyn {
            if col.capacity() < max_entities {
                col.reserve_exact(max_entities - col.capacity());
            }
        }
        Ok(())
    }

    pub fn install_entity(
        &mut self,
        entity: Entity,
        cache_policy: EntityCachePolicy,
        triggers: Rc<[TriggerRule]>,
    ) -> Result<usize, String> {
        let motion_schema = Rc::new(collect_motion_state_schema(&entity.dyn_figure));
        let scanned = is_scanned_figure(&entity.dyn_figure);
        if let Some((slot, i)) = self.entities.reusable_free_row(self.tick) {
            self.clear_num_fields_at(i);
            self.clear_sym_fields_at(i);
            Ok(self.entities.reuse_free_row(slot, entity, self.tick, scanned, cache_policy, triggers, motion_schema))
        } else {
            self.entities.push_row(entity, self.tick, scanned, cache_policy, triggers, motion_schema)
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
        self.fields.sym_values.push(Vec::new());
        self.fields.sym_slots.insert(field, slot);
        slot
    }

    pub fn sym_field_value_at(&self, i: usize, field: FieldName) -> Option<Symbol> {
        let slot = self.sym_field_slot(field)?;
        self.entities.get(i)?;
        self.fields.sym_values.get(slot)?.get(i).copied().flatten()
    }

    pub fn sym_field_set_at(&mut self, i: usize, field: FieldName, value: Symbol) {
        let slot = self.intern_sym_field_slot(field);
        if self.entities.get(i).is_none() {
            return;
        }
        let values = &mut self.fields.sym_values[slot];
        if values.len() <= i {
            values.resize(i + 1, None);
        }
        values[i] = Some(value);
    }

    pub fn sym_fields_for_view(&self, row: usize) -> Vec<(Rc<str>, Rc<str>)> {
        self.fields
            .sym_values
            .iter()
            .enumerate()
            .filter_map(|(slot, values)| {
                let value = values.get(row).copied().flatten()?;
                let field = self.symbols.resolve(*self.fields.sym_names.get(slot)?)?;
                let value = self.symbols.resolve(value)?;
                Some((field.into(), value.into()))
            })
            .collect()
    }

    fn clear_sym_fields_at(&mut self, row: usize) {
        for values in &mut self.fields.sym_values {
            if let Some(value) = values.get_mut(row) {
                *value = None;
            }
        }
    }

    pub fn sym_field_resolved_at(&self, i: usize, field: &str) -> Option<&str> {
        let field = self.symbols.lookup(field)?;
        let value = self.sym_field_value_at(i, field)?;
        self.symbols.resolve(value)
    }

    pub fn sym_field_matches_at(&self, i: usize, field: &str, value: &str) -> bool {
        let Some(field) = self.symbols.lookup(field) else { return false };
        let Some(value) = self.symbols.lookup(value) else { return false };
        self.sym_field_value_at(i, field) == Some(value)
    }

    pub fn sym_field_missing_at(&self, i: usize, field: &str) -> bool {
        self.symbols
            .lookup(field)
            .is_none_or(|field| self.sym_field_value_at(i, field).is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64) -> Pose {
        Pose::point(x, 0.0)
    }

    #[test]
    fn trace_cache_is_dense_and_cap_trimmed() {
        let mut cache = TraceCache::with_capacity(4);
        cache.push_row();
        cache.push_row();

        cache.push(0, p(1.0), 2);
        cache.push(0, p(2.0), 2);
        cache.push(0, p(3.0), 2);
        cache.push(1, p(10.0), 3);

        assert_eq!(cache.samples(0), &[p(2.0), p(3.0)]);
        assert_eq!(cache.samples(1), &[p(10.0)]);

        cache.push(0, p(4.0), 4);
        assert_eq!(cache.samples(0), &[p(2.0), p(3.0), p(4.0)]);
        assert_eq!(cache.samples(1), &[p(10.0)]);

        cache.clear(0);
        assert!(cache.samples(0).is_empty());
        assert_eq!(cache.samples(1), &[p(10.0)]);
    }

    #[test]
    fn runtime_schema_extension_is_lazy_stage_only() {
        let mut store = EntityStore::with_capacity(1);
        let dyn_pose = DynPose::pose_node(Rc::new(DynNode::Const(Pose::IDENTITY)));

        let err = store
            .extend_motion_schema_for_lazy_stage(0, MotionStateKey::Node(MotionNodeId(0)), &dyn_pose)
            .unwrap_err();

        assert!(err.contains("only supported for lazy stages"));
    }
}
