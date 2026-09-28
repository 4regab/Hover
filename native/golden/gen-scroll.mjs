// The page with its scrollbars shown, as WebView2 shows them (Playwright hides them
// unless asked): the thread's thin scrollbar takes its width from the content, and a
// code block or table wider than the drawer scrolls sideways under a bar of its own.
// Records the thread's inner width and height and every scrolling box's place, size and
// scroll width, for fixtures/rich.md and fixtures/wide.md. Needs Playwright:
//   npm i playwright && node native/golden/gen-scroll.mjs
import { chromium } from 'playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { markdown } from '../../web/office/md.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fx = JSON.parse(readFileSync(join(here, 'fixtures', 'office-state.json'), 'utf8'));
const b = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] });
const p = await b.newPage({ viewport: { width: 1104, height: 424 } });
await p.clock.install({ time: fx.now });
await p.addInitScript(() => { const L = []; window.chrome = { webview: { addEventListener: (t, f) => L.push(f), postMessage() {} } }; window.__send = m => L.forEach(f => f({ data: m })); });
await p.goto(pathToFileURL(join(repo, 'src', 'Hover', 'Assets', 'kiro-office.html')).href);
await p.waitForFunction(() => window.READY);
await p.evaluate(s => window.__send(s), fx.state);
await p.clock.runFor(3000);
await p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), 1);
await p.clock.runFor(600);
const out = {};
for (const name of ['rich', 'wide']) {
  const src = readFileSync(join(here, 'fixtures', `${name}.md`), 'utf8');
  out[name] = await p.evaluate(h => {
    const th = document.querySelector('#thread'), a = th.querySelector('.ans');
    // One turn: the fixture session's prompt and steps, this answer.
    a.innerHTML = h;
    const top = a.getBoundingClientRect().y;
    const box = e => { const r = e.getBoundingClientRect(); return { y: +(r.y - top).toFixed(2), h: +r.height.toFixed(2), w: +r.width.toFixed(2), sw: e.scrollWidth, cw: e.clientWidth }; };
    return {
      thread: { cw: th.clientWidth, off: th.offsetWidth - th.clientWidth },
      answer: +a.getBoundingClientRect().height.toFixed(2),
      boxes: [...a.querySelectorAll('pre, .table, figure')].map(box),
    };
  }, markdown(src, { image: u => u }));
}
writeFileSync(join(here, 'expected', 'scroll.json'), JSON.stringify(out, null, 1));
console.log(JSON.stringify(out));
await b.close();
