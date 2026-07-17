#!/bin/sh
# Interleaved minimally-instrumented versus attributed walls. This diagnoses
# instrumentation disagreement; optimization verdicts still use perf-spec A/B.
set -eu
repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd); cd "$repo_root"
workload=${1:-}; tier=${2:-}; environment=${3:-}; out=${4:-}
[ -f "$workload" ] && [ -f "$environment" ] && [ -n "$out" ] || { echo "usage: $0 WORKLOAD TIER ENVIRONMENT OUTPUT_DIR" >&2; exit 2; }
[ "$tier" != native-macroquad-compat ] || { echo "minimal Macroquad walls are not yet supported" >&2; exit 2; }
mkdir -p "$out"; revision=$(jq -r .source_revision crates/js/maku/wasm/release.json)
MAKU_SOURCE_REVISION="$revision" cargo build --release --locked --manifest-path crates/Cargo.toml -p maku-bench --bin maku-bench-native
n=0
for mode in instrumented minimal minimal instrumented; do
  n=$((n+1)); MAKU_SOURCE_REVISION="$revision" crates/target/release/maku-bench-native "$workload" --tier "$tier" --wall-mode "$mode" --environment "$environment" --output "$out/$n-$mode.json"
done
bun scripts/check-benchmarks.mjs "$out"/*.json
bun - "$out" <<'EOF'
import {readdirSync,readFileSync,writeFileSync} from 'node:fs';
const dir=process.argv[2], values={instrumented:[],minimal:[]};
for(const name of readdirSync(dir).filter(n=>n.endsWith('.json'))){const r=JSON.parse(readFileSync(`${dir}/${name}`,'utf8'));if(values[r.policy?.wall_mode])values[r.policy.wall_mode].push(r.summaries.wall_ns.median);}
const mean=a=>a.reduce((x,y)=>x+y,0)/a.length, instrumented=mean(values.instrumented), minimal=mean(values.minimal), relative=(instrumented-minimal)/minimal;
const report={instrumented_median_wall_ns:instrumented,minimal_median_wall_ns:minimal,relative_disagreement:relative,threshold:0.10,status:Math.abs(relative)>0.10?'disagreement':'consistent'};
writeFileSync(`${dir}/wall-comparison.json`,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
EOF
