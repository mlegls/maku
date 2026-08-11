//! The hot layer: poses, dyn nodes, signal evaluation, scanned motion.

use super::*;
use crate::edn::Form;
use crate::fxhash::FxHashMap;
use crate::sim::kernel::{
    FallbackPolicy, IterationDomain, KernelBindings, KernelInputBinding, KernelInputSource,
    KernelOutputBinding, KernelOutputTarget, KernelPlan, MergePolicy,
};
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// Per-bullet scanned state keyed by stable lowered motion ids.
#[derive(Debug, Clone)]
pub enum Cell {
    N([f64; 2]),
    D(DynPose),
    V(EvolveCell),
}
pub type MotionState = FxHashMap<MotionStateKey, Cell>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionNodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionStateKey {
    /// Stable lowered node id for dense entity state.
    Node(MotionNodeId),
    /// The stock integrator's current-tick evaluated Cartesian components.
    IntegratorComponents(MotionNodeId),
    /// Expression-local stateful sites under a scanned node. These are
    /// discovered from sited evolves during expression lowering.
    ScanSite { base: MotionNodeId, index: u32 },
    /// A stage segment's exit parameter cell (pos/vel), written at the stage
    /// boundary. Keyed by the slot token's stable lowered id — slot ptrs are
    /// seeded into the owning spec schema's node_ids alongside node ptrs.
    StageExit { base: MotionNodeId, field: StageExitField },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageExitField {
    Pos,
    Vel,
}

#[derive(Debug)]
pub struct StageExitSlot;

pub(crate) fn stage_exit_key(
    slot: &Rc<StageExitSlot>,
    readers: &MotionReaders,
    field: StageExitField,
) -> MotionStateKey {
    let ptr = Rc::as_ptr(slot) as usize;
    if let Some(base) = readers.node_ids.borrow().get(&ptr).copied() {
        return MotionStateKey::StageExit { base, field };
    }
    panic!("stage exit slot has no stable lowered id for pointer {ptr:#x}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateN2SlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateDynSlotId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateValSlotId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct MotionStateSchema {
    pub n2_slots: FxHashMap<MotionStateKey, StateN2SlotId>,
    pub n2_keys: Vec<MotionStateKey>,
    pub dyn_slots: FxHashMap<MotionStateKey, StateDynSlotId>,
    pub dyn_keys: Vec<MotionStateKey>,
    pub val_slots: FxHashMap<MotionStateKey, StateValSlotId>,
    pub val_keys: Vec<MotionStateKey>,
    pub node_ids: FxHashMap<usize, MotionNodeId>,
    /// node_ids in the shape MotionReaders wants, built once per schema.
    /// Entity schemas are complete at load, so per-row readers can share
    /// this instead of cloning the map per entity per phase; only ad-hoc
    /// direct evaluation seeds ids lazily, through its own fresh maps.
    shared_node_ids: std::cell::OnceCell<Rc<RefCell<FxHashMap<usize, MotionNodeId>>>>,
}

impl MotionStateSchema {
    pub fn shared_node_ids(&self) -> Rc<RefCell<FxHashMap<usize, MotionNodeId>>> {
        self.shared_node_ids
            .get_or_init(|| Rc::new(RefCell::new(self.node_ids.clone())))
            .clone()
    }

    pub fn intern_node(&mut self, ptr: usize) -> MotionNodeId {
        if let Some(id) = self.node_ids.get(&ptr).copied() {
            return id;
        }
        let id = MotionNodeId(self.node_ids.len() as u32);
        self.node_ids.insert(ptr, id);
        id
    }

    pub fn intern_n2(&mut self, key: MotionStateKey) -> StateN2SlotId {
        if let Some(slot) = self.n2_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateN2SlotId(self.n2_keys.len() as u32);
        self.n2_keys.push(key);
        self.n2_slots.insert(key, slot);
        slot
    }

    pub fn intern_dyn(&mut self, key: MotionStateKey) -> StateDynSlotId {
        if let Some(slot) = self.dyn_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateDynSlotId(self.dyn_keys.len() as u32);
        self.dyn_keys.push(key);
        self.dyn_slots.insert(key, slot);
        slot
    }

    pub fn intern_val(&mut self, key: MotionStateKey) -> StateValSlotId {
        if let Some(slot) = self.val_slots.get(&key).copied() {
            return slot;
        }
        let slot = StateValSlotId(self.val_keys.len() as u32);
        self.val_keys.push(key);
        self.val_slots.insert(key, slot);
        slot
    }
}

/// Scan-context IO for stateful signal evaluation: carries the bullet's state
/// cells plus a per-evaluation site counter (stable for a fixed expr tree).
pub struct ScanIo {
    pub state: MotionState,
    pub base: usize,
    pub dense_base: Option<MotionNodeId>,
    pub counter: usize,
    pub advance: bool,
    pub dt: f64,
    pub readers: Option<MotionReaders>,
    pub mirror_legacy: bool,
    pub n2_writes: Vec<(MotionStateKey, [f64; 2])>,
    pub val_writes: Vec<(MotionStateKey, EvolveCell)>,
}
pub type ScanShared = Rc<std::cell::RefCell<ScanIo>>;

/// One row's state cells, snapshotted at reader construction. Reads must see
/// the values as of construction even while the step pass writes through to
/// the world's columns. Values sit at their schema slot index; key lookup is
/// a linear scan over the schema's key vectors — a schema holds one entry per
/// stateful site of a motion tree, so the vectors are tiny.
pub struct RowStateSnapshot {
    pub(crate) schema: Rc<MotionStateSchema>,
    pub(crate) n2: Vec<Option<[f64; 2]>>,
    pub(crate) dyns: Vec<Option<DynPose>>,
    pub(crate) vals: Vec<Option<EvolveCell>>,
}

#[derive(Clone)]
enum ReaderBacking {
    /// No state cells behind these readers: ad-hoc evaluation and rows
    /// whose schema holds no state.
    Empty,
    Row(Rc<RowStateSnapshot>),
    /// n2-only schemas with at most two cells — the vel-integrator bullet
    /// case. Values sit inline, so construction allocates nothing.
    RowN2 {
        schema: Rc<MotionStateSchema>,
        n2: [Option<[f64; 2]>; 2],
    },
}

#[derive(Clone)]
pub struct MotionReaders {
    backing: ReaderBacking,
    components: Option<([ColName; 2], [f64; 2])>,
    pub node_ids: Rc<RefCell<FxHashMap<usize, MotionNodeId>>>,
}

impl MotionReaders {
    pub fn component(&self, column: ColName) -> Option<f64> {
        let (names, values) = self.components?;
        if names[0] == column { Some(values[0]) }
        else if names[1] == column { Some(values[1]) }
        else { None }
    }

    pub fn with_components(mut self, names: [ColName; 2], values: [f64; 2]) -> MotionReaders {
        self.components = Some((names, values));
        self
    }

    pub fn n2(&self, key: MotionStateKey) -> Option<[f64; 2]> {
        match &self.backing {
            ReaderBacking::Empty => None,
            ReaderBacking::Row(snap) => snap
                .schema
                .n2_keys
                .iter()
                .position(|k| *k == key)
                .and_then(|slot| snap.n2.get(slot).copied().flatten()),
            ReaderBacking::RowN2 { schema, n2 } => schema
                .n2_keys
                .iter()
                .position(|k| *k == key)
                .and_then(|slot| n2.get(slot).copied().flatten()),
        }
    }

    pub fn dyns(&self, key: MotionStateKey) -> Option<DynPose> {
        match &self.backing {
            ReaderBacking::Empty | ReaderBacking::RowN2 { .. } => None,
            ReaderBacking::Row(snap) => snap
                .schema
                .dyn_keys
                .iter()
                .position(|k| *k == key)
                .and_then(|slot| snap.dyns.get(slot).cloned().flatten()),
        }
    }

    pub fn vals(&self, key: MotionStateKey) -> Option<EvolveCell> {
        match &self.backing {
            ReaderBacking::Empty | ReaderBacking::RowN2 { .. } => None,
            ReaderBacking::Row(snap) => snap
                .schema
                .val_keys
                .iter()
                .position(|k| *k == key)
                .and_then(|slot| snap.vals.get(slot).cloned().flatten()),
        }
    }

    pub fn legacy() -> MotionReaders {
        MotionReaders {
            backing: ReaderBacking::Empty,
            components: None,
            node_ids: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    /// Readers for a row whose schema holds no state cells — the common
    /// stateless-bullet case: no snapshot, just the shared node-id map.
    pub fn stateless(node_ids: Rc<RefCell<FxHashMap<usize, MotionNodeId>>>) -> MotionReaders {
        MotionReaders { backing: ReaderBacking::Empty, components: None, node_ids }
    }

    pub(crate) fn for_row_snapshot(snapshot: RowStateSnapshot) -> MotionReaders {
        let node_ids = snapshot.schema.shared_node_ids();
        MotionReaders {
            backing: ReaderBacking::Row(Rc::new(snapshot)),
            components: None,
            node_ids,
        }
    }

    /// Caller guarantees the schema holds no dyn/val cells and at most two
    /// n2 cells, with `n2` in schema slot order.
    pub(crate) fn for_row_n2(
        schema: Rc<MotionStateSchema>,
        n2: [Option<[f64; 2]>; 2],
    ) -> MotionReaders {
        let node_ids = schema.shared_node_ids();
        MotionReaders {
            backing: ReaderBacking::RowN2 { schema, n2 },
            components: None,
            node_ids,
        }
    }

    pub fn for_node(d: &DynNode) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_dyn_node_ids(d, &readers);
        readers
    }

    pub fn for_pose(d: &DynPose) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_pose_nodes(d, &readers);
        readers
    }

    pub fn for_figure(d: &DynFigure) -> MotionReaders {
        let readers = MotionReaders::legacy();
        seed_reader_figure_nodes(d, &readers);
        readers
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRange {
    pub offset: usize,
    pub len: usize,
}

#[derive(Clone, Debug, Default)]
pub struct CaptureLayout {
    ranges: FxHashMap<usize, CaptureRange>,
    width: usize,
}

impl CaptureLayout {
    pub fn range(&self, node: &DynNode) -> Option<CaptureRange> {
        self.ranges.get(&(node as *const DynNode as usize)).copied()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn insert(&mut self, node: &DynNode, len: usize) {
        let ptr = node as *const DynNode as usize;
        let range = CaptureRange { offset: self.width, len };
        assert!(self.ranges.insert(ptr, range).is_none(), "rand node occurs twice in one spec tree");
        self.width += len;
    }
}

#[derive(Clone, Copy)]
pub struct MotionEvalCtx<'a> {
    pub state: &'a MotionState,
    pub sig: &'a SigEnv,
    pub readers: &'a MotionReaders,
    pub tick_rate: f64,
    pub capture_layout: Option<&'a CaptureLayout>,
    pub captures: &'a [f32],
    pub row_frame: Option<Pose>,
    /// When false the caller provably discards theta, so nodes whose
    /// heading costs extra evaluation (ClosedPt's second sample, Vel's
    /// integrand, RotExpr) may skip it. Frame re-enables it for the
    /// parent: compose rotates the child offset by the parent's theta.
    pub need_theta: bool,
}

impl<'a> MotionEvalCtx<'a> {
    pub fn new(state: &'a MotionState, sig: &'a SigEnv, readers: &'a MotionReaders) -> MotionEvalCtx<'a> {
        MotionEvalCtx::with_tick_rate(state, sig, readers, TickTiming::default().rate())
    }

    pub fn with_tick_rate(
        state: &'a MotionState,
        sig: &'a SigEnv,
        readers: &'a MotionReaders,
        tick_rate: f64,
    ) -> MotionEvalCtx<'a> {
        MotionEvalCtx {
            state,
            sig,
            readers,
            tick_rate,
            capture_layout: None,
            captures: &[],
            row_frame: None,
            need_theta: true,
        }
    }

    pub fn with_row(
        mut self,
        capture_layout: &'a CaptureLayout,
        captures: &'a [f32],
        row_frame: Option<Pose>,
    ) -> Self {
        self.capture_layout = Some(capture_layout);
        self.captures = captures;
        self.row_frame = row_frame;
        self
    }

    pub fn caps_for(&self, node: &DynNode, rand: &'a Option<Rc<RandCell>>) -> Vec<f64> {
        let stored = if let Some(range) = self.capture_layout.and_then(|layout| layout.range(node)) {
            &self.captures[range.offset..range.offset + range.len]
        } else {
            stored_caps_of(rand)
        };
        stored.iter().map(|value| *value as f64).collect()
    }

    pub fn compiled_for(&self, node: &DynNode, rand: &'a Option<Rc<RandCell>>) -> Option<&'a ExtractedSig> {
        self.capture_layout
            .and_then(|layout| layout.range(node))
            .and_then(|_| match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Some(ex),
                _ => None,
            })
    }

    pub fn pos_only(mut self) -> Self {
        self.need_theta = false;
        self
    }

    fn with_theta(mut self) -> Self {
        self.need_theta = true;
        self
    }

    pub fn node_key(&self, ptr: usize) -> MotionStateKey {
        state_key_for_node(ptr, self.readers)
    }
}

pub struct MotionStepCtx<'a> {
    pub state: &'a mut MotionState,
    pub sig: &'a SigEnv,
    pub capture_layout: Option<&'a CaptureLayout>,
    pub captures: &'a [f32],
    pub world: Option<&'a mut World>,
    pub readers: &'a MotionReaders,
    pub tick_rate: f64,
    pub rng_base: Option<u64>,
    pub mirror_legacy: bool,
    pub write_n2: &'a mut dyn FnMut(MotionStateKey, [f64; 2]),
    pub write_col: &'a mut dyn FnMut(ColName, f64),
    pub write_dyn: &'a mut dyn FnMut(MotionStateKey, DynPose),
    pub write_val: &'a mut dyn FnMut(MotionStateKey, EvolveCell),
}

impl<'a> MotionStepCtx<'a> {
    pub fn node_key(&self, ptr: usize) -> MotionStateKey {
        state_key_for_node(ptr, self.readers)
    }

    pub fn caps_for<'b>(
        &'b self,
        node: &DynNode,
        rand: &'b Option<Rc<RandCell>>,
    ) -> Vec<f64> {
        let stored = if let Some(range) = self.capture_layout.and_then(|layout| layout.range(node)) {
            &self.captures[range.offset..range.offset + range.len]
        } else {
            stored_caps_of(rand)
        };
        stored.iter().map(|value| *value as f64).collect()
    }

    pub fn compiled_for<'b>(
        &'b self,
        node: &DynNode,
        rand: &'b Option<Rc<RandCell>>,
    ) -> Option<&'b ExtractedSig> {
        self.capture_layout
            .and_then(|layout| layout.range(node))
            .and_then(|_| match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Some(ex),
                _ => None,
            })
    }
}

