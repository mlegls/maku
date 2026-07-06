// Interactive browser host: card/tutorial picker, editable vfs source, wasm
// simulation loop, canvas renderer, and the debug wire protocol.
import initMaku, { createMaku } from '../../js/maku/dist/index.js';
import { ALL_CARDS, CARD_FILES, DEMO_CARDS, TUTORIALS, assetUrl } from './manifest.js';
import { markdownToHtml } from './markdown.js';
import {
  delimiterMarks,
  formatMaku,
  highlightCodeBlocks,
  highlightMaku,
  indentFor,
} from './maku-highlight.js';

const BOOT = 'cards/tutorials/t01.maku';
const TICK_RATE = 120;
const PPU = 40;

const cv = document.getElementById('cv');
const ctx = cv.getContext('2d');
const CX = cv.width / 2;
const CY = cv.height / 2 + 60;
const sx = x => CX + x * PPU;
const sy = y => CY - y * PPU;

const els = {
  tutorialList: document.getElementById('tutorial-list'),
  demoList: document.getElementById('demo-list'),
  title: document.getElementById('current-title'),
  path: document.getElementById('current-path'),
  sourceName: document.getElementById('source-name'),
  source: document.getElementById('source'),
  sourceHighlight: document.querySelector('#source-highlight code'),
  evalHighlight: document.querySelector('#eval-highlight code'),
  apply: document.getElementById('apply-source'),
  reset: document.getElementById('reset-source'),
  formatSource: document.getElementById('format-source'),
  docsToggle: document.getElementById('docs-toggle'),
  docsClose: document.getElementById('docs-close'),
  docsDrawer: document.getElementById('docs-drawer'),
  docTitle: document.getElementById('doc-title'),
  docPath: document.getElementById('doc-path'),
  docBody: document.getElementById('doc-body'),
  play: document.getElementById('play'),
  scrub: document.getElementById('scrub'),
  tick: document.getElementById('tick'),
  patterns: document.getElementById('patterns'),
  hud: document.getElementById('hud'),
  status: document.getElementById('status'),
  evalCode: document.getElementById('eval-code'),
  formatEval: document.getElementById('format-eval'),
  bindingRows: document.getElementById('binding-rows'),
  constRows: document.getElementById('const-rows'),
  resetBindings: document.getElementById('reset-bindings'),
  addButtonBinding: document.getElementById('add-button-binding'),
  addAxisBinding: document.getElementById('add-axis-binding'),
  addConstBinding: document.getElementById('add-const-binding'),
};

const keys = new Set();
const pressed = new Set();
const sources = new Map();
const docs = new Map();
const editingTags = new Set(['INPUT', 'TEXTAREA', 'SELECT']);
let selected = ALL_CARDS.find(card => card.path === BOOT) || ALL_CARDS[0];
let maku;
let last = performance.now();
let acc = 0;
let scrubbing = false;
let lastPatternKey = '';
let sourceDirty = false;
let mouse = [0, -3];
let bindings = defaultBindings();
let capturing = null;

function defaultBindings() {
  return {
    rows: [
      { type: 'axis', neg: 'KeyA', pos: 'KeyD', channel: 'move-x' },
      { type: 'axis', neg: 'KeyS', pos: 'KeyW', channel: 'move-y' },
      { type: 'axis', neg: 'ArrowLeft', pos: 'ArrowRight', channel: 'move-x' },
      { type: 'axis', neg: 'ArrowDown', pos: 'ArrowUp', channel: 'move-y' },
      { type: 'axis', neg: 'KeyA', pos: 'KeyD', channel: 'p1-move-x' },
      { type: 'axis', neg: 'KeyS', pos: 'KeyW', channel: 'p1-move-y' },
      { type: 'axis', neg: 'ArrowLeft', pos: 'ArrowRight', channel: 'p2-move-x' },
      { type: 'axis', neg: 'ArrowDown', pos: 'ArrowUp', channel: 'p2-move-y' },
      { type: 'button', key: 'ShiftLeft', mode: 'hold', channel: 'focus-firing', latch: false, tap: false },
      { type: 'button', key: 'KeyX', mode: 'hold', channel: 'bomb', latch: false, tap: false },
    ],
    consts: [{ channel: 'rank', value: 1.0 }],
  };
}

