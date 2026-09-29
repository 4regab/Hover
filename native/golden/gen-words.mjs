// What a double and a triple click select in the real page: every character of
// fixtures/words.md, shown in the drawer's .ans, is clicked at 1/4 and 3/4 of its width
// with a click count of 1, 2 and 3, and the selection is read back each time. The single
// click gives the caret the page's hit test found there (it works in whole pixels, so a
// narrow glyph's halves are not exact); the test starts from that caret. The native
// chat's word and paragraph selection is tested against these. Needs Playwright:
//   npm i playwright && node native/golden/gen-words.mjs
// Chromium on Linux uses the Unix editing behaviour; WebView2 (Windows) also selects
// the whitespace after a double-clicked word. That one rule is applied in the test.
import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { markdown } from '../../web/office/md.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fx = JSON.parse(readFileSync(join(here, 'fixtures', 'office-state.json'), 'utf8'));
const src = readFileSync(join(here, 'fixtures', 'words.md'), 'utf8');
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 1104, height: 424 } });
await p.clock.install({ time: fx.now });
await p.addInitScript(() => { const L = []; window.chrome = { webview: { addEventListener: (t, f) => L.push(f), postMessage() {} } }; window.__send = m => L.forEach(f => f({ data: m })); });
await p.goto(pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href);
await p.waitForFunction(() => window.READY);
await p.evaluate(s => window.__send(s), fx.state);
await p.clock.runFor(3000);
await p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), 1);
await p.clock.runFor(600);
const nLeaves = await p.evaluate(h => {
  const a = document.querySelector('#thread .ans'); a.innerHTML = h;
  // The boxes the native chat lays out as one text each: a li's own line, not its sub-list.
  window.__leaves = [...a.querySelectorAll('h3, h4, h5, p, li, th, td, pre code')];
  return window.__leaves.length;
}, markdown(src, { image: u => u }));
// The leaf's own text nodes (a li stops at its sub-list), and a way to aim at one
// character: it is scrolled into the thread's (and a pre's) view, and the point at
// `at` of its width is returned if a click there really lands on the leaf. Points at
// 1/4 and 3/4 of a character keep the result away from the caret's rounding at its
// middle. Links are skipped: a click on one opens it and leaves the selection alone.
await p.evaluate(() => {
  window.__own = leaf => {
    const w = document.createTreeWalker(leaf, NodeFilter.SHOW_TEXT), nodes = [];
    for (let n; (n = w.nextNode());) if (leaf.tagName !== 'LI' || n.parentElement.closest('ul, ol') === leaf.parentElement) nodes.push(n);
    return nodes;
  };
  window.__aim = (k, off, at) => {
    const leaf = window.__leaves[k];
    // A click inside the last selection would only clear it.
    getSelection().removeAllRanges();
    let n, i = off;
    for (n of window.__own(leaf)) { if (i < n.length) break; i -= n.length; }
    if (n.parentElement.closest('a')) return null;
    const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + (n.data.codePointAt(i) > 0xffff ? 2 : 1));
    const th = document.querySelector('#thread'), box = th.getBoundingClientRect();
    let q = [...r.getClientRects()].find(q => q.width > 0.5);
    if (!q) return null;
    th.scrollTop += q.y + q.height / 2 - (box.y + box.height / 2);
    const pre = leaf.closest('pre');
    if (pre) { const pb = pre.getBoundingClientRect(); q = r.getClientRects()[0]; pre.scrollLeft += q.x - (pb.x + pb.width / 2); }
    q = [...r.getClientRects()].find(q => q.width > 0.5);
    const x = q.x + q.width * at, y = q.y + q.height / 2;
    const e = document.elementFromPoint(x, y);
    return e && leaf.contains(e) ? [x, y] : null;
  };
});
const out = [];
for (let k = 0; k < nLeaves; k++) {
  const { text, offs, lines } = await p.evaluate(k => {
    const text = window.__own(window.__leaves[k]).map(n => n.data).join('');
    const offs = [];
    for (let i = 0; i < text.length; i++) if (!(text.charCodeAt(i) >= 0xdc00 && text.charCodeAt(i) < 0xe000)) offs.push(i);
    // Where the page's lines start (the test knows which carets end a soft-wrapped line).
    const lines = [];
    let y = null, u = 0;
    for (const n of window.__own(window.__leaves[k])) {
      for (let i = 0; i < n.length; i++, u++) {
        const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + 1);
        const q = r.getClientRects()[0];
        if (q && q.width > 0 && (y === null || q.y > y + 2)) { if (y !== null) lines.push(u); y = q.y; }
      }
    }
    return { text, offs, lines };
  }, k);
  const probes = [];
  for (const i of offs) {
    for (const at of [0.25, 0.75]) {
      const got = [];
      for (const clickCount of [1, 2, 3]) {
        const pt = await p.evaluate(([k, i, at]) => window.__aim(k, i, at), [k, i, at]);
        if (!pt) break;
        await p.mouse.click(pt[0], pt[1], { clickCount });
        got.push(await p.evaluate(k => {
          const leaf = window.__leaves[k], s = getSelection();
          if (!s.rangeCount) return null;
          const r = s.getRangeAt(0), pre = document.createRange();
          pre.setStart(leaf, 0);
          // Where the selection starts in the leaf's text (-1: outside it).
          let start = -1;
          if (leaf.contains(r.startContainer)) { pre.setEnd(r.startContainer, r.startOffset); start = pre.toString().length; }
          return [start, s.toString()];
        }, k));
      }
      // [offset, at, caret, [word start, copy], [paragraph start, copy]]
      if (got.length === 3) probes.push([i, at, got[0][0], got[1], got[2]]);
    }
  }
  out.push({ text, lines, probes });
  console.log(k, JSON.stringify(text.slice(0, 40)), probes.length);
}
writeFileSync(join(here, 'expected', 'words.json'), JSON.stringify(out, null, 1));
await b.close();