/// Frame smart constructor: folds constant parents at build time.
/// Pose composition is associative SE(2), and nodes are immutable after
/// construction, so Frame(Const a, Const b) is Const(a∘b) and
/// Frame(Const a, Frame(Const b, c)) is Frame(Const(a∘b), c). Folding
/// happens before spawn-time schema lowering, so node identity used for
/// state keys is unaffected (Const/Frame carry no state).
pub fn frame_node(parent: Rc<DynNode>, child: Rc<DynNode>) -> Rc<DynNode> {
    if let DynNode::Const(p) = &*parent {
        match &*child {
            DynNode::Const(q) => return Rc::new(DynNode::Const(p.compose(q))),
            DynNode::Frame(inner, gc) => {
                if let DynNode::Const(q) = &**inner {
                    return const_frame_node(p.compose(q), gc.clone());
                }
            }
            DynNode::ConstFrame { pose, child: gc, .. } => {
                return const_frame_node(p.compose(pose), gc.clone());
            }
            _ => {}
        }
        return const_frame_node(*p, child);
    }
    Rc::new(DynNode::Frame(parent, child))
}

/// Constant-parent frame constructor: pre-bakes the parent rotation so
/// per-eval composition skips the sincos (`Pose::compose_with_rot`).
fn const_frame_node(pose: Pose, child: Rc<DynNode>) -> Rc<DynNode> {
    let rot = pose.heading_rot();
    Rc::new(DynNode::ConstFrame { pose, rot, child })
}

pub(crate) fn state_key_for_node(ptr: usize, readers: &MotionReaders) -> MotionStateKey {
    if let Some(id) = readers.node_ids.borrow().get(&ptr).copied() {
        return MotionStateKey::Node(id);
    }
    panic!("motion node has no stable lowered id for pointer {ptr:#x}")
}

fn integrator_component_key(ptr: usize, readers: &MotionReaders) -> MotionStateKey {
    let MotionStateKey::Node(id) = state_key_for_node(ptr, readers) else { unreachable!() };
    MotionStateKey::IntegratorComponents(id)
}

/// Rand-as-capture-slots (compiled-dyn milestone B, input slots): a node's
/// rand sites extract to `(%capture i)` marker forms ONCE at node
/// construction; each spawned entity then draws a small capture vector
/// instead of cloning a substituted form tree, and the marker programs are
/// shared by the whole group — which is what lets rand-bearing rows join
/// the vel batch lanes. The node field is `Option<Rc<RandCell>>` (one word,
/// None for rand-free forms) because DynNode size is hot: every pose walk
/// chases these enums, and inlining the slot data measurably regressed the
/// wall (88 → 120 bytes cost ~60% on the scaled fruit rig).
#[derive(Debug)]
pub struct CaptureData {
    values: Rc<[f32]>,
    env_names: Rc<[Rc<str>]>,
    env_offset: usize,
}

impl CaptureData {
    pub(crate) fn new(values: Rc<[f32]>, env_names: Rc<[Rc<str>]>, env_offset: usize) -> CaptureData {
        CaptureData { values, env_names, env_offset }
    }
}

#[derive(Debug)]
pub enum RandCell {
    /// Spec node: extraction + lowering, built at construction. The node's
    /// own forms stay untouched. Uncaptured rand in rowless signal contexts
    /// is a deterministic constant; rowful scratch contexts draw keyed
    /// per-entity-per-tick randomness.
    Compiled(ExtractedSig),
    /// Rand present but the marker forms didn't lower: spawn instantiation
    /// falls back to per-entity form substitution, the pre-slot path.
    Bail,
    /// Entity clone: the drawn capture values, slot-indexed.
    Caps(Rc<CaptureData>),
}

/// Canonical id for one fully compared typed motion program plus plan
/// binding set. It is minted only after structural interning has compared
/// the complete program, bindings, policies, and auxiliary source metadata;
/// hot group lookup can therefore use it without pointer identity or
/// re-walking the op stream per lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MotionProgramIdentity(u64);

/// A validated MotionRows plan paired with the permanent specialized F64
/// executor artifact. The typed program and bindings are the compile/cache
/// identity; hot scalar and lane execution deliberately stays on
/// `NumProgram` so its exact operation order does not change.
#[derive(Debug)]
pub struct MotionProgram {
    backend: Rc<NumProgram>,
    plan: KernelPlan,
    identity: MotionProgramIdentity,
}

impl MotionProgram {
    pub fn backend(&self) -> &NumProgram {
        &self.backend
    }

    pub fn plan(&self) -> &KernelPlan {
        &self.plan
    }
    pub fn identity(&self) -> MotionProgramIdentity {
        self.identity
    }


    pub fn n_inputs(&self) -> usize {
        self.backend.n_inputs
    }

    pub fn aux_free(&self) -> bool {
        self.backend.aux_free()
    }

    /// Full structural plan identity. Program ids select an intern bucket,
    /// but equality still checks the complete typed program and every
    /// binding so a hash collision or differently bound input cannot merge.
    pub fn same_identity(&self, other: &Self) -> bool {
        self.plan.id() == other.plan.id()
            && self.plan.program() == other.plan.program()
            && self.plan.bindings() == other.plan.bindings()
            && self.plan.domain() == other.plan.domain()
            && self.plan.fallback() == other.plan.fallback()
            && self.plan.merge() == other.plan.merge()
            // Aux tables carry the exact named/stream channel identity;
            // common Channel bindings name their per-plan component slot.
            && self.backend.aux == other.backend.aux
    }
}

fn motion_input_source(program: &NumProgram, source: NumKernelInputSource) -> Option<KernelInputSource> {
    match source {
        NumKernelInputSource::Capture(slot) => Some(KernelInputSource::Capture { slot }),
        NumKernelInputSource::Tick => Some(KernelInputSource::Tick),
        NumKernelInputSource::Axis => Some(KernelInputSource::Axis),
        NumKernelInputSource::PositionX => Some(KernelInputSource::State { slot: 0 }),
        NumKernelInputSource::PositionY => Some(KernelInputSource::State { slot: 1 }),
        NumKernelInputSource::Aux(slot) => match program.aux.as_deref()?.slots.get(usize::from(slot))? {
            // Slots 0/1 are the current position components. Scan cells
            // follow them in the motion plan's state namespace.
            crate::interp::lower::AuxSlot::Scan(index) => Some(KernelInputSource::State {
                slot: u16::try_from(index.checked_add(2)?).ok()?,
            }),
            // Channel slots are flattened x/y components. For numeric
            // channels only the even (x) slot is present.
            crate::interp::lower::AuxSlot::ChanX(channel) => Some(KernelInputSource::Channel {
                channel: channel.checked_mul(2)?,
            }),
            crate::interp::lower::AuxSlot::ChanY(channel) => Some(KernelInputSource::Channel {
                channel: channel.checked_mul(2)?.checked_add(1)?,
            }),
        },
    }
}