function keyLabel(code) {
  const labels = {
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ShiftLeft: 'LShift',
    ShiftRight: 'RShift',
    Space: 'Space',
  };
  if (labels[code]) return labels[code];
  if (code.startsWith('Key')) return code.slice(3);
  if (code.startsWith('Digit')) return code.slice(5);
  return code;
}

function updateSourceHighlight() {
  els.sourceHighlight.innerHTML = highlightMaku(
    els.source.value,
    delimiterMarks(els.source.value, els.source.selectionStart),
  );
  syncSourceHighlightScroll();
}

function updateEvalHighlight() {
  els.evalHighlight.innerHTML = highlightMaku(
    els.evalCode.value,
    delimiterMarks(els.evalCode.value, els.evalCode.selectionStart),
  );
  syncEvalHighlightScroll();
}

function syncHighlightScroll(textarea, code) {
  const pre = code.parentElement;
  pre.style.width = `${textarea.clientWidth}px`;
  pre.style.height = `${textarea.clientHeight}px`;
  pre.scrollTop = textarea.scrollTop;
  pre.scrollLeft = textarea.scrollLeft;
}

function syncSourceHighlightScroll() {
  syncHighlightScroll(els.source, els.sourceHighlight);
}

function syncEvalHighlightScroll() {
  syncHighlightScroll(els.evalCode, els.evalHighlight);
}

function cleanChannel(s) {
  return s.trim().replace(/^\$/, '') || 'chan';
}

function lineBounds(textarea) {
  const value = textarea.value;
  const start = value.lastIndexOf('\n', textarea.selectionStart - 1) + 1;
  let end = value.indexOf('\n', textarea.selectionEnd);
  if (end === -1) end = value.length;
  return { start, end };
}

function replaceEditorRange(textarea, start, end, text, selectMode = 'end') {
  textarea.setRangeText(text, start, end, selectMode);
  textarea.dispatchEvent(new Event('input', { bubbles: true }));
}

function indentSelection(textarea, delta) {
  if (textarea.selectionStart === textarea.selectionEnd) {
    if (delta > 0) {
      replaceEditorRange(textarea, textarea.selectionStart, textarea.selectionEnd, '  ', 'end');
    } else {
      const pos = textarea.selectionStart;
      const value = textarea.value;
      const lineStart = value.lastIndexOf('\n', pos - 1) + 1;
      const before = value.slice(lineStart, pos);
      const remove = before.endsWith('  ') ? 2 : before.endsWith(' ') ? 1 : 0;
      if (remove) replaceEditorRange(textarea, pos - remove, pos, '', 'end');
    }
    return;
  }
  const value = textarea.value;
  const { start, end } = lineBounds(textarea);
  const before = textarea.selectionStart;
  const block = value.slice(start, end);
  const next = block.split('\n').map(line => {
    if (delta > 0) return `  ${line}`;
    if (line.startsWith('  ')) return line.slice(2);
    if (line.startsWith(' ')) return line.slice(1);
    return line;
  }).join('\n');
  replaceEditorRange(textarea, start, end, next, 'select');
  if (textarea.selectionStart === textarea.selectionEnd) {
    textarea.setSelectionRange(Math.max(start, before + delta * 2), Math.max(start, before + delta * 2));
  }
}

function autoEnter(textarea) {
  const pos = textarea.selectionStart;
  const indent = indentFor(textarea.value, pos);
  replaceEditorRange(textarea, textarea.selectionStart, textarea.selectionEnd, `\n${' '.repeat(indent)}`, 'end');
}

