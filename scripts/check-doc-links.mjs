#!/usr/bin/env bun
import { existsSync, lstatSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';

const root = resolve(import.meta.dir, '..');
const ignored = new Set(['.git', 'target', 'node_modules']);
const markdown = [];
function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (ignored.has(entry.name)) continue;
    const path = join(dir, entry.name);
    if (relative(root, path) === join('crates', 'web', 'static', 'pkg')) continue;
    if (entry.isDirectory()) walk(path);
    else if (entry.name.endsWith('.md')) markdown.push(path);
  }
}
for (const start of ['README.md', 'docs', 'crates']) {
  const path = join(root, start);
  if (lstatSync(path).isDirectory()) walk(path); else markdown.push(path);
}

const failures = [];
for (const file of markdown) {
  const source = readFileSync(file, 'utf8');
  const links = source.matchAll(/\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g);
  for (const match of links) {
    const raw = match[1];
    if (/^(?:https?:|mailto:|#)/.test(raw)) continue;
    const clean = decodeURIComponent(raw.split('#', 1)[0].split('?', 1)[0]);
    if (!clean) continue;
    const target = resolve(dirname(file), clean);
    if (!existsSync(target)) failures.push(`${relative(root, file)}: missing link ${raw}`);
  }
  if (file.includes(`${join('docs', 'tutorials')}`)) {
    for (const match of source.matchAll(/Runnable companion:\s+\*\*`([^`]+\.maku)`\*\*/g)) {
      if (!existsSync(join(root, match[1]))) {
        failures.push(`${relative(root, file)}: missing runnable companion ${match[1]}`);
      }
    }
  }
}
if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log(`documentation links OK (${markdown.length} Markdown files)`);
