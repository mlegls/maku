import { highlightMaku } from './maku-highlight.js';

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, ch => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  })[ch]);
}

export function markdownToHtml(md) {
  const lines = md.replace(/\r\n/g, '\n').split('\n');
  const out = [];
  let inCode = false;
  let code = [];
  let inList = false;
  let inTable = false;

  const closeList = () => {
    if (inList) out.push('</ul>');
    inList = false;
  };
  const closeTable = () => {
    if (inTable) out.push('</tbody></table>');
    inTable = false;
  };
  const rewriteLink = href => {
    if (/^(?:https?:|mailto:|#|\/)/.test(href)) return href;
    const file = href.split('/').pop();
    if (!file?.endsWith('.md')) return href;
    if (file === 'from-dmk.md') return 'from-dmk.html';
    return `tutorials.html#${file.replace(/\.md$/, '')}`;
  };
  const inline = text => escapeHtml(text)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, label, href) => `<a href="${rewriteLink(href)}">${label}</a>`);
  const tableRow = (line, tag) => {
    const cells = line.trim().replace(/^\||\|$/g, '').split('|');
    return `<tr>${cells.map(cell => `<${tag}>${inline(cell.trim())}</${tag}>`).join('')}</tr>`;
  };

  for (const line of lines) {
    if (line.startsWith('```')) {
      if (inCode) {
        out.push(`<pre><code>${highlightMaku(code.join('\n'))}</code></pre>`);
        code = [];
        inCode = false;
      } else {
        closeList();
        closeTable();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      code.push(line);
      continue;
    }
    if (/^\|.+\|$/.test(line)) {
      closeList();
      if (/^\|\s*-/.test(line)) continue;
      if (!inTable) {
        out.push('<table><tbody>');
        inTable = true;
      }
      out.push(tableRow(line, out[out.length - 1] === '<table><tbody>' ? 'th' : 'td'));
      continue;
    }
    closeTable();
    if (line.startsWith('# ')) {
      closeList();
      out.push(`<h1>${inline(line.slice(2))}</h1>`);
    } else if (line.startsWith('## ')) {
      closeList();
      out.push(`<h2>${inline(line.slice(3))}</h2>`);
    } else if (line.startsWith('### ')) {
      closeList();
      out.push(`<h3>${inline(line.slice(4))}</h3>`);
    } else if (line.startsWith('- ')) {
      if (!inList) {
        out.push('<ul>');
        inList = true;
      }
      out.push(`<li>${inline(line.slice(2))}</li>`);
    } else if (line.trim() === '') {
      closeList();
    } else {
      closeList();
      out.push(`<p>${inline(line)}</p>`);
    }
  }
  closeList();
  closeTable();
  return out.join('\n');
}