function maybeDedentCloser(textarea, closer) {
  const pos = textarea.selectionStart;
  const value = textarea.value;
  const lineStart = value.lastIndexOf('\n', pos - 1) + 1;
  if (value.slice(lineStart, pos).trim() !== '') return false;
  const indent = Math.max(0, indentFor(value, pos) - 2);
  replaceEditorRange(textarea, lineStart, pos, ' '.repeat(indent), 'end');
  replaceEditorRange(textarea, textarea.selectionStart, textarea.selectionEnd, closer, 'end');
  return true;
}

function installEditorKeys(textarea, updateHighlight, formatButton, onFormat = () => {}) {
  textarea.addEventListener('keydown', e => {
    if (e.key === 'Tab') {
      e.preventDefault();
      indentSelection(textarea, e.shiftKey ? -1 : 1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      autoEnter(textarea);
    } else if (')]}'.includes(e.key) && !e.metaKey && !e.ctrlKey && !e.altKey) {
      if (maybeDedentCloser(textarea, e.key)) e.preventDefault();
    }
  });
  textarea.addEventListener('keyup', updateHighlight);
  textarea.addEventListener('click', updateHighlight);
  textarea.addEventListener('select', updateHighlight);
  formatButton.onclick = () => {
    const oldStart = textarea.selectionStart;
    textarea.value = formatMaku(textarea.value);
    textarea.setSelectionRange(Math.min(oldStart, textarea.value.length), Math.min(oldStart, textarea.value.length));
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
    onFormat();
  };
}

function setConst(channel, value) {
  const clean = cleanChannel(channel);
  const row = bindings.consts.find(c => c.channel === clean);
  if (row) {
    row.value = value;
  } else {
    bindings.consts.push({ channel: clean, value });
  }
  renderBindings();
}

function isArrow(code) {
  return ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(code);
}

function captureKey(row, slot) {
  capturing = { row, slot };
  renderBindings();
}

function keyDownForBinding(code) {
  return !(maku?.paused() && isArrow(code)) && keys.has(code);
}

function writeInputChannels() {
  const acc = new Map();
  const add = (channel, value) => acc.set(channel, (acc.get(channel) || 0) + value);

  for (const row of bindings.rows) {
    if (row.type === 'axis') {
      add(cleanChannel(row.channel), (keyDownForBinding(row.pos) ? 1 : 0) - (keyDownForBinding(row.neg) ? 1 : 0));
    } else {
      if (row.mode === 'tap' && pressed.has(row.key)) row.tap = true;
      if (row.mode === 'toggle' && pressed.has(row.key)) row.latch = !row.latch;
      const value = row.mode === 'hold'
        ? (keyDownForBinding(row.key) ? 1 : 0)
        : row.mode === 'tap'
          ? (row.tap ? 1 : 0)
          : (row.latch ? 1 : 0);
      add(cleanChannel(row.channel), value);
    }
  }

  for (const [channel, value] of acc) {
    acc.set(channel, Math.max(-1, Math.min(1, value)));
  }
  for (const channel of Array.from(acc.keys())) {
    if (!channel.endsWith('-x')) continue;
    const stem = channel.slice(0, -2);
    const yChannel = `${stem}-y`;
    if (!acc.has(yChannel)) continue;
    const x = acc.get(channel) || 0;
    const y = acc.get(yChannel) || 0;
    const mag = Math.hypot(x, y);
    if (mag > 1) {
      acc.set(channel, x / mag);
      acc.set(yChannel, y / mag);
    }
  }
  for (const [channel, value] of acc) maku.input_num(channel, value);
  for (const c of bindings.consts) maku.input_num(cleanChannel(c.channel), Number(c.value) || 0);
}

function consumeTapBindings() {
  for (const row of bindings.rows) {
    if (row.type === 'button' && row.tap) {
      row.tap = false;
      maku.input_num(cleanChannel(row.channel), 0);
    }
  }
}

function hasArmedTapBinding() {
  return bindings.rows.some(row => row.type === 'button' && row.tap);
}

function selectedPattern() {
  return maku?.current_pattern() || undefined;
}

function stripWireWrapper(body) {
  const m = body.match(/^\((run|swap|add)\s+([\s\S]*)\)$/);
  return m ? m[2] : body;
}

function commandBody() {
  return els.evalCode.value
    .split('\n')
    .map(line => line.replace(/;.*$/, ''))
    .join(' ')
    .trim();
}

function setDirty(v) {
  sourceDirty = v;
  els.apply.textContent = v ? 'Apply *' : 'Apply';
}

function sourceFor(card) {
  return sources.get(card.path) || '';
}

async function fetchText(path) {
  const res = await fetch(assetUrl(path));
  if (!res.ok) throw new Error(`${path}: ${res.status}`);
  return await res.text();
}

async function loadSources() {
  const wanted = Array.from(new Set(CARD_FILES));
  for (const path of wanted) {
    sources.set(path, await fetchText(path));
  }
}

function registerVfs() {
  for (const [path, src] of sources) {
    maku.add_file(path, src);
  }
}

function renderCardList(host, cards) {
  host.replaceChildren(...cards.map(card => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `card-choice${card.path === selected.path ? ' active' : ''}`;
    const title = document.createElement('span');
    title.textContent = card.title;
    const detail = document.createElement('small');
    detail.textContent = card.detail;
    btn.append(title, detail);
    btn.onclick = () => selectCard(card);
    return btn;
  }));
}

