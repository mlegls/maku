// Reusable Canvas2D compatibility adapter for Maku frame ABI 1.
// Page UI, input policy, card selection, and site chrome stay outside this
// module. Commands are consumed in authoritative pack order.
export function createCanvas2DRenderer({ maku, context, worldToCanvas, pixelsPerUnit }) {
  const ctx = context;
  const [sx, sy] = worldToCanvas;
  const externalTextures = new Map();
  const variants = new Map();
  let manifest = { textures: [], materials: [] };

  function registerExternalTexture(key, source) {
    externalTextures.set(key, source);
  }

  async function resolveManifest() {
    const textures = [];
    for (let i = 0; i < maku.texture_count(); i++) {
      const width = maku.texture_width(i), height = maku.texture_height(i);
      const bytes = new Uint8ClampedArray(maku.texture_bytes(i));
      let surface = document.createElement('canvas');
      surface.width = width; surface.height = height;
      const externalKey = maku.texture_external_key(i);
      if (width && height) {
        surface.getContext('2d').putImageData(new ImageData(bytes, width, height), 0, 0);
      } else if (externalKey) {
        let source = externalTextures.get(externalKey);
        if (typeof source === 'string') source = await createImageBitmap(await (await fetch(source)).blob());
        if (!source) throw new Error(`external render texture '${externalKey}' is not registered`);
        surface = source;
      }
      textures.push({
        key: maku.texture_key(i), externalKey,
        width: surface.width, height: surface.height, surface,
      });
    }
    const materials = [];
    for (let i = 0; i < maku.material_count(); i++) {
      materials.push({
        key: maku.material_key(i), pipeline: maku.material_pipeline(i),
        texture: maku.material_texture(i), layout: maku.material_layout(i),
        blend: maku.material_blend(i), fixedColor: maku.material_fixed_color(i),
        minFilter: maku.material_min_filter(i), magFilter: maku.material_mag_filter(i),
        addressU: maku.material_address_u(i), addressV: maku.material_address_v(i),
      });
      const material = materials.at(-1);
      if (material.minFilter !== material.magFilter) {
        throw new Error(`Canvas2D cannot represent distinct min/mag filters for material '${material.key}'`);
      }
    }
    manifest = { textures, materials };
    variants.clear();
  }

  function materialTextureVariant(material, mode, low, high = low) {
    const texture = manifest.textures[material.texture];
    if (!texture?.surface) throw new Error(`unresolved texture ${material.texture}`);
    const key = `${material.texture}:${mode}:${low.join(',')}:${high.join(',')}`;
    if (variants.has(key)) return variants.get(key);
    const canvas = document.createElement('canvas');
    canvas.width = texture.width; canvas.height = texture.height;
    const out = canvas.getContext('2d', { willReadFrequently: mode === 'recolor' });
    out.drawImage(texture.surface, 0, 0, texture.width, texture.height);
    if (mode === 'recolor') {
      const image = out.getImageData(0, 0, canvas.width, canvas.height);
      for (let i = 0; i < image.data.length; i += 4) {
        const k = image.data[i] / 255;
        const mask = image.data[i + 3] / 255;
        image.data[i] = low[0] + (high[0] - low[0]) * k;
        image.data[i + 1] = low[1] + (high[1] - low[1]) * k;
        image.data[i + 2] = low[2] + (high[2] - low[2]) * k;
        image.data[i + 3] = mask * (low[3] + (high[3] - low[3]) * k);
      }
      out.putImageData(image, 0, 0);
    } else {
      out.globalCompositeOperation = 'source-in';
      out.fillStyle = `rgba(${low[0]},${low[1]},${low[2]},${low[3] / 255})`;
      out.fillRect(0, 0, canvas.width, canvas.height);
    }
    variants.set(key, canvas);
    return canvas;
  }

  function drawTexturedTriangle(surface, source, dest) {
    const [s0, s1, s2] = source, [d0, d1, d2] = dest;
    const denominator = s0[0] * (s1[1] - s2[1]) + s1[0] * (s2[1] - s0[1]) + s2[0] * (s0[1] - s1[1]);
    if (Math.abs(denominator) < 1e-9) return;
    const a = (d0[0] * (s1[1] - s2[1]) + d1[0] * (s2[1] - s0[1]) + d2[0] * (s0[1] - s1[1])) / denominator;
    const c = (d0[0] * (s2[0] - s1[0]) + d1[0] * (s0[0] - s2[0]) + d2[0] * (s1[0] - s0[0])) / denominator;
    const e = (d0[0] * (s1[0] * s2[1] - s2[0] * s1[1]) + d1[0] * (s2[0] * s0[1] - s0[0] * s2[1]) + d2[0] * (s0[0] * s1[1] - s1[0] * s0[1])) / denominator;
    const b = (d0[1] * (s1[1] - s2[1]) + d1[1] * (s2[1] - s0[1]) + d2[1] * (s0[1] - s1[1])) / denominator;
    const d = (d0[1] * (s2[0] - s1[0]) + d1[1] * (s0[0] - s2[0]) + d2[1] * (s1[0] - s0[0])) / denominator;
    const f = (d0[1] * (s1[0] * s2[1] - s2[0] * s1[1]) + d1[1] * (s2[0] * s0[1] - s0[0] * s2[1]) + d2[1] * (s0[0] * s1[1] - s1[0] * s0[1])) / denominator;
    ctx.save();
    ctx.beginPath(); ctx.moveTo(...d0); ctx.lineTo(...d1); ctx.lineTo(...d2); ctx.closePath(); ctx.clip();
    ctx.transform(a, b, c, d, e, f); ctx.drawImage(surface, 0, 0); ctx.restore();
  }

  function draw() {
    maku.build_render_frame();
    if (variants.size > 4096) variants.clear();
    const buffers = [maku.basic_sprites(), maku.tinted_sprites(), maku.recolor_sprites()];
    const views = buffers.map(bytes => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength));
    const strides = [maku.basic_sprite_stride(), maku.tinted_sprite_stride(), maku.recolor_sprite_stride()];
    const vertexBytes = maku.strip_vertices();
    const vertices = new DataView(vertexBytes.buffer, vertexBytes.byteOffset, vertexBytes.byteLength);
    const indices = maku.strip_indices(), commands = maku.draw_commands();
    const vstride = maku.strip_vertex_stride(), commandStride = maku.draw_command_stride();

    for (let c = 0; c < commands.length; c += commandStride) {
      const material = manifest.materials[commands[c]];
      if (!material) throw new Error(`unresolved render material ${commands[c]}`);
      const tag = commands[c + 1], start = commands[c + 2], count = commands[c + 3];
      if (material.layout !== tag) throw new Error(`material/source layout mismatch for '${material.key}'`);
      ctx.globalCompositeOperation = material.blend === 2 ? 'lighter' : material.blend === 3 ? 'screen' : 'source-over';
      ctx.imageSmoothingEnabled = material.magFilter !== 0;
      if (tag <= 2) {
        const view = views[tag], stride = strides[tag];
        for (let i = start; i < start + count; i++) {
          const at = i * stride;
          const x = view.getFloat32(at, true), y = view.getFloat32(at + 4, true);
          const rx = view.getFloat32(at + 8, true), ry = view.getFloat32(at + 12, true);
          const angle = view.getFloat32(at + 16, true) * Math.PI / 180;
          const fixed = [material.fixedColor & 255, (material.fixedColor >>> 8) & 255,
            (material.fixedColor >>> 16) & 255, (material.fixedColor >>> 24) & 255];
          const lowAt = tag === 0 ? -1 : at + 40;
          const highAt = tag === 2 ? at + 44 : lowAt;
          const low = lowAt < 0 ? fixed : [view.getUint8(lowAt), view.getUint8(lowAt + 1), view.getUint8(lowAt + 2), view.getUint8(lowAt + 3)];
          const high = highAt < 0 ? low : [view.getUint8(highAt), view.getUint8(highAt + 1), view.getUint8(highAt + 2), view.getUint8(highAt + 3)];
          if (tag === 0) low[3] = low[3] * view.getUint8(at + 36) / 255;
          const texture = manifest.textures[material.texture];
          const u0 = view.getFloat32(at + 20, true), v0 = view.getFloat32(at + 24, true);
          const u1 = view.getFloat32(at + 28, true), v1 = view.getFloat32(at + 32, true);
          const surface = materialTextureVariant(material, tag === 2 ? 'recolor' : 'tint', low, high);
          ctx.save(); ctx.translate(sx(x), sy(y)); ctx.rotate(-angle);
          ctx.drawImage(surface, u0 * texture.width, v0 * texture.height,
            (u1 - u0) * texture.width, (v1 - v0) * texture.height,
            -rx * pixelsPerUnit, -ry * pixelsPerUnit, rx * 2 * pixelsPerUnit, ry * 2 * pixelsPerUnit);
          ctx.restore();
        }
      } else if (tag === 3) {
        const indexStart = commands[c + 4], indexCount = commands[c + 5];
        for (let i = indexStart; i < indexStart + indexCount; i += 3) {
          const offsets = [indices[i], indices[i + 1], indices[i + 2]].map(index => index * vstride);
          const a = offsets[0];
          const color = [vertices.getUint8(a + 16), vertices.getUint8(a + 17), vertices.getUint8(a + 18), vertices.getUint8(a + 19)];
          const texture = manifest.textures[material.texture];
          const surface = materialTextureVariant(material, 'tint', color);
          const source = offsets.map(v => [vertices.getFloat32(v + 8, true) * texture.width, vertices.getFloat32(v + 12, true) * texture.height]);
          const dest = offsets.map(v => [sx(vertices.getFloat32(v, true)), sy(vertices.getFloat32(v + 4, true))]);
          drawTexturedTriangle(surface, source, dest);
        }
      } else {
        throw new Error(`unknown frame source tag ${tag}`);
      }
    }
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }

  return { draw, resolveManifest, registerExternalTexture, get manifest() { return manifest; } };
}
