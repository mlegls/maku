//! Wasm bindings over the supported `host::Instance` facade and the Touhou
//! render pack. The browser supplies inputs on a fixed-timestep loop, consumes
//! ordered fixed-layout sprite/ribbon buffers through Canvas2D or another
//! adapter, and uses `command_line` for live tooling. Cards arrive through the
//! VFS; import expansion remains engine-owned.

use js_sys::{Uint32Array, Uint8Array};
use maku::host::Instance;
use maku::host::Inputs;
use maku::render::RenderItem;
use maku::touhou::{
    DrawSource, TextureSource, TouhouMesh, TouhouProfile, FRAME_ABI_VERSION,
};
use std::mem::{size_of, size_of_val};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

unsafe fn byte_view<T>(values: &[T]) -> Uint8Array {
    let bytes = unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), size_of_val(values))
    };
    unsafe { Uint8Array::view(bytes) }
}

pub const MAKU_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SOURCE_REVISION: &str = match option_env!("MAKU_SOURCE_REVISION") {
    Some(value) => value,
    None => "development",
};

#[wasm_bindgen(js_name = makuVersion)]
pub fn maku_version() -> String {
    MAKU_VERSION.into()
}

#[wasm_bindgen(js_name = frameAbiVersion)]
pub fn frame_abi_version() -> u32 {
    FRAME_ABI_VERSION
}

#[wasm_bindgen(js_name = sourceRevision)]
pub fn source_revision() -> String {
    SOURCE_REVISION.into()
}

#[wasm_bindgen(js_name = stdlibSource)]
pub fn stdlib_source(name: &str) -> Option<String> {
    maku::source::stdlib(name).map(str::to_owned)
}

#[wasm_bindgen]
pub struct Maku {
    inst: Instance,
    pending: Inputs,
    mesh: TouhouMesh,
    pending_render: Vec<RenderItem>,
    packed_draws: Vec<u32>,
}

#[wasm_bindgen]
impl Maku {
    #[wasm_bindgen(constructor)]
    pub fn new(rig: Option<String>) -> Maku {
        let rig = rig.map(|source| maku::source::expand_src(&source).unwrap_or(source));
        let mut inst = Instance::new(rig);
        inst.set_render_kinds(["default", "sprite", "beam"]);
        Maku {
            inst,
            pending: Inputs::default(),
            mesh: TouhouMesh::new(Rc::new(TouhouProfile::stock())),
            pending_render: Vec::new(),
            packed_draws: Vec::new(),
        }
    }

    /// Register a card file in the virtual filesystem (path → text).
    pub fn add_file(&mut self, path: String, text: String) {
        self.inst.add_file(path, text);
    }

    pub fn boot(&mut self, path: String, pattern: Option<String>) {
        self.inst.boot(path, pattern);
    }

    /// Wire protocol (docs/player.md): run/swap/add/load/pattern/restart/
    /// clear/seek/step/snapshots/resize-entities/pause/resume.
    pub fn command(&mut self, line: &str) {
        self.inst.command_line(line);
    }

    /// Set a numeric input channel for subsequent steps ($move-x,
    /// $p2-move-x, $focus-firing, $bomb — an open vocabulary, by name).
    pub fn input_num(&mut self, name: &str, v: f64) {
        self.pending.set_num(name, v);
    }

    /// Set a point input channel ($player mock, $nearest-enemy mock, …).
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
        self.inst.channel_num("lives").unwrap_or(-1.0)
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

    /// [x, y] of a point-valued channel ($player, $boss, …), or empty.
    pub fn channel_vec(&self, name: &str) -> Vec<f32> {
        self.inst
            .channel_point(name)
            .map(|(x, y)| vec![x as f32, y as f32])
            .unwrap_or_default()
    }