function renderLists() {
  renderCardList(els.tutorialList, TUTORIALS);
  renderCardList(els.demoList, DEMO_CARDS);
}

function bootSelected(pattern = undefined) {
  maku.boot(selected.path, pattern);
  lastPatternKey = '';
}

async function selectCard(card) {
  selected = card;
  els.title.textContent = card.title;
  els.path.textContent = card.path;
  els.sourceName.textContent = card.path;
  els.source.value = sourceFor(card);
  updateSourceHighlight();
  syncSourceHighlightScroll();
  setDirty(false);
  renderLists();
  bootSelected();
  if (card.doc && els.docsDrawer.classList.contains('open')) {
    await loadDoc(card);
  }
}

async function loadDoc(card = selected) {
  els.docTitle.textContent = card.title;
  els.docPath.textContent = card.doc || 'No tutorial document for this card';
  if (!card.doc) {
    els.docBody.textContent = 'This demo card does not have a tutorial article yet.';
    return;
  }
  const htmlBase = globalThis.DANMAKU_DOC_HTML_BASE;
  if (htmlBase) {
    const slug = card.doc.split('/').pop().replace(/\.md$/, '');
    const res = await fetch(new URL(`${slug}.html`, htmlBase).toString());
    if (!res.ok) throw new Error(`${slug}.html: ${res.status}`);
    els.docBody.innerHTML = await res.text();
    highlightCodeBlocks(els.docBody);
    return;
  }
  if (!docs.has(card.doc)) {
    docs.set(card.doc, await fetchText(card.doc));
  }
  els.docBody.innerHTML = markdownToHtml(docs.get(card.doc));
  highlightCodeBlocks(els.docBody);
}

async function openDocs() {
  els.docsDrawer.classList.add('open');
  els.docsDrawer.setAttribute('aria-hidden', 'false');
  await loadDoc();
}

function closeDocs() {
  els.docsDrawer.classList.remove('open');
  els.docsDrawer.setAttribute('aria-hidden', 'true');
}

function applySource() {
  sources.set(selected.path, els.source.value);
  maku.add_file(selected.path, els.source.value);
  bootSelected(selectedPattern());
  setDirty(false);
}

function resetSource() {
  els.source.value = sourceFor(selected);
  updateSourceHighlight();
  syncSourceHighlightScroll();
  setDirty(false);
}

function textInput(value, onChange) {
  const input = document.createElement('input');
  input.value = value;
  input.spellcheck = false;
  input.onchange = () => onChange(input.value);
  input.oninput = () => onChange(input.value);
  return input;
}