fn attach_motion_program(program: NumProgram) -> Option<Rc<MotionProgram>> {
    let backend = intern_program(program);
    let bridge = kernel_program_for_num(&backend).ok()?;
    let inputs = bridge
        .inputs
        .iter()
        .copied()
        .enumerate()
        .map(|(index, source)| {
            Some(KernelInputBinding {
                input: KernelInputRef::F64(u16::try_from(index).ok()?),
                source: motion_input_source(&backend, source)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let plan = KernelPlan::new(
        bridge.program,
        IterationDomain::MotionRows,
        KernelBindings {
            inputs,
            outputs: vec![KernelOutputBinding {
                output: 0,
                target: KernelOutputTarget::Driver,
            }],
        },
        FallbackPolicy::WholePlanInterpreted,
        MergePolicy::DriverOwned,
    )
    .ok()?;
    let mut candidate = MotionProgram {
        backend,
        plan,
        identity: MotionProgramIdentity(0),
    };
    thread_local! {
        static PROGRAMS: RefCell<FxHashMap<KernelProgramId, Vec<Weak<MotionProgram>>>> =
            RefCell::new(FxHashMap::default());
        static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    }
    PROGRAMS.with(|programs| {
        let mut programs = programs.borrow_mut();
        let bucket = programs.entry(candidate.plan.id()).or_default();
        bucket.retain(|program| program.strong_count() != 0);
        if let Some(existing) = bucket
            .iter()
            .filter_map(Weak::upgrade)
            .find(|program| program.same_identity(&candidate))
        {
            return Some(existing);
        }
        candidate.identity = NEXT_ID.with(|next| {
            let identity = next.get();
            next.set(identity.checked_add(1).expect("motion program identity overflow"));
            MotionProgramIdentity(identity)
        });
        let program = Rc::new(candidate);
        bucket.push(Rc::downgrade(&program));
        Some(program)
    })
}

#[derive(Debug)]
pub struct ExtractedSig {
    /// Marker forms in node order — (a, b) for pair nodes, one for RotExpr.
    pub forms: Vec<Form>,
    pub sites: Vec<super::spawn::RandSite>,
    /// Construction-time values of the env-capture slots (slot ids
    /// `sites.len()..`); the env is fixed per node, so these are too.
    pub env_caps: Vec<f64>,
    pub env_names: Rc<[Rc<str>]>,
    /// One plan-backed program per form, structurally interned. Every
    /// specialized backend's `n_inputs` is bumped to the node's full
    /// capture width so all programs read one capture vector.
    pub programs: Vec<Rc<MotionProgram>>,
}

/// Construction-time compile of a node's signal forms: rand-site extraction
/// (when rand is present), slot-mode lowering (numeric env captures become
/// Input slots), and structural interning — nodes that differ only in
/// captured or drawn values share ONE program and batch as lanes.
///
/// Returns the node's slot cell and its programs cell:
/// - rand-free, lowered: programs prefilled; cell = Caps(env values) when
///   the programs take slots, else None. The node is shared by its group.
/// - rand-bearing, lowered: cell = Compiled(markers + sites + programs);
///   the SPEC node's programs cell stays LAZY over the original forms
///   (direct evaluation outside spawn keeps per-eval rand semantics), and
///   spawn instantiation prefills each clone from the cell.
/// - not lowered: cell = Bail if rand is present (spawn substitutes per
///   entity, the pre-slot path), else None; programs stay lazy.
pub(crate) fn compile_sig(
    forms: &[&Form],
    env: &Env,
    sig: &SigEnv,
    allow_pos: bool,
    allow_aux: bool,
) -> (Option<Rc<RandCell>>, Option<Vec<Rc<MotionProgram>>>) {
    let has_rand = forms.iter().any(|f| super::spawn::form_has_rand(f));
    let mut sites = Vec::new();
    let marked: Vec<Form> = if has_rand {
        forms.iter().map(|f| super::spawn::extract_rand(f, &mut sites)).collect()
    } else {
        forms.iter().map(|f| (*f).clone()).collect()
    };
    let mut names: Vec<Rc<str>> = Vec::new();
    let mut programs = Vec::with_capacity(marked.len());
    let mut site_base = 0u32;
    for m in &marked {
        let opts = LowerOpts {
            env_slots: Some((&mut names, sites.len())),
            allow_aux,
            site_base,
        };
        // scan-site numbering runs across a node's forms (one shared
        // counter per eval), so form b starts after form a's sites
        site_base += form_site_count(m);
        let lowered = lower_num_form_opts(m, env, &sig.defs, opts)
            .filter(|p| allow_pos || !program_uses_pos(p));
        let Some(mut p) = lowered else {
            return (has_rand.then(|| Rc::new(RandCell::Bail)), None);
        };
        p.n_inputs = sites.len() + names.len();
        programs.push(p);
    }
    // late bump: an env slot discovered while lowering `b` widens `a` too
    let width = sites.len() + names.len();
    let programs = programs
        .into_iter()
        .map(|mut p| {
            p.n_inputs = width;
            attach_motion_program(p)
        })
        .collect::<Option<Vec<_>>>();
    let Some(programs) = programs else {
        return (has_rand.then(|| Rc::new(RandCell::Bail)), None);
    };
    // env lookups can't fail: slot mode only records names it resolved
    let env_caps: Vec<f64> = names
        .iter()
        .map(|n| match env.lookup(n) {
            Some(Val::Num(v)) => v,
            _ => unreachable!("env slot {n} vanished between lowering and capture"),
        })
        .collect();
    if has_rand {
        let cell = RandCell::Compiled(ExtractedSig {
            forms: marked,
            sites,
            env_caps,
            env_names: names.into(),
            programs,
        });
        (Some(Rc::new(cell)), None)
    } else {
        let cell = (width > 0).then(|| Rc::new(RandCell::Caps(Rc::new(CaptureData::new(
            env_caps.into_iter().map(|value| value as f32).collect(),
            names.into(),
            0,
        )))));
        (cell, Some(programs))
    }
}

/// The entity's capture vector, widened at the storage boundary.
pub(crate) fn caps_of(rand: &Option<Rc<RandCell>>) -> Vec<f64> {
    stored_caps_of(rand).iter().map(|value| *value as f64).collect()
}

fn stored_caps_of(rand: &Option<Rc<RandCell>>) -> &[f32] {
    match rand.as_deref() {
        Some(RandCell::Caps(caps)) => &caps.values,
        _ => &[],
    }
}

#[derive(Debug)]
pub enum DynNode {
    Const(Pose),
    /// pos = v·τ in the local frame; θ = heading.
    Linear { vx: f64, vy: f64 },
    /// Closed pose expression over slot-bound t (and u, for curve shapes).
    ClosedPt {
        a: Form,
        b: Form,
        polar: bool,
        env: Env,
        programs: OnceCell<Option<(Rc<MotionProgram>, Rc<MotionProgram>)>>,
        rand: Option<Rc<RandCell>>,
    },
    /// The recognized stock evolve `p += v·dt`. Component evaluation and
    /// materialization are owned by this node's step.
    StockIntegrator { data: Rc<StockIntegratorData> },
    /// Point-translation (the `+` of the two-op algebra): θ untouched.
    Translate { dx: f64, dy: f64, child: Rc<DynNode> },
    /// Sample a curve dyn at u = progress(t). This is the point-motion
    /// analogue of curve materialization, without expressing a curve entity.
    Path { curve: Rc<DynNode>, progress: Form, env: Env },
    Frame(Rc<DynNode>, Rc<DynNode>),
    /// Frame with a constant parent, its rotation pre-baked at
    /// construction — the plain-bullet shape after Const folding (spawn
    /// frame ∘ moving child). Carries no state of its own, like Frame.
    ConstFrame { pose: Pose, rot: (f64, f64), child: Rc<DynNode> },
    /// Entity-row root frame. The pose is supplied by MotionEvalCtx and is
    /// identity for direct/ad-hoc evaluation.
    RowFrame(Rc<DynNode>),
    /// A live injected channel as a pose (class (b): pointwise, no state).
    Live { channel: Rc<str> },
    /// A live stream BY HANDLE (class (b)): local and global streams frame
    /// identically — the node carries the store id, not a name.
    LiveStream { id: u64 },
    /// Position clamp (playfield walls). Output-clamps the child pose; for
    /// integrated children (vel under const frames) the integrator STATE is
    /// clamped after each step — pushing a wall doesn't bank phantom
    /// distance, you slide and turn back instantly.
    Clamp { lo: (f64, f64), hi: (f64, f64), child: Rc<DynNode> },
    /// Time-varying rotation frame: θ(t), stateful sites allowed inside.
    RotExpr { form: Form, env: Env, program: OnceCell<Option<Rc<MotionProgram>>>, rand: Option<Rc<RandCell>> },
    /// A user function adapted to a stateless pose dyn by calling it as (f t).
    FnPose(Val),
    /// A closed evolve used in a pose slot: the fold is replayed from epoch
    /// start at each evaluation (pure in tau), so the node carries no
    /// per-entity motion state. Memoized monotone advance is a later
    /// optimization, not a semantic need.
    Evolve(Rc<EvolveDyn>),
    /// SCANNED.md's `stages`: segment list with per-entity (idx, epoch) state.
    /// Closure segments are lowered at construction with fixed exit-pose cells.
    Stages { segs: Vec<StageSeg> },
}

#[derive(Clone, Copy, Debug)]
pub enum IntegratorComponentSpace {
    Cartesian,
    Polar,
}

#[derive(Debug)]
pub struct StockIntegratorData {
    pub a: Form,
    pub b: Form,
    pub space: IntegratorComponentSpace,
    pub env: Env,
    pub programs: OnceCell<Option<(Rc<MotionProgram>, Rc<MotionProgram>)>>,
    pub rand: Option<Rc<RandCell>>,
    pub columns: [ColName; 2],
}

impl StockIntegratorData {
    fn source_is_polar(&self) -> bool {
        matches!(self.space, IntegratorComponentSpace::Polar)
    }
}

/// `(evolve init step)` — the kernel's stateful signal constructor.
#[derive(Debug)]
pub struct EvolveDyn {
    pub init: EvolveInit,
    pub step: Val,
    pub live: bool,
}

#[derive(Debug)]
pub enum EvolveInit {
    Value(Val),
    Thunk { form: Form, env: Env },
}

#[derive(Clone, Debug)]
pub struct EvolveCell {
    pub state: Val,
    pub tick: u64,
}

pub(crate) fn evolve_tick(tau: f64, tick_rate: f64) -> u64 {
    (tau * tick_rate + 1e-9).floor().max(0.0) as u64
}

pub(crate) fn evolve_step_ctx(k: u64, dt: f64) -> Val {
    Val::Map(Rc::new(vec![
        (Val::Kw("t".into()), Val::Num(k as f64 * dt)),
        (Val::Kw("dt".into()), Val::Num(dt)),
        (Val::Kw("tick".into()), Val::Num(k as f64)),
    ]))
}

fn apply_evolve_step(ev: &EvolveDyn, state: Val, k: u64, sig: &SigEnv, tick_rate: f64, world: Option<&mut World>, rng_base: Option<u64>) -> Result<Val, String> {
    let step_ctx = evolve_step_ctx(k, 1.0 / tick_rate);
    let mut call_ctx = Ctx {
        sig: sig.clone(),
        ambient: Pose::IDENTITY,
        overrides: None,
        scan: None,
        patterns: Rc::new(HashMap::new()),
        macros: Rc::new(HashMap::new()),
        deferred: Vec::new(),
        projector_scope: None,
    };
    let mut fallback = match rng_base {
        Some(base) => World::for_eval_keyed(tick_rate, base),
        None => World::for_eval(tick_rate),
    };
    let run_world = match world {
        Some(world) => {
            if let Some(base) = rng_base { world.rebase_rng(base); }
            world
        }
        None => &mut fallback,
    };
    apply_fn(ev.step.clone(), &[state, step_ctx], &mut call_ctx, run_world, false)
}

fn resolve_evolve_init(ev: &EvolveDyn, sig: &SigEnv, tick_rate: f64, world: Option<&mut World>, rng_base: Option<u64>) -> Result<Val, String> {
    match &ev.init {
        EvolveInit::Value(value) => Ok(value.clone()),
        EvolveInit::Thunk { form, env } => {
            let mut call_ctx = Ctx {
                sig: sig.clone(),
                ambient: Pose::IDENTITY,
                overrides: None,
                scan: None,
                patterns: Rc::new(HashMap::new()),
                macros: Rc::new(HashMap::new()),
                deferred: Vec::new(),
                projector_scope: None,
                    };
            let mut fallback = match rng_base {
                Some(base) => World::for_eval_keyed(tick_rate, base),
                None => World::for_eval(tick_rate),
            };
            let run_world = match world {
                Some(world) => {
                    if let Some(base) = rng_base { world.rebase_rng(base); }
                    world
                }
                None => &mut fallback,
            };
            evaluate(form, env, &mut call_ctx, run_world)
        }
    }
}

/// The value of a closed evolve at time tau: the fold of `step` over ticks
/// 0..floor(tau·rate), starting from `init`. Steps evaluate against a
/// CLOSED SigEnv — defs carry over (pure), but channels/cells are empty so
/// live channel reads error, enforcing the closed-evolve rule.
pub fn evolve_value(ev: &EvolveDyn, tau: f64, sig: &SigEnv, tick_rate: f64) -> Result<Val, String> {
    if ev.live {
        return Err("live evolve sampled off its clock".into());
    }
    let closed_sig = SigEnv { defs: sig.defs.clone(), ..SigEnv::default() };
    let n = evolve_tick(tau, tick_rate);
    let mut s = resolve_evolve_init(ev, &closed_sig, tick_rate, None, None)?;
    for k in 0..n {
        s = apply_evolve_step(ev, s, k, &closed_sig, tick_rate, None, None)?;
    }
    Ok(s)
}

#[derive(Debug)]
pub struct StageSeg {
    pub term: StageTerm,
    pub make: StageMake,
    pub exit_slot: Option<Rc<StageExitSlot>>,
}

#[derive(Debug)]
pub enum StageTerm {
    Dur(f64),
    Until(Form, Env),
    Forever,
}

#[derive(Debug)]
pub enum StageMake {
    Ready(DynPose),
}

pub fn collect_motion_state_schema(d: &DynFigure) -> MotionStateSchema {
    let mut schema = MotionStateSchema::default();
    collect_figure_state(d, &mut schema);
    schema
}

pub fn collect_figure_state(d: &DynFigure, schema: &mut MotionStateSchema) {
    match d.repr() {
        FigureDynRepr::Pose(pose) => collect_pose_state(pose, schema),
        FigureDynRepr::Curve { frame, curve } => {
            collect_pose_state(frame, schema);
            if let CurveEval::Expr(shape) = &curve.eval {
                collect_pose_state(shape, schema);
            }
        }
    }
}

pub fn collect_pose_state(d: &DynPose, schema: &mut MotionStateSchema) {
    collect_node_state(d.node(), schema);
}

fn seed_reader_figure_nodes(d: &DynFigure, readers: &MotionReaders) {
    match d.repr() {
        FigureDynRepr::Pose(pose) => seed_reader_pose_nodes(pose, readers),
        FigureDynRepr::Curve { frame, curve } => {
            seed_reader_pose_nodes(frame, readers);
            if let CurveEval::Expr(shape) = &curve.eval {
                seed_reader_pose_nodes(shape, readers);
            }
        }
    }
}

fn seed_reader_pose_nodes(d: &DynPose, readers: &MotionReaders) {
    let mut node_ids = readers.node_ids.borrow_mut();
    let mut next = node_ids.values().map(|id| id.0).max().map(|id| id + 1).unwrap_or(0);
    seed_reader_node_ids(d.node(), &mut node_ids, &mut next);
}

fn seed_reader_dyn_node_ids(d: &DynNode, readers: &MotionReaders) {
    let mut node_ids = readers.node_ids.borrow_mut();
    let mut next = node_ids.values().map(|id| id.0).max().map(|id| id + 1).unwrap_or(0);
    seed_dyn_node_ids(d, &mut node_ids, &mut next);
}

// These pointer keys are local to one immutable motion tree/schema; they are
// never row-cache or cross-spec identity.
fn seed_reader_node_ids(
    node: &Rc<DynNode>,
    node_ids: &mut FxHashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    let ptr = Rc::as_ptr(node) as usize;
    seed_dyn_node_ids_with_ptr(&**node, ptr, node_ids, next);
}

fn seed_dyn_node_ids(
    node: &DynNode,
    node_ids: &mut FxHashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    let ptr = node as *const DynNode as usize;
    seed_dyn_node_ids_with_ptr(node, ptr, node_ids, next);
}

fn seed_dyn_node_ids_with_ptr(
    node: &DynNode,
    ptr: usize,
    node_ids: &mut FxHashMap<usize, MotionNodeId>,
    next: &mut u32,
) {
    if let std::collections::hash_map::Entry::Vacant(entry) = node_ids.entry(ptr) {
        entry.insert(MotionNodeId(*next));
        *next += 1;
    }
    match node {
        DynNode::Path { curve, .. } => seed_reader_node_ids(curve, node_ids, next),
        DynNode::Stages { segs } => {
            for seg in segs {
                // slot before child: must mirror collect_node_state's order
                // so lowered ids line up between schema and readers
                if let Some(slot) = &seg.exit_slot {
                    let slot_ptr = Rc::as_ptr(slot) as usize;
                    if let std::collections::hash_map::Entry::Vacant(entry) = node_ids.entry(slot_ptr) {
                        entry.insert(MotionNodeId(*next));
                        *next += 1;
                    }
                }
                let StageMake::Ready(d) = &seg.make;
                seed_reader_node_ids(d.node(), node_ids, next);
            }
        }
        DynNode::Translate { child, .. }
        | DynNode::Clamp { child, .. }
        | DynNode::RowFrame(child) => {
            seed_reader_node_ids(child, node_ids, next);
        }
        DynNode::Frame(a, b) => {
            seed_reader_node_ids(a, node_ids, next);
            seed_reader_node_ids(b, node_ids, next);
        }
        DynNode::ConstFrame { child, .. } => {
            seed_reader_node_ids(child, node_ids, next);
        }
        DynNode::Const(_)
        | DynNode::Linear { .. }
        | DynNode::Live { .. }
        | DynNode::LiveStream { .. }
        | DynNode::StockIntegrator { .. }
        | DynNode::ClosedPt { .. }
        | DynNode::FnPose(_)
        | DynNode::Evolve(_)
        | DynNode::RotExpr { .. } => {}
    }
}

pub fn collect_node_state(node: &Rc<DynNode>, schema: &mut MotionStateSchema) {
    let base = Rc::as_ptr(node) as usize;
    let node_id = schema.intern_node(base);
    match &**node {
        DynNode::StockIntegrator { data } => {
            schema.intern_n2(MotionStateKey::Node(node_id));
            let index = collect_scan_sites(&data.a, node_id, 0, schema);
            collect_scan_sites(&data.b, node_id, index, schema);
        }
        DynNode::ClosedPt { a, b, .. } => {
            let index = collect_scan_sites(a, node_id, 0, schema);
            collect_scan_sites(b, node_id, index, schema);
        }
        DynNode::RotExpr { form, .. } => {
            collect_scan_sites(form, node_id, 0, schema);
        }
        DynNode::Path { curve, progress, .. } => {
            collect_scan_sites(progress, node_id, 0, schema);
            collect_node_state(curve, schema);
        }
        DynNode::Stages { segs } => {
            schema.intern_n2(MotionStateKey::Node(node_id));
            for seg in segs {
                if let StageTerm::Until(pred, _) = &seg.term {
                    collect_scan_sites(pred, node_id, 0, schema);
                }
                if let Some(slot) = &seg.exit_slot {
                    let base = schema.intern_node(Rc::as_ptr(slot) as usize);
                    schema.intern_n2(MotionStateKey::StageExit { base, field: StageExitField::Pos });
                    schema.intern_n2(MotionStateKey::StageExit { base, field: StageExitField::Vel });
                }
                let StageMake::Ready(d) = &seg.make;
                collect_pose_state(d, schema);
            }
        }
        DynNode::Translate { child, .. }
        | DynNode::Clamp { child, .. }
        | DynNode::RowFrame(child) => {
            collect_node_state(child, schema);
        }
        DynNode::Frame(a, b) => {
            collect_node_state(a, schema);
            collect_node_state(b, schema);
        }
        DynNode::ConstFrame { child, .. } => {
            collect_node_state(child, schema);
        }
        DynNode::Evolve(_) => {
            schema.intern_val(MotionStateKey::Node(node_id));
        }
        DynNode::Const(_)
        | DynNode::Linear { .. }
        | DynNode::Live { .. }
        | DynNode::LiveStream { .. }
        | DynNode::FnPose(_) => {}
    }
}

pub fn collect_scan_sites(
    form: &Form,
    base: MotionNodeId,
    start_index: u32,
    schema: &mut MotionStateSchema,
) -> u32 {
    match form {
        Form::List(items) => {
            let mut index = start_index;
            if let Some(Form::Sym(s)) = items.first() {
                if &**s == "evolve" {
                    // a sited evolve: expression-embedded stateful signal,
                    // Val state at the site (evolve-design.md, sited evolves)
                    schema.intern_val(MotionStateKey::ScanSite { base, index });
                    index += 1;
                }
            }
            for item in items.iter() {
                index = collect_scan_sites(item, base, index, schema);
            }
            index
        }
        Form::Vector(items) => items
            .iter()
            .fold(start_index, |index, item| collect_scan_sites(item, base, index, schema)),
        Form::Map(kvs) => kvs.iter().fold(start_index, |index, (k, v)| {
            let index = collect_scan_sites(k, base, index, schema);
            collect_scan_sites(v, base, index, schema)
        }),
        _ => start_index,
    }
}

/// Number of stateful sites a form occupies in scan-site index order.
/// MUST mirror collect_scan_sites' walk exactly: sited-evolve evaluation
/// uses it to fast-forward the counter over skipped init/step regions so
/// every evaluation consumes the same index range the static walk saw.
pub(crate) fn form_site_count(form: &Form) -> u32 {
    match form {
        Form::List(items) => {
            let own = match items.first() {
                Some(Form::Sym(s)) if &**s == "evolve" => 1,
                _ => 0,
            };
            own + items.iter().map(form_site_count).sum::<u32>()
        }
        Form::Vector(items) => items.iter().map(form_site_count).sum(),
        Form::Map(kvs) => kvs.iter().map(|(k, v)| form_site_count(k) + form_site_count(v)).sum(),
        _ => 0,
    }
}

impl DynEval for f64 {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<f64>,
        tau: f64,
        _state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<f64, String> {
        match d.repr() {
            NumDynRepr::Const(n) => Ok(*n),
            NumDynRepr::Expr { form, env } => eval_sig_at_rate(form, env, sig, tau, 0.0, None, None, tick_rate)?.num(),
            NumDynRepr::AxisSel { .. } => {
                Err("axis-selected signal requires an entity row".into())
            }
        }
    }
}

impl DynEval for Figure {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<Figure>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<Figure, String> {
        match d.repr() {
            FigureDynRepr::Pose(p) => Ok(Figure::Pose(dyn_pose_with_tick_rate(p, tau, state, sig, tick_rate)?)),
            FigureDynRepr::Curve { frame, curve } => Ok(Figure::Curve(Curve {
                frame: dyn_pose_with_tick_rate(frame, tau, state, sig, tick_rate)?,
                spec: curve.clone(),
            })),
        }
    }
}

impl DynEval for Pose {
    fn eval_dyn_with_tick_rate(
        d: &Dyn<Pose>,
        tau: f64,
        state: &MotionState,
        sig: &SigEnv,
        tick_rate: f64,
    ) -> Result<Pose, String> {
        dyn_node_pose_with_tick_rate(d.node(), tau, state, sig, tick_rate)
    }
}

pub fn eval_curve_pose(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    eval_curve_pose_with_tick_rate(eval, tau, u, state, sig, TickTiming::default().rate())
}

pub fn eval_curve_pose_with_tick_rate(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    let readers = match eval {
        CurveEval::Straight => MotionReaders::legacy(),
        CurveEval::Expr(d) => MotionReaders::for_pose(d),
    };
    eval_curve_pose_in(
        eval,
        tau,
        u,
        MotionEvalCtx::with_tick_rate(state, sig, &readers, tick_rate),
    )
}

pub fn eval_curve_pose_in(
    eval: &CurveEval,
    tau: f64,
    u: f64,
    ctx: MotionEvalCtx<'_>,
) -> Result<Pose, String> {
    match eval {
        CurveEval::Straight => Ok(Pose::oriented(u, 0.0, 0.0)),
        CurveEval::Expr(d) => dyn_node_pose_u_in(d.node(), tau, u, ctx),
    }
}

/// Compatibility extended values before spawn lowering.
#[derive(Debug, Clone)]
pub enum CurveBacking {
    /// Surface `curve` syntax lowers to this representation.
    Parametric {
        curve: ParametricCurve,
    },
    /// Surface `pather` syntax currently lowers to a pose entity with a
    /// legacy trace cache enabled.
    Trace { window: f64 },
}

#[derive(Debug)]
pub struct ExtCurve {
    pub anchor: DynPose,
    pub backing: CurveBacking,
}

pub fn eval_sig(
    form: &Form,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
) -> Result<Val, String> {
    eval_sig_at_rate(form, env, sig, tau, u, scan, pos, TickTiming::default().rate())
}

pub fn eval_sig_at_rate(
    form: &Form,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
    tick_rate: f64,
) -> Result<Val, String> {
    let probe = crate::interp::profile::enabled().then(crate::interp::profile::open);
    let mut e = env.bind("t".into(), Val::Num(tau)).bind("u".into(), Val::Num(u));
    if let Some((px, py)) = pos {
        e = e.bind("pos".into(), Val::Pose(Pose::point(px, py)));
    }
    let mut ctx = Ctx {
        sig: sig.clone(),
        ambient: Pose::IDENTITY,
        overrides: None,
        scan,
        patterns: Rc::new(HashMap::new()),
        macros: Rc::new(HashMap::new()),
        deferred: Vec::new(),
        projector_scope: None,
    };
    let mut w = World::for_eval(tick_rate); // signals never touch the world (§2)
    if let Some(f) = probe {
        crate::interp::profile::close("sig:setup", f);
    }
    evaluate(form, &e, &mut ctx, &mut w)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_pt_at_rate(
    a: &Form,
    b: &Form,
    polar: bool,
    env: &Env,
    sig: &SigEnv,
    tau: f64,
    u: f64,
    scan: Option<ScanShared>,
    pos: Option<(f64, f64)>,
    tick_rate: f64,
) -> Result<(f64, f64), String> {
    let av = eval_sig_at_rate(a, env, sig, tau, u, scan.clone(), pos, tick_rate)?.num()?;
    let bv = eval_sig_at_rate(b, env, sig, tau, u, scan, pos, tick_rate)?.num()?;
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        Ok((av * c, av * s))
    } else {
        Ok((av, bv))
    }
}

fn eval_motion_program_pair(
    a: &MotionProgram,
    b: &MotionProgram,
    polar: bool,
    tau: f64,
    u: f64,
    pos: Option<(f64, f64)>,
    caps: &[f64],
    aux_a: &[f64],
    aux_b: &[f64],
) -> (f64, f64) {
    let av = run_num_program_caps(a.backend(), tau, u, pos, caps, aux_a);
    let bv = run_num_program_caps(b.backend(), tau, u, pos, caps, aux_b);
    if polar {
        let (s, c) = bv.to_radians().sin_cos();
        (av * c, av * s)
    } else {
        (av, bv)
    }
}

fn lower_motion_program_pair(
    a: &Form,
    b: &Form,
    env: &Env,
    sig: &SigEnv,
    allow_pos: bool,
    allow_aux: bool,
) -> Option<(Rc<MotionProgram>, Rc<MotionProgram>)> {
    let ap = lower_num_form_opts(
        a,
        env,
        &sig.defs,
        LowerOpts { allow_aux, ..LowerOpts::default() },
    )?;
    let bp = lower_num_form_opts(
        b,
        env,
        &sig.defs,
        LowerOpts { allow_aux, site_base: form_site_count(a), ..LowerOpts::default() },
    )?;
    if !allow_pos && (program_uses_pos(&ap) || program_uses_pos(&bp)) {
        return None;
    }
    Some((attach_motion_program(ap)?, attach_motion_program(bp)?))
}

fn lower_motion_program(
    form: &Form,
    env: &Env,
    sig: &SigEnv,
    allow_pos: bool,
    allow_aux: bool,
) -> Option<Rc<MotionProgram>> {
    let prog = lower_num_form_opts(
        form,
        env,
        &sig.defs,
        LowerOpts { allow_aux, ..LowerOpts::default() },
    )?;
    if !allow_pos && program_uses_pos(&prog) {
        return None;
    }
    attach_motion_program(prog)
}

/// Fill a program's aux slice: scan cells through the row's readers (and
/// the legacy state map), channels/streams through the eval's SigEnv — the
/// same sources the interpreter reads. False = a value is missing or
/// mistyped (evolve state that isn't a Num, a channel whose runtime kind
/// differs from the consumed kind, an unprovided channel): the caller
/// reruns interpreted, which reproduces the interpreter's own result or
/// error exactly.
fn fetch_aux(
    prog: &MotionProgram,
    node_ptr: usize,
    state: &MotionState,
    sig: &SigEnv,
    readers: &MotionReaders,
    buf: &mut Vec<f64>,
) -> bool {
    let prog = prog.backend();
    buf.clear();
    let Some(aux) = prog.aux.as_deref() else {
        return true;
    };
    let mut chan_vals: Vec<(f64, f64)> = Vec::with_capacity(aux.chans.len());
    for (r, kind) in &aux.chans {
        let v = match r {
            crate::interp::lower::ChanRef::Stream(id) => sig.stream_val(*id),
            crate::interp::lower::ChanRef::Named(n) => match sig.stream_id(n) {
                Some(id) => sig.stream_val(id),
                None => sig.channel(n),
            },
        };
        match (kind, v) {
            (crate::interp::lower::ChanKind::Num, Some(Val::Num(x))) => chan_vals.push((x, 0.0)),
            (crate::interp::lower::ChanKind::Pose, Some(Val::Pose(p))) => chan_vals.push((p.x, p.y)),
            _ => return false,
        }
    }
    let MotionStateKey::Node(base) = state_key_for_node(node_ptr, readers) else {
        unreachable!("node keys are always stable")
    };
    for slot in &aux.slots {
        let v = match slot {
            crate::interp::lower::AuxSlot::Scan(index) => {
                let site = MotionStateKey::ScanSite { base, index: *index };
                let cell = readers.vals(site).or_else(|| match state.get(&site) {
                    Some(Cell::V(c)) => Some(c.clone()),
                    _ => None,
                });
                match cell {
                    Some(EvolveCell { state: Val::Num(v), .. }) => v,
                    _ => return false,
                }
            }
            crate::interp::lower::AuxSlot::ChanX(c) => chan_vals[*c as usize].0,
            crate::interp::lower::AuxSlot::ChanY(c) => chan_vals[*c as usize].1,
        };
        buf.push(v);
    }
    true
}

thread_local! {
    /// Aux scratch for the pair-eval driver (a, then b — sequential runs).
    static AUX_A: std::cell::RefCell<Vec<f64>> = const { std::cell::RefCell::new(Vec::new()) };
    static AUX_B: std::cell::RefCell<Vec<f64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// One row's batchable motion step (milestone B): a point figure whose pose
/// tree is constant wrappers over a single compiled-integrand Vel node.
/// Stepping such a row is exactly `state += programs(tau, pos=state)·dt` —
/// no scan sites, no other state cells — so rows sharing the program pair
/// can run as lanes of one batched program run.
#[derive(Clone)]
pub struct VelStepPlan {
    /// The stock node itself: its lowered id keys the state slots, and the
    /// oracle re-runs its components through the interpreter.
    pub vel: Rc<DynNode>,
    pub ap: Rc<MotionProgram>,
    pub bp: Rc<MotionProgram>,
    pub polar: bool,
    pub columns: [ColName; 2],
}

/// A borrowed classification — the per-row scan only clones the Rcs when a
/// lane opens a new group, not per row. `caps` is the row's capture vector
/// (rand draws), one lane's slice of the group's caps at stride n_inputs.
pub struct VelStepPlanRef<'a> {
    pub vel: &'a Rc<DynNode>,
    pub ap: &'a Rc<MotionProgram>,
    pub bp: &'a Rc<MotionProgram>,
    pub polar: bool,
    pub columns: [ColName; 2],
    pub caps: &'a [f32],
}

impl VelStepPlanRef<'_> {
    pub fn to_plan(&self) -> VelStepPlan {
        VelStepPlan {
            vel: self.vel.clone(),
            ap: self.ap.clone(),
            bp: self.bp.clone(),
            polar: self.polar,
            columns: self.columns,
        }
    }
}

pub fn vel_step_plan<'a>(
    fig: &'a DynFigure,
    sig: &SigEnv,
    capture_layout: Option<&'a CaptureLayout>,
    captures: &'a [f32],
) -> Option<VelStepPlanRef<'a>> {
    if fig.curve().is_some() {
        return None;
    }
    let mut node = fig.pose_dyn();
    loop {
        match &**node {
            DynNode::ConstFrame { child, .. }
            | DynNode::Translate { child, .. }
            | DynNode::RowFrame(child) => node = child,
            DynNode::StockIntegrator { data } => {
                let extracted = match data.rand.as_deref() {
                    Some(RandCell::Compiled(ex)) => Some(ex),
                    _ => None,
                };
                let (ap, bp) = match extracted {
                    Some(ex) => (&ex.programs[0], &ex.programs[1]),
                    None => data.programs
                        .get_or_init(|| lower_motion_program_pair(&data.a, &data.b, &data.env, sig, true, true))
                        .as_ref()
                        .map(|(ap, bp)| (ap, bp))?,
                };
                // aux programs never batch: the step must run the
                // interpreted scan advance, and channel fetches are
                // per-row driver work
                if !ap.aux_free() || !bp.aux_free() {
                    return None;
                }
                return Some(VelStepPlanRef {
                    vel: node,
                    ap,
                    bp,
                    polar: data.source_is_polar(),
                    columns: data.columns,
                    caps: capture_layout
                        .and_then(|layout| layout.range(node))
                        .map(|range| &captures[range.offset..range.offset + range.len])
                        .unwrap_or_else(|| stored_caps_of(&data.rand)),
                });
            }
            _ => return None,
        }
    }
}

/// One row's batchable pos-only POSE fill (milestone B): a point figure
/// whose pose tree is constant wrappers over a single compiled aux-free
/// ClosedPt node. Its pos-only pose is one program-pair sample at
/// (tau, u=0) — no state, no readers — pushed through the wrappers, so
/// rows sharing the interned pair evaluate as lanes of one batched run.
pub struct ClosedChainRef<'a> {
    /// The figure's root pose node: wrapper composition (and the oracle)
    /// walk from here.
    pub root: &'a Rc<DynNode>,
    pub ap: &'a Rc<MotionProgram>,
    pub bp: &'a Rc<MotionProgram>,
    pub polar: bool,
    pub caps: &'a [f32],
}