    /// Numeric channel ($lives, $boss-hp, $graze, …); NaN when absent.
    pub fn channel_num(&self, name: &str) -> f64 {
        self.inst.channel_num(name).unwrap_or(f64::NAN)
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

    /// Build the pack frame once. Consume the zero-copy typed-array views
    /// before the next mutating wasm call: another build reuses their backing
    /// vectors, and any wasm-memory growth invalidates JavaScript views.
    pub fn build_render_frame(&mut self) -> Result<(), JsValue> {
        self.benchmark_render_transport();
        self.benchmark_build_pack()
    }

    /// Build and retain only the typed core transport. Returns its lane count.
    #[doc(hidden)]
    pub fn benchmark_render_transport(&mut self) -> usize {
        self.pending_render = self.inst.render_frame();
        self.pending_render.iter().map(|item| match item {
            RenderItem::Row(_) => 1,
            RenderItem::Batch(batch) => batch.len,
        }).sum()
    }

    /// Build the Touhou pack from the retained transport without regenerating it.
    #[doc(hidden)]
    pub fn benchmark_build_pack(&mut self) -> Result<(), JsValue> {
        for kind in ["sprite", "beam"] {
            if let Some(schema) = self.inst.declared_render_schema(kind) {
                self.mesh.bind_schema(kind, schema).map_err(|e| JsValue::from_str(&e.to_string()))?;
            }
        }
        self.mesh.build(&self.pending_render).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.packed_draws.clear();
        self.packed_draws.reserve(self.mesh.frame().draws.len() * 8);
        for draw in &self.mesh.frame().draws {
            let (tag, fields) = match draw.source {
                DrawSource::BasicSprites { start, count } => (0, [start, count, 0, 0, 0, 0]),
                DrawSource::TintedSprites { start, count } => (1, [start, count, 0, 0, 0, 0]),
                DrawSource::RecolorSprites { start, count } => (2, [start, count, 0, 0, 0, 0]),
                DrawSource::Indexed { vertex_start, vertex_count, index_start, index_count } =>
                    (3, [vertex_start, vertex_count, index_start, index_count, 0, 0]),
            };
            self.packed_draws.extend_from_slice(&[draw.material.0, tag]);
            self.packed_draws.extend_from_slice(&fields);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn benchmark_digest(&mut self) -> String { format!("{:016x}", self.inst.benchmark_digest()) }
    #[doc(hidden)]
    pub fn benchmark_collider_projections(&self) -> usize { self.inst.benchmark_counters().collider_projections }
    #[doc(hidden)]
    pub fn benchmark_active_query_pairs(&self) -> usize { self.inst.benchmark_counters().active_query_pairs }
    #[doc(hidden)]
    pub fn benchmark_collision_candidates(&self) -> usize { self.inst.benchmark_counters().collision_candidates }
    #[doc(hidden)]
    pub fn benchmark_contacts(&self) -> usize { self.inst.benchmark_counters().contacts }
    #[doc(hidden)]
    pub fn benchmark_predicate_matches(&self) -> usize { self.inst.benchmark_counters().predicate_matches }
    #[doc(hidden)]
    pub fn benchmark_rule_actions(&self) -> usize { self.inst.benchmark_counters().rule_actions }

    pub fn frame_abi_version(&self) -> u32 { FRAME_ABI_VERSION }
    pub fn basic_sprite_stride(&self) -> usize { size_of::<maku::touhou::BasicSpriteInstance>() }
    pub fn tinted_sprite_stride(&self) -> usize { size_of::<maku::touhou::TintedSpriteInstance>() }
    pub fn recolor_sprite_stride(&self) -> usize { size_of::<maku::touhou::RecolorSpriteInstance>() }
    pub fn strip_vertex_stride(&self) -> usize { size_of::<maku::touhou::StripVertex>() }
    pub fn draw_command_stride(&self) -> usize { 8 }

    pub fn basic_sprites(&self) -> Uint8Array { unsafe { byte_view(&self.mesh.frame().basic_sprites) } }
    pub fn tinted_sprites(&self) -> Uint8Array { unsafe { byte_view(&self.mesh.frame().tinted_sprites) } }
    pub fn recolor_sprites(&self) -> Uint8Array { unsafe { byte_view(&self.mesh.frame().recolor_sprites) } }
    pub fn strip_vertices(&self) -> Uint8Array { unsafe { byte_view(&self.mesh.frame().vertices) } }
    pub fn strip_indices(&self) -> Uint32Array { unsafe { Uint32Array::view(&self.mesh.frame().indices) } }
    pub fn draw_commands(&self) -> Uint32Array { unsafe { Uint32Array::view(&self.packed_draws) } }

    /// Deduplicated profile fallback diagnostics from the latest and prior
    /// frame builds. One line per unknown style/color encountered.
    pub fn render_diagnostics(&self) -> String {
        self.mesh.diagnostics().iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("\n")
    }

    pub fn texture_count(&self) -> usize { self.mesh.profile().textures().len() }
    pub fn texture_key(&self, index: usize) -> String {
        self.mesh.profile().textures().get(index).map(|v| v.key.to_string()).unwrap_or_default()
    }
    pub fn texture_width(&self, index: usize) -> u32 {
        match self.mesh.profile().textures().get(index).map(|v| &v.source) {
            Some(TextureSource::BuiltinRgba8 { width, .. }) => *width,
            _ => 0,
        }
    }
    pub fn texture_height(&self, index: usize) -> u32 {
        match self.mesh.profile().textures().get(index).map(|v| &v.source) {
            Some(TextureSource::BuiltinRgba8 { height, .. }) => *height,
            _ => 0,
        }
    }
    pub fn texture_external_key(&self, index: usize) -> String {
        match self.mesh.profile().textures().get(index).map(|v| &v.source) {
            Some(TextureSource::External { key }) => key.to_string(),
            _ => String::new(),
        }
    }
    pub fn texture_bytes(&self, index: usize) -> Uint8Array {
        match self.mesh.profile().textures().get(index).map(|v| &v.source) {
            Some(TextureSource::BuiltinRgba8 { bytes, .. }) => unsafe { Uint8Array::view(bytes) },
            _ => Uint8Array::new_with_length(0),
        }
    }

    pub fn material_count(&self) -> usize { self.mesh.profile().materials().len() }
    pub fn material_key(&self, index: usize) -> String {
        self.mesh.profile().materials().get(index).map(|v| v.key.to_string()).unwrap_or_default()
    }
    pub fn material_pipeline(&self, index: usize) -> String {
        self.mesh.profile().materials().get(index).map(|v| v.pipeline.to_string()).unwrap_or_default()
    }
    pub fn material_texture(&self, index: usize) -> u32 {
        self.mesh.profile().materials().get(index).map(|v| v.texture.0).unwrap_or(u32::MAX)
    }
    pub fn material_layout(&self, index: usize) -> u32 {
        use maku::touhou::SourceLayout::*;
        self.mesh.profile().materials().get(index).map(|v| match v.layout {
            BasicSprite => 0, TintedSprite => 1, RecolorSprite => 2, IndexedStrip => 3,
        }).unwrap_or(u32::MAX)
    }
    pub fn material_blend(&self, index: usize) -> u32 {
        use maku::touhou::BlendMode::*;
        self.mesh.profile().materials().get(index).map(|v| match v.blend {
            Opaque => 0, Alpha => 1, Additive => 2, SoftAdditive => 3,
        }).unwrap_or(u32::MAX)
    }
    pub fn material_fixed_color(&self, index: usize) -> u32 {
        self.mesh.profile().materials().get(index).and_then(|v| v.fixed_color).map(|v| {
            let [r, g, b, a] = v.0;
            u32::from_le_bytes([r, g, b, a])
        }).unwrap_or(0)
    }
    pub fn material_min_filter(&self, index: usize) -> u32 {
        use maku::touhou::TextureFilter::*;
        self.mesh.profile().materials().get(index).map(|v| match v.sampler.min_filter {
            Nearest => 0, Linear => 1,
        }).unwrap_or(u32::MAX)
    }
    pub fn material_mag_filter(&self, index: usize) -> u32 {
        use maku::touhou::TextureFilter::*;
        self.mesh.profile().materials().get(index).map(|v| match v.sampler.mag_filter {
            Nearest => 0, Linear => 1,
        }).unwrap_or(u32::MAX)
    }
    pub fn material_address_u(&self, index: usize) -> u32 {
        use maku::touhou::AddressMode::*;
        self.mesh.profile().materials().get(index).map(|v| match v.sampler.address_u {
            Clamp => 0, Repeat => 1, Mirror => 2,
        }).unwrap_or(u32::MAX)
    }
    pub fn material_address_v(&self, index: usize) -> u32 {
        use maku::touhou::AddressMode::*;
        self.mesh.profile().materials().get(index).map(|v| match v.sampler.address_v {
            Clamp => 0, Repeat => 1, Mirror => 2,
        }).unwrap_or(u32::MAX)
    }

    /// Recent positioned events for effect flashes: [code, age_ticks, x, y]*
    /// Event symbols are converted to this host's numeric effect ids here.
    /// Stateless — they replay under scrubbing.
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