function keyButton(label, row, slot) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'key-capture';
  if (capturing?.row === row && capturing?.slot === slot) {
    btn.classList.add('capturing');
    btn.textContent = 'press key';
  } else {
    btn.textContent = label;
  }
  btn.onclick = () => captureKey(row, slot);
  return btn;
}

function removeButton(onClick) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.textContent = 'x';
  btn.onclick = onClick;
  return btn;
}

function renderBindings() {
  els.bindingRows.replaceChildren(...bindings.rows.map((row, i) => {
    const el = document.createElement('div');
    el.className = `binding-row ${row.type}`;
    el.append(textInput(`$${row.channel}`, v => { row.channel = cleanChannel(v); }));
    if (row.type === 'axis') {
      el.append(
        keyButton(`-${keyLabel(row.neg)}`, i, 'neg'),
        keyButton(`+${keyLabel(row.pos)}`, i, 'pos'),
      );
      const kind = document.createElement('span');
      kind.className = 'subtle';
      kind.textContent = 'axis';
      el.append(kind);
    } else {
      el.append(keyButton(keyLabel(row.key), i, 'key'));
      const mode = document.createElement('select');
      for (const value of ['hold', 'tap', 'toggle']) {
        const opt = document.createElement('option');
        opt.value = value;
        opt.textContent = value;
        mode.append(opt);
      }
      mode.value = row.mode;
      mode.onchange = () => {
        row.mode = mode.value;
        row.tap = false;
        row.latch = false;
      };
      el.append(mode);
    }
    el.append(removeButton(() => {
      bindings.rows.splice(i, 1);
      renderBindings();
    }));
    return el;
  }));

  els.constRows.replaceChildren(...bindings.consts.map((row, i) => {
    const el = document.createElement('div');
    el.className = 'const-row';
    el.append(
      textInput(`$${row.channel}`, v => { row.channel = cleanChannel(v); }),
      textInput(String(row.value), v => {
        const n = Number(v);
        if (Number.isFinite(n)) row.value = n;
      }),
      removeButton(() => {
        bindings.consts.splice(i, 1);
        renderBindings();
      }),
    );
    return el;
  }));
}

