#!/bin/sh
# Build the wasm package and serve the web host.
#   ./build.sh          build only
#   ./build.sh serve    build + serve the repo root on :8000
set -e
cd "$(dirname "$0")"
SOURCE_REVISION="${MAKU_SOURCE_REVISION:-development}"
export MAKU_SOURCE_REVISION="$SOURCE_REVISION"
wasm-pack build . --target web --out-dir static/pkg --out-name maku
cp static/pkg/maku.js static/pkg/maku.d.ts static/pkg/maku_bg.wasm static/pkg/maku_bg.wasm.d.ts ../js/maku/wasm/
VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "maku-web") | .version')
cat > static/pkg/release.json <<EOF
{
  "maku_version": "$VERSION",
  "frame_abi_version": 1,
  "source_revision": "$SOURCE_REVISION",
  "rustc": "$(rustc --version)",
  "wasm_pack": "$(wasm-pack --version)",
  "bun": "$(bun --version)"
}
EOF
cp static/pkg/release.json ../js/maku/wasm/release.json
(cd ../js/maku && bun run build)
bun build editor-src/maku-codemirror.js --outfile static/maku-codemirror.js --target browser --format esm
echo "built -> crates/web/static/pkg"
if [ "$1" = "serve" ]; then
  PORT="${PORT:-8000}"
  export PORT
  echo "open http://localhost:${PORT}/crates/web/static/"
  cd ../..
  exec bun -e '
const root = process.cwd();
const port = Number(process.env.PORT || 8000);
const mime = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".md": "text/markdown; charset=utf-8",
  ".maku": "text/plain; charset=utf-8",
};
Bun.serve({
  port,
  async fetch(req) {
    const url = new URL(req.url);
    const rawPath = decodeURIComponent(url.pathname === "/" ? "/crates/web/static/" : url.pathname);
    const path = rawPath.endsWith("/") ? rawPath + "index.html" : rawPath;
    const file = Bun.file(root + path);
    if (!(await file.exists())) return new Response("not found", { status: 404 });
    const ext = path.slice(path.lastIndexOf("."));
    return new Response(file, { headers: { "content-type": mime[ext] || "application/octet-stream" } });
  },
});
console.log("serving " + root);
await new Promise(() => {});
'
fi
