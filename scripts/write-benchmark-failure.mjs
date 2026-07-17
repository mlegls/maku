import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
const [workloadPath, host, tierArg, environmentPath, outputPath, failureClass='runtime-error', ...messageParts]=process.argv.slice(2);
if(!outputPath) throw new Error('usage: write-benchmark-failure.mjs WORKLOAD native|browser TIER ENV OUTPUT CLASS MESSAGE');
const workload=JSON.parse(readFileSync(resolve(workloadPath),'utf8'));
const fixture=JSON.parse(readFileSync(resolve(`bench/fixtures/v1/${workload.id}.fixture.json`),'utf8'));
const env=JSON.parse(readFileSync(resolve(environmentPath),'utf8'));
const revision=JSON.parse(readFileSync('crates/js/maku/wasm/release.json','utf8')).source_revision;
const browser=host==='browser'?{name:env.browser.name,version:env.browser.version}:null;
const display=host==='browser'?{width_css:env.display.viewport_width_css,height_css:env.display.viewport_height_css,dpr:env.display.dpr}:null;
const stageTier=tierArg==='web-canvas2d'||tierArg==='native-macroquad-compat'?'host-draw':tierArg;
const adapter=['web-canvas2d','native-macroquad-compat'].includes(tierArg)?tierArg:'none';
const zero={live_entities:0,render_lanes:0,sprite_instances:{},sprite_layers:0,beam_segments:0,vertices:0,indices:0,triangles:0,draw_commands:0,collider_projections:0,active_query_pairs:0,collision_candidates:0,contacts:0,rules:{},predicate_matches:0,rule_actions:0};
const inferredLast = workload.entities.plateau >= 1_000_000 ? 100_000 : workload.entities.plateau >= 100_000 ? 10_000 : null;
const now=new Date().toISOString();
const out={schema_version:1,series:'maku-v1-f64',run_id:`${now.replace(/[-:.]/g,'')}-${workload.id}-${tierArg}-failure`,captured_at:now,
 source:{revision,dirty:false,workload_schema:1,result_schema:1,generator:workload.generator_version,expanded_source_sha256:fixture.expanded_source_sha256,input_tape_sha256:fixture.input_tape_sha256},fixture:{id:workload.id,family:workload.family,workload_sha256:fixture.workload_sha256,seed:workload.seed,parameters:workload},
 stage:{executor:host==='browser'?'interpreter-wasm':'interpreter-native',tier:stageTier,adapter},environment:{environment_id:env.environment_id,os:env.host.os,arch:env.host.arch,cpu:env.host.cpu,gpu:env.host.gpu??null,memory_bytes:env.host.memory_bytes,browser,display,build_profile:'release',rustflags:env.build.rustflags??'',tool_versions:env.tools,power:{source:env.power.source,low_power_mode:env.power.low_power_mode??null,notes:'controlled bounded-failure record'}},
 policy:{tick_hz:workload.cadence.tick_hz,presentation_hz:workload.cadence.presentation_hz,warmup_ticks:workload.cadence.warmup_ticks,sample_frames:workload.cadence.sample_frames,sample_batches:workload.cadence.sample_batches,percentile_method:'nearest-rank',wall_mode:'instrumented',elapsed_clamp_ms:host==='browser'?250:null,canvas_texture_cache_warm:null},
 correctness:{valid:false,state_digest:'0000000000000000',expected:workload.expect,observed:{live_entities:0,render_lanes:0,render_lanes_by_kind:{},contacts_per_tick:0,rule_matches_per_tick:0,rule_actions_per_tick:0,state_digest:'0000000000000000'},errors:[messageParts.join(' ')||failureClass]},counters:zero,
 memory:{rss_start_bytes:null,rss_peak_bytes:null,wasm_start_bytes:null,wasm_peak_bytes:null,allocations:null,allocated_bytes:null},cold_setup:{load_ns:null,schema_bind_ns:null,resource_setup_ns:null},samples:[],summaries:{},headroom:null,
 outcome:{status:'bounded-failure',last_successful_plateau:process.env.MAKU_LAST_SUCCESSFUL_PLATEAU?Number(process.env.MAKU_LAST_SUCCESSFUL_PLATEAU):inferredLast,failure_class:failureClass,message:messageParts.join(' ')||failureClass}};
writeFileSync(resolve(outputPath),JSON.stringify(out,null,2)+'\n'); console.log(resolve(outputPath));