pub fn closed_chain_plan<'a>(
    fig: &'a DynFigure,
    sig: &SigEnv,
    capture_layout: Option<&'a CaptureLayout>,
    captures: &'a [f32],
) -> Option<ClosedChainRef<'a>> {
    if fig.curve().is_some() {
        return None;
    }
    let root = fig.pose_dyn();
    let mut node = root;
    loop {
        match &**node {
            DynNode::ConstFrame { child, .. }
            | DynNode::Translate { child, .. }
            | DynNode::RowFrame(child) => node = child,
            DynNode::ClosedPt { a, b, polar, env, programs, rand } => {
                let extracted = match rand.as_deref() {
                    Some(RandCell::Compiled(ex)) => Some(ex),
                    _ => None,
                };
                let (ap, bp) = match extracted {
                    Some(ex) => (&ex.programs[0], &ex.programs[1]),
                    None => programs
                        .get_or_init(|| lower_motion_program_pair(a, b, env, sig, false, false))
                        .as_ref()
                        .map(|(ap, bp)| (ap, bp))?,
                };
                let caps = capture_layout
                    .and_then(|layout| layout.range(node))
                    .map(|range| &captures[range.offset..range.offset + range.len])
                    .unwrap_or_else(|| stored_caps_of(rand));
                return Some(ClosedChainRef { root, ap, bp, polar: *polar, caps });
            }
            _ => return None,
        }
    }
}

