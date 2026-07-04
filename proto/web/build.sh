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
  exec python3 -m http.server 8000
fi
