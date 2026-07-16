#!/usr/bin/env bun
import { readdir, readFile } from 'node:fs/promises';
import { extname, join } from 'node:path';

const roots = ['cards', 'crates/core/lib', 'crates/core/src', 'crates/web/editor-src', 'crates/web/static', 'docs'];
const textExtensions = new Set(['.rs', '.maku', '.md', '.js', '.mjs', '.ts', '.html']);
const inventory = await readFile('compatibility.toml', 'utf8');
const removed = [...inventory.matchAll(/\[\[form\]\]([\s\S]*?)(?=\n\[\[form\]\]|$)/g)]
  .map(([, block]) => ({
    name: block.match(/^name = "([^"]+)"/m)?.[1],
    kind: block.match(/^kind = "([^"]+)"/m)?.[1],
    disposition: block.match(/^disposition = "([^"]+)"/m)?.[1],
  }))
  .filter(({ kind, disposition }) => disposition === 'remove' && ['source-alias', 'library-alias'].includes(kind));

const escaped = removed.map(({ name }) => name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
const banned = new RegExp(`(?<![%\\w-])(?:${escaped.join('|')})\\b`);
const violations = [];

async function walk(path) {
  for (const entry of await readdir(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'pkg') await walk(child);
    } else if (textExtensions.has(extname(entry.name))) {
      const lines = (await readFile(child, 'utf8')).split('\n');
      lines.forEach((line, index) => {
        if (!banned.test(line)) return;
        const context = `${lines[index - 2] ?? ''}\n${lines[index - 1] ?? ''}\n${line}`;
        if (!/compatibility-(?:migration|rejection)/.test(context)) {
          violations.push(`${child}:${index + 1}:${line.trim()}`);
        }
      });
    }
  }
}

for (const root of roots) await walk(root);
if (violations.length) {
  console.error('removed compatibility forms remain without an explicit migration/rejection label:');
  console.error(violations.join('\n'));
  process.exit(1);
}
console.log(`compatibility corpus OK (${removed.length} removed source/library aliases)`);
