// Images that don't load, as the page lays them out: each case is an answer whose
// images all fail (every https request is aborted), and the record is where each
// paragraph and image box sits relative to the answer's top, and the answer's height.
// A pending image looks the same in Chromium. Needs Playwright:
//   npm i playwright && node native/golden/gen-broken.mjs
import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { markdown } from '../../web/office/md.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fx = JSON.parse(readFileSync(join(here, 'fixtures', 'office-state.json'), 'utf8'));
const CASES = [
  'Before\n\n![The flow](https://e.x/a.png)\n\nAfter',
  'Before\n\n![](https://e.x/b.png)\n\nAfter',
  'Text ![Alt here](https://e.x/c.png) more\n\nAfter',
  'Before\n\n![One](https://e.x/d.png)![Two](https://e.x/e.png)\n\nAfter',
  'Before\n\n![A much longer alternative text that will certainly need to wrap onto more lines than one](https://e.x/f.png)\n\nAfter',
];
const b = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] });
const p = await b.newPage({ viewport: { width: 1104, height: 424 } });
await p.clock.install({ time: fx.now });
await p.addInitScript(() => { const L = []; window.chrome = { webview: { addEventListener: (t, f) => L.push(f), postMessage() {} } }; window.__send = m => L.forEach(f => f({ data: m })); });
await p.route('https://**', r => r.abort());
await p.goto(pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href);
await p.waitForFunction(() => window.READY);
await p.evaluate(s => window.__send(s), fx.state);
await p.clock.runFor(3000);
await p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), 1);
await p.clock.runFor(600);
const out = [];
for (const src of CASES) {
  out.push({ src, ...await p.evaluate(async h => {
    const a = document.querySelector('#thread .ans'); a.innerHTML = h;
    // renderDrawer: each code block in an answer gets its language and a Copy button.
    for (const pre of a.querySelectorAll('pre')) { const box = document.createElement('div'); box.className = 'cb'; box.innerHTML = `<div class="ch">${pre.dataset.lang || 'code'}<button type="button" data-copy>Copy</button></div>`; pre.replaceWith(box); box.appendChild(pre); }
    await Promise.all([...a.querySelectorAll('img')].map(i => new Promise(r => { if (i.complete) r(); else { i.onerror = r; setTimeout(r, 2000); } })));
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    const box = a.getBoundingClientRect();
    const r2 = e => { const q = e.getBoundingClientRect(); return [+(q.x - box.x).toFixed(2), +(q.y - box.y).toFixed(2), +q.width.toFixed(2), +q.height.toFixed(2)]; };
    return { width: +box.width.toFixed(2), height: +box.height.toFixed(2), imgs: [...a.querySelectorAll('img')].map(r2), ps: [...a.querySelectorAll('p')].map(r2) };
  }, markdown(src, { image: u => u })) });
}
writeFileSync(join(here, 'expected', 'broken.json'), JSON.stringify(out, null, 1));
console.log(JSON.stringify(out));
await b.close();
