// The office page's 3D scene alone, for comparing the native renderer against it:
// capture-office.mjs's set-up (the fixture's state and clock, 1104 x 424 at 100 %),
// with everything over the canvas hidden (tags, HUD, the new-task circle). The
// vignette (#office::after) stays, as the native office draws it too.
//   node port/phase2/capture-scene.mjs outDir      (needs `npm i playwright`)
import { chromium } from 'playwright';
import { readFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = process.argv[2] || join(repo, 'port', 'phase2', 'shots');
mkdirSync(out, { recursive: true });
const fx = JSON.parse(readFileSync(join(repo, 'native', 'golden', 'fixtures', 'office-state.json'), 'utf8'));
const page0 = pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href;
const browser = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'], args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--font-render-hinting=none'] });
async function view(name, { time = 'night', empty = false, zoom = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1104, height: 424 }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  await page.clock.install({ time: fx.now });
  await page.addInitScript(() => {
    const listeners = [];
    window.chrome = { webview: { addEventListener: (t, f) => listeners.push(f), postMessage: () => {} } };
    window.__send = m => listeners.forEach(f => f({ data: m }));
  });
  await page.goto(`${page0}?time=${time}`);
  await page.waitForFunction(() => window.READY);
  await page.addStyleTag({ content: '#office > *:not(#gl){visibility:hidden!important}' });
  const state = empty ? { ...fx.state, sessions: [] } : fx.state;
  await page.evaluate(s => window.__send(s), state);
  if (zoom) await page.evaluate(() => { for (let i = 0; i < 2; i++) window.dispatchEvent(new KeyboardEvent('keydown', { key: '+' })); });
  await page.clock.runFor(6000);
  await page.screenshot({ path: join(out, `${name}.png`) });
  await ctx.close();
}
await view('page-scene-night');
await view('page-scene-day', { time: 'day' });
await view('page-scene-empty', { empty: true });
await browser.close();
