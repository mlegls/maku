// Headless smoke test of the built wasm package (run: bun smoke.mjs).
// Exercises exactly what main.js does, minus the canvas.
import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import init, { Maku, stdlibSource } from './pkg/maku.js';
import { CARD_FILES } from './manifest.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../../..');
await init({ module_or_path: readFileSync(join(here, 'pkg/maku_bg.wasm')) });

const rig = stdlibSource('player-rig');
const maku = new Maku(rig + '\n(player-rig)');
for (const f of CARD_FILES) {
  maku.add_file(f, readFileSync(join(root, f), 'utf8'));
}
maku.boot('cards/tutorials/t01.maku', undefined);
if (!maku.running()) throw new Error('tutorial boot failed: ' + maku.status());
maku.step(2);
if (maku.dots().length === 0) throw new Error('tutorial rendered nothing: ' + maku.status());

maku.boot('cards/reimu_vs_mima.maku', undefined);
if (!maku.running()) throw new Error('boot failed: ' + maku.status());

// play 5 seconds with slight movement
maku.input_vec2('player', 0, -3);
maku.input_vec2('nearest-enemy', 0, -3);
for (let k = 0; k < 600; k++) {
  maku.input_num('move-x', k % 100 < 50 ? 0.5 : -0.5);
  maku.step(1);
}
console.log('status:', maku.status(), '| entities', maku.entity_count(), '| tick', maku.tick());
const dots = maku.dots(), pp = maku.player_pos();
console.log('tick', maku.tick(), 'dots', dots.length / 7, 'beams', maku.beams().length,
            'player', pp[0].toFixed(2), pp[1].toFixed(2),
            'lives', maku.lives(), 'graze', maku.graze());
if (dots.length === 0) throw new Error('nothing rendered');

// wire protocol: hot-eval + scrub
maku.command('(run (spawn (circle 6 (linear c[1 0]))))');
maku.input_num('move-x', 0); maku.step(1);
if (maku.entity_count() < 6) throw new Error('run failed: ' + maku.status());
maku.seek(100);
if (maku.tick() !== 100 || !maku.paused()) throw new Error('seek failed');
console.log('wire protocol + scrub OK — timeline', maku.timeline().join('/'));
console.log('WASM SMOKE PASS');