/// The fast pos_only pose shape: constant wrappers over a Vel node
/// (compiled or not — the pos_only Vel arm never evaluates its integrand,
/// so the pose is pure integrator state pushed through the wrappers).
/// Returns the Vel node's spec-internal address for state-slot resolution;
/// row hot paths memoize the resulting slot by generational SpecId.
pub fn vel_chain_ptr(fig: &DynFigure) -> Option<usize> {
    if fig.curve().is_some() {
        return None;
    }
    let mut node = fig.pose_dyn();
    loop {
        match &**node {
            DynNode::ConstFrame { child, .. }
            | DynNode::Translate { child, .. }
            | DynNode::RowFrame(child) => node = child,
            DynNode::StockIntegrator { .. } => return Some(Rc::as_ptr(node) as usize),
            _ => return None,
        }
    }
}

/// pos_only pose of a wrapper-chain-over-Vel figure given the Vel's
/// integrator state: the same arms `dyn_node_pose_u_in` takes with
/// need_theta=false, minus the readers/dispatch scaffolding.
pub fn wrapper_chain_pos_pose(
    node: &Rc<DynNode>,
    state: [f64; 2],
    row_frame: Option<Pose>,
) -> Pose {
    match &**node {
        DynNode::ConstFrame { pose, rot, child } => {
            pose.compose_with_rot(*rot, &wrapper_chain_pos_pose(child, state, row_frame))
        }
        DynNode::Translate { dx, dy, child } => {
            let p = wrapper_chain_pos_pose(child, state, row_frame);
            Pose { x: p.x + dx, y: p.y + dy, theta: p.theta }
        }
        DynNode::RowFrame(child) => row_frame
            .unwrap_or(Pose::IDENTITY)
            .compose(&wrapper_chain_pos_pose(child, state, row_frame)),
        _ => Pose::point(state[0], state[1]),
    }
}

/// Batch-path oracle: re-run one lane's integrand through the interpreter,
/// the same check `step_motion_in`'s compiled Vel arm makes per row.
pub fn oracle_check_vel_step(
    vel: &DynNode,
    captures: &[f64],
    tau: f64,
    dt: f64,
    pos: (f64, f64),
    got: (f64, f64),
    sig: &SigEnv,
    readers: &MotionReaders,
    tick_rate: f64,
) -> Result<(), String> {
    let DynNode::StockIntegrator { data } = vel else {
        return Ok(());
    };
    let extracted = match data.rand.as_deref() {
        Some(RandCell::Compiled(ex)) if !captures.is_empty() => Some(ex),
        _ => None,
    };
    let (source_a, source_b) = extracted
        .map(|ex| (&ex.forms[0], &ex.forms[1]))
        .unwrap_or((&data.a, &data.b));
    let stored_caps;
    let caps = if captures.is_empty() {
        stored_caps = caps_of(&data.rand);
        stored_caps.as_slice()
    } else {
        captures
    };
    let (a, b) = &oracle_forms(source_a, source_b, &caps);
    let env = capture_env(&data.env, extracted, &data.rand, caps);
    let key = vel as *const DynNode as usize;
    let mut state = MotionState::default();
    let ((ivx, ivy), _) = advance_sites_with_writes(&mut state, key, dt, readers.clone(), false, |scan| {
        eval_pt_at_rate(a, b, data.source_is_polar(), &env, sig, tau, 0.0, Some(scan), Some(pos), tick_rate)
    })?;
    assert_num_close("vel-batch/a", a, got.0, ivx);
    assert_num_close("vel-batch/b", b, got.1, ivy);
    Ok(())
}

/// Oracle bridge for marker forms: an interpreter re-run needs the entity's
/// drawn capture values substituted back in (no-op clone when cap-free).
fn oracle_form(f: &Form, caps: &[f64]) -> Form {
    if caps.is_empty() { f.clone() } else { super::spawn::subst_captures(f, caps) }
}

fn oracle_forms(a: &Form, b: &Form, caps: &[f64]) -> (Form, Form) {
    (oracle_form(a, caps), oracle_form(b, caps))
}

fn capture_env(
    env: &Env,
    extracted: Option<&ExtractedSig>,
    rand: &Option<Rc<RandCell>>,
    caps: &[f64],
) -> Env {
    let (names, offset) = match extracted {
        Some(ex) => (ex.env_names.as_ref(), ex.sites.len()),
        None => match rand.as_deref() {
            Some(RandCell::Caps(data)) => (data.env_names.as_ref(), data.env_offset),
            _ => return env.clone(),
        },
    };
    names.iter().zip(caps.iter().skip(offset)).fold(env.clone(), |env, (name, value)| {
        env.bind(name.clone(), Val::Num(*value))
    })
}

fn assert_num_close(label: &str, form: &Form, got: f64, expected: f64) {
    assert!(
        (got - expected).abs() <= 1e-9,
        "{} compiled/interpreted mismatch for {:?}: compiled={}, interpreted={}",
        label,
        form,
        got,
        expected
    );
}

/// Read-only scan context over a clone of the bullet's state.
pub(crate) fn read_scan_in(
    state: &MotionState,
    base: usize,
    readers: MotionReaders,
) -> ScanShared {
    let MotionStateKey::Node(dense_base) = state_key_for_node(base, &readers) else {
        unreachable!("node keys are always stable")
    };
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base,
        dense_base: Some(dense_base),
        counter: 0,
        advance: false,
        dt: 0.0,
        readers: Some(readers),
        mirror_legacy: false,
        n2_writes: Vec::new(),
        val_writes: Vec::new(),
    }))
}

pub(crate) fn read_scan(state: &MotionState, base: MotionNodeId) -> ScanShared {
    Rc::new(std::cell::RefCell::new(ScanIo {
        state: state.clone(),
        base: 0,
        dense_base: Some(base),
        counter: 0,
        advance: false,
        dt: 0.0,
        readers: None,
        mirror_legacy: false,
        n2_writes: Vec::new(),
        val_writes: Vec::new(),
    }))
}

pub fn dyn_node_pose(d: &DynNode, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_node_pose_u_in(d, tau, 0.0, ctx)
}

pub fn dyn_node_pose_with_tick_rate(
    d: &DynNode,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    dyn_node_pose_u_with_tick_rate(d, tau, 0.0, state, sig, tick_rate)
}

