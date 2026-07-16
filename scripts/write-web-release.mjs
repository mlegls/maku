#!/usr/bin/env bun
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dir, '..');
const runtimePaths = [
  'crates/web/static/pkg/maku_bg.wasm',
  'crates/web/static/pkg/maku.js',
  'crates/web/static/pkg/maku.d.ts',
  'crates/web/static/pkg/maku_bg.wasm.d.ts',
  'crates/js/maku/dist/index.js',
  'crates/js/maku/dist/index.d.ts',
  'crates/web/static/main.js',
  'crates/web/static/canvas-renderer.js',
  'crates/web/static/manifest.js',
  'crates/web/static/index.html',
  'crates/web/static/tutorials.html',
  'crates/web/static/reader.js',
  'crates/web/static/markdown.js',
  'crates/web/static/maku-highlight.js',
  'crates/web/static/maku-codemirror.js',
];
const syncManifestPath = 'crates/web/release-sync.json';
const syncManifest = JSON.parse(readFileSync(join(root, syncManifestPath), 'utf8'));
const paths = Array.from(new Set([
  ...runtimePaths,
  syncManifestPath,
  ...syncManifest.files.map(entry => entry.source).filter(path => path !== 'crates/js/maku/wasm/release.json'),
]));
const artifacts = Object.fromEntries(paths.map(path => {
  const bytes = readFileSync(join(root, path));
  return [path, {
    bytes: bytes.byteLength,
    sha256: createHash('sha256').update(bytes).digest('hex'),
  }];
}));
const required = name => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
};
const release = {
  maku_version: required('MAKU_ENGINE_VERSION'),
  render_pack_version: required('MAKU_RENDER_VERSION'),
  web_host_version: required('MAKU_WEB_VERSION'),
  npm_package: '@mlegls/maku',
  npm_version: required('MAKU_NPM_VERSION'),
  frame_abi_version: Number(required('MAKU_FRAME_ABI_VERSION')),
  source_revision: required('MAKU_SOURCE_REVISION'),
  rustc: required('MAKU_RUSTC_VERSION'),
  wasm_pack: required('MAKU_WASM_PACK_VERSION'),
  bun: required('MAKU_BUN_VERSION'),
  artifacts,
};
const json = `${JSON.stringify(release, null, 2)}\n`;
writeFileSync(join(root, 'crates/web/static/pkg/release.json'), json);
writeFileSync(join(root, 'crates/js/maku/wasm/release.json'), json);
console.log(`release manifest: ${paths.length} artifacts`);