function installEvents() {
  addEventListener('keydown', e => {
    if (capturing) {
      if (e.code !== 'Escape') {
        const row = bindings.rows[capturing.row];
        if (row) {
          if (row.type === 'axis' && capturing.slot === 'neg') row.neg = e.code;
          if (row.type === 'axis' && capturing.slot === 'pos') row.pos = e.code;
          if (row.type === 'button' && capturing.slot === 'key') row.key = e.code;
        }
      }
      capturing = null;
      renderBindings();
      e.preventDefault();
      return;
    }
    if (editingTags.has(e.target?.tagName)) return;
    if (!keys.has(e.code)) pressed.add(e.code);
    keys.add(e.code);
    if (e.code === 'Space') {
      maku.toggle_pause();
      e.preventDefault();
    }
    if (e.code >= 'Digit1' && e.code <= 'Digit9') maku.select(+e.code.slice(5) - 1);
    if (e.code === 'KeyR') maku.restart();
    if (e.code === 'KeyT') setConst('rank', 0.7);
    if (e.code === 'KeyY') setConst('rank', 1.0);
    if (e.code === 'KeyU') setConst('rank', 1.4);
    if (e.code === 'KeyI') setConst('rank', 2.0);
    if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.code)) {
      if (maku.paused()) {
        const d = { ArrowRight: 1, ArrowLeft: -1, ArrowUp: 30, ArrowDown: -30 }[e.code];
        maku.seek(maku.tick() + d);
      }
      e.preventDefault();
    }
  });
  addEventListener('keyup', e => {
    keys.delete(e.code);
  });
  cv.addEventListener('mousemove', e => {
    const r = cv.getBoundingClientRect();
    const scaleX = cv.width / r.width;
    const scaleY = cv.height / r.height;
    const x = (e.clientX - r.left) * scaleX;
    const y = (e.clientY - r.top) * scaleY;
    mouse = [(x - CX) / PPU, (CY - y) / PPU];
  });
  els.scrub.addEventListener('input', () => {
    scrubbing = true;
    maku.seek(+els.scrub.value);
  });
  els.scrub.addEventListener('change', () => {
    scrubbing = false;
  });
  els.play.onclick = () => maku.toggle_pause();
  els.apply.onclick = applySource;
  els.reset.onclick = resetSource;
  installEditorKeys(els.source, updateSourceHighlight, els.formatSource, () => setDirty(els.source.value !== sourceFor(selected)));
  installEditorKeys(els.evalCode, updateEvalHighlight, els.formatEval);
  els.source.addEventListener('input', () => {
    updateSourceHighlight();
    setDirty(els.source.value !== sourceFor(selected));
  });
  els.source.addEventListener('scroll', syncSourceHighlightScroll);
  els.evalCode.addEventListener('input', updateEvalHighlight);
  els.evalCode.addEventListener('scroll', syncEvalHighlightScroll);
  updateEvalHighlight();
  els.docsToggle.onclick = openDocs;
  els.docsClose.onclick = closeDocs;
  els.resetBindings.onclick = () => {
    bindings = defaultBindings();
    capturing = null;
    renderBindings();
  };
  els.addButtonBinding.onclick = () => {
    bindings.rows.push({ type: 'button', key: 'Space', mode: 'hold', channel: 'chan', latch: false, tap: false });
    renderBindings();
  };
  els.addAxisBinding.onclick = () => {
    bindings.rows.push({ type: 'axis', neg: 'Comma', pos: 'Period', channel: 'chan' });
    renderBindings();
  };
  els.addConstBinding.onclick = () => {
    bindings.consts.push({ channel: 'chan', value: 0 });
    renderBindings();
  };
  for (const cmd of ['run', 'swap', 'add']) {
    document.getElementById(cmd).onclick = () => {
      maku.command(`(${cmd} ${stripWireWrapper(commandBody())})`);
    };
  }
  document.getElementById('restart').onclick = () => maku.restart();
}

async function boot() {
  renderLists();
  await initMaku();
  await loadSources();
  maku = createMaku();
  registerVfs();
  installEvents();
  renderBindings();
  await selectCard(selected);
  requestAnimationFrame(frame);
}

function frame(now) {
  acc += Math.min((now - last) / 1000, 0.1);
  last = now;
  const steps = Math.floor(acc * TICK_RATE);
  acc -= steps / TICK_RATE;

  maku.input_vec2('player', mouse[0], mouse[1]);
  maku.input_vec2('nearest-enemy', mouse[0], mouse[1]);
  writeInputChannels();
  if (steps > 0 && hasArmedTapBinding()) {
    maku.step(1);
    consumeTapBindings();
    maku.step(steps - 1);
  } else {
    maku.step(steps);
  }
  pressed.clear();

  draw();
  requestAnimationFrame(frame);
}