pub fn dyn_pose(d: &DynPose, tau: f64, state: &MotionState, sig: &SigEnv) -> Result<Pose, String> {
    let readers = MotionReaders::for_pose(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_pose_in(d, tau, ctx)
}

pub fn dyn_pose_with_tick_rate(
    d: &DynPose,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_pose(d);
    let ctx = MotionEvalCtx::with_tick_rate(state, sig, &readers, tick_rate);
    dyn_pose_in(d, tau, ctx)
}

pub fn dyn_figure_pose(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_figure(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_figure_pose_in(d, tau, ctx)
}

pub fn dyn_figure_pose_in(d: &DynFigure, tau: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    dyn_node_pose_u_in(d.pose_dyn(), tau, 0.0, ctx)
}

pub fn eval_dyn_figure(
    d: &DynFigure,
    tau: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Figure, String> {
    match d.repr() {
        FigureDynRepr::Pose(p) => Ok(Figure::Pose(dyn_pose(p, tau, state, sig)?)),
        FigureDynRepr::Curve { frame, curve } => Ok(Figure::Curve(Curve {
            frame: dyn_pose(frame, tau, state, sig)?,
            spec: curve.clone(),
        })),
    }
}

pub fn dyn_pose_u(
    d: &DynPose,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    dyn_node_pose_u(d.node(), tau, u, state, sig)
}

pub fn dyn_pose_in(d: &DynPose, tau: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    dyn_node_pose_u_in(d.node(), tau, 0.0, ctx)
}

pub fn dyn_node_pose_u(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::new(state, sig, &readers);
    dyn_node_pose_u_in(d, tau, u, ctx)
}

pub fn dyn_node_pose_u_with_tick_rate(
    d: &DynNode,
    tau: f64,
    u: f64,
    state: &MotionState,
    sig: &SigEnv,
    tick_rate: f64,
) -> Result<Pose, String> {
    let readers = MotionReaders::for_node(d);
    let ctx = MotionEvalCtx::with_tick_rate(state, sig, &readers, tick_rate);
    dyn_node_pose_u_in(d, tau, u, ctx)
}

pub fn dyn_node_pose_u_in(d: &DynNode, tau: f64, u: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    if crate::interp::profile::enabled() {
        let frame = crate::interp::profile::open();
        let r = dyn_node_pose_u_in_inner(d, tau, u, ctx);
        crate::interp::profile::close(dyn_node_name(d), frame);
        return r;
    }
    dyn_node_pose_u_in_inner(d, tau, u, ctx)
}

fn dyn_node_name(d: &DynNode) -> &'static str {
    match d {
        DynNode::Const(_) => "dyn:const",
        DynNode::Linear { .. } => "dyn:linear",
        DynNode::ClosedPt { programs, .. } => match programs.get() {
            Some(Some(_)) => "dyn:closed-pt-c",
            _ => "dyn:closed-pt",
        },
        DynNode::StockIntegrator { data } => match data.programs.get() {
            Some(Some(_)) => "dyn:stock-integrator-c",
            _ => "dyn:stock-integrator",
        },
        DynNode::Translate { .. } => "dyn:translate",
        DynNode::Path { .. } => "dyn:path",
        DynNode::Frame(..) | DynNode::ConstFrame { .. } | DynNode::RowFrame(..) => "dyn:frame",
        DynNode::Live { .. } | DynNode::LiveStream { .. } => "dyn:live",
        DynNode::Clamp { .. } => "dyn:clamp",
        DynNode::RotExpr { .. } => "dyn:rot-expr",
        DynNode::FnPose(_) => "dyn:fn-pose",
        DynNode::Evolve(_) => "dyn:evolve",
        DynNode::Stages { .. } => "dyn:stages",
    }
}

fn dyn_node_pose_u_in_inner(d: &DynNode, tau: f64, u: f64, ctx: MotionEvalCtx<'_>) -> Result<Pose, String> {
    let state = ctx.state;
    // Scoped overrides ride ctx.sig (SigEnv.overrides): every read below —
    // stream_val, eval_sig, evolve steps, lowered aux — resolves through
    // them with no threading and no per-node work.
    let sig = ctx.sig;
    let readers = ctx.readers;
    let tick_rate = ctx.tick_rate;
    match d {
        DynNode::Const(p) => Ok(*p),
        DynNode::Linear { vx, vy } => Ok(Pose {
            x: vx * tau,
            y: vy * tau,
            theta: Some(vy.atan2(*vx).to_degrees()),
        }),
        DynNode::ClosedPt { a, b, polar, env, programs, rand } => {
            let key = d as *const DynNode as usize;
            let extracted = ctx.compiled_for(d, rand);
            let caps = ctx.caps_for(d, rand);
            let captured_env = capture_env(env, extracted, rand, &caps);
            let source_a = extracted.map(|ex| &ex.forms[0]).unwrap_or(a);
            let source_b = extracted.map(|ex| &ex.forms[1]).unwrap_or(b);
            let lowered = extracted
                .map(|ex| Some((&ex.programs[0], &ex.programs[1])))
                .unwrap_or_else(|| programs
                    .get_or_init(|| lower_motion_program_pair(a, b, env, sig, false, false))
                    .as_ref()
                    .map(|(ap, bp)| (ap, bp)));
            if let Some((ap, bp)) = lowered {
                let (x, y) = eval_motion_program_pair(ap, bp, *polar, tau, u, None, &caps, &[], &[]);
                if !ctx.need_theta {
                    if oracle_enabled() {
                        let (a, b) = &oracle_forms(source_a, source_b, &caps);
                        let (ix, iy) = eval_pt_at_rate(
                            a,
                            b,
                            *polar,
                            &captured_env,
                            sig,
                            tau,
                            u,
                            Some(read_scan_in(state, key, readers.clone())),
                            None,
                            tick_rate,
                        )?;
                        assert_num_close("closed-pt/a", a, x, ix);
                        assert_num_close("closed-pt/b", b, y, iy);
                    }
                    return Ok(Pose::point(x, y));
                }
                let eps = 1.0 / tick_rate;
                let (x2, y2) = eval_motion_program_pair(ap, bp, *polar, tau + eps, u, None, &caps, &[], &[]);
                if oracle_enabled() {
                    let (a, b) = &oracle_forms(source_a, source_b, &caps);
                    let (ix, iy) = eval_pt_at_rate(
                        a,
                        b,
                        *polar,
                        &captured_env,
                        sig,
                        tau,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        None,
                        tick_rate,
                    )?;
                    let (ix2, iy2) = eval_pt_at_rate(
                        a,
                        b,
                        *polar,
                        &captured_env,
                        sig,
                        tau + eps,
                        u,
                        Some(read_scan_in(state, key, readers.clone())),
                        None,
                        tick_rate,
                    )?;
                    assert_num_close("closed-pt/a", a, x, ix);
                    assert_num_close("closed-pt/b", b, y, iy);
                    assert_num_close("closed-pt/a+eps", a, x2, ix2);
                    assert_num_close("closed-pt/b+eps", b, y2, iy2);
                }
                return Ok(Pose::oriented(x, y, (y2 - y).atan2(x2 - x).to_degrees()));
            }
            let (x, y) = eval_pt_at_rate(
                source_a,
                source_b,
                *polar,
                &captured_env,
                sig,
                tau,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?;
            if !ctx.need_theta {
                return Ok(Pose::point(x, y));
            }
            let eps = 1.0 / tick_rate;
            let (x2, y2) = eval_pt_at_rate(
                source_a,
                source_b,
                *polar,
                &captured_env,
                sig,
                tau + eps,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?;
            Ok(Pose::oriented(x, y, (y2 - y).atan2(x2 - x).to_degrees()))
        }
        DynNode::StockIntegrator { data } => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let [x, y] = readers.n2(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            if !ctx.need_theta {
                return Ok(Pose::point(x, y));
            }
            let component_key = integrator_component_key(key, readers);
            let [vx, vy] = match (
                readers.component(data.columns[0]),
                readers.component(data.columns[1]),
            ) {
                (Some(vx), Some(vy)) => [vx, vy],
                _ => match state.get(&component_key) {
                    Some(Cell::N(v)) => *v,
                    _ => [0.0, 0.0],
                },
            };
            Ok(Pose::oriented(x, y, vy.atan2(vx).to_degrees()))
        }
        DynNode::Live { channel } => {
            let (x, y) = sig.channel_pos(channel);
            Ok(Pose::point(x, y))
        }
        DynNode::LiveStream { id } => match sig.stream_val(*id) {
            Some(Val::Pose(p)) => Ok(Pose::point(p.x, p.y)),
            _ => Ok(Pose::point(0.0, 0.0)),
        },
        DynNode::Clamp { lo, hi, child } => {
            let p = dyn_node_pose_u_in(child, tau, 0.0, ctx)?;
            Ok(Pose { x: p.x.clamp(lo.0, hi.0), y: p.y.clamp(lo.1, hi.1), theta: p.theta })
        }
        DynNode::RotExpr { form, env, program, rand } => {
            if !ctx.need_theta {
                // a rot-expr's pose IS its theta; nothing else to compute
                return Ok(Pose::point(0.0, 0.0));
            }
            let key = d as *const DynNode as usize;
            let extracted = ctx.compiled_for(d, rand);
            let caps = ctx.caps_for(d, rand);
            let captured_env = capture_env(env, extracted, rand, &caps);
            let source_form = extracted.map(|ex| &ex.forms[0]).unwrap_or(form);
            let lowered = extracted
                .map(|ex| Some(&ex.programs[0]))
                .unwrap_or_else(|| program
                    .get_or_init(|| lower_motion_program(form, env, sig, true, true))
                    .as_ref());
            if let Some(prog) = lowered {
                let fetched = AUX_A.with(|aa| {
                    let mut aa = aa.borrow_mut();
                    fetch_aux(prog, key, state, sig, readers, &mut aa)
                        .then(|| run_num_program_caps(prog.backend(), tau, u, Some((0.0, 0.0)), &caps, &aa))
                });
                if let Some(th) = fetched {
                    if oracle_enabled() {
                        let form = &oracle_form(source_form, &caps);
                        let ith = eval_sig_at_rate(
                            form,
                            &captured_env,
                            sig,
                            tau,
                            u,
                            Some(read_scan_in(state, key, readers.clone())),
                            Some((0.0, 0.0)),
                            tick_rate,
                        )?
                        .num()?;
                        assert_num_close("rot-expr", form, th, ith);
                    }
                    return Ok(Pose::oriented(0.0, 0.0, th));
                }
            }
            let th = eval_sig_at_rate(
                source_form,
                &captured_env,
                sig,
                tau,
                u,
                Some(read_scan_in(state, key, readers.clone())),
                Some((0.0, 0.0)),
                tick_rate,
            )?
            .num()?;
            Ok(Pose::oriented(0.0, 0.0, th))
        }
        DynNode::FnPose(f) => {
            let mut call_ctx = Ctx {
                sig: sig.clone(),
                ambient: Pose::IDENTITY,
                overrides: None,
                scan: None,
                patterns: Rc::new(HashMap::new()),
                macros: Rc::new(HashMap::new()),
                deferred: Vec::new(),
                projector_scope: None,
                    };
            let mut w = World::for_eval(tick_rate);
            match apply_fn(f.clone(), &[Val::Num(tau)], &mut call_ctx, &mut w, false)? {
                Val::Pose(p) => Ok(p),
                Val::DynPose(d) => dyn_pose_with_tick_rate(&d, tau, state, sig, tick_rate),
                other => Err(format!("fn-backed dyn expected fn to return pose, got {:?}", other)),
            }
        }
        DynNode::Evolve(ev) => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let tick = evolve_tick(tau, tick_rate);
            let cell = readers.vals(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::V(v)) => Some(v.clone()),
                    _ => None,
                });
            let value = match cell {
                Some(cell) if cell.tick == tick => cell.state,
                // Post-boundary window: sampling after the world tick
                // increments, before the new boundary's step pass runs. A
                // live evolve cannot replay, and its settled state IS the
                // pre-step boundary value, so accept one-behind for live
                // only. Closed evolves keep exact-match-else-replay —
                // memoization must be invisible (evolve-design rule 2).
                Some(cell) if ev.live && cell.tick + 1 == tick => cell.state,
                Some(_) if ev.live => return Err("live evolve sampled off its clock".into()),
                _ => evolve_value(ev, tau, sig, tick_rate)?,
            };
            match value {
                Val::Pose(p) => Ok(p),
                other => Err(format!(
                    "evolve in a pose slot expected pose state, got {:?}",
                    other
                )),
            }
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let [idx, epoch] = readers.n2(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
            dyn_node_pose_u_in(cur.node(), tau - epoch, u, ctx)
        }
        DynNode::Translate { dx, dy, child } => {
            let p = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(Pose { x: p.x + dx, y: p.y + dy, theta: p.theta })
        }
        DynNode::Path { curve, progress, env } => {
            let key = d as *const DynNode as usize;
            let u = eval_sig_at_rate(
                progress,
                env,
                sig,
                tau,
                0.0,
                Some(read_scan_in(state, key, readers.clone())),
                None,
                tick_rate,
            )?
            .num()?;
            dyn_node_pose_u_in(curve, tau, u, ctx)
        }
        DynNode::Frame(parent, child) => {
            // constant parent: skip the recursion (and its profile scope)
            if let DynNode::Const(pp) = &**parent {
                let cp = dyn_node_pose_u_in(child, tau, u, ctx)?;
                return Ok(pp.compose(&cp));
            }
            // compose rotates the child offset by the parent theta, so the
            // parent needs its heading even when the caller discards ours
            let pp = dyn_node_pose_u_in(parent, tau, u, ctx.with_theta())?;
            let cp = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(pp.compose(&cp))
        }
        DynNode::ConstFrame { pose, rot, child } => {
            let cp = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(pose.compose_with_rot(*rot, &cp))
        }
        DynNode::RowFrame(child) => {
            let cp = dyn_node_pose_u_in(child, tau, u, ctx)?;
            Ok(ctx.row_frame.unwrap_or(Pose::IDENTITY).compose(&cp))
        }
    }
}

/// The dyn for the current segment of a Stages node.
pub(crate) fn stage_dyn_in(
    segs: &[StageSeg],
    idx: usize,
    _state: &MotionState,
    _key: usize,
    _readers: &MotionReaders,
) -> Result<DynPose, String> {
    let seg = segs.get(idx).ok_or("stages: segment index out of range")?;
    match &seg.make {
        StageMake::Ready(d) => Ok(d.clone()),
    }
}

/// Advance the Scanned leaves of a motion tree by one tick.
pub fn step_motion(
    d: &DynNode,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
) -> Result<(), String> {
    let readers = MotionReaders::for_node(d);
    let mut ignore_n2 = |_, _| {};
    let mut ignore_dyn = |_, _| {};
    let mut ignore_val = |_, _| {};
    let mut ctx = MotionStepCtx {
        state,
        sig,
        capture_layout: None,
        captures: &[],
        world: None,
        readers: &readers,
        tick_rate: TickTiming::default().rate(),
        rng_base: None,
        mirror_legacy: true,
        write_n2: &mut ignore_n2,
        write_col: &mut |_, _| {},
        write_dyn: &mut ignore_dyn,
        write_val: &mut ignore_val,
    };
    step_motion_in(d, tau, dt, &mut ctx)
}

pub fn step_motion_in(
    d: &DynNode,
    tau: f64,
    dt: f64,
    ctx: &mut MotionStepCtx<'_>,
) -> Result<(), String> {
    match d {
        DynNode::StockIntegrator { data } => {
            let (a, b, env, programs, rand) = (&data.a, &data.b, &data.env, &data.programs, &data.rand);
            let polar = data.source_is_polar();
            let range = ctx.capture_layout.and_then(|layout| layout.range(d));
            let caps = ctx.caps_for(d, rand);
            let extracted = range.and_then(|_| match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Some(ex),
                _ => None,
            });
            let captured_env = capture_env(env, extracted, rand, &caps);
            let source_a = extracted.map(|ex| &ex.forms[0]).unwrap_or(a);
            let source_b = extracted.map(|ex| &ex.forms[1]).unwrap_or(b);
            let lowered = extracted
                .map(|ex| Some((&ex.programs[0], &ex.programs[1])))
                .unwrap_or_else(|| programs
                    .get_or_init(|| lower_motion_program_pair(a, b, env, ctx.sig, true, true))
                    .as_ref()
                    .map(|(ap, bp)| (ap, bp)));
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let component_key = integrator_component_key(key, ctx.readers);
            let state = &mut *ctx.state;
            let sig = ctx.sig;
            let readers = ctx.readers;
            let tick_rate = ctx.tick_rate;
            let mirror_legacy = ctx.mirror_legacy;
            let write_n2 = &mut *ctx.write_n2;
            let write_col = &mut *ctx.write_col;
            let write_val = &mut *ctx.write_val;
            let [x, y] = readers.n2(dense_key)
                .or_else(|| match state.get(&dense_key) {
                    Some(Cell::N(v)) => Some(*v),
                    _ => None,
                })
                .unwrap_or([0.0, 0.0]);
            // scan-free integrands (lowered programs) have no sites to
            // advance, so the step is just the compiled velocity sample.
            // Aux programs (scan/channel reads) take the interpreted path:
            // their sited evolves must ADVANCE here.
            let (vx, vy) = if let Some((ap, bp)) = lowered
                .filter(|(ap, bp)| ap.aux_free() && bp.aux_free())
            {
                let (vx, vy) =
                    eval_motion_program_pair(ap, bp, polar, tau, 0.0, Some((x, y)), &caps, &[], &[]);
                if oracle_enabled() {
                    let (a, b) = &oracle_forms(source_a, source_b, &caps);
                    let ((ivx, ivy), _) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                        eval_pt_at_rate(a, b, polar, &captured_env, sig, tau, 0.0, Some(scan), Some((x, y)), tick_rate)
                    })?;
                    assert_num_close("vel-step/a", a, vx, ivx);
                    assert_num_close("vel-step/b", b, vy, ivy);
                }
                (vx, vy)
            } else {
                let (a, b) = &oracle_forms(source_a, source_b, &caps);
                let ((vx, vy), writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                    eval_pt_at_rate(a, b, polar, &captured_env, sig, tau, 0.0, Some(scan), Some((x, y)), tick_rate)
                })?;
                for (key, value) in writes.n2 {
                    write_n2(key, value);
                }
                for (key, value) in writes.val {
                    write_val(key, value);
                }
                (vx, vy)
            };
            let next = [x + vx * dt, y + vy * dt];
            // the state map is only read back on the legacy path (Empty
            // readers); the sim path reads through the snapshot and applies
            // the buffered writes to the world's columns
            if mirror_legacy {
                state.insert(component_key, Cell::N([vx, vy]));
                state.insert(dense_key, Cell::N(next));
            }
            write_n2(dense_key, next);
            write_col(data.columns[0], vx);
            write_col(data.columns[1], vy);
            Ok(())
        }
        DynNode::RotExpr { form, env, program, rand } => {
            let range = ctx.capture_layout.and_then(|layout| layout.range(d));
            let caps = ctx.caps_for(d, rand);
            let extracted = range.and_then(|_| match rand.as_deref() {
                Some(RandCell::Compiled(ex)) => Some(ex),
                _ => None,
            });
            let captured_env = capture_env(env, extracted, rand, &caps);
            let source_form = extracted.map(|ex| &ex.forms[0]).unwrap_or(form);
            // a lowered AUX-FREE program is scan-free: nothing to advance.
            // Aux programs still carry sited evolves that must advance.
            let lowered = extracted
                .map(|ex| Some(&ex.programs[0]))
                .unwrap_or_else(|| program
                    .get_or_init(|| lower_motion_program(form, env, ctx.sig, true, true))
                    .as_ref());
            if lowered.is_some_and(|p| p.aux_free()) {
                return Ok(());
            }
            let source_form = oracle_form(source_form, &caps);
            let state = &mut *ctx.state;
            let sig = ctx.sig;
            let readers = ctx.readers;
            let tick_rate = ctx.tick_rate;
            let mirror_legacy = ctx.mirror_legacy;
            let write_n2 = &mut *ctx.write_n2;
            let write_val = &mut *ctx.write_val;
            let key = d as *const DynNode as usize;
            let (_, writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                eval_sig_at_rate(&source_form, &captured_env, sig, tau, 0.0, Some(scan), Some((0.0, 0.0)), tick_rate)?.num()
            })?;
            for (key, value) in writes.n2 {
                write_n2(key, value);
            }
            for (key, value) in writes.val {
                write_val(key, value);
            }
            Ok(())
        }
        DynNode::Path { curve, progress, env } => {
            {
                let state = &mut *ctx.state;
                let sig = ctx.sig;
                let readers = ctx.readers;
                let tick_rate = ctx.tick_rate;
                let mirror_legacy = ctx.mirror_legacy;
                let write_n2 = &mut *ctx.write_n2;
                let write_val = &mut *ctx.write_val;
                let key = d as *const DynNode as usize;
                let (_, writes) = advance_sites_with_writes(state, key, dt, readers.clone(), mirror_legacy, |scan| {
                    eval_sig_at_rate(progress, env, sig, tau, 0.0, Some(scan), None, tick_rate)?.num()
                })?;
                for (key, value) in writes.n2 {
                    write_n2(key, value);
                }
                for (key, value) in writes.val {
                    write_val(key, value);
                }
            }
            step_motion_in(curve, tau, dt, ctx)
        }
        DynNode::Stages { segs } => {
            let key = d as *const DynNode as usize;
            let (cur, epoch, _mirror_legacy) = {
                let dense_key = ctx.node_key(key);
                let state = &mut *ctx.state;
                let sig = ctx.sig;
                let readers = ctx.readers;
                let tick_rate = ctx.tick_rate;
                let mirror_legacy = ctx.mirror_legacy;
                let write_n2 = &mut *ctx.write_n2;
                let [mut idx, mut epoch] = readers.n2(dense_key)
                    .or_else(|| match state.get(&dense_key) {
                        Some(Cell::N(v)) => Some(*v),
                        _ => None,
                    })
                    .unwrap_or([0.0, 0.0]);
                let seg = segs.get(idx as usize).ok_or("stages: bad segment")?;
                let local = tau - epoch;
                let done = match &seg.term {
                    StageTerm::Dur(dsec) => local >= *dsec,
                    StageTerm::Until(pred, penv) => {
                        let scan = read_scan_in(state, key, readers.clone());
                        truthy(&eval_sig_at_rate(pred, penv, sig, local, 0.0, Some(scan), None, tick_rate)?)
                    }
                    StageTerm::Forever => false,
                };
                if done && (idx as usize) + 1 < segs.len() {
                    let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
                    let eval_ctx = MotionEvalCtx::with_tick_rate(state, sig, readers, tick_rate);
                    let eval_ctx = match ctx.capture_layout {
                        Some(layout) => eval_ctx.with_row(layout, ctx.captures, None),
                        None => eval_ctx,
                    };
                    let p1 = dyn_node_pose_u_in(cur.node(), local, 0.0, eval_ctx)?;
                    let p0 = dyn_node_pose_u_in(cur.node(), (local - dt).max(0.0), 0.0, eval_ctx)?;
                    let exit = Val::Map(Rc::new(vec![
                        (Val::Kw("pos".into()), Val::Pose(Pose::point(p1.x, p1.y))),
                        (
                            Val::Kw("vel".into()),
                            Val::Pose(Pose::point((p1.x - p0.x) / dt, (p1.y - p0.y) / dt)),
                        ),
                        (Val::Kw("pose".into()), Val::Pose(p1)),
                    ]));
                    idx += 1.0;
                    epoch = tau;
                    if let Some(slot) = &segs[idx as usize].exit_slot {
                        let pos_key = stage_exit_key(slot, readers, StageExitField::Pos);
                        let vel_key = stage_exit_key(slot, readers, StageExitField::Vel);
                        let pos = match map_path_get(&exit, "pos") {
                            Some(Val::Pose(p)) => [p.x, p.y],
                            _ => return Err("stages: internal exit pos missing".into()),
                        };
                        let vel = match map_path_get(&exit, "vel") {
                            Some(Val::Pose(p)) => [p.x, p.y],
                            _ => return Err("stages: internal exit vel missing".into()),
                        };
                        // unconditional: the new segment's dyn may read these
                        // exit cells within this same step call, before the
                        // buffered writes reach the world's columns
                        state.insert(pos_key, Cell::N(pos));
                        state.insert(vel_key, Cell::N(vel));
                        write_n2(pos_key, pos);
                        write_n2(vel_key, vel);
                    }
                }
                let next = [idx, epoch];
                state.insert(dense_key, Cell::N(next));
                write_n2(dense_key, next);
                let cur = stage_dyn_in(segs, idx as usize, state, key, readers)?;
                (cur, epoch, mirror_legacy)
            };
            // step the inner dyn on the segment-local clock
            step_motion_in(cur.node(), tau - epoch, dt, ctx)
        }
        DynNode::Translate { child, .. } => {
            step_motion_in(child, tau, dt, ctx)
        }
        DynNode::Frame(a, b) => {
            step_motion_in(a, tau, dt, ctx)?;
            step_motion_in(b, tau, dt, ctx)
        }
        DynNode::ConstFrame { child, .. } | DynNode::RowFrame(child) => {
            step_motion_in(child, tau, dt, ctx)
        }
        DynNode::Clamp { lo, hi, child } => {
            // the clamp correction reads the child's just-stepped integrator
            // state, so mirror this subtree into the state map even on the
            // buffered path (Vel's insert is otherwise legacy-only)
            let saved = ctx.mirror_legacy;
            ctx.mirror_legacy = true;
            let stepped = step_motion_in(child, tau, dt, ctx);
            ctx.mirror_legacy = saved;
            stepped?;
            clamp_integrator(
                child,
                *lo,
                *hi,
                ctx.state,
                ctx.readers,
                saved,
                ctx.write_n2,
            );
            Ok(())
        }
        DynNode::Evolve(ev) => {
            let key = d as *const DynNode as usize;
            let dense_key = ctx.node_key(key);
            let target_tick = evolve_tick(tau, ctx.tick_rate);
            let closed_sig = SigEnv { defs: ctx.sig.defs.clone(), ..SigEnv::default() };
            let step_sig = if ev.live { ctx.sig } else { &closed_sig };
            let mut world = if ev.live { ctx.world.as_deref_mut() } else { None };
            // Per-cell keying: two live evolves on one row (and an init next
            // to its first step) must not share a draw stream — mix in the
            // stable node id and an init/step salt.
            let cell_base = ctx.rng_base.map(|b| match dense_key {
                MotionStateKey::Node(MotionNodeId(id)) => rng_mix(b, id as u64),
                _ => b,
            });
            let mut cell = ctx.readers.vals(dense_key)
                .or_else(|| match ctx.state.get(&dense_key) {
                    Some(Cell::V(v)) => Some(v.clone()),
                    _ => None,
                })
                .map(Ok)
                .unwrap_or_else(|| resolve_evolve_init(ev, step_sig, ctx.tick_rate, world.as_deref_mut(), cell_base.map(|b| rng_mix(b, 0))).map(|state| EvolveCell { state, tick: 0 }))?;
            if cell.tick < target_tick {
                let next = apply_evolve_step(ev, cell.state, cell.tick, step_sig, ctx.tick_rate, world.as_deref_mut(), cell_base.map(|b| rng_mix(b, 1)))?;
                cell = EvolveCell { state: next, tick: cell.tick + 1 };
            }
            if ctx.mirror_legacy {
                ctx.state.insert(dense_key, Cell::V(cell.clone()));
            }
            (ctx.write_val)(dense_key, cell);
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn step_dyn_figure(
    d: &DynFigure,
    tau: f64,
    dt: f64,
    state: &mut MotionState,
    sig: &SigEnv,
) -> Result<(), String> {
    let readers = MotionReaders::for_figure(d);
    let mut ignore_n2 = |_, _| {};
    let mut ignore_dyn = |_, _| {};
    let mut ignore_val = |_, _| {};
    let mut ctx = MotionStepCtx {
        state,
        sig,
        capture_layout: None,
        captures: &[],
        world: None,
        readers: &readers,
        tick_rate: TickTiming::default().rate(),
        rng_base: None,
        mirror_legacy: true,
        write_n2: &mut ignore_n2,
        write_col: &mut |_, _| {},
        write_dyn: &mut ignore_dyn,
        write_val: &mut ignore_val,
    };
    step_dyn_figure_in(d, tau, dt, &mut ctx)
}

pub fn step_dyn_figure_in(
    d: &DynFigure,
    tau: f64,
    dt: f64,
    ctx: &mut MotionStepCtx<'_>,
) -> Result<(), String> {
    step_motion_in(d.pose_dyn(), tau, dt, ctx)?;
    if let Some(curve) = d.curve() {
        if let CurveEval::Expr(shape) = &curve.eval {
            step_motion_in(shape.node(), tau, dt, ctx)?;
        }
    }
    Ok(())
}

/// Walk through unrotated const offsets to an integrating Vel node and
/// clamp its state (bounds shifted into the integrator's local frame).
/// Anything else: the output clamp in dyn_pose is the only effect.
pub(crate) fn clamp_integrator(
    d: &Rc<DynNode>,
    lo: (f64, f64),
    hi: (f64, f64),
    state: &mut MotionState,
    readers: &MotionReaders,
    mirror_legacy: bool,
    write_n2: &mut dyn FnMut(MotionStateKey, [f64; 2]),
) {
    match &**d {
        DynNode::StockIntegrator { .. } => {
            // State readers map nodes within the row's owning immutable spec.
            let key = Rc::as_ptr(d) as *const DynNode as usize;
            let dense_key = state_key_for_node(key, readers);
            if let Some([x, y]) = match state.get(&dense_key) {
                Some(Cell::N(v)) => Some(*v),
                _ => None,
            }
            .or_else(|| readers.n2(dense_key))
            {
                let next = [x.clamp(lo.0, hi.0), y.clamp(lo.1, hi.1)];
                if mirror_legacy {
                    state.insert(dense_key, Cell::N(next));
                }
                write_n2(dense_key, next);
            }
        }
        DynNode::Frame(a, b) => {
            if let DynNode::Const(p) = &**a {
                if p.angle_or(0.0).abs() < 1e-12 {
                    clamp_integrator(
                        b,
                        (lo.0 - p.x, lo.1 - p.y),
                        (hi.0 - p.x, hi.1 - p.y),
                        state,
                        readers,
                        mirror_legacy,
                        write_n2,
                    );
                }
            }
        }
        DynNode::ConstFrame { pose, child, .. } => {
            if pose.angle_or(0.0).abs() < 1e-12 {
                clamp_integrator(
                    child,
                    (lo.0 - pose.x, lo.1 - pose.y),
                    (hi.0 - pose.x, hi.1 - pose.y),
                    state,
                    readers,
                    mirror_legacy,
                    write_n2,
                );
            }
        }
        DynNode::Translate { dx, dy, child } => {
            clamp_integrator(child, (lo.0 - dx, lo.1 - dy), (hi.0 - dx, hi.1 - dy), state, readers, mirror_legacy, write_n2);
        }
        _ => {}
    }
}

/// Site writes produced by an advancing scan evaluation.
pub(crate) struct ScanWrites {
    pub n2: Vec<(MotionStateKey, [f64; 2])>,
    pub val: Vec<(MotionStateKey, EvolveCell)>,
}

/// Run an evaluation with an advancing scan context over the bullet's state,
/// then merge the (possibly grown) state back.
pub(crate) fn advance_sites_with_writes<T>(
    state: &mut MotionState,
    base: usize,
    dt: f64,
    readers: MotionReaders,
    mirror_legacy: bool,
    f: impl FnOnce(ScanShared) -> Result<T, String>,
) -> Result<(T, ScanWrites), String> {
    let MotionStateKey::Node(dense_base) = state_key_for_node(base, &readers) else {
        unreachable!("node keys are always stable")
    };
    let io = Rc::new(std::cell::RefCell::new(ScanIo {
        state: std::mem::take(state),
        base,
        dense_base: Some(dense_base),
        counter: 0,
        advance: true,
        dt,
        readers: Some(readers),
        mirror_legacy,
        n2_writes: Vec::new(),
        val_writes: Vec::new(),
    }));
    let r = f(io.clone());
    let io = Rc::try_unwrap(io)
        .map_err(|_| "scan context escaped".to_string())?
        .into_inner();
    *state = io.state;
    r.map(|value| (value, ScanWrites { n2: io.n2_writes, val: io.val_writes }))
}

pub fn integrator_columns(d: &DynNode) -> Option<[ColName; 2]> {
    match d {
        DynNode::StockIntegrator { data } => Some(data.columns),
        DynNode::Path { curve, .. }
        | DynNode::Translate { child: curve, .. }
        | DynNode::ConstFrame { child: curve, .. }
        | DynNode::RowFrame(curve)
        | DynNode::Clamp { child: curve, .. } => integrator_columns(curve),
        DynNode::Stages { segs } => segs.iter().find_map(|seg| {
            let StageMake::Ready(d) = &seg.make;
            integrator_columns(d.node())
        }),
        DynNode::Frame(a, b) => integrator_columns(a).or_else(|| integrator_columns(b)),
        _ => None,
    }
}

pub fn integrator_figure_columns(d: &DynFigure) -> Option<[ColName; 2]> {
    integrator_columns(d.pose_dyn()).or_else(|| {
        d.curve().and_then(|curve| match &curve.eval {
            CurveEval::Expr(shape) => integrator_columns(shape.node()),
            CurveEval::Straight => None,
        })
    })
}

/// Columns claimed by integrators that can be ACTIVE at the same instant.
/// Stages segments are mutually exclusive, so integrators in different
/// segments may share component columns (only the active one steps and
/// writes); concurrent claims (Frame parent+child, both Frame sides) would
/// cross-wire columns and headings and are an error.
pub fn concurrent_integrator_columns(d: &DynNode) -> Result<Vec<ColName>, String> {
    fn merge(mut a: Vec<ColName>, b: Vec<ColName>, concurrent: bool) -> Result<Vec<ColName>, String> {
        for column in b {
            if a.contains(&column) {
                if concurrent {
                    return Err(
                        "spawn: concurrent integrators in one motion tree are not supported \
                         (their component columns would collide)"
                            .into(),
                    );
                }
            } else {
                a.push(column);
            }
        }
        Ok(a)
    }
    match d {
        DynNode::StockIntegrator { data } => Ok(data.columns.to_vec()),
        DynNode::Path { curve, .. } => concurrent_integrator_columns(curve),
        DynNode::Stages { segs } => {
            let mut out = Vec::new();
            for seg in segs {
                let StageMake::Ready(d) = &seg.make;
                out = merge(out, concurrent_integrator_columns(d.node())?, false)?;
            }
            Ok(out)
        }
        DynNode::Translate { child, .. }
        | DynNode::ConstFrame { child, .. }
        | DynNode::RowFrame(child)
        | DynNode::Clamp { child, .. } => concurrent_integrator_columns(child),
        DynNode::Frame(a, b) => merge(
            concurrent_integrator_columns(a)?,
            concurrent_integrator_columns(b)?,
            true,
        ),
        _ => Ok(Vec::new()),
    }
}

/// Validates concurrency and returns every column any integrator in the
/// figure claims (for reserved-name checks) without building `DynNum`s.
pub fn concurrent_integrator_figure_columns(d: &DynFigure) -> Result<Vec<ColName>, String> {
    let mut pose = concurrent_integrator_columns(d.pose_dyn())?;
    if let Some(curve) = d.curve() {
        if let CurveEval::Expr(shape) = &curve.eval {
            let curve_cols = concurrent_integrator_columns(shape.node())?;
            for column in curve_cols {
                if pose.contains(&column) {
                    return Err(
                        "spawn: concurrent integrators in one motion tree are not supported \
                         (their component columns would collide)"
                            .into(),
                    );
                }
                pose.push(column);
            }
        }
    }
    Ok(pose)
}

pub fn is_scanned(d: &DynNode) -> bool {
    match d {
        DynNode::StockIntegrator { .. }
        | DynNode::RotExpr { .. }
        | DynNode::Stages { .. }
        | DynNode::Path { .. }
        | DynNode::Evolve(_) => true,
        DynNode::Translate { child, .. } => is_scanned(child),
        DynNode::Frame(a, b) => is_scanned(a) || is_scanned(b),
        DynNode::ConstFrame { child, .. } | DynNode::RowFrame(child) => is_scanned(child),
        DynNode::Clamp { child, .. } => is_scanned(child),
        _ => false,
    }
}

pub fn is_scanned_figure(d: &DynFigure) -> bool {
    is_scanned(d.pose_dyn())
        || d.curve()
            .and_then(|curve| match &curve.eval {
                CurveEval::Expr(shape) => Some(is_scanned(shape.node())),
                CurveEval::Straight => None,
            })
            .unwrap_or(false)
}

/// Is this form time-dependent — does it reference the slot-bound
/// parameters t/u (F12), or contain a (live …) read? live means
/// "re-read at eval time" (§3's snap boundary), so a wall-clock signal
/// like (cart m"(live($tick) - t0)/120" 0) must defer exactly like a
/// t-dependent one instead of constant-folding at spawn.
pub(crate) fn contains_t(form: &Form) -> bool {
    match form {
        Form::Sym(s) => &**s == "t" || &**s == "u",
        Form::List(items) => {
            if matches!(items.first(), Some(Form::Sym(s)) if &**s == "live") {
                return true;
            }
            items.iter().any(contains_t)
        }
        Form::Vector(items) => items.iter().any(contains_t),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| contains_t(k) || contains_t(v)),
        _ => false,
    }
}

