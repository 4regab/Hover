// What the office copies: a whole thread selected in the real page (kiro-office.html,
// host mode, the office-state fixture) and read as the clipboard would get it. The
// native chat's copy is tested against these. Needs Playwright:
//   npm i playwright && node native/golden/gen-copy.mjs
import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { markdown } from '../../web/office/md.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fx = JSON.parse(readFileSync(join(here, 'fixtures', 'office-state.json'), 'utf8'));
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 1104, height: 424 } });
await p.clock.install({ time: fx.now });
await p.addInitScript(() => { const L = []; window.chrome = { webview: { addEventListener: (t, f) => L.push(f), postMessage() {} } }; window.__send = m => L.forEach(f => f({ data: m })); });
await p.goto(pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href);
await p.waitForFunction(() => window.READY);
await p.evaluate(s => window.__send(s), fx.state);
await p.clock.runFor(3000);
const out = {};
// open: the step list opened by a click on its summary, as a user would.
for (const [name, k, open] of [['done-rich', 1], ['failed', 3], ['stopped', 4], ['failed-steps-open', 3, true], ['working-live', 0]]) {
  await p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), k);
  await p.clock.runFor(600);
  if (open) { await p.evaluate(() => document.querySelector('#thread details.work summary').click()); await p.clock.runFor(300); }
  out[name] = await p.evaluate(() => {
    const th = document.querySelector('#thread'), s = getSelection(), r = document.createRange();
    r.selectNodeContents(th); s.removeAllRanges(); s.addRange(r);
    const all = s.toString();
    // The answer alone, too (what a drag over just the reply copies).
    const ans = th.querySelector('.ans');
    if (ans) { r.selectNodeContents(ans); s.removeAllRanges(); s.addRange(r); }
    // Where each step row sits, for the layout test (flex row, shrunk, ellipsis).
    const t = th.getBoundingClientRect();
    const rows = [...th.querySelectorAll('details.work[open] > div > div')].map(d => { const b = d.getBoundingClientRect(); return [+(b.x - t.x).toFixed(2), +b.width.toFixed(2)]; });
    return { thread: all, answer: ans ? s.toString() : '', rows };
  });
  await p.evaluate(() => document.querySelector('#dClose').click());
  await p.clock.runFor(600);
}
// A seeded corpus of answers, each shown in the real drawer's .ans and copied whole.
const PARTS = ['Para with **bold** and `code`.', 'Two\nlines', '### Head', '#### Sub', '- a\n- b', '1. one\n2. two\n   - nested', '- [x] done\n- [ ] todo',
  '> quoted\n>\n> more', '> - in quote', '| a | b |\n|:--|--:|\n| 1 | 2 |', '```ts\nlet x = 1;\n\nlet y;\n```', '```mermaid\ngraph LR\nA-->B\n```', '---',
  '![i](https://x.y/a.png)', 'Text ![i](https://x.y/a.png) more', 'Line\n![i](https://x.y/a.png)\nafter', '[link](https://a.b) and https://c.d/e.', '~~gone~~ *em* _em_'];
let seed = 2026; const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
const corpus = [];
for (let i = 0; i < 600; i++) { const n = 1 + (rnd() * 5 | 0); const parts = []; for (let j = 0; j < n; j++) parts.push(PARTS[rnd() * PARTS.length | 0]); corpus.push(parts.join('\n\n')); }
const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
await p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), 1);
await p.clock.runFor(600);
out.answers = await p.evaluate(([hs, png]) => hs.map(([src, h]) => {
  const a = document.querySelector('#thread .ans'); a.innerHTML = h;
  for (const i of a.querySelectorAll('img')) i.src = png;
  const s = getSelection(), r = document.createRange(); r.selectNodeContents(a); s.removeAllRanges(); s.addRange(r);
  return [src, s.toString()];
}), [corpus.map(c => [c, markdown(c, { image: u => u })]), png]);
writeFileSync(join(here, 'expected', 'copy.json'), JSON.stringify(out, null, 1));
console.log(Object.fromEntries(Object.entries(out).map(([k, v]) => [k, v.thread?.length ?? v.length])));
await b.close();
