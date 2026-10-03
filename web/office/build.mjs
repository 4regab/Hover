// Builds the Kiro office: main.js and three.js bundled by esbuild into one page, with
// the pixel font inlined. It is written to web/office/dist/kiro-office.html (not tracked), which scripts/build-macos.sh
// puts in Hover.app; the same file opened in a browser plays with demo sessions.
//
//   npm ci          (once, in web/office)
//   node build.mjs
import { build } from 'esbuild';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const js = (await build({ entryPoints: [join(here, 'main.js')], bundle: true, minify: true, format: 'iife', write: false, target: 'es2022', legalComments: 'inline' })).outputFiles[0].text;
const font = readFileSync(join(here, 'fonts', 'PixelifySans.ttf')).toString('base64');
const page = readFileSync(join(here, 'page.html'), 'utf8')
  .replace('/*FONT*/', () => `@font-face{font-family:"Pixelify Sans";font-weight:400 700;src:url(data:font/ttf;base64,${font}) format("truetype")}`)
  .replace('/*APP*/', () => js)
  .replace(/\r?\n/g, '\r\n');
mkdirSync(join(here, 'dist'), { recursive: true });
const out = join(here, 'dist', 'kiro-office.html');
writeFileSync(out, page);
console.log(`${out} ${(page.length / 1048576).toFixed(2)} MB`);
