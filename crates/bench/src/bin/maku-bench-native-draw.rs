use maku::{render::{Column, RenderItem}, sim::Sim};
use maku_bench::{
    generate, load_workload, summary::{summarize, summarize_headroom, FrameStages}, verify, ColdSetup,
    Correctness, Display, Environment, FixtureIdentity, FrameSample, Headroom, Memory, Outcome,
    Power, ResultEnvelope, SourceIdentity, StageIdentity, TimingPolicy, WorkCounters,
    RESULT_SCHEMA_VERSION, WORKLOAD_SCHEMA_VERSION,
};
use maku::touhou::{TouhouMesh, TouhouProfile};
use maku::macroquad::{prepare_frame, submit_frame, RenderResources};
use macroquad::prelude::{clear_background, next_frame, BLACK};
use std::{collections::BTreeMap, hint::black_box, path::PathBuf, rc::Rc, time::Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tier { Draw }
impl Tier {
    fn parse(value: &str) -> Result<Self, String> { match value {
        "native-macroquad-compat" => Ok(Self::Draw), _ => Err(format!("unknown tier {value}")),
    }}
    fn name(self) -> &'static str { "native-macroquad-compat" }
}

struct Args { workload: PathBuf, output: PathBuf, tier: Tier, smoke: bool, environment: PathBuf }
fn args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let workload = PathBuf::from(it.next().ok_or("usage: maku-bench-native WORKLOAD --tier TIER --output FILE [--smoke]")?);
    let mut output = None; let mut tier = Tier::Draw; let mut smoke = false;
    let mut environment = PathBuf::from("bench/environments/m4-pro-macos15-chrome150.json");
    while let Some(arg) = it.next() { match arg.as_str() {
        "--tier" => tier = Tier::parse(&it.next().ok_or("--tier requires a value")?)?,
        "--output" => output = Some(PathBuf::from(it.next().ok_or("--output requires a value")?)),
        "--environment" => environment = PathBuf::from(it.next().ok_or("--environment requires a value")?),
        "--smoke" => smoke = true,
        _ => return Err(format!("unknown argument {arg}")),
    }}
    Ok(Args { workload, output: output.ok_or("--output is required")?, tier, smoke, environment })
}

fn command(args: &[&str]) -> String {
    std::process::Command::new(args[0]).args(&args[1..]).output().ok()
        .filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string()).unwrap_or_else(|| "unknown".into())
}
fn revision() -> String {
    std::env::var("MAKU_SOURCE_REVISION").ok().filter(|s| s.len() == 40)
        .unwrap_or_else(|| command(&["git", "rev-parse", "HEAD"]))
}
fn dirty() -> bool { !command(&["git", "status", "--porcelain"]).is_empty() }
fn captured_at() -> String { command(&["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"]) }

fn peak_rss_bytes() -> Option<u64> {
    #[cfg(unix)] unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 { return None; }
        #[cfg(target_os = "macos")] { return Some(usage.ru_maxrss as u64); }
        #[cfg(not(target_os = "macos"))] { return Some(usage.ru_maxrss as u64 * 1024); }
    }
    #[cfg(not(unix))] { None }
}

fn environment(path: &PathBuf) -> Result<Environment, String> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let host = &value["host"]; let power = &value["power"]; let display = &value["display"];
    let tools = value["tools"].as_object().ok_or("environment.tools missing")?.iter()
        .map(|(k,v)| (k.clone(), v.as_str().unwrap_or("unknown").to_string())).collect();
    Ok(Environment {
        environment_id: value["environment_id"].as_str().unwrap_or("unknown").into(),
        os: host["os"].as_str().unwrap_or("unknown").into(), arch: host["arch"].as_str().unwrap_or(std::env::consts::ARCH).into(),
        cpu: host["cpu"].as_str().unwrap_or("unknown").into(), gpu: host["gpu"].as_str().map(str::to_string),
        memory_bytes: host["memory_bytes"].as_u64().unwrap_or(1), browser: None,
        display: Some(Display { width_css: display["viewport_width_css"].as_u64().unwrap_or(0) as u32, height_css: display["viewport_height_css"].as_u64().unwrap_or(0) as u32, dpr: display["dpr"].as_f64().unwrap_or(1.0) }),
        build_profile: "release".into(), rustflags: std::env::var("RUSTFLAGS").unwrap_or_default(), tool_versions: tools,
        power: Power { source: power["source"].as_str().unwrap_or("unknown").into(), low_power_mode: power["low_power_mode"].as_bool(), notes: "controlled reference configuration".into() },
    })
}

fn consume_transport(items: &[RenderItem]) -> usize {
    let mut checksum = 0usize;
    for item in items { match item {
        RenderItem::Row(row) => { checksum ^= row.nums.len() ^ row.syms.len(); black_box(&row.data); }
        RenderItem::Batch(batch) => {
            checksum ^= batch.len ^ batch.schema.cols.len();
            for column in &batch.cols { checksum ^= match column { Column::Num(_) => 1, Column::NumOpt(v, _) => v.len(), Column::SymConst(v) => v.len(), Column::Syms(v) => v.len() }; }
            black_box((&batch.x, &batch.y, &batch.theta, &batch.scale, &batch.alpha, &batch.hue));
        }
    }}
    black_box(checksum)
}

