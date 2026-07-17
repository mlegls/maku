import { chromium } from 'playwright';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = resolve(import.meta.dir, '..');
const argv = process.argv.slice(2);
const workloadPath = argv.shift();
const take = flag => { const i = argv.indexOf(flag); if (i < 0) return null; const value = argv[i + 1]; argv.splice(i, 2); return value; };
const tierArg = take('--tier') ?? 'byo-transport';
const outputPath = take('--output');
const environmentPath = take('--environment');
const smoke = argv.includes('--smoke');
if (!workloadPath || !outputPath || !['simulation-only', 'byo-transport', 'touhou-pack', 'web-canvas2d'].includes(tierArg)) {
  throw new Error('usage: run-browser-benchmark.mjs WORKLOAD --tier TIER --output FILE [--environment FILE] [--smoke]');
}
const workload = JSON.parse(readFileSync(resolve(workloadPath), 'utf8'));
const fixture = JSON.parse(readFileSync(resolve(root, `bench/fixtures/v1/${workload.id}.fixture.json`), 'utf8'));
const source = readFileSync(resolve(root, fixture.expanded_source), 'utf8');
const envRecord = environmentPath ? JSON.parse(readFileSync(resolve(environmentPath), 'utf8')) : null;
const command = (...args) => spawnSync(args[0], args.slice(1), { cwd: root, encoding: 'utf8' }).stdout.trim();
const dirty = command('git', 'status', '--porcelain').length > 0;

const mime = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.wasm': 'application/wasm', '.json': 'application/json', '.maku': 'text/plain' };
const server = Bun.serve({ port: 0, async fetch(request) {
  const url = new URL(request.url); let path = decodeURIComponent(url.pathname);
  if (path === '/') path = '/bench/browser/runner.html';
  const file = Bun.file(root + path); if (!(await file.exists())) return new Response('not found', { status: 404 });
  const ext = path.slice(path.lastIndexOf('.'));
  return new Response(file, { headers: { 'content-type': mime[ext] ?? 'application/octet-stream' } });
}});

