const FORM_WORDS = new Set([
  'def', 'defn', 'defmacro', 'defpattern', 'defvar', 'defcell', 'defchannel',
  'bind-channel!', 'export', 'import', 'fn', 'let', 'if', 'when', 'unless',
  'seq', 'par', 'fork', 'finally', 'wait', 'wait-for', 'until', 'race',
  'states', 'goto', 'phases', 'spawn', 'spawn-bullet', 'spawn-shot',
  'spawn-enemy', 'spawn-boss', 'dotimes', 'for', 'move', 'pose', 'linear',
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

export function highlightMaku(src) {
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
      out += span('tok-delim', ch);
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
