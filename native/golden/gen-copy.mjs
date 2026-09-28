// What the office copies: a whole thread selected in the real page (kiro-office.html,
// host mode, the office-state fixture) and read as the clipboard would get it. The
// native chat's copy is tested against these. Needs Playwright:
//   npm i playwright && node native/golden/gen-copy.mjs
import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fx = JSON.parse(readFileSync(join(here, 'fixtures', 'office-state.json'), 'utf8'));
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 1104, height: 424 } });
await p.clock.install({ time: fx.now });
await p.addInitScript(() => { const L = []; window.chrome = { webview: { addEventListener: (t, f) => L.push(f), postMessage() {} } }; window.__send = m => L.forEach(f => f({ data: m })); });
await p.goto(pathToFileURL(join(repo, 'src', 'Hover', 'Assets', 'kiro-office.html')).href);
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
writeFileSync(join(here, 'expected', 'copy.json'), JSON.stringify(out, null, 1));
console.log(Object.fromEntries(Object.entries(out).map(([k, v]) => [k, v.thread.length])));
await b.close();