function draw() {
  ctx.fillStyle = '#12121a';
  ctx.fillRect(0, 0, cv.width, cv.height);

  ctx.strokeStyle = 'rgba(255,255,255,0.08)';
  ctx.lineWidth = 1;
  ctx.strokeRect(sx(-3.8), sy(4.4), 7.6 * PPU, 8.8 * PPU);

  const bm = maku.beams();
  for (let i = 0; i < bm.length;) {
    const active = bm[i] > 0.5;
    const r = bm[i + 1], g = bm[i + 2], b = bm[i + 3], a = bm[i + 4], n = bm[i + 5];
    i += 6;
    ctx.beginPath();
    for (let k = 0; k < n; k++, i += 2) {
      const x = sx(bm[i]), y = sy(bm[i + 1]);
      k ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    }
    ctx.strokeStyle = `rgba(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0},${(active ? 1 : 0.45) * a})`;
    ctx.lineWidth = active ? 5 : 1.5;
    ctx.stroke();
  }

  const d = maku.dots();
  for (let i = 0; i < d.length; i += 7) {
    ctx.beginPath();
    ctx.arc(sx(d[i]), sy(d[i + 1]), d[i + 2] * PPU, 0, 7);
    ctx.fillStyle = `rgba(${d[i + 3] * 255 | 0},${d[i + 4] * 255 | 0},${d[i + 5] * 255 | 0},${d[i + 6]})`;
    ctx.fill();
    ctx.strokeStyle = `rgba(255,255,255,${0.3 * d[i + 6]})`;
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  const fl = maku.flashes(24);
  const FC = ['96,230,255', '255,60,80', '255,200,80', '255,150,50'];
  for (let i = 0; i < fl.length; i += 4) {
    const k = fl[i + 1] / 24;
    ctx.beginPath();
    ctx.arc(sx(fl[i + 2]), sy(fl[i + 3]), 8 + k * 26, 0, 7);
    ctx.strokeStyle = `rgba(${FC[fl[i]]},${(1 - k).toFixed(2)})`;
    ctx.lineWidth = 2;
    ctx.stroke();
  }

  const pilots = maku.positions('pilot');
  const pp = pilots.length ? pilots : maku.player_pos();
  for (let i = 0; i < pp.length; i += 2) {
    const x = sx(pp[i]), y = sy(pp[i + 1]);
    ctx.strokeStyle = 'rgba(255,255,255,0.25)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(x, y, 0.35 * PPU, 0, 7);
    ctx.stroke();
    ctx.strokeStyle = 'rgba(255,255,255,0.8)';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(x, y, 7, 0, 7);
    ctx.stroke();
    ctx.fillStyle = '#fff';
    ctx.beginPath();
    ctx.arc(x, y, 0.06 * PPU, 0, 7);
    ctx.fill();
    if (maku.iframes() && (maku.tick() / 6 | 0) % 2 === 0) {
      ctx.strokeStyle = 'rgba(255,80,80,0.8)';
      ctx.beginPath();
      ctx.arc(x, y, 13, 0, 7);
      ctx.stroke();
    }
  }

  const bossHp = maku.channel_num('boss-hp');
  if (bossHp > 0) {
    ctx.fillStyle = 'rgba(255,255,255,0.1)';
    ctx.fillRect(20, 10, cv.width - 40, 6);
    ctx.fillStyle = 'rgba(255,90,120,0.9)';
    ctx.fillRect(20, 10, (cv.width - 40) * Math.min(bossHp / 100, 1), 6);
  }

  updateHud();
}

function updateHud() {
  const lives = maku.lives();
  els.hud.textContent =
    `tick ${maku.tick()}  entities ${maku.entity_count()}  graze ${maku.graze()}` +
    `  hits ${maku.hits()}  lives ${lives < 0 ? '-' : lives}` +
    (maku.paused() ? '  [paused]' : '') +
    (sourceDirty ? '  [source edited]' : '');
  const cells = maku.cells();
  els.status.textContent = maku.status() + (cells ? `\ncells: ${cells.replaceAll('\n', '  ')}` : '');
  els.play.textContent = maku.paused() ? '>' : '||';
  const tl = maku.timeline();
  if (tl.length && !scrubbing) {
    els.scrub.max = Math.max(tl[1], 1);
    els.scrub.value = tl[0];
    els.tick.textContent = `${tl[0]} / ${tl[1]}`;
  }

  const cur = maku.current_pattern();
  const patterns = maku.patterns().split('\n').filter(Boolean);
  const key = `${patterns.join('|')}|${cur}`;
  if (key !== lastPatternKey) {
    lastPatternKey = key;
    els.patterns.replaceChildren(...patterns.map((name, i) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = `${i + 1} ${name}`;
      btn.className = name === cur ? 'active' : '';
      btn.onclick = () => maku.select(i);
      return btn;
    }));
  }
}

boot().catch(err => {
  els.status.textContent = err.stack || String(err);
});
