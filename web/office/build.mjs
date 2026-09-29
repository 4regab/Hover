// Builds the Kiro office: main.js and three.js bundled by esbuild into one page, with
// the pixel font inlined. It goes to native/golden/page/kiro-office.html (the page the goldens and captures run); the
// same file opened in a browser plays with demo sessions.
//
//   npm ci          (once, in web/office)
//   node build.mjs
import { build } from 'esbuild';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const js = (await build({ entryPoints: [join(here, 'main.js')], bundle: true, minify: true, format: 'iife', write: false, target: 'es2022', legalComments: 'inline' })).outputFiles[0].text;
const font = readFileSync(join(here, 'fonts', 'PixelifySans.ttf')).toString('base64');
const page = readFileSync(join(here, 'page.html'), 'utf8')
  .replace('/*FONT*/', () => `@font-face{font-family:"Pixelify Sans";font-weight:400 700;src:url(data:font/ttf;base64,${font}) format("truetype")}`)
  .replace('/*APP*/', () => js)
  .replace(/\r?\n/g, '\r\n');
const out = join(here, '..', '..', 'native', 'golden', 'page', 'kiro-office.html');
writeFileSync(out, page);
console.log(`${out} ${(page.length / 1048576).toFixed(2)} MB`);
