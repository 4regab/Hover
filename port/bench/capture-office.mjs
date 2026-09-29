// Baseline screenshots of the office page, driven the way Hover drives it: a stand-in
// chrome.webview bridge sends the `state` message from a fixture, the clock is fixed and
// advanced by hand, and the viewport is the notch's Default office (1120 x 440 DIP less
// the 8 px margin) at 100 %. Chromium here, not WebView2; the Windows run repeats it.
//   node port/bench/capture-office.mjs [outDir]    (needs `npm i playwright`)
import { chromium } from 'playwright';
import { readFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = process.argv[2] || join(repo, 'port', 'phase0', 'baseline');
mkdirSync(out, { recursive: true });
const fx = JSON.parse(readFileSync(join(repo, 'native', 'golden', 'fixtures', 'office-state.json'), 'utf8'));
const page0 = pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href;
const W = 1104, H = 424;

// BROWSER_CHANNEL=msedge on Windows: Edge is WebView2's engine, with the same system fonts.
const channel = process.env.BROWSER_CHANNEL;
// Playwright hides scrollbars unless told not to; WebView2 shows them, and they take width.
const browser = await chromium.launch({ ...(channel ? { channel } : {}), ignoreDefaultArgs: ['--hide-scrollbars'], args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--font-render-hinting=none'] });
// ONLY=a,b captures just those views.
const only = process.env.ONLY?.split(',');
async function view(name, { time = 'night', state = fx.state, steps = [], settle = 6000, query = '' } = {}) {
  if (only && !only.includes(name)) return;
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  await page.clock.install({ time: fx.now });
  await page.addInitScript(() => {
    const listeners = [];
    window.__posted = [];
    window.chrome = { webview: { addEventListener: (t, f) => listeners.push(f), postMessage: m => window.__posted.push(m) } };
    window.__send = m => listeners.forEach(f => f({ data: m }));
  });
  await page.goto(`${page0}?time=${time}${query}`);
  await page.waitForFunction(() => window.READY);
  await page.evaluate(s => window.__send(s), state);
  await page.clock.runFor(settle);
  for (const s of steps) { await s(page); await page.clock.runFor(700); }
  await page.screenshot({ path: join(out, `${name}.png`) });
  await ctx.close();
  console.log(name);
}
// DOM clicks: with the clock frozen, Playwright's wait for a settled element never ends.
const c = sel => p => p.evaluate(q => document.querySelector(q).click(), sel);
const tag = i => p => p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), i);
const empty = { ...fx.state, sessions: [], history: [] };
await view('office-night');
await view('office-day', { time: 'day' });
await view('office-night-empty', { state: empty });
await view('office-zoom', { steps: [p => p.mouse.move(560, 220), p => p.mouse.wheel(0, -400)] });
// The board and the TV are props in the scene; the page also opens them with ?panel=.
await view('panel-board', { query: '&panel=board' });
await view('panel-tv', { query: '&panel=tv' });
await view('panel-history', { steps: [c('#histBtn')] });
await view('panel-history-empty', { state: empty, steps: [c('#histBtn')] });
await view('fab-pick', { steps: [c('#fabMain')] });
await view('fab-open', { steps: [c('#fabMain'), c('#fabTools [data-tool="kiro"]')] });
await view('fab-open-draft', { steps: [c('#fabMain'), c('#fabTools [data-tool="kiro"]'), p => p.evaluate(() => { const i = document.querySelector('#nInput'); i.value = 'Add a test for the refresh expiry'; i.dispatchEvent(new Event('input')); })] });
await view('menu-model', { steps: [c('#fabMain'), c('#fabTools [data-tool="kiro"]'), c('#nModel')] });
await view('chat-working', { steps: [tag(0)] });
await view('chat-rich', { steps: [tag(1)] });
await view('chat-rich-scrolled', { steps: [tag(1), p => p.evaluate(() => { document.querySelector('#thread').scrollTop = 420; })] });
await view('chat-waking', { steps: [tag(2)] });
await view('chat-failed', { steps: [tag(3)] });
await view('chat-stopped', { steps: [tag(4)] });
await view('chat-composer-text', { steps: [tag(1), p => p.evaluate(() => { const i = document.querySelector('#input'); i.value = 'Ship it'; i.dispatchEvent(new Event('input')); })] });
await view('chat-composer-stop', { steps: [tag(0)] });
await view('confirm', { steps: [tag(1), c('#dDel')] });
await browser.close();
