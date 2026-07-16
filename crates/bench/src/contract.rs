use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKLOAD_SCHEMA_VERSION: u32 = 1;
pub const RESULT_SCHEMA_VERSION: u32 = 1;
pub const GENERATOR_VERSION: &str = "maku-bench-generator-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)] pub description: String,
    pub family: WorkloadFamily,
    pub seed: u32,
    #[serde(default = "generator_version")] pub generator_version: String,
    pub cadence: Cadence,
    pub entities: Entities,
    pub motion: Motion,
    pub render: RenderShape,
    pub collision: Collision,
    pub rules: Rules,
    #[serde(default)] pub input_tape: Vec<InputFrame>,
    pub expect: Expectations,
    #[serde(default)] pub continuity: Option<Continuity>,
}

fn generator_version() -> String { GENERATOR_VERSION.into() }

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkloadFamily { BulletScale, Collision, Rules, Corner, Continuity }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cadence {
    pub tick_hz: u32,
    pub presentation_hz: u32,
    pub warmup_ticks: u64,
    pub sample_frames: u64,
    pub ticks_per_frame: u32,
    #[serde(default = "default_batches")] pub sample_batches: u32,
}
fn default_batches() -> u32 { 30 }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entities { pub plateau: usize, #[serde(default)] pub capacity: Option<usize> }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub shape: MotionShape,
    #[serde(default)] pub speed: f64,
    #[serde(default = "yes")] pub bounded: bool,
}
fn yes() -> bool { true }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MotionShape { Static, Linear, Polar, Composed }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenderShape {
    pub shape: RenderKind,
    pub layers: u32,
    #[serde(default)] pub family: Option<String>,
    #[serde(default)] pub variant: Option<String>,
    #[serde(default)] pub color: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RenderKind { None, PointBasic, PointTinted, PointRecolor, Ribbon }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Collision {
    pub geometry: ColliderGeometry,
    pub query_pairs: u32,
    #[serde(default)] pub active_layers: u32,
    pub contact_density: ContactDensity,
    #[serde(default)] pub contact_fraction: Option<f64>,
    #[serde(default)] pub capsule_segments: Option<u32>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ColliderGeometry { None, Circle, CapsuleChain }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContactDensity { None, Sparse, Controlled, Dense }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub class: RuleClass,
    pub count: u32,
    pub match_rate: MatchRate,
    #[serde(default)] pub match_fraction: Option<f64>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuleClass { None, FilterOnly, RenderOnly, MaskedUpdate, EffectAction }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MatchRate { None, Half, All }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputFrame { pub tick: u64, pub channels: BTreeMap<String, f64> }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    pub live_entities: usize,
    pub render_lanes: usize,
    #[serde(default)] pub contacts_per_tick: u64,
    #[serde(default)] pub rule_matches_per_tick: u64,
    #[serde(default)] pub rule_actions_per_tick: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Continuity { pub card: String, pub pattern: String, pub ticks: u64 }

impl Workload {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != WORKLOAD_SCHEMA_VERSION { return Err(format!("unsupported workload schema {}", self.schema_version)); }
        if self.generator_version != GENERATOR_VERSION { return Err(format!("unsupported generator {}", self.generator_version)); }
        if self.cadence.tick_hz != 120 || self.cadence.presentation_hz != 60 { return Err("v1 cadence must be 120 Hz simulation / 60 Hz presentation".into()); }
        if self.cadence.sample_frames == 0 || self.cadence.sample_batches == 0 || self.cadence.ticks_per_frame == 0 { return Err("sample counts and ticks_per_frame must be nonzero".into()); }
        if self.entities.plateau > 1_000_000 { return Err("v1 plateau exceeds attempted 1M ceiling".into()); }
        if self.expect.live_entities != self.entities.plateau { return Err("expected live_entities must equal the declared plateau".into()); }
        if self.render.layers == 0 && self.expect.render_lanes != 0 { return Err("zero render layers require zero expected lanes".into()); }
        if self.render.layers > 0 && self.expect.render_lanes != self.entities.plateau * self.render.layers as usize { return Err("render lanes must equal plateau times layers".into()); }
        if let Some(f) = self.collision.contact_fraction { if !(0.0..=1.0).contains(&f) { return Err("contact_fraction must be in [0,1]".into()); } }
        let wanted = match self.rules.match_rate { MatchRate::None => 0.0, MatchRate::Half => 0.5, MatchRate::All => 1.0 };
        if self.rules.match_fraction.is_some_and(|f| f != wanted) { return Err("match_fraction disagrees with match_rate".into()); }
        Ok(())
    }
}
