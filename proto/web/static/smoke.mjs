// Headless smoke test of the built wasm package (run: bun smoke.mjs).
// Exercises exactly what main.js does, minus the canvas.
import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import init, { Danmaku } from './pkg/danmaku_web.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../../..');
await init({ module_or_path: readFileSync(join(here, 'pkg/danmaku_web_bg.wasm')) });

const rig = readFileSync(join(root, 'cards/player-rig.dmk'), 'utf8');
const dk = new Danmaku(rig + '\n(player-rig)');
for (const f of ['cards/reimu_vs_mima.dmk',
                 'cards/translations/ph_boss2_spell2.dmk',
                 'cards/translations/player_homing.dmk']) {
  dk.add_file(f, readFileSync(join(root, f), 'utf8'));
}
dk.boot('cards/reimu_vs_mima.dmk', undefined);
if (!dk.running()) throw new Error('boot failed: ' + dk.status());

// play 5 seconds with slight movement
dk.input_vec2('player', 0, -3);
dk.input_vec2('nearest-enemy', 0, -3);
for (let k = 0; k < 600; k++) {
  dk.input_num('move-x', k % 100 < 50 ? 0.5 : -0.5);
  dk.step(1);
}
console.log('status:', dk.status(), '| entities', dk.entity_count(), '| tick', dk.tick());
const dots = dk.dots(), pp = dk.player_pos();
console.log('tick', dk.tick(), 'dots', dots.length / 6, 'beams', dk.beams().length,
            'player', pp[0].toFixed(2), pp[1].toFixed(2),
            'lives', dk.lives(), 'graze', dk.graze());
if (dots.length === 0) throw new Error('nothing rendered');

// wire protocol: hot-eval + scrub
dk.command('(run (spawn (circle 6 (linear c[1 0]))))');
dk.input_num('move-x', 0); dk.step(1);
if (dk.entity_count() < 6) throw new Error('run failed: ' + dk.status());
dk.seek(100);
if (dk.tick() !== 100 || !dk.paused()) throw new Error('seek failed');
console.log('wire protocol + scrub OK — timeline', dk.timeline().join('/'));
console.log('WASM SMOKE PASS');
