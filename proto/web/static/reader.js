import { TUTORIALS, assetUrl } from './manifest.js';
import { markdownToHtml } from './markdown.js';
import { highlightCodeBlocks } from './maku-highlight.js';

const list = document.getElementById('tutorial-list');
const body = document.getElementById('doc-body');
let selected = TUTORIALS[0];

function renderList() {
  list.replaceChildren(...TUTORIALS.map(item => {
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = item === selected ? 'active' : '';
    btn.textContent = item.title;
    btn.onclick = () => select(item);
    return btn;
  }));
}

async function select(item) {
  selected = item;
  renderList();
  body.textContent = 'Loading...';
  const htmlBase = globalThis.DANMAKU_DOC_HTML_BASE;
  if (htmlBase) {
    const slug = item.doc.split('/').pop().replace(/\.md$/, '');
    const res = await fetch(new URL(`${slug}.html`, htmlBase).toString());
    body.innerHTML = await res.text();
  } else {
    const md = await (await fetch(assetUrl(item.doc))).text();
    body.innerHTML = markdownToHtml(md);
  }
  highlightCodeBlocks(body);
  history.replaceState(null, '', `#${item.doc.split('/').pop().replace(/\.md$/, '')}`);
}

const slug = location.hash.slice(1);
const initial = TUTORIALS.find(item => item.doc.includes(slug)) || selected;
select(initial);
