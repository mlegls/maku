#!/usr/bin/env bun
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const [mode, destinationArg] = process.argv.slice(2);
if (!['--check', '--write'].includes(mode) || !destinationArg) {
  console.error('usage: bun scripts/sync-neen-maku.mjs --check|--write /path/to/neen-ink/projects/maku');
  process.exit(2);
}
const root = resolve(import.meta.dir, '..');
const destination = resolve(destinationArg);
const manifest = JSON.parse(readFileSync(join(root, 'crates/web/release-sync.json'), 'utf8'));
const mismatches = [];
for (const entry of manifest.files) {
  const sourcePath = join(root, entry.source);
  const targetPath = join(destination, entry.target);
  const source = readFileSync(sourcePath);
  let target;
  try { target = readFileSync(targetPath); } catch { target = null; }
  if (!target?.equals(source)) {
    mismatches.push(`${entry.target} <= ${entry.source}`);
    if (mode === '--write') {
      mkdirSync(dirname(targetPath), { recursive: true });
      writeFileSync(targetPath, source);
    }
  }
}
if (mode === '--check' && mismatches.length) {
  console.error(`downstream Maku snapshot differs in ${mismatches.length} declared files:\n${mismatches.join('\n')}`);
  process.exit(1);
}
console.log(`${mode === '--write' ? 'synchronized' : 'verified'} ${manifest.files.length} files (${mismatches.length} changed)`);
console.log(`preserved site-owned paths: ${manifest.preserve.join(', ')}`);
