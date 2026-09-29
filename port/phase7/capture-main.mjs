// Main's office page (55111fc) in Chromium at the notch's Default office, for the
// OpenCode views and the side-by-side checks of phase 7. The bridge is a stand-in, as
// in port/bench/capture-office.mjs; the fixture gains OpenCode and a question.
//   node port/phase7/capture-main.mjs OUT    (needs `npm i playwright`)
import { chromium } from 'playwright';
import { readFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = process.argv[2] || join(repo, 'port', 'phase7', 'main');
mkdirSync(out, { recursive: true });
const fx = JSON.parse(readFileSync(join(repo, 'native', 'golden', 'fixtures', 'office-state.json'), 'utf8'));
const page0 = pathToFileURL(join(repo, 'native', 'golden', 'page', 'kiro-office.html')).href;
const W = +(process.env.W || 1104), H = +(process.env.H || 424);
const oc = { id: 'opencode', name: 'OpenCode', ready: true, hint: '', access: 'full', readOnly: true, hideSteps: false,
  models: [{ id: '', name: 'Default', levels: null }, { id: 'p/a', name: 'A B · Prov', levels: ['low', 'high'] }, { id: 'p/m', name: 'M · Prov', levels: [] }],
  model: 'p/a', efforts: [], effort: 'high', effortLabel: 'Variant', questions: true };
const state = JSON.parse(JSON.stringify(fx.state));
state.tools = state.tools.map(t => ({ ...t, models: t.models.map(m => ({ ...m, levels: null })), effortLabel: 'Effort', questions: false })).concat([oc]);
const q = { id: 'que_1', kind: 'question', title: 'Asks you a question', line: 'Asks you Indent', command: null, path: null, preview: null, added: 0, removed: 0,
  reason: 'Tabs or spaces?', danger: false, allow: 'Answer', more: 0,
  questions: [{ header: 'Indent', question: 'Tabs or spaces?', options: [{ label: 'Tabs', description: 'Indent with tab characters' }, { label: 'Spaces', description: '' }], multiple: false, custom: true }] };
const asked = JSON.parse(JSON.stringify(state));
asked.sessions[0] = { ...asked.sessions[0], tool: 'opencode', stage: 'waiting', ask: q };
asked.sessions[0].turns.at(-1).stage = 'waiting';
const browser = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'], args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--font-render-hinting=none'] });
const only = process.env.ONLY?.split(',');
async function view(name, { time = 'night', st = state, steps = [], settle = 6000 } = {}) {
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
  await page.goto(`${page0}?time=${time}`);
  await page.waitForFunction(() => window.READY);
  await page.evaluate(s => window.__send(s), st);
  await page.clock.runFor(settle);
  for (const s of steps) { await s(page); await page.clock.runFor(700); }
  await page.screenshot({ path: join(out, `${name}.png`) });
  await ctx.close();
  console.log(name);
}
const c = sel => p => p.evaluate(q => document.querySelector(q).click(), sel);
const tag = i => p => p.evaluate(k => document.querySelectorAll('.tag .nm')[k].click(), i);
await view('office-night');
await view('fab-pick', { steps: [c('#fabMain')] });
await view('fab-open-opencode', { steps: [c('#fabMain'), c('#fabTools [data-tool="opencode"]')] });
await view('menu-model-opencode', { steps: [c('#fabMain'), c('#fabTools [data-tool="opencode"]'), c('#nModel')] });
await view('menu-model-kiro', { steps: [c('#fabMain'), c('#fabTools [data-tool="kiro"]'), c('#nModel')] });
await view('chat-rich', { steps: [tag(1)] });
await view('question-over', { st: asked });
await view('question-chat', { st: asked, steps: [tag(0)] });
await view('question-chat-picked', { st: asked, steps: [tag(0), p => p.evaluate(() => [...document.querySelectorAll('[data-opt="Tabs"]')].pop().click())] });
await view('panel-history', { steps: [c('#histBtn')] });
await browser.close();
