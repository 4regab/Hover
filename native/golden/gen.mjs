// Writes the expected output of the office's own md.js and diagram.js for every fixture,
// so the Rust ports are checked against the JavaScript itself, not against a reading of it.
//   node native/golden/gen.mjs
import { readFileSync, writeFileSync, readdirSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { markdown } from '../../web/office/md.js';
import { flowchart } from '../../web/office/diagram.js';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const fx = join(here, 'fixtures'), out = join(here, 'expected');
mkdirSync(out, { recursive: true });

// main.js's imageFor, for a session with a files host (copied: it is not exported).
const session = { files: 'fabc123def456.hover', folder: 'C:\\proj\\app' };
function imageFor(s) {
  return src => {
    if (/^https?:\/\//i.test(src)) return src;
    if (!s.files) return null;
    let q = src.replace(/^file:\/+/i, '').replace(/\\/g, '/');
    try { q = decodeURIComponent(q); } catch {}
    const root = (s.folder || '').replace(/\\/g, '/').replace(/\/+$/, '') + '/';
    if (/^[a-z]:\//i.test(q)) { if (!q.toLowerCase().startsWith(root.toLowerCase())) return null; q = q.slice(root.length); }
    q = q.replace(/^\.\//, '');
    if (q.startsWith('/') || q.split('/').includes('..')) return null;
    return `https://${s.files}/` + q.split('/').map(encodeURIComponent).join('/');
  };
}

let n = 0;
for (const f of readdirSync(fx).filter(f => f.endsWith('.md')).sort()) {
  const src = readFileSync(join(fx, f), 'utf8');
  writeFileSync(join(out, f.replace(/\.md$/, '.html')), markdown(src, { image: imageFor(session) }));
  writeFileSync(join(out, f.replace(/\.md$/, '.noimg.html')), markdown(src));
  n++;
}
const cases = readFileSync(join(fx, 'mermaid-cases.txt'), 'utf8').split(/\r?\n===\r?\n/);
cases.forEach((c, i) => { const svg = flowchart(c); writeFileSync(join(out, `mermaid-${i}.svg`), svg ?? 'null'); n++; });
const paths = ['https://x/y.png', 'shot.png', './img/a b.png', 'C:\\proj\\app\\docs\\d.png', 'c:/PROJ/APP/x.png', 'C:\\other\\x.png', '../up.png', '/abs.png', 'file:///C:/proj/app/f.png', 'a/%2e%2e/b.png', 'bad%zz.png', 'dir\\sub\\x%20y.png'];
writeFileSync(join(out, 'image-paths.json'), JSON.stringify(paths.map(p => [p, imageFor(session)(p)]), null, 1));
console.log(`wrote ${n} goldens`);

// A seeded random corpus: fragments that stress the regexes, combined at random.
function rng(s) { return () => { s |= 0; s = s + 0x6D2B79F5 | 0; let t = Math.imul(s ^ s >>> 15, 1 | s); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
const R = rng(2026);
const bits = ['*', '**', '_', '__', '~~', '`', '``', '[', ']', '(', ')', '![', 'http://a.b/c', 'https://x.y/_z_', ' ', '  ', '\n', '\n\n', '#', '## ', '> ', '- ', '1. ', '12) ', '   ', '|', '|---|', ':--:', '```', '~~~', '```mermaid\ngraph LR\na-->b\n```', '\\|', '&', '<b>', '"', "'", 'word', 'snake_case', 'é', '日本', '😀', '\u00a0', '\u2028', '\t', '[ ] ', '[x] ', '---', '***', 'x', '2*3', '"t"', '.', ',', '!', '\u0001'];
const random = [];
for (let k = 0; k < 4000; k++) { let s = ''; const n = 1 + (R() * 40 | 0); for (let j = 0; j < n; j++) s += bits[R() * bits.length | 0]; // md.js throws on some inputs and never returns on others (see MARKDOWN.md): recorded as such.
  let h; try { h = vm.runInNewContext('f(s)', { f: x => markdown(x, { image: imageFor(session) }), s }, { timeout: 300 }); } catch (e) { h = e.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT' ? '\u0000HANG' : '\u0000THROW'; } random.push([s, h]); }
writeFileSync(join(out, 'random.json'), JSON.stringify(random));
console.log('wrote random corpus');