/// Syntactic liveness for `(evolve init step)` (evolve-design.md):
/// channel reads, rand, and world-reading heads mark the evolve live.
/// Keyword access is live only when it reads OUTSIDE the fold — an
/// access rooted at one of the step fn's own params (`(:x s)`,
/// `(:dt c)`, chains like `(:x (:vel s))`) is the fold's own state and
/// stays closed; an access rooted at a capture (`(:hp e)`) reads world
/// state through a view and marks live. The init form has no binders,
/// so any capture-rooted access there is live — which is exactly the
/// `(evolve (:pos e) ...)` continuity case. Conservative direction per
/// the design doc: false-live only forbids off-clock sampling.
pub(crate) fn evolve_is_live(init: &Form, step: &Form) -> bool {
    if evolve_form_is_live(init, &[]) {
        return true;
    }
    if let Form::List(items) = step {
        if let [Form::Sym(head), Form::Vector(params), body @ ..] = &items[..] {
            if &**head == "fn" {
                let locals = params
                    .iter()
                    .filter_map(|p| match p {
                        Form::Sym(s) if &**s != "&" => Some(&**s),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                return body.iter().any(|f| evolve_form_is_live(f, &locals));
            }
        }
    }
    evolve_form_is_live(step, &[])
}

fn evolve_form_is_live(form: &Form, locals: &[&str]) -> bool {
    match form {
        Form::Sym(s) => s.starts_with('$') || &**s == "rand",
        Form::List(items) => {
            match &items[..] {
                // keyword access: live iff its root escapes the locals
                [Form::Kw(_), base] => match base {
                    Form::Sym(s) => !locals.contains(&&**s),
                    other => evolve_form_is_live(other, locals),
                },
                _ => {
                    let live_head = matches!(items.first(), Some(Form::Sym(s)) if matches!(&**s,
                        "live" | "rand" | "entities-where" | "nearest-entity"
                        | "entity-col" | "sum-entities" | "count-entities"
                        | "collisions" | "curve-samples" | "on-curve"
                        | "matches"));
                    live_head || items.iter().any(|f| evolve_form_is_live(f, locals))
                }
            }
        }
        Form::Vector(items) => items.iter().any(|f| evolve_form_is_live(f, locals)),
        Form::Map(kvs) => kvs.iter().any(|(k, v)| evolve_form_is_live(k, locals) || evolve_form_is_live(v, locals)),
        _ => false,
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    fn compiled(src: &str, speed: f64) -> Rc<MotionProgram> {
        let form = crate::edn::read_one(src).unwrap();
        let env = Env::empty().bind("speed".into(), Val::Num(speed));
        let (_, programs) = compile_sig(&[&form], &env, &SigEnv::default(), true, true);
        programs.expect("supported motion expression").remove(0)
    }

    #[test]
    fn motion_plan_declares_every_driver_input() {
        let program = compiled("(+ speed t u (:x pos) (live $boost))", 2.0);
        let plan = program.plan();
        assert_eq!(plan.domain(), IterationDomain::MotionRows);
        assert_eq!(plan.fallback(), FallbackPolicy::WholePlanInterpreted);
        assert_eq!(plan.merge(), MergePolicy::DriverOwned);
        assert_eq!(
            plan.bindings().inputs,
            [
                KernelInputBinding {
                    input: KernelInputRef::F64(0),
                    source: KernelInputSource::Capture { slot: 0 },
                },
                KernelInputBinding {
                    input: KernelInputRef::F64(1),
                    source: KernelInputSource::Tick,
                },
                KernelInputBinding {
                    input: KernelInputRef::F64(2),
                    source: KernelInputSource::Axis,
                },
                KernelInputBinding {
                    input: KernelInputRef::F64(3),
                    source: KernelInputSource::State { slot: 0 },
                },
                KernelInputBinding {
                    input: KernelInputRef::F64(4),
                    source: KernelInputSource::Channel { channel: 0 },
                },
            ]
        );
        assert_eq!(
            plan.bindings().outputs,
            [KernelOutputBinding {
                output: 0,
                target: KernelOutputTarget::Driver,
            }]
        );
        assert_eq!(plan.program().ops().len(), program.backend().ops.len());
    }

    #[test]
    fn captures_share_full_typed_plan_identity() {
        let a = compiled("(+ speed (* 2 t))", 2.0);
        let b = compiled("(+ speed (* 2 t))", 3.0);
        assert!(Rc::ptr_eq(&a, &b), "capture values must not specialize the typed plan");
        assert!(a.same_identity(&b));
        assert_eq!(a.plan().program(), b.plan().program());
        assert_eq!(a.plan().bindings(), b.plan().bindings());
    }
}

#[cfg(test)]
mod size_probe {
    /// DynNode size is hot: every pose walk chases these enums, and the
    /// round-22 inline rand-slot draft (88 → 120 bytes) cost ~60% wall on
    /// the scaled fruit rig. New per-variant data goes behind an
    /// `Option<Rc<..>>` (see RandCell), not inline.
    #[test]
    fn dyn_node_stays_small() {
        assert!(
            std::mem::size_of::<super::DynNode>() <= 96,
            "DynNode grew to {} bytes — box new fields (see RandCell)",
            std::mem::size_of::<super::DynNode>()
        );
    }
}
