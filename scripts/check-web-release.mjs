#!/usr/bin/env bun
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dir, '..');
const releasePath = join(root, 'crates/web/static/pkg/release.json');
const release = JSON.parse(readFileSync(releasePath, 'utf8'));
const copied = JSON.parse(readFileSync(join(root, 'crates/js/maku/wasm/release.json'), 'utf8'));
if (JSON.stringify(release) !== JSON.stringify(copied)) {
  throw new Error('web and npm release manifests differ');
}
if (release.npm_package !== '@mlegls/maku' || release.frame_abi_version !== 1) {
  throw new Error('unexpected browser release identity');
}
const npm = JSON.parse(readFileSync(join(root, 'crates/js/maku/package.json'), 'utf8'));
if (release.npm_version !== npm.version || release.maku_version !== npm.version) {
  throw new Error(`package version mismatch: ${JSON.stringify(release)}`);
}
if (JSON.stringify(release.render_packs) !== JSON.stringify([{ id: 'touhou', contract_version: 1 }])) {
  throw new Error(`unexpected bundled render packs: ${JSON.stringify(release.render_packs)}`);
}
for (const [path, expected] of Object.entries(release.artifacts || {})) {
  const bytes = readFileSync(join(root, path));
  const actual = createHash('sha256').update(bytes).digest('hex');
  if (bytes.byteLength !== expected.bytes || actual !== expected.sha256) {
    throw new Error(`artifact integrity mismatch: ${path}`);
  }
}
if (!Object.keys(release.artifacts || {}).some(path => path.endsWith('.wasm'))
    || !Object.keys(release.artifacts || {}).includes('crates/web/static/main.js')
    || !Object.keys(release.artifacts || {}).includes('crates/web/static/canvas-renderer.js')) {
  throw new Error('release manifest omits wasm or renderer integration');
}
const selection = await import(pathToFileURL(join(root, 'crates/web/static/manifest.js')));
const sync = JSON.parse(readFileSync(join(root, 'crates/web/release-sync.json'), 'utf8'));
const synchronizedSources = new Set(sync.files.map(entry => entry.source));
for (const path of [...selection.LIB_FILES, ...selection.CARD_FILES, ...selection.TUTORIALS.map(v => v.doc)]) {
  if (!synchronizedSources.has(path)) throw new Error(`web selection is absent from downstream sync: ${path}`);
  if (!release.artifacts[path]) throw new Error(`web selection is absent from release integrity: ${path}`);
}
console.log(`web release integrity OK (${Object.keys(release.artifacts).length} artifacts)`);
