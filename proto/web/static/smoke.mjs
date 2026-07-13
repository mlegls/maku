// Headless smoke test of the built wasm package (run: bun smoke.mjs).
// Exercises exactly what main.js does, minus the canvas.
import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import initMaku, { createMaku } from '../../js/maku/dist/index.js';
import { CARD_FILES } from './manifest.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../../..');
await initMaku({ moduleOrPath: readFileSync(join(here, '../../js/maku/wasm/maku_bg.wasm')) });

const maku = createMaku();
for (const f of CARD_FILES) {
  maku.add_file(f, readFileSync(join(root, f), 'utf8'));
}
maku.boot('cards/tutorials/t01.maku', undefined);
if (!maku.running()) throw new Error('tutorial boot failed: ' + maku.status());
maku.step(2);
maku.build_render_frame();
validateFrameViews(maku);
if (maku.draw_commands().length === 0) throw new Error('tutorial rendered nothing: ' + maku.status());

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
maku.build_render_frame();
validateFrameViews(maku);
const draws = maku.draw_commands(), pp = maku.player_pos();
console.log('tick', maku.tick(), 'draw commands', draws.length / maku.draw_command_stride(),
            'player', pp[0].toFixed(2), pp[1].toFixed(2),
            'lives', maku.lives(), 'graze', maku.graze());
if (draws.length === 0) throw new Error('nothing rendered');
if (maku.material_count() === 0 || maku.texture_count() === 0) throw new Error('missing render manifest');

// wire protocol: hot-eval + scrub
maku.command('(run (spawn (circle 6 (linear c[1 0]))))');
maku.input_num('move-x', 0); maku.step(1);
if (maku.entity_count() < 6) throw new Error('run failed: ' + maku.status());
maku.seek(100);
if (maku.tick() !== 100 || !maku.paused()) throw new Error('seek failed');
console.log('wire protocol + scrub OK — timeline', maku.timeline().join('/'));
console.log('WASM SMOKE PASS');

function validateFrameViews(maku) {
  const basic = maku.basic_sprites(), tinted = maku.tinted_sprites();
  const recolor = maku.recolor_sprites(), vertices = maku.strip_vertices();
  const indices = maku.strip_indices(), draws = maku.draw_commands();
  const commandStride = maku.draw_command_stride();
  if (draws.length % commandStride) throw new Error('partial packed draw command');
  const counts = [
    basic.length / maku.basic_sprite_stride(),
    tinted.length / maku.tinted_sprite_stride(),
    recolor.length / maku.recolor_sprite_stride(),
  ];
  const vertexCount = vertices.length / maku.strip_vertex_stride();
  for (let i = 0; i < draws.length; i += commandStride) {
    const material = draws[i], tag = draws[i + 1], start = draws[i + 2], count = draws[i + 3];
    if (material >= maku.material_count()) throw new Error('draw material out of bounds');
    if (tag < 3 && start + count > counts[tag]) throw new Error('sprite view range out of bounds');
    if (tag === 3 && (start + count > vertexCount || draws[i + 4] + draws[i + 5] > indices.length)) {
      throw new Error('indexed view range out of bounds');
    }
  }
  // Access the original zero-copy views after all sibling views were created;
  // they remain valid until the next build_render_frame call.
  if (basic.length) basic[basic.length - 1];
  if (indices.length) indices[indices.length - 1];
}
