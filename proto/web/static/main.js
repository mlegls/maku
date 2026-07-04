// The browser host: fetch cards into the vfs, drive the wasm Instance with
// a fixed-timestep loop, render flat buffers to canvas 2d, forward the wire
// protocol from the eval box. Everything host-generic lives in Rust.
import init, { Danmaku } from './pkg/danmaku_web.js';

const FILES = [
  'cards/player-rig.dmk',
  'cards/reimu_vs_mima.dmk',
  'cards/duel.dmk',
  'cards/translations/ph_boss2_spell2.dmk',
  'cards/translations/player_homing.dmk',
  'cards/translations/130_bowap.dmk',
  'cards/translations/200_cradle.dmk',
];
const BOOT = 'cards/reimu_vs_mima.dmk';
const TICK_RATE = 120, PPU = 40; // world units → px

const cv = document.getElementById('cv'), ctx = cv.getContext('2d');
const CX = cv.width / 2, CY = cv.height / 2 + 60;
const sx = x => CX + x * PPU, sy = y => CY - y * PPU;

const keys = new Set();
addEventListener('keydown', e => {
  if (e.target.tagName === 'TEXTAREA') return;
  keys.add(e.code);
  if (e.code === 'Space') { dk.toggle_pause(); e.preventDefault(); }
  if (e.code >= 'Digit1' && e.code <= 'Digit9') dk.select(+e.code.slice(5) - 1);
  if (e.code === 'KeyR') dk.restart();
  if (['ArrowUp','ArrowDown','ArrowLeft','ArrowRight'].includes(e.code)) {
    if (dk.paused()) {
      const d = { ArrowRight: 1, ArrowLeft: -1, ArrowUp: 30, ArrowDown: -30 }[e.code];
      dk.seek(dk.tick() + d);
    }
    e.preventDefault();
  }
});
addEventListener('keyup', e => keys.delete(e.code));
let mouse = [0, -3];
cv.addEventListener('mousemove', e => {
  const r = cv.getBoundingClientRect();
  mouse = [(e.clientX - r.left - CX) / PPU, (CY - (e.clientY - r.top)) / PPU];
});

const scrub = document.getElementById('scrub');
let scrubbing = false;
scrub.addEventListener('input', () => { scrubbing = true; dk.seek(+scrub.value); });
scrub.addEventListener('change', () => { scrubbing = false; });
document.getElementById('play').onclick = () => dk.toggle_pause();
for (const cmd of ['run', 'swap', 'add']) {
  document.getElementById(cmd).onclick = () => {
    // one wire line: strip comments, join, wrap in the chosen verb
    const body = document.getElementById('code').value
      .split('\n').map(l => l.replace(/;.*$/, '')).join(' ').trim();
    dk.command(`(${cmd} ${body})`);
  };
}
document.getElementById('restart').onclick = () => dk.restart();

let dk;
async function boot() {
  await init();
  const rigSrc = await (await fetch('/cards/player-rig.dmk')).text();
  dk = new Danmaku(rigSrc + '\n(player-rig)');
  for (const f of FILES) dk.add_file(f, await (await fetch('/' + f)).text());
  dk.boot(BOOT, undefined);
  requestAnimationFrame(frame);
}

let last = performance.now(), acc = 0;
function frame(now) {
  acc += Math.min((now - last) / 1000, 0.1); last = now;
  const steps = Math.floor(acc * TICK_RATE); acc -= steps / TICK_RATE;
  const ax = (keys.has('KeyD') ? 1 : 0) - (keys.has('KeyA') ? 1 : 0)
           + (!dk.paused() ? (keys.has('ArrowRight') ? 1 : 0) - (keys.has('ArrowLeft') ? 1 : 0) : 0);
  const ay = (keys.has('KeyW') ? 1 : 0) - (keys.has('KeyS') ? 1 : 0)
           + (!dk.paused() ? (keys.has('ArrowUp') ? 1 : 0) - (keys.has('ArrowDown') ? 1 : 0) : 0);
  const m = Math.hypot(ax, ay);
  const nx = m > 1 ? ax / m : ax, ny = m > 1 ? ay / m : ay;
  dk.step(steps, mouse[0], mouse[1], nx, ny,
          keys.has('ShiftLeft') || keys.has('ShiftRight'), keys.has('KeyX'));
  draw();
  requestAnimationFrame(frame);
}