let browser;
try {
  browser = await chromium.launch({ headless: true });
  const width = envRecord?.display?.viewport_width_css ?? 1280;
  const height = envRecord?.display?.viewport_height_css ?? 720;
  const dpr = envRecord?.display?.dpr ?? 1;
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, locale: 'en-US', timezoneId: 'UTC', reducedMotion: 'reduce' });
  const page = await context.newPage();
  await page.goto(`http://127.0.0.1:${server.port}/`, { waitUntil: 'load' });
  const browserVersion = browser.version();
  const observed = await page.evaluate(async ({ workload, source, tierArg, smoke }) => {
    const api = await import('/crates/js/maku/dist/index.js');
    const wasm = await api.initMaku();
    const maku = api.createMaku(null);
    const wasmStart = wasm.memory.buffer.byteLength;
    let wasmPeak = wasmStart;
    const loadStart = performance.now();
    maku.add_file('/bench.maku', source);
    maku.boot('/bench.maku', 'bench');
    maku.command(`(resize-entities ${workload.entities.capacity ?? workload.entities.plateau + 1024})`);
    const loadNs = (performance.now() - loadStart) * 1e6;
    const warmup = smoke ? Math.min(workload.cadence.warmup_ticks, 2) : workload.cadence.warmup_ticks;
    maku.step(warmup);
    maku.benchmark_render_transport();
    maku.benchmark_build_pack();
    let renderer = null, resourceSetupNs = null;
    if (tierArg === 'web-canvas2d') {
      const canvas = document.createElement('canvas'); canvas.width = innerWidth * devicePixelRatio; canvas.height = innerHeight * devicePixelRatio; document.body.append(canvas);
      const { createCanvas2DRenderer } = await import('/crates/web/static/canvas-renderer.js');
      renderer = createCanvas2DRenderer({ maku, context: canvas.getContext('2d'), worldToCanvas: [x => canvas.width / 2 + x * 55, y => canvas.height / 2 - y * 55], pixelsPerUnit: 55 });
      const start = performance.now(); await renderer.resolveManifest(); renderer.drawBuiltFrame(); resourceSetupNs = (performance.now() - start) * 1e6;
    }
    const frames = smoke ? Math.min(workload.cadence.sample_frames, 3) : workload.cadence.sample_frames;
    const batches = smoke ? 1 : workload.cadence.sample_batches;
    const samples = []; let priorRaf = null;
    for (let batch = 0; batch < batches; batch++) for (let frame = 0; frame < frames; frame++) {
      let rafDelta = null;
      if (tierArg === 'web-canvas2d') await new Promise(resolve => requestAnimationFrame(now => { if (priorRaf !== null) rafDelta = now - priorRaf; priorRaf = now; resolve(); }));
      const frameStart = performance.now();
      const simStart = performance.now(); maku.step(workload.cadence.ticks_per_frame); const simulationNs = (performance.now() - simStart) * 1e6;
      let transportNs = null, packBuildNs = null, adapterSubmissionNs = null;
      if (tierArg !== 'simulation-only') { const start = performance.now(); maku.benchmark_render_transport(); transportNs = (performance.now() - start) * 1e6; }
      if (tierArg === 'touhou-pack' || tierArg === 'web-canvas2d') { const start = performance.now(); maku.benchmark_build_pack(); packBuildNs = (performance.now() - start) * 1e6; }
      if (renderer) { const start = performance.now(); renderer.drawBuiltFrame(); adapterSubmissionNs = (performance.now() - start) * 1e6; }
      wasmPeak = Math.max(wasmPeak, wasm.memory.buffer.byteLength);
      samples.push({ batch, frame, ticks: workload.cadence.ticks_per_frame, simulation_ns: simulationNs, transport_ns: transportNs, pack_build_ns: packBuildNs,
        host_overhead_ns: 0, adapter_submission_ns: adapterSubmissionNs, completion_ns: null, presentation_ns: rafDelta === null ? null : rafDelta * 1e6,
        elapsed_clamped_ns: rafDelta === null ? 0 : Math.max(0, rafDelta - 250) * 1e6, memory_bytes: wasm.memory.buffer.byteLength, raf_ticks: renderer ? 1 : null, wall_ns: (performance.now() - frameStart) * 1e6 });
    }
    const lanes = maku.benchmark_render_transport(); maku.benchmark_build_pack();
    const basic = maku.basic_sprites().byteLength / maku.basic_sprite_stride(), tinted = maku.tinted_sprites().byteLength / maku.tinted_sprite_stride(), recolor = maku.recolor_sprites().byteLength / maku.recolor_sprite_stride();
    const indices = maku.strip_indices().length, vertices = maku.strip_vertices().byteLength / maku.strip_vertex_stride();
    return { samples, warmup, frames, batches, loadNs, resourceSetupNs, wasmStart, wasmPeak, lanes,
      digest: maku.benchmark_digest(), live: maku.entity_count(), contacts: maku.benchmark_contacts(), projections: maku.benchmark_collider_projections(), pairs: maku.benchmark_active_query_pairs(), candidates: maku.benchmark_collision_candidates(), matches: maku.benchmark_predicate_matches(), actions: maku.benchmark_rule_actions(),
      basic, tinted, recolor, indices, vertices, commands: maku.draw_commands().length / maku.draw_command_stride(), identity: api.releaseIdentity() };
  }, { workload, source, tierArg, smoke });

  const nearest = (values, p) => [...values].sort((a,b) => a-b)[Math.max(0, Math.ceil(values.length * p) - 1)];
  const summary = (unit, values) => ({ unit, count: values.length, median: nearest(values, .5), p95: nearest(values, .95), p99: nearest(values, .99), max: Math.max(...values) });
  const summaries = {};
  for (const key of ['simulation_ns','transport_ns','pack_build_ns','adapter_submission_ns','presentation_ns']) { const values = observed.samples.map(s => s[key]).filter(v => v !== null); if (values.length) summaries[key] = summary('ns', values); }
  const period = 1000 / workload.cadence.presentation_hz;
  const costs = observed.samples.map(s => { const sim=s.simulation_ns/1e6,t=(s.transport_ns??0)/1e6,b=(s.pack_build_ns??0)/1e6,h=(s.host_overhead_ns??0)/1e6,d=(s.adapter_submission_ns??0)/1e6; return [sim+t+h,sim+t+b+h,sim+t+b+h+d]; });
  const headroomSummary = values => { const d=summary('ms',values); return {...d,median:period-d.median,p95:period-d.p95,p99:period-d.p99,max:period-d.max}; };
  const errors = [];
  for (const [label, actual, expected] of [['live entities',observed.live,workload.expect.live_entities],['render lanes',observed.lanes,workload.expect.render_lanes],['contacts/tick',observed.contacts,workload.expect.contacts_per_tick],['rule matches/tick',observed.matches,workload.expect.rule_matches_per_tick],['rule actions/tick',observed.actions,workload.expect.rule_actions_per_tick]]) if (actual !== expected) errors.push(`${label}: expected ${expected}, got ${actual}`);
  const now = new Date().toISOString();
  const runtimeRevision = observed.identity.sourceRevision;
  if (!/^[0-9a-f]{40}$/.test(runtimeRevision)) throw new Error(`benchmark wasm lacks a release source revision: ${runtimeRevision}`);
  const baseHost = envRecord?.host ?? {};
  const envelope = { schema_version:1, series:'maku-v1-f64', run_id:`${now.replace(/[-:.]/g,'')}-${workload.id}-${tierArg}`, captured_at:now,
    source:{revision:runtimeRevision,dirty,workload_schema:1,result_schema:1,generator:workload.generator_version,expanded_source_sha256:fixture.expanded_source_sha256,input_tape_sha256:fixture.input_tape_sha256},
    fixture:{id:workload.id,family:workload.family,workload_sha256:fixture.workload_sha256,seed:workload.seed,parameters:workload},
    stage:{executor:'interpreter-wasm',tier:tierArg==='web-canvas2d'?'host-draw':tierArg,adapter:tierArg==='web-canvas2d'?'web-canvas2d':'none'},
    environment:{environment_id:envRecord?.environment_id??`playwright-${process.platform}-${process.arch}`,os:baseHost.os??process.platform,arch:baseHost.arch??process.arch,cpu:baseHost.cpu??'CI/unspecified',gpu:baseHost.gpu??null,memory_bytes:baseHost.memory_bytes??1,browser:{name:'Playwright Chromium',version:browserVersion},display:{width_css:width,height_css:height,dpr},build_profile:'release',rustflags:envRecord?.build?.rustflags??'',tool_versions:{...(envRecord?.tools??{}),playwright:'1.55.0'},power:{source:envRecord?.power?.source??'unknown',low_power_mode:envRecord?.power?.low_power_mode??null,notes:envRecord?'controlled reference configuration':'structural smoke environment'}},
    policy:{tick_hz:workload.cadence.tick_hz,presentation_hz:workload.cadence.presentation_hz,warmup_ticks:observed.warmup,sample_frames:observed.frames,sample_batches:observed.batches,percentile_method:'nearest-rank',wall_mode:'instrumented',elapsed_clamp_ms:tierArg==='web-canvas2d'?250:null,canvas_texture_cache_warm:tierArg==='web-canvas2d'?true:null},
    correctness:{valid:errors.length===0,state_digest:observed.digest,expected:workload.expect,observed:{live_entities:observed.live,render_lanes:observed.lanes,render_lanes_by_kind:{sprite:observed.lanes},contacts_per_tick:observed.contacts,rule_matches_per_tick:observed.matches,rule_actions_per_tick:observed.actions,state_digest:observed.digest},errors},
    counters:{live_entities:observed.live,render_lanes:observed.lanes,sprite_instances:{basic:observed.basic,tinted:observed.tinted,recolor:observed.recolor},sprite_layers:observed.basic+observed.tinted+observed.recolor,beam_segments:observed.indices/6,vertices:observed.vertices,indices:observed.indices,triangles:observed.indices/3,draw_commands:observed.commands,collider_projections:observed.projections,active_query_pairs:observed.pairs,collision_candidates:observed.candidates,contacts:observed.contacts,rules:{[workload.rules.class]:workload.rules.count},predicate_matches:observed.matches,rule_actions:observed.actions},
    memory:{rss_start_bytes:null,rss_peak_bytes:null,wasm_start_bytes:observed.wasmStart,wasm_peak_bytes:observed.wasmPeak,allocations:null,allocated_bytes:null},cold_setup:{load_ns:observed.loadNs,schema_bind_ns:null,resource_setup_ns:observed.resourceSetupNs},samples:observed.samples,summaries,
    headroom:{period_ms:period,byo_ms:headroomSummary(costs.map(v=>v[0])),bundled_draw_ms:headroomSummary(costs.map(v=>v[1])),end_to_end_ms:headroomSummary(costs.map(v=>v[2]))},
    outcome:{status:errors.length?'invalid':'success',last_successful_plateau:errors.length?null:workload.entities.plateau,failure_class:errors.length?'semantic-mismatch':null,message:null}};
  mkdirSync(dirname(resolve(outputPath)),{recursive:true}); writeFileSync(resolve(outputPath),JSON.stringify(envelope,null,2)+'\n');
  console.log(resolve(outputPath)); if(errors.length) process.exitCode=1;
  await context.close();
} finally { if(browser) await browser.close(); server.stop(true); }
