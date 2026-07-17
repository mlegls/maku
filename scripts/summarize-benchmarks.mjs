import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const args = process.argv.slice(2);
const take = flag => { const i=args.indexOf(flag); if(i<0) return null; const v=args[i+1]; args.splice(i,2); return v; };
const output = take('--output'), csvOutput = take('--csv');
if (!output || !csvOutput || !args.length) throw new Error('usage: summarize-benchmarks.mjs RESULTS... --output REPORT.md --csv TABLE.csv');
const results = args.map(path => ({ path, value: JSON.parse(readFileSync(resolve(path), 'utf8')) }));

function assertClaimReady(path, r) {
  const missing=[];
  if (!r.fixture?.id || !r.fixture?.workload_sha256) missing.push('fixture identity');
  if (!r.source?.revision?.match(/^[0-9a-f]{40}$/) || r.source.dirty) missing.push('clean source revision');
  if (!r.stage?.executor || !r.stage?.adapter || !r.stage?.tier) missing.push('backend/adapter identity');
  if (!r.environment?.environment_id || !r.environment?.cpu || !r.environment?.build_profile) missing.push('environment/build identity');
  if (r.policy?.tick_hz !== 120 || r.policy?.presentation_hz !== 60 || !r.policy?.sample_frames) missing.push('cadence/sample policy');
  if (!r.counters || r.counters.live_entities === undefined || r.counters.render_lanes === undefined || r.counters.predicate_matches === undefined) missing.push('workload counters');
  const bounded = r.outcome?.status === 'bounded-failure';
  if (!bounded && (!r.correctness?.valid || r.outcome?.status !== 'success')) missing.push('successful semantic verification');
  const stage = r.summaries?.simulation_ns;
  if (!bounded && (!stage || stage.p95 === undefined || stage.p99 === undefined)) missing.push('p95/p99 distributions');
  if (missing.length) throw new Error(`${path} is not claim-ready: ${missing.join(', ')}`);
}
for (const {path,value} of results) assertClaimReady(path,value);
results.sort((a,b) => a.value.counters.live_entities-b.value.counters.live_entities || a.value.fixture.id.localeCompare(b.value.fixture.id) || a.value.stage.tier.localeCompare(b.value.stage.tier));
const ms = value => value == null ? '' : (value/1e6).toFixed(3);
const margin = r => r.headroom?.end_to_end_ms?.p95;
const rows = results.map(({value:r}) => [r.fixture.id,r.stage.executor,r.stage.tier,r.stage.adapter,r.counters.live_entities,r.counters.render_lanes,r.counters.contacts,r.counters.predicate_matches,ms(r.summaries.simulation_ns?.p95),ms(r.summaries.transport_ns?.p95),ms(r.summaries.pack_build_ns?.p95),ms(r.summaries.adapter_submission_ns?.p95),margin(r)?.toFixed(3)??'',r.memory.rss_peak_bytes??r.memory.wasm_peak_bytes??'',r.outcome.last_successful_plateau??'',r.outcome.status,r.outcome.failure_class??'',r.source.revision]);
const headers=['fixture','executor','tier','adapter','entities','render lanes','contacts','predicate matches','simulation p95 ms','transport p95 ms','pack p95 ms','adapter p95 ms','end-to-end p95 margin ms','peak memory bytes','last successful plateau','outcome','failure class','revision'];
const esc = v => `"${String(v).replaceAll('"','""')}"`;
writeFileSync(resolve(csvOutput),[headers,...rows].map(row=>row.map(esc).join(',')).join('\n')+'\n');
const md=[];
md.push('# Maku benchmark summary','',`Results: ${rows.length} claim-ready envelopes.`,'',
  '> Every row is fixture-specific. Canvas2D, native compatibility, and BYO tiers are distinct; these results do not imply a universal maximum bullet count.','',
  '| '+headers.slice(0,-1).join(' | ')+' |','|'+headers.slice(0,-1).map(()=> '---').join('|')+'|');
for(const row of rows) md.push('| '+row.slice(0,-1).join(' | ')+' |');
md.push('','## Provenance','');
for(const {value:r} of results) md.push(`- \`${r.run_id}\`: fixture \`${r.fixture.id}\`, ${r.stage.executor}/\`${r.stage.adapter}\` \`${r.stage.tier}\`, ${r.policy.tick_hz} Hz simulation / ${r.policy.presentation_hz} Hz presentation, nearest-rank p95/p99, environment \`${r.environment.environment_id}\`, revision \`${r.source.revision}\`.`);
writeFileSync(resolve(output),md.join('\n')+'\n');
console.log(`${resolve(output)} (${rows.length} results)`);
