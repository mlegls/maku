use crate::{summary::Distribution, Expectations, SemanticObservation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultEnvelope {
    pub schema_version: u32,
    pub series: String,
    pub run_id: String,
    pub captured_at: String,
    pub source: SourceIdentity,
    pub fixture: FixtureIdentity,
    pub stage: StageIdentity,
    pub environment: Environment,
    pub policy: TimingPolicy,
    pub correctness: Correctness,
    pub counters: WorkCounters,
    pub memory: Memory,
    pub cold_setup: ColdSetup,
    pub samples: Vec<FrameSample>,
    pub summaries: BTreeMap<String, Distribution>,
    pub headroom: Option<Headroom>,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub revision: String, pub dirty: bool, pub workload_schema: u32, pub result_schema: u32,
    pub generator: String, pub expanded_source_sha256: String, pub input_tape_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixtureIdentity {
    pub id: String, pub family: String, pub workload_sha256: String, pub seed: u32,
    pub parameters: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageIdentity { pub executor: String, pub tier: String, pub adapter: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub environment_id: String, pub os: String, pub arch: String, pub cpu: String,
    pub gpu: Option<String>, pub memory_bytes: u64, pub browser: Option<Browser>,
    pub display: Option<Display>, pub build_profile: String, pub rustflags: String,
    pub tool_versions: BTreeMap<String, String>, pub power: Power,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Browser { pub name: String, pub version: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Display { pub width_css: u32, pub height_css: u32, pub dpr: f64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Power { pub source: String, pub low_power_mode: Option<bool>, pub notes: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimingPolicy {
    pub tick_hz: u32, pub presentation_hz: u32, pub warmup_ticks: u64,
    pub sample_frames: u64, pub sample_batches: u32, pub percentile_method: String,
    pub wall_mode: String, pub elapsed_clamp_ms: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Correctness {
    pub valid: bool, pub state_digest: String, pub expected: Expectations,
    pub observed: SemanticObservation, pub errors: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkCounters {
    pub live_entities: usize, pub render_lanes: usize,
    pub sprite_instances: BTreeMap<String, usize>, pub sprite_layers: usize, pub beam_segments: usize,
    pub vertices: usize, pub indices: usize, pub triangles: usize, pub draw_commands: usize,
    pub collider_projections: usize, pub active_query_pairs: usize,
    pub collision_candidates: usize, pub contacts: usize,
    pub rules: BTreeMap<String, usize>, pub predicate_matches: usize, pub rule_actions: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub rss_start_bytes: Option<u64>, pub rss_peak_bytes: Option<u64>,
    pub wasm_start_bytes: Option<u64>, pub wasm_peak_bytes: Option<u64>,
    pub allocations: Option<u64>, pub allocated_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColdSetup { pub load_ns: Option<f64>, pub schema_bind_ns: Option<f64>, pub resource_setup_ns: Option<f64> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameSample {
    pub batch: u32, pub frame: u64, pub ticks: u32,
    pub simulation_ns: Option<f64>, pub transport_ns: Option<f64>, pub pack_build_ns: Option<f64>,
    pub host_overhead_ns: Option<f64>, pub adapter_submission_ns: Option<f64>,
    pub completion_ns: Option<f64>, pub presentation_ns: Option<f64>,
    pub elapsed_clamped_ns: Option<f64>, pub memory_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Headroom {
    pub period_ms: f64, pub byo_ms: Distribution, pub bundled_draw_ms: Distribution,
    pub end_to_end_ms: Distribution,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub status: String, pub last_successful_plateau: Option<usize>,
    pub failure_class: Option<String>, pub message: Option<String>,
}
