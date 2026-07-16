const FORM_WORDS = new Set([
  'def', 'defn', 'defmacro', 'defpattern', 'defchannel', 'defrender-kind',
  'defcollider', 'deftick', 'bind!', 'export!', 'set!', 'emit', 'import',
  'fn', 'let', 'if', 'when', 'unless', 'seq', 'par', 'fork', 'finally',
  'wait', 'wait-for', 'until', 'race', 'states', 'goto', 'phases', 'spawn',
  'bullet', 'shot', 'enemy', 'boss', 'player', 'laser', 'laser-shot',
  'dotimes', 'for', 'move', 'pose', 'linear',
  'circle', 'polar', 'rot', 'aim', 'style', 'collider', 'contact',
]);

function escapeHtml(s) {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
}

function span(cls, text) {
  return `<span class="${cls}">${escapeHtml(text)}</span>`;
}

function tokenClass(tok) {
  if (/^-?(?:\d+\.\d+|\d+|\.\d+)(?:[eE][+-]?\d+)?$/.test(tok)) return 'tok-number';
  if (tok.startsWith(':')) return 'tok-keyword';
  if (tok.startsWith('$')) return 'tok-channel';
  if (FORM_WORDS.has(tok)) return 'tok-form';
  return 'tok-symbol';
}

const OPEN = '([{';
const CLOSE = ')]}';
const PAIRS = { '(': ')', '[': ']', '{': '}' };
const CLOSERS = { ')': '(', ']': '[', '}': '{' };

function scan(src, visit) {
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === ';') {
      const j = src.indexOf('\n', i);
      i = j === -1 ? src.length : j;
    } else if (ch === '"') {
      let j = i + 1;
      while (j < src.length) {
        if (src[j] === '\\') {
          j += 2;
        } else if (src[j] === '"') {
          j += 1;
          break;
        } else {
          j += 1;
        }
      }
      i = j;
    } else {
      visit(ch, i);
      i += 1;
    }
  }
}

export function delimiterMarks(src, cursor) {
  const stack = [];
  const unmatched = new Set();
  const pairs = new Map();
  scan(src, (ch, i) => {
    if (OPEN.includes(ch)) {
      stack.push({ ch, i });
    } else if (CLOSE.includes(ch)) {
      const top = stack[stack.length - 1];
      if (top && top.ch === CLOSERS[ch]) {
        stack.pop();
        pairs.set(top.i, i);
        pairs.set(i, top.i);
      } else {
        unmatched.add(i);
      }
    }
  });
  for (const item of stack) unmatched.add(item.i);

  const match = new Set();
  for (const i of [cursor, cursor - 1]) {
    if (pairs.has(i)) {
      match.add(i);
      match.add(pairs.get(i));
      break;
    }
  }
  return { match, unmatched };
}

export function highlightMaku(src, marks = {}) {
  const match = marks.match || new Set();
  const unmatched = marks.unmatched || new Set();
  let out = '';
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === ';') {
      const j = src.indexOf('\n', i);
      const end = j === -1 ? src.length : j;
      out += span('tok-comment', src.slice(i, end));
      i = end;
    } else if (ch === '"') {
      let j = i + 1;
      while (j < src.length) {
        if (src[j] === '\\') {
          j += 2;
        } else if (src[j] === '"') {
          j += 1;
          break;
        } else {
          j += 1;
        }
      }
      out += span('tok-string', src.slice(i, j));
      i = j;
    } else if ('()[]{}'.includes(ch)) {
      const cls = unmatched.has(i)
        ? 'tok-delim tok-unmatched'
        : match.has(i)
          ? 'tok-delim tok-match'
          : 'tok-delim';
      out += span(cls, ch);
      i += 1;
    } else if (/\s/.test(ch)) {
      out += ch;
      i += 1;
    } else {
      let j = i + 1;
      while (j < src.length && !/[\s()[\]{}";]/.test(src[j])) j += 1;
      const tok = src.slice(i, j);
      out += span(tokenClass(tok), tok);
      i = j;
    }
  }
  return out.endsWith('\n') ? `${out} ` : out;
}

export function highlightCodeBlocks(root) {
  for (const code of root.querySelectorAll('pre > code')) {
    code.innerHTML = highlightMaku(code.textContent || '');
  }
}

export function indentFor(src, pos) {
  const stack = [];
  scan(src.slice(0, pos), (ch, i) => {
    if (OPEN.includes(ch)) {
      stack.push({ ch, i });
    } else if (CLOSE.includes(ch)) {
      const top = stack[stack.length - 1];
      if (top && top.ch === CLOSERS[ch]) stack.pop();
    }
  });
  if (stack.length === 0) return 0;
  const top = stack[stack.length - 1];
  const lineStart = src.lastIndexOf('\n', top.i) + 1;
  return top.i - lineStart + 2;
}

export function formatMaku(src) {
  const lines = src.replace(/\r\n/g, '\n').split('\n');
  let out = '';
  let level = 0;
  for (let i = 0; i < lines.length; i += 1) {
    const raw = lines[i];
    const trimmed = raw.trimStart();
    const leadingClosers = (trimmed.match(/^[)\]}]+/) || [''])[0].length;
    const indent = Math.max(0, level - leadingClosers) * 2;
    const line = trimmed ? `${' '.repeat(indent)}${trimmed}` : '';
    out += i === lines.length - 1 ? line : `${line}\n`;
    scan(trimmed, ch => {
      if (OPEN.includes(ch)) level += 1;
      if (CLOSE.includes(ch)) level = Math.max(0, level - 1);
    });
  }
  return out;
}
