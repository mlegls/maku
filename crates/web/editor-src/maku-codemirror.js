import { basicSetup, EditorView } from 'codemirror';
import { EditorState, Compartment } from '@codemirror/state';
import { defaultKeymap, indentWithTab } from '@codemirror/commands';
import { keymap } from '@codemirror/view';
import {
  bracketMatching,
  foldGutter,
  indentOnInput,
  indentUnit,
  syntaxHighlighting,
  HighlightStyle,
  StreamLanguage,
} from '@codemirror/language';
import { tags } from '@lezer/highlight';

const FORM_WORDS = new Set([
  'def', 'defn', 'defmacro', 'defpattern', 'defchannel', 'defrender-kind',
  'defcollider', 'deftick', 'bind!', 'export!', 'set!', 'emit', 'import',
  'fn', 'let', 'if', 'when', 'unless', 'seq', 'par', 'fork', 'finally',
  'wait', 'wait-for', 'until', 'race', 'states', 'goto', 'phases', 'spawn',
  'bullet', 'shot', 'enemy', 'boss', 'player', 'laser', 'laser-shot',
  'dotimes', 'for', 'move', 'pose', 'linear',
  'circle', 'polar', 'rot', 'aim', 'style', 'collider', 'contact',
]);

const makuLanguage = StreamLanguage.define({
  name: 'maku',
  startState: () => ({ inString: false }),
  token(stream, state) {
    if (state.inString) {
      let escaped = false;
      while (!stream.eol()) {
        const ch = stream.next();
        if (escaped) {
          escaped = false;
        } else if (ch === '\\') {
          escaped = true;
        } else if (ch === '"') {
          state.inString = false;
          break;
        }
      }
      return 'string';
    }
    if (stream.eatSpace()) return null;
    if (stream.peek() === ';') {
      stream.skipToEnd();
      return 'comment';
    }
    const ch = stream.next();
    if (ch === '"') {
      state.inString = true;
      return 'string';
    }
    if ('()[]{}'.includes(ch)) return 'bracket';
    stream.eatWhile(/[^\s()[\]{}";]/);
    const tok = stream.current();
    if (/^-?(?:\d+\.\d+|\d+|\.\d+)(?:[eE][+-]?\d+)?$/.test(tok)) return 'number';
    if (tok.startsWith(':')) return 'keyword';
    if (tok.startsWith('$')) return 'variableName.special';
    if (FORM_WORDS.has(tok)) return 'keyword.control';
    return 'variableName';
  },
  languageData: {
    commentTokens: { line: ';' },
    closeBrackets: { brackets: ['(', '[', '{', '"'] },
  },
});

const makuHighlight = HighlightStyle.define([
  { tag: tags.comment, color: '#6f778a' },
  { tag: tags.string, color: '#c3df8c' },
  { tag: tags.keyword, color: '#f2b56b' },
  { tag: tags.controlKeyword, color: '#78d6c6' },
  { tag: tags.special(tags.variableName), color: '#83d7ff' },
  { tag: tags.number, color: '#b99cff' },
  { tag: tags.bracket, color: '#aeb6c8' },
  { tag: tags.variableName, color: '#e7e9f1' },
]);

const editorTheme = EditorView.theme({
  '&': {
    height: '100%',
    minHeight: '100%',
    background: 'transparent',
    color: 'var(--text)',
    font: 'inherit',
  },
  '.cm-scroller': {
    font: 'inherit',
    lineHeight: '18px',
    overflow: 'auto',
  },
  '.cm-content': {
    padding: '12px',
    caretColor: 'var(--text)',
  },
  '.cm-line': {
    padding: '0',
  },
  '&.cm-focused': {
    outline: 'none',
  },
  '.cm-cursor': {
    borderLeftColor: 'var(--text)',
  },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
    backgroundColor: 'rgba(120, 214, 198, 0.22)',
  },
  '.cm-matchingBracket': {
    backgroundColor: 'rgba(120, 214, 198, 0.22)',
    color: '#f2fffb',
  },
  '.cm-nonmatchingBracket': {
    backgroundColor: 'rgba(255, 94, 114, 0.28)',
    color: '#ffd7dc',
  },
  '.cm-gutters': {
    display: 'none',
  },
});

const singleLineTheme = EditorView.theme({
  '&': {
    minHeight: '120px',
  },
  '.cm-content': {
    padding: '8px',
  },
});

function makuExtensions(onChange, extra = []) {
  return [
    basicSetup,
    makuLanguage,
    syntaxHighlighting(makuHighlight),
    bracketMatching(),
    foldGutter(),
    indentOnInput(),
    indentUnit.of('  '),
    keymap.of([indentWithTab, ...defaultKeymap]),
    editorTheme,
    EditorView.lineWrapping,
    EditorView.updateListener.of(update => {
      if (update.docChanged) onChange(update.state.doc.toString());
    }),
    ...extra,
  ];
}

export function createMakuEditor({ parent, value = '', onChange = () => {}, compact = false }) {
  const theme = new Compartment();
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: value,
      extensions: makuExtensions(onChange, [theme.of(compact ? singleLineTheme : [])]),
    }),
  });
  return {
    view,
    get value() {
      return view.state.doc.toString();
    },
    setValue(next) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: next },
      });
    },
    format() {
      // Keep formatting conservative until the language has a real parser.
      const lines = view.state.doc.toString().replace(/\r\n/g, '\n').split('\n');
      let level = 0;
      const out = lines.map(line => {
        const trimmed = line.trimStart();
        const leadingClosers = (trimmed.match(/^[)\]}]+/) || [''])[0].length;
        const indent = Math.max(0, level - leadingClosers) * 2;
        for (const ch of trimmed) {
          if ('([{'.includes(ch)) level += 1;
          if (')]}'.includes(ch)) level = Math.max(0, level - 1);
        }
        return trimmed ? `${' '.repeat(indent)}${trimmed}` : '';
      }).join('\n');
      this.setValue(out);
    },
    focus() {
      view.focus();
    },
    destroy() {
      view.destroy();
    },
  };
}