function draw() {
  ctx.fillStyle = '#12121a';
  ctx.fillRect(0, 0, cv.width, cv.height);

  // beams: [active, r, g, b, n, pts…]
  const bm = dk.beams();
  for (let i = 0; i < bm.length;) {
    const active = bm[i] > 0.5, r = bm[i+1], g = bm[i+2], b = bm[i+3], n = bm[i+4];
    i += 5;
    ctx.beginPath();
    for (let k = 0; k < n; k++, i += 2) {
      const X = sx(bm[i]), Y = sy(bm[i+1]);
      k ? ctx.lineTo(X, Y) : ctx.moveTo(X, Y);
    }
    ctx.strokeStyle = `rgba(${r*255|0},${g*255|0},${b*255|0},${active ? 1 : 0.45})`;
    ctx.lineWidth = active ? 5 : 1.5;
    ctx.stroke();
  }

  // dots: [x, y, radius, r, g, b]
  const d = dk.dots();
  for (let i = 0; i < d.length; i += 6) {
    ctx.beginPath();
    ctx.arc(sx(d[i]), sy(d[i+1]), d[i+2] * PPU, 0, 7);
    ctx.fillStyle = `rgb(${d[i+3]*255|0},${d[i+4]*255|0},${d[i+5]*255|0})`;
    ctx.fill();
    ctx.strokeStyle = 'rgba(255,255,255,0.3)';
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  // event flashes: [code, age, x, y] — replay under scrubbing
  const fl = dk.flashes(24), FC = ['96,230,255', '255,60,80', '255,200,80', '255,150,50'];
  for (let i = 0; i < fl.length; i += 4) {
    const k = fl[i+1] / 24;
    ctx.beginPath();
    ctx.arc(sx(fl[i+2]), sy(fl[i+3]), (8 + k * 26), 0, 7);
    ctx.strokeStyle = `rgba(${FC[fl[i]]},${(1-k).toFixed(2)})`;
    ctx.lineWidth = 2;
    ctx.stroke();
  }

  // player marker: hitbox + graze ring (+ iframe flicker)
  const pp = dk.player_pos();
  if (pp.length) {
    const X = sx(pp[0]), Y = sy(pp[1]);
    ctx.strokeStyle = 'rgba(255,255,255,0.25)'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.arc(X, Y, 0.35 * PPU, 0, 7); ctx.stroke();
    ctx.strokeStyle = 'rgba(255,255,255,0.8)'; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(X, Y, 7, 0, 7); ctx.stroke();
    ctx.fillStyle = '#fff';
    ctx.beginPath(); ctx.arc(X, Y, 0.06 * PPU, 0, 7); ctx.fill();
    if (dk.iframes() && (dk.tick() / 6 | 0) % 2 === 0) {
      ctx.strokeStyle = 'rgba(255,80,80,0.8)';
      ctx.beginPath(); ctx.arc(X, Y, 13, 0, 7); ctx.stroke();
    }
  }

  // HUD / timeline / menu (DOM built with textContent — no innerHTML)
  const lives = dk.lives();
  document.getElementById('hud').textContent =
    `tick ${dk.tick()}  entities ${dk.entity_count()}  graze ${dk.graze()}` +
    `  hits ${dk.hits()}  lives ${lives < 0 ? '-' : lives}` +
    (dk.paused() ? '  [paused]' : '');
  document.getElementById('status').textContent = dk.status();
  document.getElementById('play').textContent = dk.paused() ? '▶' : '⏸';
  const tl = dk.timeline();
  if (tl.length && !scrubbing) {
    scrub.max = Math.max(tl[1], 1);
    scrub.value = tl[0];
    document.getElementById('tick').textContent = `${tl[0]} / ${tl[1]}`;
  }
  const cur = dk.current_pattern();
  const menu = document.getElementById('menu');
  menu.replaceChildren(...dk.patterns().split('\n').map((p, i) => {
    const el = document.createElement('div');
    el.textContent = `${i + 1} ${p}`;
    el.className = p === cur ? 'sel' : '';
    el.onclick = () => dk.select(i);
    return el;
  }));
}

boot();