fn ns(start: Instant) -> f64 { start.elapsed().as_nanos() as f64 }

async fn run() -> Result<(), String> {
    let args = args()?; let workload = load_workload(&args.workload)?; let generated = generate(&workload)?;
    let rss_start = peak_rss_bytes();
    let load_start = Instant::now();
    let mut sim = Sim::load(&generated.source, Some("bench"))?;
    sim.resize_entity_capacity(workload.entities.capacity.unwrap_or(workload.entities.plateau + 1024))?;
    let load_ns = ns(load_start);
    let mut mesh = TouhouMesh::new(Rc::new(TouhouProfile::stock()));
    let bind_start = Instant::now();
    if let Some(schema) = sim.declared_render_schema("sprite") { mesh.bind_schema("sprite", schema).map_err(|e| format!("bind sprite: {e:?}"))?; }
    if let Some(schema) = sim.declared_render_schema("beam") { mesh.bind_schema("beam", schema).map_err(|e| format!("bind beam: {e:?}"))?; }
    let bind_ns = ns(bind_start);
    let resource_start = Instant::now();
    let resources = RenderResources::resolve(mesh.profile())?;
    let resource_setup_ns = ns(resource_start);
    let warmup = if args.smoke { workload.cadence.warmup_ticks.min(2) } else { workload.cadence.warmup_ticks };
    for _ in 0..warmup { sim.step()?; }
    let warm = sim.render_frame(); consume_transport(&warm); mesh.build(&warm).map_err(|e| format!("pack warmup: {e:?}"))?;
    let prepared = prepare_frame(mesh.frame(), &resources, 640.0, 360.0); clear_background(BLACK); submit_frame(&prepared, &resources); next_frame().await;
    let frames = if args.smoke { workload.cadence.sample_frames.min(3) } else { workload.cadence.sample_frames };
    let batches = if args.smoke { 1 } else { workload.cadence.sample_batches };
    let mut samples = Vec::with_capacity(frames as usize * batches as usize);
    maku_bench::alloc::reset();
    for batch in 0..batches { for frame in 0..frames {
        let start = Instant::now();
        for _ in 0..workload.cadence.ticks_per_frame { sim.step()?; }
        let simulation_ns = ns(start);
        let start = Instant::now(); let items = sim.render_frame(); consume_transport(&items); let transport_ns = Some(ns(start));
        let start = Instant::now(); mesh.build(&items).map_err(|e| format!("pack build: {e:?}"))?; let pack_build_ns = Some(ns(start));
        let start = Instant::now(); let prepared = prepare_frame(mesh.frame(), &resources, 640.0, 360.0); let host_overhead_ns = Some(ns(start));
        clear_background(BLACK); let start = Instant::now(); submit_frame(&prepared, &resources); let adapter_submission_ns = Some(ns(start));
        let start = Instant::now(); next_frame().await; let presentation_ns = Some(ns(start));
        samples.push(FrameSample { batch, frame, ticks: workload.cadence.ticks_per_frame, simulation_ns: Some(simulation_ns), transport_ns, pack_build_ns,
            host_overhead_ns, adapter_submission_ns, completion_ns: None, presentation_ns, elapsed_clamped_ns: Some(0.0), memory_bytes: peak_rss_bytes(), raf_ticks: None, wall_ns: Some(simulation_ns + transport_ns.unwrap_or(0.0) + pack_build_ns.unwrap_or(0.0) + host_overhead_ns.unwrap_or(0.0) + adapter_submission_ns.unwrap_or(0.0)) });
    }}
    let allocation_snapshot = maku_bench::alloc::snapshot();
    let verification = verify(&mut sim, &workload);
    let core = sim.benchmark_counters(); let frame = mesh.frame();
    let mut sprite_instances = BTreeMap::new();
    sprite_instances.insert("basic".into(), frame.basic_sprites.len()); sprite_instances.insert("tinted".into(), frame.tinted_sprites.len()); sprite_instances.insert("recolor".into(), frame.recolor_sprites.len());
    let sprite_layers = sprite_instances.values().sum();
    let counters = WorkCounters { live_entities: core.live_entities, render_lanes: verification.observed.render_lanes, sprite_instances, sprite_layers,
        beam_segments: frame.indices.len() / 6, vertices: frame.vertices.len(), indices: frame.indices.len(), triangles: frame.indices.len() / 3, draw_commands: frame.draws.len(),
        collider_projections: core.collider_projections, active_query_pairs: core.active_query_pairs, collision_candidates: core.collision_candidates, contacts: core.contacts,
        rules: BTreeMap::from([(format!("{:?}", workload.rules.class).to_lowercase(), workload.rules.count as usize)]), predicate_matches: core.predicate_matches, rule_actions: core.rule_actions };
    let stage_values = |f: fn(&FrameSample) -> Option<f64>| samples.iter().filter_map(f).collect::<Vec<_>>();
    let mut summaries = BTreeMap::new();
    for (name, values) in [("simulation_ns", stage_values(|s| s.simulation_ns)), ("transport_ns", stage_values(|s| s.transport_ns)), ("pack_build_ns", stage_values(|s| s.pack_build_ns)), ("host_overhead_ns", stage_values(|s| s.host_overhead_ns)), ("adapter_submission_ns", stage_values(|s| s.adapter_submission_ns)), ("presentation_ns", stage_values(|s| s.presentation_ns))] {
        if let Some(summary) = summarize("ns", &values) { summaries.insert(name.into(), summary); }
    }
    let period = 1000.0 / workload.cadence.presentation_hz as f64;
    let mut byo = Vec::new(); let mut bundled = Vec::new(); let mut end = Vec::new();
    for sample in &samples { let stages = FrameStages { simulation_ms: sample.simulation_ns.unwrap_or(0.0)/1e6, transport_ms: sample.transport_ns.unwrap_or(0.0)/1e6, pack_build_ms: sample.pack_build_ns.unwrap_or(0.0)/1e6, host_overhead_ms: sample.host_overhead_ns.unwrap_or(0.0)/1e6, adapter_submission_ms: sample.adapter_submission_ns.unwrap_or(0.0)/1e6 }; let h=stages.headroom(0.0); byo.push(-h.0); bundled.push(-h.1); end.push(-h.2); }
    let headroom = Some(Headroom { period_ms: period, byo_ms: summarize_headroom(period, &byo).unwrap(), bundled_draw_ms: summarize_headroom(period, &bundled).unwrap(), end_to_end_ms: summarize_headroom(period, &end).unwrap() });
    let rev = revision(); if rev.len() != 40 { return Err("source revision must be a full 40-character hash".into()); }
    let captured = captured_at();
    let envelope = ResultEnvelope { schema_version: RESULT_SCHEMA_VERSION, series: "maku-v1-f64".into(), run_id: format!("{}-{}-{}", captured.replace([':', '-'], ""), workload.id, args.tier.name()), captured_at: captured,
        source: SourceIdentity { revision: rev, dirty: dirty(), workload_schema: WORKLOAD_SCHEMA_VERSION, result_schema: RESULT_SCHEMA_VERSION, generator: workload.generator_version.clone(), expanded_source_sha256: generated.source_sha256, input_tape_sha256: generated.input_tape_sha256 },
        fixture: FixtureIdentity { id: workload.id.clone(), family: format!("{:?}", workload.family).to_lowercase(), workload_sha256: generated.workload_sha256, seed: workload.seed, parameters: serde_json::to_value(&workload).unwrap() },
        stage: StageIdentity { executor: "interpreter-native".into(), tier: "host-draw".into(), adapter: args.tier.name().into() }, environment: environment(&args.environment)?,
        policy: TimingPolicy { tick_hz: 120, presentation_hz: 60, warmup_ticks: warmup, sample_frames: frames, sample_batches: batches, percentile_method: "nearest-rank".into(), wall_mode: "instrumented".into(), elapsed_clamp_ms: None, canvas_texture_cache_warm: None },
        correctness: Correctness { valid: verification.valid, state_digest: verification.observed.state_digest.clone(), expected: workload.expect.clone(), observed: verification.observed, errors: verification.errors }, counters,
        memory: Memory { rss_start_bytes: rss_start, rss_peak_bytes: peak_rss_bytes(), wasm_start_bytes: None, wasm_peak_bytes: None, allocations: allocation_snapshot.map(|v| v.0), allocated_bytes: allocation_snapshot.map(|v| v.1) },
        cold_setup: ColdSetup { load_ns: Some(load_ns), schema_bind_ns: Some(bind_ns), resource_setup_ns: Some(resource_setup_ns) }, samples, summaries, headroom,
        outcome: Outcome { status: if verification.valid { "success" } else { "invalid" }.into(), last_successful_plateau: verification.valid.then_some(workload.entities.plateau), failure_class: (!verification.valid).then(|| "semantic-mismatch".into()), message: None } };
    if let Some(parent) = args.output.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    std::fs::write(&args.output, serde_json::to_vec_pretty(&envelope).unwrap()).map_err(|e| e.to_string())?;
    if !envelope.correctness.valid { return Err(format!("semantic verification failed: {:?}", envelope.correctness.errors)); }
    println!("{}", args.output.display()); Ok(())
}

fn window_conf() -> macroquad::prelude::Conf {
    macroquad::prelude::Conf { window_title: "Maku native benchmark".into(), window_width: 1280, window_height: 720, high_dpi: false, ..Default::default() }
}

#[macroquad::main(window_conf)]
async fn main() {
    if let Err(error) = run().await { eprintln!("{error}"); std::process::exit(1); }
}
