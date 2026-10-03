// Markdown for agents' answers, with no library: headings, paragraphs, bold, italic,
// strikethrough, inline code, links, images, lists (nested, numbered, task), quotes,
// fenced code, tables and rules, and ```mermaid flowcharts drawn by diagram.js.
// Everything the answer says is escaped; the only markup is what this file writes,
// so an answer can't put its own HTML (or script) into the office.
import { flowchart } from './diagram.js';

const esc = s => s.replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

/// A link the office may follow: web addresses only. The rest is shown as text.
const safeUrl = u => /^https?:\/\//i.test(u) ? u : null;

// Inline marks, on text already split from code spans.
function inline(text, o) {
  const codes = [];
  // Code spans first, so nothing inside them is read as a mark.
  text = text.replace(/(`+)([\s\S]*?[^`])\1(?!`)/g, (_, _t, c) => { codes.push(c.trim()); return `\u0000${codes.length - 1}\u0000`; });
  let h = esc(text);
  // Links and images are set aside while marks are read, so a _ or * in an address
  // is left alone.
  const keep = [], put = html => { keep.push(html); return `\u0001${keep.length - 1}\u0001`; };
  // Images and links: ![alt](src "title") and [text](href).
  h = h.replace(/!\[([^\]]*)\]\(\s*([^)\s]+)(?:\s+&quot;[^)]*&quot;)?\s*\)/g, (m, alt, src) => {
    const url = o.image?.(src.replace(/&amp;/g, '&'));
    return url ? put(`<img src="${esc(url)}" alt="${alt}" loading="lazy">`) : m;
  });
  h = h.replace(/\[([^\]]+)\]\(\s*([^)\s]+)(?:\s+&quot;[^)]*&quot;)?\s*\)/g, (m, label, href) => {
    const url = safeUrl(href.replace(/&amp;/g, '&'));
    return url ? `${put(`<a href="${esc(url)}" title="${esc(url)}">`)}${label}${put('</a>')}` : label;
  });
  // Bare web addresses become links too.
  h = h.replace(/(^|[\s(])(https?:\/\/[^\s<)\u0001]+[^\s<).,;:!?\u0001])/g, (_, pre, url) => pre + put(`<a href="${url}" title="${url}">${url}</a>`));
  h = h.replace(/\*\*(?=\S)([\s\S]*?\S)\*\*|__(?=\S)([\s\S]*?\S)__/g, (_, a, b) => `<strong>${a ?? b}</strong>`);
  h = h.replace(/(^|[^*\w])\*(?=\S)([^*]*?\S)\*(?!\*)|(^|[^_\w])_(?=\S)([^_]*?\S)_(?!\w)/g, (_, p1, a, p2, b) => `${p1 ?? p2}<em>${a ?? b}</em>`);
  h = h.replace(/~~(?=\S)([\s\S]*?\S)~~/g, '<del>$1</del>');
  h = h.replace(/\u0001(\d+)\u0001/g, (_, i) => keep[+i]);
  return h.replace(/\u0000(\d+)\u0000/g, (_, i) => `<code>${esc(codes[+i])}</code>`);
}

const cells = row => row.trim().replace(/^\||\|$/g, '').split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, '|'));

/// Markdown as HTML. o.image(src) gives the URL an image may load from, or null.
export function markdown(src, o = {}) {
  const lines = String(src || '').replace(/\r\n?/g, '\n').split('\n');
  const out = [];
  let i = 0;
  const para = [];
  const flush = () => { if (para.length) { out.push(`<p>${inline(para.join('\n'), o).replace(/\n/g, '<br>')}</p>`); para.length = 0; } };
  while (i < lines.length) {
    const line = lines[i];
    // Fenced code, and diagrams.
    const fence = line.match(/^\s*(`{3,}|~{3,})\s*([\w+#.-]*)/);
    if (fence) {
      flush();
      const body = []; i++;
      while (i < lines.length && !lines[i].trim().startsWith(fence[1])) body.push(lines[i++]);
      i++;
      const lang = fence[2].toLowerCase(), code = body.join('\n');
      const svg = lang === 'mermaid' ? flowchart(code) : null;
      out.push(svg ? `<figure class="diagram">${svg}</figure>`
        : `<pre class="code"${lang ? ` data-lang="${esc(lang)}"` : ''}><code>${esc(code)}</code></pre>`);
      continue;
    }
    if (!line.trim()) { flush(); i++; continue; }
    const h = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (h) { flush(); const n = Math.min(6, h[1].length + 2); out.push(`<h${n}>${inline(h[2], o)}</h${n}>`); i++; continue; }
    if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) { flush(); out.push('<hr>'); i++; continue; }
    // Tables: a header row, then a |---|:--:| row.
    if (line.includes('|') && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(lines[i + 1])) {
      flush();
      const head = cells(line), align = cells(lines[i + 1]).map(c => c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : '');
      i += 2;
      const rows = [];
      while (i < lines.length && lines[i].includes('|') && lines[i].trim()) rows.push(cells(lines[i++]));
      const td = (tag, c, k) => `<${tag}${align[k] ? ` style="text-align:${align[k]}"` : ''}>${inline(c, o)}</${tag}>`;
      out.push(`<div class="table"><table><thead><tr>${head.map((c, k) => td('th', c, k)).join('')}</tr></thead><tbody>${rows.map(r => `<tr>${head.map((_, k) => td('td', r[k] ?? '', k)).join('')}</tr>`).join('')}</tbody></table></div>`);
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      flush();
      const body = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i])) body.push(lines[i++].replace(/^\s{0,3}>\s?/, ''));
      out.push(`<blockquote>${markdown(body.join('\n'), o)}</blockquote>`);
      continue;
    }
    if (/^\s*([-*+]|\d+[.)])\s+/.test(line)) { flush(); i = list(lines, i, out, o); continue; }
    para.push(line.trim()); i++;
  }
  flush();
  return out.join('');
}

// A list, and the lists nested in it by indent.
function list(lines, i, out, o) {
  const indent = lines[i].match(/^\s*/)[0].length;
  const ordered = /^\s*\d+[.)]/.test(lines[i]);
  const start = ordered ? parseInt(lines[i].trim(), 10) : 1;
  const items = [];
  while (i < lines.length) {
    const m = lines[i].match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
    if (!m || m[1].length < indent) { if (!lines[i].trim() && i + 1 < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[i + 1])) { i++; continue; } break; }
    if (m[1].length > indent) { const sub = []; i = list(lines, i, sub, o); if (items.length) items[items.length - 1] += sub.join(''); continue; }
    // A numbered list after a bulleted one (or the other way round) is a new list.
    if (/^\d/.test(m[2]) !== ordered) break;
    let text = m[3]; i++;
    // Lines that carry on the item.
    while (i < lines.length && lines[i].trim() && !/^\s*([-*+]|\d+[.)])\s+/.test(lines[i]) && /^\s+/.test(lines[i])) text += '\n' + lines[i++].trim();
    const task = text.match(/^\[([ xX])\]\s+/);
    items.push(task ? `<span class="task${task[1] === ' ' ? '' : ' done'}"></span>${inline(text.slice(task[0].length), o)}` : inline(text, o));
  }
  const tag = ordered ? 'ol' : 'ul';
  out.push(`<${tag}${ordered && start !== 1 ? ` start="${start}"` : ''}>${items.map(x => `<li>${x}</li>`).join('')}</${tag}>`);
  return i;
}
