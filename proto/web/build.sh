#!/bin/sh
# Build the wasm package and serve the web host.
#   ./build.sh          build only
#   ./build.sh serve    build + serve the repo root on :8000
set -e
cd "$(dirname "$0")"
wasm-pack build --target web --out-dir static/pkg
echo "built -> proto/web/static/pkg"
if [ "$1" = "serve" ]; then
  echo "open http://localhost:8000/proto/web/static/"
  cd ../..
  exec bun -e '
const root = process.cwd();
const mime = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".md": "text/markdown; charset=utf-8",
  ".dmk": "text/plain; charset=utf-8",
};
Bun.serve({
  port: 8000,
  async fetch(req) {
    const url = new URL(req.url);
    const path = decodeURIComponent(url.pathname === "/" ? "/proto/web/static/" : url.pathname);
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
