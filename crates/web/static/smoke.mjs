// Headless smoke test of the built wasm package (run: bun smoke.mjs).
// Exercises exactly what main.js does, minus the canvas.
import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import initMaku, {
  assertRuntimeIdentity,
  createMaku,
  releaseIdentity,
} from '../../js/maku/dist/index.js';
import { CARD_FILES, TUTORIALS } from './manifest.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '../../..');
await initMaku({ moduleOrPath: readFileSync(join(here, '../../js/maku/wasm/maku_bg.wasm')) });
const release = JSON.parse(readFileSync(join(here, '../../js/maku/wasm/release.json'), 'utf8'));
const identity = releaseIdentity();
if (identity.makuVersion !== release.maku_version
    || identity.frameAbiVersion !== release.frame_abi_version
    || identity.sourceRevision !== release.source_revision) {
  throw new Error(`release identity mismatch: ${JSON.stringify({ identity, release })}`);
}
for (const mismatch of [
  { ...identity, makuVersion: 'mixed-wrapper' },
  { ...identity, frameAbiVersion: identity.frameAbiVersion + 1 },
]) {
  try {
    assertRuntimeIdentity(mismatch);
    throw new Error('mixed release identity was accepted');
  } catch (error) {
    if (!String(error).includes('loaded wasm')) throw error;
  }
}

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

maku.boot('cards/render-pack-showcase.maku', 'showcase');
if (!maku.running()) throw new Error('showcase boot failed: ' + maku.status());
maku.step(2);
maku.build_render_frame();
validateFrameViews(maku);
validateRenderManifest(maku);
const showcaseDraws = maku.draw_commands();
const showcaseTags = new Set();
for (let i = 0; i < showcaseDraws.length; i += maku.draw_command_stride()) {
  showcaseTags.add(showcaseDraws[i + 1]);
}
if (![0, 1, 3].every(tag => showcaseTags.has(tag))) {
  throw new Error(`showcase is missing fixed/tinted/ribbon sources: ${[...showcaseTags]}`);
}
const showcaseVertices = maku.strip_vertices();
const view = new DataView(showcaseVertices.buffer, showcaseVertices.byteOffset, showcaseVertices.byteLength);
const ribbonXs = { warning: [], active: [] };
for (let offset = 0; offset < showcaseVertices.byteLength; offset += maku.strip_vertex_stride()) {
  const x = view.getFloat32(offset, true);
  (x < 0 ? ribbonXs.warning : ribbonXs.active).push(x);
}
const spread = xs => Math.max(...xs) - Math.min(...xs);
if (!ribbonXs.warning.length || !ribbonXs.active.length
    || spread(ribbonXs.active) <= spread(ribbonXs.warning) * 2) {
  throw new Error('showcase does not expose distinct warning/active ribbon widths');
}
const showcaseDiagnostics = maku.render_diagnostics();
if (!showcaseDiagnostics.includes('showcase-unknown-family')
    || !showcaseDiagnostics.includes('showcase-unknown-color')) {
  throw new Error(`showcase fallback diagnostics missing: ${showcaseDiagnostics}`);
}

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
validateRenderManifest(maku);

// wire protocol: hot-eval + scrub
maku.command('(run (spawn (circle 6 (linear c[1 0]))))');
maku.input_num('move-x', 0); maku.step(1);
if (maku.entity_count() < 6) throw new Error('run failed: ' + maku.status());
maku.seek(100);
if (maku.tick() !== 100 || !maku.paused()) throw new Error('seek failed');
console.log('wire protocol + scrub OK — timeline', maku.timeline().join('/'));
validateDocumentationRoutes();
console.log('WASM SMOKE PASS');

function validateDocumentationRoutes() {
  const playerHtml = readFileSync(join(here, 'index.html'), 'utf8');
  const tutorialsHtml = readFileSync(join(here, 'tutorials.html'), 'utf8');
  if (!playerHtml.includes('main.js') || !tutorialsHtml.includes('reader.js')) {
    throw new Error('upstream player/tutorial routes do not load their entry modules');
  }
  for (const tutorial of TUTORIALS) {
    const source = readFileSync(join(root, tutorial.doc), 'utf8');
    if (!source.includes('Runnable companion:')) {
      throw new Error(`tutorial route has no checked companion: ${tutorial.doc}`);
    }
  }
}

function validateRenderManifest(maku) {
  for (let i = 0; i < maku.texture_count(); i++) {
    if (!maku.texture_key(i)) throw new Error(`texture ${i} has no key`);
    const width = maku.texture_width(i), height = maku.texture_height(i);
    const bytes = maku.texture_bytes(i), externalKey = maku.texture_external_key(i);
    if (width || height) {
      if (!width || !height || bytes.length !== width * height * 4) {
        throw new Error(`builtin texture ${i} has invalid dimensions/bytes`);
      }
    } else if (!externalKey || bytes.length) {
      throw new Error(`external texture ${i} has invalid source metadata`);
    }
  }
  for (let i = 0; i < maku.material_count(); i++) {
    const fields = {
      key: maku.material_key(i), pipeline: maku.material_pipeline(i),
      texture: maku.material_texture(i), layout: maku.material_layout(i),
      blend: maku.material_blend(i), fixedColor: maku.material_fixed_color(i),
      minFilter: maku.material_min_filter(i), magFilter: maku.material_mag_filter(i),
      addressU: maku.material_address_u(i), addressV: maku.material_address_v(i),
    };
    if (!fields.key || !fields.pipeline) throw new Error(`material ${i} has no key/pipeline`);
    if (fields.texture >= maku.texture_count()) throw new Error(`material ${i} texture out of bounds`);
    if (fields.layout > 3 || fields.blend > 3
        || fields.minFilter > 1 || fields.magFilter > 1
        || fields.addressU > 2 || fields.addressV > 2) {
      throw new Error(`material ${i} has invalid enum metadata: ${JSON.stringify(fields)}`);
    }
  }
}

function validateFrameViews(maku) {
  if (maku.basic_sprite_stride() !== 40 || maku.tinted_sprite_stride() !== 44
      || maku.recolor_sprite_stride() !== 48 || maku.strip_vertex_stride() !== 20
      || maku.draw_command_stride() !== 8) {
    throw new Error('frame ABI v1 stride mismatch');
  }
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
    if (tag > 3 || maku.material_layout(material) !== tag) {
      throw new Error('draw source/material layout mismatch');
    }
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
