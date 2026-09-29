// Kiro page preview · the agent office. Every Kiro session is a little bot at its own
// desk. Click a bot (or its chip in the dock) to open that session. The room is an
// isometric voxel office drawn with three.js: the static room is one merged mesh,
// each bot is a handful of boxes. Built by build.mjs into one kiro-page.html.
import * as THREE from 'three';
import { markdown } from './md.js';
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js';

const $ = s => document.querySelector(s), TAU = Math.PI * 2;
const Q = new URLSearchParams(window.QUERY || location.search.slice(1));
const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
function rng(s) { return () => { s |= 0; s = s + 0x6D2B79F5 | 0; let t = Math.imul(s ^ s >>> 15, 1 | s); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; }; }
const R = rng(11);
const ease = (v, to, k, dt) => v + (to - v) * (1 - Math.exp(-k * dt));
const angTo = (a, b, k, dt) => { const d = ((b - a + Math.PI) % TAU + TAU) % TAU - Math.PI; return a + d * (1 - Math.exp(-k * dt)); };

// ── Renderer and camera ─────────────────────────────────────────────────
const view = $('#office'), canvas = $('#gl');
// Pixel art needs no smoothing: one pixel per CSS pixel and no antialiasing look the
// same and keep the frame buffers a quarter of the size.
const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, alpha: true, powerPreference: 'low-power' });
renderer.setPixelRatio(1);
renderer.setClearColor(0, 0);
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
// The room never moves, so the shadow map is redrawn only when something that casts
// one has moved (see frame()).
renderer.shadowMap.autoUpdate = false;
const scene = new THREE.Scene();
const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 1, 90);
const ISO = new THREE.Vector3(1, 0.86, 1).normalize().multiplyScalar(40);
const RIGHT = new THREE.Vector3(1, 0, -1).normalize();
const cam = { x: 0, y: 1.7, z: 0, zoom: 1 }, camTo = { ...cam };
let aspect = 1, W = 1, H = 1;
function resize() { W = view.clientWidth; H = view.clientHeight; renderer.setSize(W, H, false); aspect = W / H; }
function halfWidth(zoom) { return Math.max(6.2 * aspect, 9.2) / zoom; }
function placeCamera() {
  const t = new THREE.Vector3(cam.x, cam.y, cam.z);
  camera.position.copy(t).add(ISO); camera.lookAt(t);
  const w = halfWidth(cam.zoom), h = w / aspect;
  camera.left = -w; camera.right = w; camera.top = h; camera.bottom = -h; camera.updateProjectionMatrix();
}

// ── Building blocks ─────────────────────────────────────────────────────
const unit = new THREE.BoxGeometry(1, 1, 1);
const voxMat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.88 });
// Boxes gathered into one mesh with a colour per box; x, y, z is the low corner.
class Vox {
  constructor() { this.g = []; }
  box(x, y, z, w, h, d, c, j = 0.04) {
    const g = unit.clone(); g.scale(w, h, d); g.translate(x + w / 2, y + h / 2, z + d / 2);
    const col = new THREE.Color(c); if (j) col.multiplyScalar(1 + (R() - 0.5) * j * 2);
    const n = g.attributes.position.count, a = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) { a[i * 3] = col.r; a[i * 3 + 1] = col.g; a[i * 3 + 2] = col.b; }
    g.setAttribute('color', new THREE.BufferAttribute(a, 3)); this.g.push(g); return this;
  }
  mesh(cast = true) { const m = new THREE.Mesh(mergeGeometries(this.g), voxMat); m.castShadow = cast; m.receiveShadow = true; this.g.forEach(g => g.dispose()); this.g = []; return m; }
}
const glowTex = (() => { const c = document.createElement('canvas'); c.width = c.height = 64; const x = c.getContext('2d'), g = x.createRadialGradient(32, 32, 0, 32, 32, 32);
  g.addColorStop(0, 'rgba(255,255,255,1)'); g.addColorStop(0.35, 'rgba(255,255,255,.4)'); g.addColorStop(1, 'rgba(255,255,255,0)'); x.fillStyle = g; x.fillRect(0, 0, 64, 64); return new THREE.CanvasTexture(c); })();
function glow(color, size, opacity = 1) {
  const s = new THREE.Sprite(new THREE.SpriteMaterial({ map: glowTex, color, opacity, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
  s.scale.setScalar(size); return s;
}
function pixelCanvas(w, h) {
  const c = document.createElement('canvas'); c.width = w; c.height = h;
  const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace; t.magFilter = THREE.NearestFilter; t.minFilter = THREE.LinearFilter; t.generateMipmaps = false;
  return { c, x: c.getContext('2d'), t };
}
function screen(tex, w, h) { return new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ map: tex, toneMapped: false })); }
function box(parent, w, h, d, x, y, z, mat, shadow = true) { const m = new THREE.Mesh(unit, mat); m.scale.set(w, h, d); m.position.set(x, y, z); m.castShadow = shadow; m.receiveShadow = true; parent.add(m); return m; }
function pivot(parent, x, y, z) { const g = new THREE.Group(); g.position.set(x, y, z); parent.add(g); return g; }

// ── The room ────────────────────────────────────────────────────────────
// 14 × 11 tiles. The back wall is at z = Z0, the left wall at x = X0; the camera looks
// in from the open front-right corner.
const RW = 14, RD = 11, WH = 4, X0 = -7, Z0 = -5.5;
const walls = new Vox(), room = new Vox();
room.box(X0 - 0.3, -0.7, Z0 - 0.3, RW + 0.3, 0.6, RD + 0.3, 0x1c1215, 0);
for (let i = 0; i < RW; i++) for (let k = 0; k < RD; k++) room.box(X0 + i, -0.1, Z0 + k, 1, 0.1, 1, (i + k) % 2 ? 0x5a3c34 : 0x48302b, 0.05);
room.box(X0 - 0.3, -0.7, Z0 - 0.3 + RD + 0.3 - 0.02, RW + 0.3, 0.6, 0.02, 0x140c0f, 0);
// Walls: planks above a darker wainscot, a rail, a cap and a baseboard.
for (let x = 0; x < RW; x += 0.5) walls.box(X0 + x, -0.7, Z0 - 0.3, 0.5, WH + 0.7, 0.3, (x * 2) % 2 ? 0x6e4643 : 0x684240, 0.03);
for (let z = 0; z < RD; z += 0.5) walls.box(X0 - 0.3, -0.7, Z0 + z, 0.3, WH + 0.7, 0.5, (z * 2) % 2 ? 0x633e3c : 0x5e3a38, 0.03);
walls.box(X0 - 0.3, -0.7, Z0 - 0.3, 0.3, WH + 0.7, 0.3, 0x5e3a38, 0);
walls.box(X0, 0, Z0, RW, 1.15, 0.035, 0x4b2f2c, 0.02).box(X0, 0, Z0, 0.035, 1.15, RD, 0x462b29, 0.02);
walls.box(X0, 1.15, Z0, RW, 0.07, 0.06, 0x80564d, 0).box(X0, 1.15, Z0, 0.06, 0.07, RD, 0x7a524a, 0);
walls.box(X0, 0, Z0, RW, 0.16, 0.07, 0x33201d, 0).box(X0, 0, Z0, 0.07, 0.16, RD, 0x301e1b, 0);
walls.box(X0 - 0.3, WH, Z0 - 0.3, RW + 0.3, 0.1, 0.3, 0x8d5f57, 0).box(X0 - 0.3, WH, Z0 - 0.3, 0.3, 0.1, RD + 0.3, 0x86594f, 0);
walls.box(X0 - 0.3, -0.7, Z0 + RD - 0.02, 0.3, WH + 0.8, 0.02, 0x3a2422, 0).box(X0 + RW - 0.02, -0.7, Z0 - 0.3, 0.02, WH + 0.8, 0.3, 0x3a2422, 0);

// Door on the back wall; the bots come in and leave through it. The panel swings.
const DOOR = { x: -5.65, z: Z0 + 0.35 };
walls.box(-6.3, 0, Z0, 1.3, 2.42, 0.08, 0x2c1b17, 0).box(-6.2, 0, Z0 + 0.01, 1.1, 2.3, 0.08, 0x0b0708, 0);
const doorV = new Vox();
doorV.box(0, 0, 0, 1.1, 2.3, 0.07, 0x5c3b2b, 0.02).box(0.14, 1.28, 0.07, 0.82, 0.82, 0.02, 0x6b4633, 0).box(0.14, 0.24, 0.07, 0.82, 0.86, 0.02, 0x6b4633, 0).box(0.9, 1.05, 0.07, 0.08, 0.08, 0.06, 0xe0ab4c, 0);
const door = pivot(scene, -6.2, 0, Z0 + 0.02); door.add(doorV.mesh());
room.box(-6.35, 0, Z0 + 0.12, 1.4, 0.02, 0.75, 0x6f3b2a, 0.02).box(-6.2, 0.02, Z0 + 0.22, 1.1, 0.005, 0.55, 0x8a4c34, 0);
const exitLamp = box(scene, 0.34, 0.12, 0.08, DOOR.x, 2.62, Z0 + 0.05, new THREE.MeshBasicMaterial({ color: 0xffb35c, toneMapped: false }), false);
const exitGlow = glow(0xffa24a, 1.4, 0.55); exitGlow.position.set(DOOR.x, 2.62, Z0 + 0.2); scene.add(exitGlow);

// Window with curtains; the sky behind it is a small pixel canvas.
walls.box(0.1, 3.25, Z0, 2.8, 0.12, 0.12, 0x3a2620, 0).box(-0.05, 1.22, Z0, 3.1, 0.12, 0.26, 0x4a3026, 0)
  .box(0.1, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).box(2.78, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0)
  .box(1.46, 1.34, Z0, 0.08, 1.92, 0.1, 0x3a2620, 0).box(0.2, 2.26, Z0, 2.6, 0.07, 0.1, 0x3a2620, 0)
  .box(-0.5, 3.52, Z0 + 0.1, 4, 0.05, 0.05, 0x241612, 0);
for (const [cx, dir] of [[-0.45, 1], [2.9, -1]]) for (let i = 0; i < 4; i++) walls.box(cx + i * 0.13, 1.0, Z0 + 0.06 + (i % 2) * 0.05, 0.14, 2.52, 0.1, i % 2 ? 0x9b5230 : 0x8a4629, 0);
const sky = pixelCanvas(128, 96);
const skyPane = screen(sky.t, 2.56, 1.9); skyPane.position.set(1.5, 2.3, Z0 + 0.012); scene.add(skyPane);

// Wall TV with the office stats, and a low cabinet under it.
walls.box(4.1, 1.62, Z0, 2.6, 1.52, 0.1, 0x0c0b10, 0);
const tv = pixelCanvas(208, 118);
const tvPane = screen(tv.t, 2.44, 1.38); tvPane.position.set(5.4, 2.38, Z0 + 0.105); scene.add(tvPane);
const tvGlow = glow(0x5aa8ff, 3.6, 0.22); tvGlow.position.set(5.4, 2.3, Z0 + 0.6); scene.add(tvGlow);
room.box(4.3, 0, Z0 + 0.02, 2.2, 0.55, 0.5, 0x4c3028).box(4.25, 0.55, Z0 + 0.02, 2.3, 0.05, 0.54, 0x5e3c30, 0)
  .box(4.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).box(5.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0)
  .box(4.45, 0.6, Z0 + 0.12, 0.26, 0.42, 0.26, 0x22212a).box(6.1, 0.6, Z0 + 0.1, 0.24, 0.1, 0.3, 0xc8a24a).box(6.12, 0.7, Z0 + 0.1, 0.2, 0.08, 0.3, 0x5a7aa0);

// Coffee counter between the door and the window.
room.box(-4.6, 0, Z0 + 0.02, 1.9, 0.86, 0.62, 0x5a3a2c).box(-4.65, 0.86, Z0 + 0.02, 2, 0.06, 0.66, 0xd8cdbf, 0.02)
  .box(-4.5, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).box(-3.6, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0)
  .box(-4.45, 0.92, Z0 + 0.1, 0.42, 0.56, 0.4, 0x26252d).box(-4.4, 1.2, Z0 + 0.5, 0.32, 0.18, 0.02, 0x121118)
  .box(-3.8, 0.92, Z0 + 0.2, 0.12, 0.14, 0.12, 0xeeeeee).box(-3.6, 0.92, Z0 + 0.25, 0.12, 0.14, 0.12, 0x9b6bff)
  .box(-3.3, 0.92, Z0 + 0.12, 0.34, 0.4, 0.3, 0x7a8a95).box(-4.6, 1.85, Z0, 1.9, 0.05, 0.3, 0x4a2f24, 0);
for (let i = 0; i < 5; i++) room.box(-4.5 + i * 0.36, 1.9, Z0 + 0.06, 0.2, 0.22 + (i % 2) * 0.08, 0.18, [0xc8a24a, 0x9a4f2e, 0x7a9a6a, 0xdcd2c4, 0xb46a3a][i]);
const coffeeLed = glow(0x7ee0ff, 0.35, 0.9); coffeeLed.position.set(-4.24, 1.29, Z0 + 0.53); scene.add(coffeeLed);
const steam = [0, 1, 2].map(i => { const s = glow(0xffffff, 0.2, 0.25); scene.add(s); return s; });

// Left wall: bookcase, the session board, the clock, a painting over the sofa.
room.box(X0, 0, -4.3, 0.46, 2.42, 0.06, 0x4a2e24).box(X0, 0, -2.76, 0.46, 2.42, 0.06, 0x4a2e24).box(X0, 0, -4.3, 0.05, 2.42, 1.6, 0x3a241c, 0);
const BOOKS = [0x8a3b2e, 0xc9a24a, 0x4a6b4a, 0x7b5aa6, 0xd07a3a, 0x3a5a8a, 0xb8b0a0, 0x9a4a5a];
for (const y of [0, 0.6, 1.2, 1.8, 2.36]) {
  room.box(X0, y, -4.26, 0.46, 0.06, 1.52, 0x55352a, 0.02);
  if (y > 2) continue;
  for (let z = -4.2; z < -2.9;) { const w = 0.06 + R() * 0.07, h = 0.26 + R() * 0.2; if (z + w > -2.82) break; room.box(X0 + 0.06, y + 0.06, z, 0.32 + R() * 0.06, Math.min(h, 0.5), w, BOOKS[R() * BOOKS.length | 0], 0.06); z += w + (R() < 0.12 ? 0.08 : 0.006); }
}
walls.box(X0, 1.46, -2.2, 0.07, 1.74, 3, 0x3a2620, 0);
const board = pixelCanvas(480, 280);
const boardPane = screen(board.t, 2.84, 1.6); boardPane.rotation.y = Math.PI / 2; boardPane.position.set(X0 + 0.075, 2.33, -0.7); scene.add(boardPane);
walls.box(X0, 2.42, 1.22, 0.09, 0.6, 1.16, 0x0f0d12, 0);
const clock = pixelCanvas(96, 44);
const clockPane = screen(clock.t, 1.04, 0.48); clockPane.rotation.y = Math.PI / 2; clockPane.position.set(X0 + 0.095, 2.72, 1.8); scene.add(clockPane);
const clockGlow = glow(0xff7a2a, 1.6, 0.35); clockGlow.position.set(X0 + 0.3, 2.72, 1.8); scene.add(clockGlow);
walls.box(X0, 1.6, 3.55, 0.06, 1.02, 1.42, 0x2e1c18, 0).box(X0 + 0.06, 1.68, 3.63, 0.01, 0.86, 1.26, 0x41628f, 0)
  .box(X0 + 0.07, 1.68, 3.63, 0.01, 0.3, 1.26, 0x3f6a44, 0).box(X0 + 0.075, 1.9, 3.75, 0.01, 0.3, 0.5, 0x5a7a5a, 0)
  .box(X0 + 0.075, 1.95, 4.2, 0.01, 0.42, 0.55, 0x6a8a6a, 0).box(X0 + 0.08, 2.24, 4.45, 0.01, 0.13, 0.13, 0xffd070, 0)
  .box(X0 + 0.075, 2.28, 4.3, 0.01, 0.09, 0.3, 0xe8f0ff, 0);

// Lounge: sofa, coffee table, rug and a floor lamp.
room.box(-6.8, 0, 2.95, 2.8, 0.02, 2.5, 0x6f4430, 0.02).box(-6.5, 0.02, 3.25, 2.2, 0.01, 1.9, 0x8a5a3c, 0.02);
room.box(X0 + 0.05, 0, 3.2, 0.95, 0.42, 2.1, 0x5b3a6a).box(X0 + 0.05, 0.42, 3.2, 0.26, 0.55, 2.1, 0x4f3160)
  .box(X0 + 0.05, 0.42, 3.02, 0.95, 0.24, 0.2, 0x553565).box(X0 + 0.05, 0.42, 5.28, 0.95, 0.24, 0.2, 0x553565)
  .box(X0 + 0.32, 0.42, 3.24, 0.66, 0.1, 0.98, 0x6b4a7a).box(X0 + 0.32, 0.42, 4.28, 0.66, 0.1, 0.98, 0x6b4a7a)
  .box(X0 + 0.33, 0.52, 3.4, 0.14, 0.34, 0.42, 0xd9a64a).box(X0 + 0.33, 0.52, 4.8, 0.14, 0.3, 0.36, 0x5aa8a0);
room.box(-5.55, 0.32, 3.7, 0.8, 0.06, 1.25, 0x6b4a36).box(-5.5, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).box(-4.85, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c)
  .box(-5.5, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).box(-4.85, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c)
  .box(-5.3, 0.38, 3.9, 0.3, 0.05, 0.4, 0x3a5a8a).box(-5.0, 0.38, 4.5, 0.12, 0.14, 0.12, 0xeeeeee);
room.box(-6.75, 0, 2.55, 0.26, 0.04, 0.26, 0x241a16).box(-6.64, 0.04, 2.66, 0.04, 1.6, 0.04, 0x241a16);
const floorShade = box(scene, 0.44, 0.3, 0.44, -6.62, 1.78, 2.68, new THREE.MeshBasicMaterial({ color: 0xffc27a, toneMapped: false }), false);

// Beanbags at the front right.
room.box(4.7, 0, 3.8, 0.9, 0.3, 0.9, 0x7a4a9a).box(4.8, 0.3, 3.9, 0.7, 0.14, 0.7, 0x8a5aaa).box(4.75, 0.3, 3.82, 0.2, 0.34, 0.84, 0x6a3a8a)
  .box(5.9, 0, 4.4, 0.8, 0.28, 0.8, 0x2f8a7a).box(6.0, 0.28, 4.5, 0.6, 0.12, 0.6, 0x3a9a8a);

function plant(v, x, z, s = 1, y = 0, seed = 1) {
  const r = rng(seed);
  v.box(x - 0.2 * s, y, z - 0.2 * s, 0.4 * s, 0.34 * s, 0.4 * s, 0xa4552e).box(x - 0.23 * s, y + 0.3 * s, z - 0.23 * s, 0.46 * s, 0.07 * s, 0.46 * s, 0xb8653a)
    .box(x - 0.03 * s, y + 0.34 * s, z - 0.03 * s, 0.06 * s, 0.55 * s, 0.06 * s, 0x4a3a22);
  for (let i = 0; i < 11; i++) { const a = r() * TAU, rr = r() * 0.3 * s, h = (0.45 + r() * 0.6) * s, w = (0.14 + r() * 0.16) * s;
    v.box(x + Math.cos(a) * rr - w / 2, y + h, z + Math.sin(a) * rr - w / 2, w, w * 0.7, w, [0x4f7a3a, 0x3e6630, 0x6a9a45, 0x5a8a3a][i % 4], 0.08); }
}
plant(room, 6.4, Z0 + 0.55, 1.6, 0, 3); plant(room, -0.6, Z0 + 0.5, 1.2, 0, 5); plant(room, 6.4, 5, 1.5, 0, 7); plant(room, -3.3, 5.0, 1, 0, 9); plant(room, -3.95, Z0 + 0.3, 0.55, 0.92, 4);

// Desks: two rows of three. A bot sits on the -x side facing +x, so its face is lit
// by the screen and turned towards the camera.
const DESKS = [[-3.2, -1.6], [0.6, -1.6], [4.4, -1.6], [-3.2, 2.1], [0.6, 2.1], [4.4, 2.1]].map(([x, z]) => ({ x, z, seat: x - 0.67 }));
for (const z of [-1.6, 2.1]) room.box(-4.9, 0, z - 1.15, 10.4, 0.015, 2.3, z < 0 ? 0x3f4a3a : 0x6e4a2a, 0.02).box(-4.7, 0.015, z - 0.95, 10, 0.006, 1.9, z < 0 ? 0x4a5846 : 0x7e5634, 0.02);
const lamps = [], shades = [], deskGlows = [];
for (const d of DESKS) {
  const { x, z } = d;
  room.box(x - 0.42, 0.64, z - 0.78, 0.84, 0.07, 1.56, 0x6e4c37, 0.02);
  for (const [lx, lz] of [[-0.38, -0.74], [0.3, -0.74], [-0.38, 0.68], [0.3, 0.68]]) room.box(x + lx, 0, z + lz, 0.07, 0.64, 0.07, 0x3e2a1f, 0);
  room.box(x - 0.3, 0.12, z + 0.3, 0.66, 0.5, 0.42, 0x5e4030).box(x - 0.31, 0.38, z + 0.36, 0.01, 0.04, 0.3, 0xc8a24a, 0);
  room.box(x + 0.02, 0.71, z - 0.14, 0.24, 0.03, 0.28, 0x1c1c24).box(x + 0.1, 0.74, z - 0.04, 0.06, 0.2, 0.08, 0x1c1c24)
    .box(x + 0.02, 0.88, z - 0.4, 0.09, 0.46, 0.8, 0x1a1d28).box(x + 0.11, 0.92, z - 0.12, 0.02, 0.26, 0.24, 0x2a2f3e, 0)
    .box(x - 0.36, 0.71, z - 0.26, 0.17, 0.025, 0.52, 0x2c3040).box(x - 0.34, 0.735, z - 0.24, 0.13, 0.008, 0.48, 0x454a5e, 0)
    .box(x - 0.34, 0.71, z + 0.36, 0.1, 0.03, 0.07, 0x2c3040).box(x - 0.1, 0.71, z + 0.3, 0.26, 0.04, 0.34, 0xece6da, 0.02)
    .box(x + 0.12, 0.71, z + 0.55, 0.11, 0.13, 0.11, [0xeeeeee, 0x9b6bff, 0xff9a4a][DESKS.indexOf(d) % 3]);
  room.box(x + 0.14, 0.71, z - 0.65, 0.18, 0.03, 0.18, 0x2a2a30).box(x + 0.21, 0.74, z - 0.58, 0.04, 0.4, 0.04, 0x2a2a30);
  const shade = box(scene, 0.26, 0.14, 0.26, x + 0.23, 1.16, z - 0.56, new THREE.MeshBasicMaterial({ color: 0xffc27a, toneMapped: false }), false); shades.push(shade);
  const lamp = new THREE.PointLight(0xffa860, 0, 4.2, 1.6); lamp.position.set(x - 0.1, 1.1, z - 0.3); scene.add(lamp); lamps.push(lamp);
  // The chair.
  const s = d.seat;
  room.box(s - 0.24, 0.38, z - 0.24, 0.48, 0.07, 0.48, 0x3b2d4c).box(s - 0.29, 0.45, z - 0.22, 0.07, 0.54, 0.44, 0x33263f)
    .box(s - 0.03, 0.08, z - 0.03, 0.06, 0.3, 0.06, 0x1c1c22).box(s - 0.22, 0.04, z - 0.03, 0.44, 0.04, 0.06, 0x1c1c22).box(s - 0.03, 0.04, z - 0.22, 0.06, 0.04, 0.44, 0x1c1c22);
  const g = glow(0x7fb8ff, 1.3, 0); g.position.set(x - 0.22, 1.02, z); scene.add(g); deskGlows.push(g);
}

scene.add(walls.mesh(false), room.mesh(true));

// A little robot vacuum doing laps at the front.
const vac = new THREE.Group(); scene.add(vac);
{ const a = new THREE.Mesh(new THREE.CylinderGeometry(0.27, 0.28, 0.08, 24), new THREE.MeshStandardMaterial({ color: 0x2a2a33, roughness: 0.5 })); a.position.y = 0.05; a.castShadow = true; vac.add(a);
  const b = new THREE.Mesh(new THREE.CylinderGeometry(0.17, 0.17, 0.02, 20), new THREE.MeshStandardMaterial({ color: 0x4a4a58, roughness: 0.4 })); b.position.y = 0.1; vac.add(b);
  const l = glow(0x4ade80, 0.22, 0.9); l.position.set(0, 0.13, 0.2); vac.add(l); }

// Light through the window: a pane-shaped patch on the floor and a faint beam.
const beamTex = (() => { const c = document.createElement('canvas'); c.width = 4; c.height = 64; const x = c.getContext('2d'), g = x.createLinearGradient(0, 0, 0, 64);
  g.addColorStop(0, 'rgba(255,255,255,.9)'); g.addColorStop(1, 'rgba(255,255,255,0)'); x.fillStyle = g; x.fillRect(0, 0, 4, 64); return new THREE.CanvasTexture(c); })();
const patchTex = (() => { const c = document.createElement('canvas'); c.width = c.height = 64; const x = c.getContext('2d'); x.filter = 'blur(2px)'; x.fillStyle = '#fff';
  for (const [px, py] of [[4, 4], [34, 4], [4, 34], [34, 34]]) x.fillRect(px, py, 26, 26); return new THREE.CanvasTexture(c); })();
function quad(pts, tex) {
  const g = new THREE.BufferGeometry(); g.setAttribute('position', new THREE.Float32BufferAttribute(pts.flat(), 3));
  g.setAttribute('uv', new THREE.Float32BufferAttribute([0, 1, 1, 1, 1, 0, 0, 0], 2)); g.setIndex([0, 3, 1, 1, 3, 2]);
  return new THREE.Mesh(g, new THREE.MeshBasicMaterial({ map: tex, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide, toneMapped: false }));
}
const patch = quad([[0.3, 0.02, Z0 + 1.2], [2.9, 0.02, Z0 + 1.2], [3.9, 0.02, Z0 + 3.7], [1.3, 0.02, Z0 + 3.7]], patchTex);
const beam = quad([[0.2, 3.25, Z0 + 0.02], [2.8, 3.25, Z0 + 0.02], [3.9, 0.02, Z0 + 3.7], [1.3, 0.02, Z0 + 3.7]], beamTex);
scene.add(patch, beam);
const dust = (() => { const r = rng(4), n = 46, p = new Float32Array(n * 3); for (let i = 0; i < n; i++) { const f = r(); p[i * 3] = 0.4 + r() * 2.4 + f * 1.1; p[i * 3 + 1] = 3.1 * (1 - f) + r() * 0.3; p[i * 3 + 2] = Z0 + 0.3 + f * 3.2; }
  const g = new THREE.BufferGeometry(); g.setAttribute('position', new THREE.BufferAttribute(p, 3));
  const m = new THREE.Points(g, new THREE.PointsMaterial({ color: 0xffe2a8, size: 0.05, transparent: true, opacity: 0.8, depthWrite: false, blending: THREE.AdditiveBlending })); scene.add(m); return m; })();

// ── Lights and time of day ──────────────────────────────────────────────
const hemi = new THREE.HemisphereLight(); scene.add(hemi);
const sun = new THREE.DirectionalLight(); sun.position.set(-1.5, 10, -12); sun.target.position.set(1.5, 0, 1.5); scene.add(sun, sun.target);
sun.castShadow = true; sun.shadow.mapSize.set(1536, 1536); Object.assign(sun.shadow.camera, { left: -12, right: 12, top: 12, bottom: -12, near: 1, far: 40 });
sun.shadow.bias = -0.0004; sun.shadow.normalBias = 0.03;
const fill = new THREE.DirectionalLight(); fill.position.set(8, 6, 10); scene.add(fill);
const floorLamp = new THREE.PointLight(0xffa860, 0, 5, 1.5); floorLamp.position.set(-6.5, 1.6, 2.8); scene.add(floorLamp);
const TIMES = {
  night: { hemi: [0x8a78b8, 0x2a1812, 1.05], sun: [0x8fa2ff, 0.6], fill: [0xffc8a0, 0.5], lamp: 3.4, exposure: 1.3, patch: [0x6f86ff, 0.1], beam: [0x6f86ff, 0.05], dust: 0, shade: 0xffc27a },
  day: { hemi: [0xfff1de, 0x6a4a3a, 1.5], sun: [0xffdcaa, 3.2], fill: [0xfff0e0, 0.9], lamp: 0, exposure: 1.0, patch: [0xffc070, 0.42], beam: [0xffd79a, 0.13], dust: 0.8, shade: 0x8a7a66 },
};
let time = 'night';
function applyTime(name) {
  time = name; const T = TIMES[name];
  document.body.dataset.time = name;
  hemi.color.set(T.hemi[0]); hemi.groundColor.set(T.hemi[1]); hemi.intensity = T.hemi[2];
  sun.color.set(T.sun[0]); sun.intensity = T.sun[1]; fill.color.set(T.fill[0]); fill.intensity = T.fill[1];
  lamps.forEach(l => l.intensity = T.lamp); floorLamp.intensity = T.lamp * 0.9; shades.forEach(s => s.material.color.set(T.shade)); floorShade.material.color.set(T.shade);
  renderer.toneMappingExposure = T.exposure;
  patch.material.color.set(T.patch[0]); patch.material.opacity = T.patch[1]; beam.material.color.set(T.beam[0]); beam.material.opacity = T.beam[1]; dust.material.opacity = T.dust; dust.visible = T.dust > 0;
  drawSky(); drawTV(0);
}
function drawSky() {
  const { x, t } = sky, r = rng(3), night = time === 'night';
  const g = x.createLinearGradient(0, 0, 0, 96);
  if (night) { g.addColorStop(0, '#070a24'); g.addColorStop(1, '#2a2458'); } else { g.addColorStop(0, '#5eb0ff'); g.addColorStop(1, '#cfe8ff'); }
  x.fillStyle = g; x.fillRect(0, 0, 128, 96);
  if (night) { for (let i = 0; i < 40; i++) { x.fillStyle = r() < 0.3 ? '#fff' : '#9aa6ff'; x.fillRect(r() * 128 | 0, r() * 55 | 0, 1, 1); }
    x.fillStyle = '#fff2cc'; x.fillRect(92, 12, 12, 12); x.fillRect(90, 14, 16, 8); x.fillStyle = '#e6d6a8'; x.fillRect(96, 16, 3, 3); x.fillRect(100, 20, 2, 2); }
  else { x.fillStyle = '#fff6d8'; x.fillRect(96, 10, 12, 12); x.fillStyle = '#fff'; for (const [cx, cy, w] of [[14, 20, 26], [58, 12, 20], [70, 34, 30]]) { x.fillRect(cx, cy, w, 5); x.fillRect(cx + 4, cy - 3, w - 10, 3); } }
  for (let bx = 0; bx < 128;) { const w = 8 + (r() * 14 | 0), h = 18 + (r() * 36 | 0);
    x.fillStyle = night ? '#120e2a' : '#8fb2d6'; x.fillRect(bx, 96 - h, w, h);
    for (let wy = 96 - h + 3; wy < 94; wy += 4) for (let wx = bx + 2; wx < bx + w - 2; wx += 3) if (r() < (night ? 0.35 : 0.2)) { x.fillStyle = night ? (r() < 0.8 ? '#ffd27a' : '#b99bff') : '#dbe9f8'; x.fillRect(wx, wy, 1, 2); }
    bx += w + 1; }
  t.needsUpdate = true;
}


// ── The bots (Hover's own mascot) ───────────────────────────────────────
// A small boxy bot: a rounded head with a dark visor and two pixel eyes, headphone
// pads, and an antenna bulb whose colour is the session's stage.
const EYE = new THREE.MeshBasicMaterial({ color: 0xaaf6ff, toneMapped: false });
const VISOR = new THREE.MeshStandardMaterial({ color: 0x111018, roughness: 0.22, metalness: 0.35 });
const hitMat = new THREE.MeshBasicMaterial({ visible: false });
const ringGeo = new THREE.RingGeometry(0.42, 0.52, 40);
const BOTS = [['Pip', 0x9b6bff], ['Juno', 0x2fc9b0], ['Moss', 0xff9a4a], ['Nova', 0xff6fae], ['Ada', 0x5aa8ff], ['Rue', 0xb4e04a]];
const BULB = { waking: 0xffd24a, working: 0xc4a2ff, waiting: 0xffb340, done: 0x4ade80, failed: 0xff5b52, stopped: 0x55505f };
const SCREEN = { waking: [0x7fb8ff, 0.25], working: [0x7fb8ff, 0.6], waiting: [0xffb340, 0.6], done: [0x4ade80, 0.42], failed: [0xff5b52, 0.5], stopped: [0, 0] };
const hits = [];

class Bot {
  constructor(name, color) {
    this.name = name; this.color = color; this.css = '#' + new THREE.Color(color).getHexString();
    this.t = R() * 10; this.since = 0; this.sinceSeat = 99; this.stage = 'waking'; this.act = null;
    this.x = 0; this.z = 0; this.face = 0; this.yaw = 0; this.path = []; this.seated = false; this.sit = 0; this.walk = 0; this.phase = 0; this.hot = false;
    this.P = { lean: 0, hx: 0, hy: 0, hz: 0, aL: 0, aR: 0, sL: 0, sR: 0, lx: 0, ly: 0 };
    const main = new THREE.MeshStandardMaterial({ color, roughness: 0.5 });
    const dark = new THREE.MeshStandardMaterial({ color: new THREE.Color(color).multiplyScalar(0.5), roughness: 0.6 });
    const pale = new THREE.MeshStandardMaterial({ color: new THREE.Color(color).lerp(new THREE.Color(0xffffff), 0.4), roughness: 0.45 });
    this.bulbMat = new THREE.MeshBasicMaterial({ color: BULB.waking, toneMapped: false });
    this.mats = [main, dark, pale, this.bulbMat];
    this.root = new THREE.Group(); scene.add(this.root);
    const hips = pivot(this.root, 0, 0.24, 0);
    this.legs = [-1, 1].map(s => { const p = pivot(hips, s * 0.1, 0, 0); box(p, 0.13, 0.2, 0.15, 0, -0.1, 0, dark); box(p, 0.15, 0.06, 0.21, 0, -0.21, 0.03, pale); return p; });
    this.upper = pivot(hips, 0, 0, 0);
    box(this.upper, 0.42, 0.3, 0.3, 0, 0.15, 0, dark);
    box(this.upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, pale);
    this.head = pivot(this.upper, 0, 0.3, 0);
    box(this.head, 0.58, 0.44, 0.48, 0, 0.22, 0, main);
    box(this.head, 0.5, 0.05, 0.4, 0, 0.465, 0, pale);
    box(this.head, 0.5, 0.36, 0.4, 0, 0.22, -0.02, pale).scale.set(0.5, 0.36, 0.49);
    box(this.head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, VISOR, false);
    [-1, 1].forEach(s => { box(this.head, 0.07, 0.2, 0.22, s * 0.315, 0.22, 0, dark); box(this.head, 0.02, 0.1, 0.1, s * 0.355, 0.22, 0, pale); });
    box(this.head, 0.03, 0.14, 0.03, 0.14, 0.53, -0.08, dark);
    box(this.head, 0.1, 0.1, 0.1, 0.14, 0.64, -0.08, this.bulbMat, false);
    this.halo = glow(BULB.waking, 0.55, 0.8); this.halo.position.set(0.14, 0.64, -0.08); this.head.add(this.halo); this.mats.push(this.halo.material);
    this.eyes = [-1, 1].map(s => {
      const g = pivot(this.head, s * 0.1, 0.21, 0.258);
      const open = box(g, 0.075, 0.11, 0.01, 0, 0, 0, EYE, false);
      const happy = new THREE.Group(); g.add(happy);
      box(happy, 0.055, 0.024, 0.01, -0.018, 0, 0, EYE, false).rotation.z = 0.75;
      box(happy, 0.055, 0.024, 0.01, 0.018, 0, 0, EYE, false).rotation.z = -0.75;
      const shut = box(g, 0.085, 0.022, 0.01, 0, -0.02, 0, EYE, false);
      return { g, open, happy, shut, s };
    });
    this.arms = [-1, 1].map(s => { const p = pivot(this.upper, s * 0.27, 0.27, 0); box(p, 0.1, 0.22, 0.12, 0, -0.1, 0, main); box(p, 0.11, 0.07, 0.13, 0, -0.23, 0, pale); return p; });
    this.hit = box(this.root, 0.8, 1.3, 0.8, 0, 0.65, 0, hitMat, false); this.hit.userData.bot = this; hits.push(this.hit);
    this.ring = new THREE.Mesh(ringGeo, new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0, depthWrite: false, toneMapped: false })); this.ring.rotation.x = -Math.PI / 2; this.root.add(this.ring); this.mats.push(this.ring.material);
  }
  place(x, z, seated) { this.x = x; this.z = z; this.seated = seated; this.sit = seated ? 1 : 0; this.yaw = this.face = seated ? Math.PI / 2 : Math.PI; this.sinceSeat = 99; }
  go(path, done) { this.path = path.slice(); this.seated = false; this.arrive = done; }
  sync(stage, act) { if (stage !== this.stage) this.since = 0; this.stage = stage; this.act = act; }
  head3(v) { return v.set(this.x, this.root.position.y + (this.seated ? 1.28 : 1.22), this.z); }
  step(dt) {
    this.t += dt; this.since += dt; this.sinceSeat += dt;
    const t = this.t, P = this.P;
    let walking = false;
    if (this.path.length && this.sit < 0.05) {
      const [px, pz] = this.path[0], dx = px - this.x, dz = pz - this.z, d = Math.hypot(dx, dz), sp = 1.7 * dt;
      if (d <= sp) { this.x = px; this.z = pz; this.path.shift(); if (!this.path.length) { const f = this.arrive; this.arrive = null; f && f(); } }
      else { this.x += dx / d * sp; this.z += dz / d * sp; this.face = Math.atan2(dx, dz); walking = true; }
    }
    this.walk = ease(this.walk, walking ? 1 : 0, 10, dt);
    if (walking) this.phase += dt * 11;
    const seatNow = this.seated && !this.path.length;
    if (seatNow) this.face = Math.PI / 2;
    this.sit = ease(this.sit, seatNow ? 1 : 0, 7, dt);
    this.yaw = angTo(this.yaw, this.face, 9, dt);

    let lean = 0, hx = 0, hy = 0, hz = 0, aL = 0, aR = 0, sL = 0, sR = 0, lx = 0, ly = 0, eyes = 'open', blinkBulb = false, halo = 0.8;
    const sw = Math.sin(this.phase), bulb = BULB[this.stage];
    if (!seatNow) { aL = sw * 0.6 * this.walk; aR = -sw * 0.6 * this.walk; hx = 0.05; if (this.stage === 'done') eyes = 'happy'; }
    else switch (this.stage) {
      case 'waking': {
        const w = this.sinceSeat;
        if (w < 1.6) { aL = aR = -2.9; sL = -0.35; sR = 0.35; lean = -0.14; hx = -0.25; eyes = w < 0.6 ? 'shut' : 'happy'; }
        else { aL = aR = -1.35; hx = 0.08; lx = Math.sin(t * 1.3) * 0.02; }
        blinkBulb = true; break;
      }
      case 'working':
        halo = 0.5 + 0.35 * Math.sin(t * 3); aL = aR = -1.4; lean = 0.08; hx = 0.12;
        if (this.act === 'Thinking') { aR = -2.25; sR = 0.55; hx = -0.2; hz = Math.sin(t * 0.9) * 0.14; ly = 0.02; lx = 0.02; }
        else if (this.act === 'Reading') { hy = Math.sin(t * 1.5) * 0.14; lx = Math.sin(t * 1.5) * 0.022; ly = -0.012; }
        else if (this.act === 'Editing') { aL = -1.4 + Math.sin(t * 22) * 0.14; aR = -1.4 + Math.sin(t * 22 + 2) * 0.14; hx = 0.16; }
        else if (this.act === 'Running') { aL = aR = -1.15; lean = 0.2; hx = 0.05; halo = Math.sin(t * 11) > 0 ? 0.9 : 0.35; }
        break;
      case 'done':
        eyes = 'happy';
        if (this.since < 1.8) { aL = -3 + Math.sin(t * 13) * 0.3; aR = -3 - Math.sin(t * 13) * 0.3; sL = -0.3; sR = 0.3; lean = -0.1; hx = -0.2; }
        else { aL = aR = -2.75; sL = 0.6; sR = -0.6; lean = -0.2; hx = -0.12; hz = Math.sin(t * 0.7) * 0.06; }
        break;
      // Asking the user: a hand up and waving, the bulb blinking amber.
      case 'waiting': blinkBulb = true; aL = -1.4; aR = -2.95 + Math.sin(t * 6) * 0.22; sR = 0.35 + Math.sin(t * 6) * 0.12; lean = -0.06; hx = -0.12; break;
      case 'failed': eyes = 'sad'; blinkBulb = true; aL = aR = -1.5; lean = 0.25; hx = 0.35; break;
      case 'stopped': eyes = 'shut'; halo = 0; aL = aR = -1.55; lean = 0.45 + Math.sin(t * 1.6) * 0.02; hx = 0.42; hz = 0.1; break;
    }
    if (eyes === 'open' && (t % 3.7) < 0.12) eyes = 'shut';
    const k = still ? 30 : 14;
    for (const [n, v] of Object.entries({ lean, hx, hy, hz, aL, aR, sL, sR, lx, ly })) P[n] = ease(P[n], v, k, dt);

    const bob = seatNow ? Math.sin(t * 2) * 0.006 : Math.abs(sw) * 0.035 * this.walk;
    this.root.position.set(this.x, this.sit * 0.21 + bob, this.z);
    this.root.rotation.y = this.yaw;
    const legW = sw * 0.6 * this.walk;
    this.legs[0].rotation.x = -Math.PI / 2 * this.sit + legW; this.legs[1].rotation.x = -Math.PI / 2 * this.sit - legW;
    this.upper.rotation.x = P.lean; this.head.rotation.set(P.hx, P.hy, P.hz);
    this.arms[0].rotation.set(P.aL, 0, P.sL); this.arms[1].rotation.set(P.aR, 0, P.sR);
    for (const e of this.eyes) {
      e.g.position.x = e.s * 0.1 + P.lx; e.g.position.y = 0.21 + P.ly;
      e.open.visible = eyes === 'open' || eyes === 'sad'; e.open.scale.y = eyes === 'sad' ? 0.065 : 0.11; e.open.rotation.z = eyes === 'sad' ? -e.s * 0.45 : 0;
      e.happy.visible = eyes === 'happy'; e.shut.visible = eyes === 'shut';
    }
    const on = !blinkBulb || Math.sin(t * 9) > -0.2;
    this.bulbMat.color.set(bulb).multiplyScalar(on ? 1 : 0.35);
    this.halo.material.color.set(bulb); this.halo.material.opacity = on ? halo : 0.05;
    this.ring.position.y = 0.02 - this.root.position.y;
    this.ring.material.opacity = ease(this.ring.material.opacity, this.hot ? 0.95 : 0, 12, dt); this.ring.visible = this.ring.material.opacity > 0.02;
  }
  dispose() { scene.remove(this.root); this.mats.forEach(m => m.dispose()); hits.splice(hits.indexOf(this.hit), 1); }
}



// ── Sessions ────────────────────────────────────────────────────────────
// In Hover the page is shown in WebView2: the sessions come from Hover as state
// messages, and what the user asks for goes back as messages. Opened on its own in
// a browser it plays with demo sessions instead, so the design can be looked at.
const host = window.chrome?.webview || null;
if (host) document.body.classList.add('host');
const min = 60e3, T0 = Date.now();
const DONE_TEXT = 'Done. Settings now keeps the Kiro model and effort you pick, and the Kiro page reads them when a task starts.\n\nI changed Settings.cs only. The build is clean and all 81 tests pass.';
let defaultFolder = host ? null : 'B:\\hover', canStart = true, maxRunning = 3;
// The agent tools, in the picker's order. Hover says which are installed and signed in.
const TOOLS = { kiro: ['Kiro', '#b48cff'], codex: ['Codex', '#3fd6a0'], cursor: ['Cursor', '#7cc0ff'], opencode: ['OpenCode', '#e8e8ec'] };
// Each tool's own logo, only to show which tool is picked (from the MIT-licensed
// LobeHub icon set; the marks belong to their owners).
const LOGOS = {
  kiro: '<svg viewBox="0 0 24 24"><path fill-rule="evenodd" d="M4.594 6.677C6.67-2.226 18.746-2.211 21.16 6.632c.353 1.297 1.725 7.582-1.673 13.747-1.545 2.797-5.841 5.49-6.99 1.883C8.6 25.477 3.315 24.1 5.789 18.609l-.318.143c-3.57 1.305-3.863-1.208-3.173-2.513.45-.84.727-1.335.937-1.897.353-.975.458-1.568.593-2.498.27-1.837.277-3.607.765-5.167zm8.37.01a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.214-.705 1.214-1.89 0-.622-.127-1.125-.367-1.455a1.014 1.014 0 00-.855-.435zm4.08 0a.92.92 0 00-.81.428c-.217.323-.33.825-.33 1.462 0 .705.15 1.89 1.14 1.89h.008c.757 0 1.215-.705 1.215-1.89 0-.622-.128-1.125-.368-1.455a1.014 1.014 0 00-.855-.435z"/></svg>',
  codex: '<svg viewBox="3 2.9 18 18.2"><defs><linearGradient id="cg$" x1="12" x2="12" y1="3" y2="21" gradientUnits="userSpaceOnUse"><stop stop-color="#B1A7FF"/><stop offset=".5" stop-color="#7A9DFF"/><stop offset="1" stop-color="#3941FF"/></linearGradient></defs><path fill="url(#cg$)" d="M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z"/></svg>',
  cursor: '<svg viewBox="0 0 24 24"><path fill-rule="evenodd" d="M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z"/></svg>',
  // OpenCode's hollow square, drawn for Hover in its style (the same as the notch's).
  opencode: '<svg viewBox="0 0 24 24"><path fill-rule="evenodd" d="M4 2h16v20H4zM8 6v12h8V6zM8 12h8v6H8z"/></svg>',
};
// The gradient id has to be unique on the page, and the logo shows twice.
let logoN = 0;
const logo = id => (LOGOS[id] || LOGOS.kiro).replaceAll('cg$', 'cg' + logoN++);
// The demo's own list, until Hover sends the real ones.
let tools = Object.keys(TOOLS).map(id => ({ id, name: TOOLS[id][0], ready: true, hint: '', access: 'full', readOnly: id !== 'codex', models: [{ id: 'auto', name: 'Auto' }, { id: 'm1', name: 'GPT-6 Astra' }], model: 'm1', efforts: ['low', 'medium', 'high'], effort: 'high' })), newTool = 'kiro', toolPicked = false;
// The access the new-task box picked, by tool, for the tasks it starts next.
const newAccess = {};
const newAccessOf = x => { const a = newAccess[x.id] || x.access; return ACCESS[a] && (a !== 'read' || x.readOnly) ? a : 'full'; };
const toolOf = s => tools.find(x => x.id === (s?.tool || 'kiro')) || tools[0];
// The tool shows as its own logo, never its name (that is in the tooltip).
const badge = id => `<span class="lg mini ${TOOLS[id] ? id : 'kiro'}" title="${(TOOLS[id] || TOOLS.kiro)[0]}" role="img" aria-label="${(TOOLS[id] || TOOLS.kiro)[0]}">${logo(id)}</span>`;
let sessions = host ? [] : demo();
if (!host) sessions.forEach((s, i) => s.tool = ['kiro', 'codex', 'kiro', 'cursor', 'codex'][i]);
let nextId = 6, sel = null, drawerOpen = false, panel = null, newFolder = defaultFolder, firstState = true;
const timers = {}, leaving = [];
const last = s => { for (let i = s.turns.length - 1; i >= 0; i--) if (!s.turns[i].queued) return s.turns[i]; return s.turns[0]; };
const busy = s => ['waking', 'working', 'waiting'].includes(last(s).stage);
// viewing: a session from the history, shown in the chat without a desk. A reply to
// it brings it back (pendingKey is the one waited for).
let viewing = null, pendingKey = null, history = [];
// The new-task circle: 'rest' (the circle), 'pick' (the logos out) or 'open' (the box).
let fab = 'rest';
const cur = () => viewing || sessions.find(s => s.id === sel);
const short = f => (f || '').split(/[\\/]/).pop();
const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const ago = ms => { const m = Math.round((Date.now() - ms) / min); return m < 1 ? 'now' : m < 60 ? `${m} min ago` : m < 24 * 60 ? `${Math.round(m / 60)} h ago` : `${Math.round(m / 1440)} d ago`; };
const WORD = { waking: 'Starting', working: 'Working', waiting: 'Waiting for you', done: 'Done', failed: 'Couldn’t finish', stopped: 'Stopped' };

function demo() {
  const F = 'B:\\hover';
  return [
    { id: 1, bot: 0, desk: 0, title: 'Add refresh token expiry', folder: F, ctx: 31, turns: [{ prompt: 'Refresh tokens never expire. Make them expire after 30 days and return 401 when one is used after that.', stage: 'working', act: 'Reading', file: 'src/auth/refresh.ts', target: 'src/auth/refresh.ts', t0: T0 - 1.4 * min, woke: 2.3,
      steps: [['read', 'Read src/auth/session.ts'], ['read', 'Read src/auth/refresh.ts']], final: 'Done. Refresh tokens now expire after 30 days, and using an expired one returns 401.\n\nI changed refresh.ts and added two tests. All 83 tests pass.' }] },
    { id: 2, bot: 1, desk: 1, title: 'Fix the notch flicker on resize', folder: F, ctx: 48, turns: [{ prompt: 'The notch blinks when I change the workspace size in Settings. Find out why and fix it.', stage: 'done', t0: T0 - 26 * min, woke: 2.1, took: 3 * min + 12e3,
      steps: [{ k: 'read', verb: 'Read', name: 'Notch.cs', dir: 'src/Hover/Owl', status: 'completed' }, { k: 'read', verb: 'Read', name: 'HostWindow.cs', dir: 'src/Hover/Interop', status: 'completed' },
        { k: 'search', verb: 'Searched', cmd: 'SetWindowPos(', status: 'completed' },
        { k: 'edit', verb: 'Edited', name: 'Notch.cs', dir: 'src/Hover/Owl', status: 'completed', add: 2, del: 1, ms: 2100, diff: '  private void Layout()\n- _window.Width = w; _window.Left = x;\n+ _window.PlaceDevice(x, top, w, h);\n+ // size and place in one call' },
        { k: 'run', verb: 'Ran', cmd: 'dotnet test .\\Hover.slnx -c Release', status: 'completed', exit: 0, ms: 14e3, out: 'Passed!  - Failed: 0, Passed: 81, Skipped: 0\nDuration: 13.8 s - Hover.Tests.dll (net10.0)' }],
      answer: 'The notch window was resized and moved in **two separate calls**, so for one frame it had the new size at the old place. That is the blink.\n\n- Size and position now go through one `SetWindowPos`.\n- Build clean, **81/81** tests pass.\n\n```csharp\n_window.PlaceDevice(x, s.Work.Top, w, h);\n```' }] },
    { id: 3, bot: 2, desk: 3, title: 'Tests for the calendar reader', folder: F, ctx: 12, arrive: true, turns: [{ prompt: 'Write tests for Calendar.cs covering all-day events and repeating events.', stage: 'waking', t0: T0, steps: [], target: 'tests/Hover.Tests/CalendarTests.cs' }] },
    { id: 4, bot: 3, desk: 4, title: 'Upgrade three.js to 0.171', folder: 'B:\\site', ctx: 22, turns: [{ prompt: 'Upgrade three to 0.171 and fix anything that breaks.', stage: 'failed', t0: T0 - 2.3 * 60 * min, woke: 2.4, took: 48e3, steps: [['read', 'Read package.json'], ['run', 'Ran npm install three@0.171.0', 'failed']],
      answer: 'I couldn\'t finish. npm install stopped with a peer dependency conflict: @react-three/fiber 8 needs three 0.170 or older.\n\nReply "upgrade fiber too" and I will move both together.' }] },
    { id: 5, bot: 4, desk: 2, title: 'Upgrade the office to three.js 0.171', folder: 'B:\\site', ctx: 9,
      ask: { id: 'a1', kind: 'execute', title: 'Wants to run a command', line: 'Wants to run npm install', command: 'npm install three@0.171.0', reason: 'Installs packages or uses the network', danger: false, allow: 'Run', more: 0 },
      turns: [{ prompt: 'Upgrade three to 0.171 and fix anything that breaks.', stage: 'waiting', act: 'Running', file: 'npm install three@0.171.0', t0: T0 - 0.6 * min, woke: 2, steps: [['read', 'Read package.json'], ['run', 'Running npm install three@0.171.0']] }] },
  ];
}

function pathIn(d) { return [[DOOR.x, 0.25], [d.seat + 0.05, 0.25], [d.seat + 0.05, d.z]]; }
function pathOut(d) { return [[d.seat + 0.05, 0.25], [DOOR.x, 0.25], [DOOR.x, Z0 + 0.1]]; }
function spawn(s, walkIn) {
  const [name, color] = BOTS[s.bot % BOTS.length], d = DESKS[s.desk];
  s.b = new Bot(name, color); s.b.sync(last(s).stage, poseOf(last(s)));
  if (walkIn) { s.b.place(DOOR.x, Z0 + 0.1, false); s.b.go(pathIn(d), () => { s.b.seated = true; s.b.sinceSeat = 0; }); }
  else s.b.place(d.seat + 0.05, d.z, true);
  const el = document.createElement('div'); el.className = 'tag';
  el.innerHTML = `<div class="in"><div class="ask-slot"></div><div class="bub"><span></span></div><button class="nm" style="--c:${s.b.css}">${badge(s.tool)}${name}</button></div>`;
  el.querySelector('button').onclick = () => openSession(s.id);
  $('#tags').appendChild(el); s.tag = el; s.tagText = null; s.tagShown = 0;
}
const poseOf = T => T.pose || T.act;
// The bot walks out and is gone; its desk is free once it has left.
function retire(s) {
  clearTimeout(timers[s.id]); sessions = sessions.filter(x => x !== s); s.tag.remove();
  const b = s.b, d = DESKS[s.desk]; b.sync('done', null); b.hot = false; leaving.push({ b, desk: s.desk });
  b.go(pathOut(d), () => { b.dispose(); leaving.splice(leaving.findIndex(l => l.b === b), 1); });
  if (sel === s.id) closeDrawer();
  changed();
}
function freeDesk() { const used = new Set([...sessions.map(s => s.desk), ...leaving.map(l => l.desk)]); return DESKS.findIndex((_, i) => !used.has(i)); }
function freeBot() { const used = new Set(sessions.map(s => s.bot)); const i = BOTS.findIndex((_, i) => !used.has(i)); return i < 0 ? 0 : i; }

// Hover's state: every session, its turns, and what the running one is doing.
function fromHost(m) {
  // In the notch the office fills the shape, edge to edge.
  document.body.classList.toggle('notch', !m.window);
  canStart = m.canStart; maxRunning = m.maxRunning; if (m.tools) tools = m.tools; if (!toolPicked && m.tool) newTool = m.tool;
  if (m.history) history = m.history;
  defaultFolder = m.folder || null; if (!newFolder) newFolder = defaultFolder;
  const seen = new Set();
  for (const h of m.sessions) {
    seen.add(h.id);
    const turns = h.turns.map(t => ({ ...t, t0: t.t0 || Date.now() }));
    let i = turns.length - 1; while (i > 0 && turns[i].queued) i--;
    Object.assign(turns[i], { act: h.act, pose: h.pose, file: h.file });
    let s = sessions.find(x => x.id === h.id);
    if (!s) {
      s = { id: h.id, key: h.key, files: h.files, tool: h.tool, bot: h.bot, desk: h.seat, title: h.title, folder: h.folder, ctx: h.ctx, ask: h.ask, access: h.access, turns };
      sessions.push(s); spawn(s, !firstState && last(s).stage === 'waking');
    } else {
      turns.forEach((t, k) => { const old = s.turns[k]; if (t.answer && !(old && old.answer)) t.fresh = !firstState; });
      if (last(s).stage === 'waking' && s.turns.length !== turns.length && s.b.seated) s.b.sinceSeat = 0;
      Object.assign(s, { files: h.files, title: h.title, folder: h.folder, ctx: h.ctx, ask: h.ask, access: h.access, turns });
    }
  }
  for (const s of [...sessions]) if (!seen.has(s.id)) retire(s);
  // A history session came back to a desk: the chat follows it there.
  const woke = (pendingKey || viewing?.key) && sessions.find(s => s.key === (pendingKey || viewing.key));
  if (woke) { pendingKey = null; openSession(woke.id); }
  // The page was made again (it is dropped while hidden): reopen the session that was open.
  if (firstState && m.open != null && sessions.some(s => s.id === m.open)) openSession(m.open);
  firstState = false;
  changed(cur());
  renderTools(); if (fab === 'open') renderNew();
}

// ── The demo's scripted runs, so a browser shows every stage in order ───
function script(T) {
  const f = T.target || 'src/Hover/Core/Settings.cs';
  return [{ stage: 'waking', ms: 800 }, { stage: 'working', act: 'Thinking', ms: 2600 },
    { stage: 'working', act: 'Reading', ms: 3000, file: f, step: ['read', 'Read ' + f] },
    { stage: 'working', act: 'Editing', ms: 4200, file: f, step: ['edit', 'Edited ' + f, '+14 −2'] },
    { stage: 'working', act: 'Running', ms: 3400, file: 'dotnet test', step: ['run', 'Ran dotnet test', '81 passed'] }, { stage: 'done' }];
}
function play(s, T, from = 0) {
  clearTimeout(timers[s.id]);
  const steps = script(T); let k = from;
  if (!from) { T.stage = 'waking'; T.act = null; T.t0 = Date.now(); T.steps = []; T.answer = ''; if (s.b?.seated) s.b.sinceSeat = 0; }
  T.queued = false;
  const next = () => {
    if (!sessions.includes(s)) return;
    // Waking lasts until the bot has walked in, sat down and stretched.
    if (T.stage === 'waking' && k > 0 && (!s.b.seated || s.b.path.length || s.b.sinceSeat < 1.9)) { timers[s.id] = setTimeout(next, 200); return; }
    const S = steps[k++];
    if (S.stage === 'working' && T.stage === 'waking') T.woke = (Date.now() - T.t0) / 1000;
    T.stage = S.stage; T.act = S.act || null; if (S.file) T.file = S.file; if (S.step) T.steps.push(S.step);
    if (S.stage === 'done') {
      T.took = Date.now() - T.t0; s.ctx = Math.min(90, s.ctx + 6); T.answer = T.final || DONE_TEXT; T.fresh = true;
      changed(s);
      const q = s.turns.find(x => x.queued); if (q) timers[s.id] = setTimeout(() => play(s, q), 1800);
      return;
    }
    changed(s);
    timers[s.id] = setTimeout(next, S.ms);
  };
  next();
}

// ── What the user asks for ──────────────────────────────────────────────
function doStop(s) {
  if (host) return host.postMessage({ type: 'stop', id: s.id });
  clearTimeout(timers[s.id]); const T = last(s); s.ask = null;
  if (busy(s)) { T.stage = 'stopped'; T.took = Date.now() - T.t0; T.answer = 'Stopped. Nothing after the last step above was changed.'; }
  s.turns.forEach(x => { if (x.queued) { x.queued = false; x.stage = 'stopped'; x.took = 0; x.steps = []; x.answer = 'Not sent: the run before it was stopped.'; } });
  changed(s);
}
function doReply(text, images = []) {
  const s = cur(); if (!s || (!text && !images.length)) return;
  if (s.archived) { if (!host) return toast('In Hover this wakes the session.'); pendingKey = s.key; return host.postMessage({ type: 'reply', key: s.key, text, images }); }
  // A reply while a question waits is its answer, in the user's own words, where the
  // question takes one; replying to anything else it asked says no to it, and the
  // words go to the agent instead.
  if (s.ask?.questions) {
    if (s.ask.questions.length === 1 && s.ask.questions[0].custom && !images.length) {
      const p = picksOf(s.ask); p.sel[0] = []; p.text[0] = text; return sendAnswers(s, s.ask);
    }
    return toast('Answer the question above first, or skip it.');
  }
  if (s.ask) answer(s, s.ask.id, 'deny');
  if (host) return host.postMessage({ type: 'reply', id: s.id, text, images });
  const wait = busy(s), T = { prompt: text, images, stage: 'waking', t0: Date.now(), steps: [], queued: wait };
  s.turns.push(T);
  if (wait) changed(s); else play(s, T);
}
function doNew(text, folder, images = []) {
  if (host) { host.postMessage({ type: 'new', prompt: text, folder, images, tool: newTool, access: newAccessOf(tools.find(x => x.id === newTool) || tools[0]) }); return true; }
  let desk = freeDesk();
  if (desk < 0) {
    const old = sessions.filter(s => !busy(s)).sort((a, b) => last(a).t0 - last(b).t0)[0];
    if (!old) { toast('All six desks are busy. Stop or remove a session first.'); return false; }
    desk = old.desk; retire(old);
  }
  const title = text || 'Look at the attached image';
  const s = { id: nextId++, tool: newTool, access: newAccessOf(tools.find(x => x.id === newTool) || tools[0]), bot: freeBot(), desk, folder, title: title.length > 60 ? title.slice(0, 59).trimEnd() + '…' : title, ctx: 3, turns: [{ prompt: text, images, stage: 'waking', t0: Date.now(), steps: [] }] };
  sessions.push(s); spawn(s, true); play(s, s.turns[0]);
  return true;
}

// ── Wall canvases: the TV, the session board, the clock ─────────────────
const count = st => sessions.filter(s => st.includes(last(s).stage)).length;
function drawTV(t) {
  const { x, t: tex } = tv;
  x.fillStyle = '#061022'; x.fillRect(0, 0, 208, 118);
  x.fillStyle = '#0b1b36'; for (let y = 0; y < 118; y += 3) x.fillRect(0, y, 208, 1);
  x.font = 'bold 11px "Pixelify Sans", monospace'; x.textBaseline = 'top';
  x.fillStyle = '#9ad2ff'; x.fillText('AGENT OFFICE', 10, 8);
  x.fillStyle = '#2f5a8a'; x.fillRect(10, 22, 188, 1);
  const rows = [['Working', count(['waking', 'working', 'waiting']), '#c4a2ff'], ['Done', count(['done']), '#4ade80'], ['Failed', count(['failed']), '#ff6b62'], ['Stopped', count(['stopped']), '#8a8fa0']];
  rows.forEach(([l, v, c], i) => { x.fillStyle = '#6fa8d8'; x.fillText(l, 10, 30 + i * 14); x.fillStyle = c; x.fillText(String(v), 70, 30 + i * 14); for (let k = 0; k < v; k++) x.fillRect(86 + k * 8, 33 + i * 14, 6, 6); });
  const s = cur() || sessions.find(busy);
  if (s) {
    const T = last(s); x.fillStyle = s.b.css; x.fillText(s.b.name, 10, 90);
    x.fillStyle = '#cfe6ff'; const what = T.stage === 'working' ? `${T.act || ''} ${short(T.file)}`.trim() : WORD[T.stage] || '';
    x.fillText(what.length > 24 ? what.slice(0, 23) + '…' : what, 46, 90);
    if (s.ctx != null) { x.fillStyle = '#1c3458'; x.fillRect(10, 105, 150, 5); x.fillStyle = '#9ad2ff'; x.fillRect(10, 105, 1.5 * s.ctx, 5); x.fillStyle = '#6fa8d8'; x.fillText(`${s.ctx}%`, 166, 101); }
  } else { x.fillStyle = '#6fa8d8'; x.fillText('No sessions yet', 10, 90); }
  if ((t * 2 | 0) % 2) { x.fillStyle = '#9ad2ff'; x.fillRect(190, 8, 6, 10); }
  tex.needsUpdate = true;
}
const COLS = [['WAKING', '#f5b83d', ['waking']], ['DOING', '#9b6bff', ['working', 'waiting']], ['FINISHED', '#2fae66', ['done', 'failed', 'stopped']]];
// The canvas is twice the board's 240 x 140 grid, so the notes can carry a title.
function fit(x, text, w) { if (x.measureText(text).width <= w) return text; while (text && x.measureText(text + '…').width > w) text = text.slice(0, -1); return text.trimEnd() + '…'; }
function drawBoard() {
  const { x, t } = board;
  x.setTransform(2, 0, 0, 2, 0, 0);
  x.fillStyle = '#e9e3d6'; x.fillRect(0, 0, 240, 140); x.fillStyle = '#d6cebd'; x.fillRect(0, 132, 240, 8);
  x.textBaseline = 'top';
  COLS.forEach(([h, c, st], i) => {
    const cx = 8 + i * 78; x.font = 'bold 11px "Pixelify Sans", monospace'; x.fillStyle = c; x.fillRect(cx, 7, 70, 14); x.fillStyle = '#fff'; x.fillText(h, cx + 5, 8);
    if (i && sessions.length) { x.fillStyle = '#cfc6b3'; x.fillRect(cx - 5, 8, 1, 118); }
    const list = sessions.filter(s => st.includes(last(s).stage)), room = list.length > 3 ? 2 : 3;
    list.slice(0, room).forEach((s, k) => {
      const ny = 27 + k * 34, T = last(s);
      x.fillStyle = 'rgba(0,0,0,.14)'; x.fillRect(cx + 1.5, ny + 1.5, 68, 31);
      x.fillStyle = '#fbf8f1'; x.fillRect(cx, ny, 68, 31); x.fillStyle = s.b.css; x.fillRect(cx, ny, 3, 31);
      x.font = 'bold 8px "Pixelify Sans", monospace'; x.fillStyle = '#2a2233'; x.fillText(s.b.name, cx + 6, ny + 3);
      const mark = T.stage === 'failed' ? '#ff453a' : T.stage === 'stopped' ? '#8e8a96' : T.stage === 'done' ? '#2fae66' : null;
      if (mark) { x.fillStyle = mark; x.fillRect(cx + 60, ny + 4, 5, 5); }
      // The title on up to two lines.
      x.font = '7px Inter, "Segoe UI", sans-serif'; x.fillStyle = '#5a5263';
      const words = (s.title || '').split(/\s+/); let line = '', row = 0;
      for (let j = 0; j < words.length && row < 2; j++) {
        const next = line ? line + ' ' + words[j] : words[j];
        if (x.measureText(next).width <= 58 || !line) { line = next; continue; }
        if (row === 1) { line = next; break; }
        x.fillText(fit(x, line, 58), cx + 6, ny + 13); row = 1; line = words[j];
      }
      if (line) x.fillText(fit(x, line, 58), cx + 6, ny + 13 + row * 8.5);
    });
    if (list.length > room) { x.font = '7px Inter, "Segoe UI", sans-serif'; x.fillStyle = '#7a7282'; x.fillText(`+${list.length - room} more`, cx + 3, 29 + room * 34); }
  });
  // An empty office says how to fill it.
  if (!sessions.length) {
    x.textAlign = 'center'; x.fillStyle = '#3a3044'; x.font = 'bold 16px "Pixelify Sans", monospace'; x.fillText('The office is quiet', 120, 46);
    x.fillStyle = '#5e5666'; x.font = '10px Inter, "Segoe UI", sans-serif';
    x.fillText('Give Kiro, Codex, Cursor or OpenCode', 120, 72); x.fillText('a task, and a bot walks in to do it.', 120, 86);
    x.textAlign = 'start';
  }
  x.setTransform(1, 0, 0, 1, 0, 0);
  t.needsUpdate = true;
}
function drawClock(t) {
  const { x, t: tex } = clock, d = new Date();
  x.fillStyle = '#0f0d12'; x.fillRect(0, 0, 96, 44);
  x.font = 'bold 30px "Pixelify Sans", monospace'; x.textBaseline = 'middle'; x.textAlign = 'center';
  x.fillStyle = '#3a1a0c'; x.fillText('88 88', 48, 23);
  // The colon blinks with the real seconds, so the clock is the PC's clock.
  x.fillStyle = '#ff8a3a'; x.fillText(`${String(d.getHours()).padStart(2, '0')}${d.getSeconds() % 2 ? ' ' : ':'}${String(d.getMinutes()).padStart(2, '0')}`, 48, 23);
  x.textAlign = 'start'; tex.needsUpdate = true;
}

// ── Things in the room that can be clicked ──────────────────────────────
const fullDate = () => new Date().toLocaleString(undefined, { weekday: 'long', day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit', second: '2-digit' });
const PROPS = [
  { hint: () => 'Office overview', at: [5.4, 2.38, Z0 + 0.2, 2.6, 1.55, 0.35], pane: tvPane, go: () => openPanel('tv') },
  { hint: () => 'Session board', at: [X0 + 0.2, 2.33, -0.7, 0.35, 1.75, 3], pane: boardPane, go: () => openPanel('board') },
  { hint: fullDate, at: [X0 + 0.2, 2.72, 1.8, 0.35, 0.62, 1.2], pane: clockPane, go: () => toast(fullDate()) },
  { hint: () => time === 'day' ? 'Make it night' : 'Make it day', at: [1.5, 2.3, Z0 + 0.2, 2.9, 2.1, 0.35], pane: skyPane, go: () => setTime(time === 'day' ? 'night' : 'day') },
  { hint: () => 'New task', at: [DOOR.x, 1.2, Z0 + 0.3, 1.3, 2.45, 0.5], go: () => openNew() },
  { hint: () => 'Session history', at: [X0 + 0.25, 1.21, -3.53, 0.5, 2.42, 1.6], go: () => openPanel('history') },
];
for (const p of PROPS) { const [x, y, z, w, h, d] = p.at; p.hit = box(scene, w, h, d, x, y, z, hitMat, false); p.hit.userData.prop = p; hits.push(p.hit); }

// ── Page UI: tags over the bots, the dock, the drawer and the panels ────
const LOGO = c => `<span class="av" style="--c:${c}"><i></i></span>`;
const ICON = {
  read: '<svg viewBox="0 0 24 24"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/></svg>',
  edit: '<svg viewBox="0 0 24 24"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>',
  run: '<svg viewBox="0 0 24 24"><path d="m4 17 6-5-6-5"/><path d="M12 19h8"/></svg>',
  search: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>',
  think: '<svg viewBox="0 0 24 24"><path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V16h8v-1.3A7 7 0 0 0 12 2Z"/></svg>',
};
// The live verb for a step that is still going: Edited → Editing.
const VERB_ON = { Read: 'Reading', Edited: 'Editing', Ran: 'Running', Searched: 'Searching', Fetched: 'Fetching', Deleted: 'Deleting', Moved: 'Moving' };
const CARET = '<svg class="car" viewBox="0 0 24 24"><path d="m9 6 6 6-6 6"/></svg>';
const CHECK = '<svg class="ckm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>';
const FOLDER = '<svg viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/></svg>';
const SHIELD = '<svg viewBox="0 0 24 24"><path d="M12 3 5 6v5c0 4.4 3 8.3 7 9.5 4-1.2 7-5.1 7-9.5V6Z"/></svg>';
// What a session may do on its own, picked when it starts.
const ACCESS = {
  full: ['Trust all', 'Never asks. Edits, runs commands and goes online on its own.'],
  risky: ['Ask first', 'Asks before commands, deletes, the network and anything outside the folder.'],
  always: ['Ask always', 'Asks before every change and every command.'],
  read: ['Read only', 'Reads and searches. Changes nothing.'],
};
const accessNote = (id, tool) => tool === 'codex' && id === 'risky' ? 'Asks to write outside the folder or go online. Codex runs the rest.' : ACCESS[id][1];
function bubbleFor(s) {
  const T = last(s), b = s.b;
  if (b.path.length) return 'On my way…';
  switch (T.stage) {
    case 'waking': return 'Waking up…';
    // The question shows over the head in place of the bubble.
    case 'waiting': return '';
    case 'working': return T.act === 'Thinking' ? 'Thinking…' : T.act === 'Writing' ? 'Writing it up…' : T.file ? `${T.act} ${short(T.file)}` : `${T.act || 'Working'}…`;
    case 'done': return b.since < 6 ? 'Done! ✓' : '';
    case 'failed': return 'Couldn’t finish';
    default: return 'z z z';
  }
}
// Answers are Markdown; each is turned into HTML once and kept a while.
const mdCache = new Map();
function md(text, s) {
  const k = (s.files || '') + '\u0000' + text;
  let h = mdCache.get(k);
  if (h == null) { h = markdown(text, { image: imageFor(s) }); if (mdCache.size > 80) mdCache.clear(); mdCache.set(k, h); }
  return h;
}
// Where an image in an answer may load from: the web, or a file in the session's
// own folder (Hover serves that folder as the session's files host).
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
const took = ms => ms == null ? '' : ms < 60e3 ? `${Math.max(1, Math.round(ms / 1000))} s` : ms < 3600e3 ? `${Math.floor(ms / 60e3)}m ${String(Math.round(ms % 60e3 / 1000) % 60).padStart(2, '0')}s` : `${Math.floor(ms / 3600e3)}h ${String(Math.floor(ms % 3600e3 / 60e3)).padStart(2, '0')}m`;
const hm = ms => ms ? new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) : '';
// A command as a row shows its program and first argument; the whole line is in
// its tooltip and its output block.
const cmdShort = c => { const w = String(c).trim().split(/\s+/); w[0] = short(w[0].replace(/^["']|["']$/g, '')); return w.slice(0, 2).join(' ') + (w.length > 2 ? ' …' : ''); };
const ended = x => x.status === 'completed' || x.status === 'failed';
// Steps from Hover are objects; the demo's are [icon, "Verb target", tag].
function stepOf(x) {
  if (!Array.isArray(x)) return x;
  const [k, text, tag] = x, sp = text.indexOf(' '), verb = sp < 0 ? text : text.slice(0, sp), rest = sp < 0 ? '' : text.slice(sp + 1);
  const o = { k, verb, status: tag === 'failed' ? 'failed' : 'completed', tag: tag === 'failed' ? null : tag };
  if (k === 'run' || k === 'search') o.cmd = rest;
  else if (rest) { const i = rest.lastIndexOf('/'); o.name = rest.slice(i + 1); o.dir = i < 0 ? null : rest.slice(0, i); }
  const m = /^\+(\d+) −(\d+)$/.exec(o.tag || ''); if (m) { o.add = +m[1]; o.del = +m[2]; o.tag = null; }
  return o;
}
// Which step blocks the user opened or closed, by session, turn and step.
const stepOpen = new Map(), turnOpen = new Map();
function stepRow(x, key, live, open) {
  const verb = live ? VERB_ON[x.verb] || x.verb : x.verb, fail = x.status === 'failed';
  const body = x.name ? `${esc(verb)} <b>${esc(x.name)}</b>${x.dir ? `<span class="pth">${esc(x.dir)}</span>` : ''}`
    : x.cmd ? `${esc(verb)} <code>${esc(x.k === 'run' ? cmdShort(x.cmd) : x.cmd.length > 48 ? x.cmd.slice(0, 47) + '…' : x.cmd)}</code>`
    : live ? `<b>${esc(verb)}</b>` : esc(verb);
  const r = [];
  if (x.add || x.del) r.push(`<span class="a">+${x.add || 0}</span><span class="d">−${x.del || 0}</span>`);
  if (fail) r.push('<span class="bad">failed</span>');
  else if (x.k === 'run' && x.exit) r.push(`<span class="bad">exit ${x.exit}</span>`);
  else if (x.tag) r.push(`<span class="okp">${CHECK} ${esc(x.tag)}</span>`);
  if (x.ms >= 1000) r.push(`<span>${took(x.ms)}</span>`);
  const blk = x.diff ? `<div class="blk"><pre>${x.diff.split('\n').map(l => `<span class="${l[0] === '+' ? 'a' : l[0] === '-' ? 'd' : 'c'}">${esc(l)}</span>`).join('')}</pre></div>`
    : x.out ? `<div class="blk"><div class="bh">Terminal${x.exit != null ? `<span class="ex${x.exit ? ' bad' : ''}">exit ${x.exit}</span>` : ''}</div><pre>${x.cmd ? `<span class="pr">$ ${esc(x.cmd)}</span>` : ''}${x.out.split('\n').map(l => `<span>${esc(l) || ' '}</span>`).join('')}</pre></div>` : '';
  const tip = x.cmd || [x.dir, x.name].filter(Boolean).join('/') || verb;
  return `<div class="s k-${x.k || 'think'}${fail ? ' fail' : ''}${live ? ' live' : ''}${blk ? ' exp' : ''}${blk && open ? ' open' : ''}" data-s="${key}" title="${esc(tip)}"${blk ? ` role="button" tabindex="0" aria-expanded="${!!open}"` : ''}>`
    + `<span class="n">${ICON[x.k] || ICON.think}</span><span class="tx">${body}</span><span class="r">${r.join('')}${blk ? CARET : ''}</span></div>${blk}`;
}
// One turn's tool calls as a timeline. Files read one after another fold into one
// row with their names under it. While the turn runs, its latest change or output
// is open.
function stepsHTML(s, T, ti, liveTurn) {
  const list = T.steps.map(stepOf), rows = [], id = s.key || s.id;
  const isLive = j => liveTurn && j === list.length - 1 && !ended(list[j]);
  const lastBlk = liveTurn ? list.findLastIndex(x => x.diff || x.out) : -1;
  for (let j = 0; j < list.length; j++) {
    const x = list[j];
    if (x.k === 'read' && x.name && !isLive(j) && x.status !== 'failed') {
      let e = j;
      while (e + 1 < list.length && list[e + 1].k === 'read' && list[e + 1].name && !isLive(e + 1) && list[e + 1].status !== 'failed') e++;
      const names = [...new Map(list.slice(j, e + 1).map(y => [y.name, [y.dir, y.name].filter(Boolean).join('/')])).entries()];
      if (names.length > 1) {
        rows.push(`<div class="s k-read"><span class="n">${ICON.read}</span><span class="tx">Read <b>${names.length} files</b></span><span class="r"></span></div>`
          + `<div class="fchips">${names.map(([n, full]) => `<span title="${esc(full)}">${esc(n)}</span>`).join('')}</div>`);
        j = e; continue;
      }
    }
    const key = `${id}:${ti}:${j}`;
    rows.push(stepRow(x, key, isLive(j), stepOpen.has(key) ? stepOpen.get(key) : j === lastBlk));
  }
  if (!rows.length) return '';
  // Over the timeline, one line: how long it worked and what it did. A finished turn
  // folds to it; the one running stays open. A click opens or folds it.
  const n = k => list.filter(x => x.k === k).length, files = new Set(list.filter(x => x.k === 'edit').map(x => x.name || x.cmd || x.verb)).size;
  const bits = [n('read') && `${n('read')} read`, files && `${files} file${files === 1 ? '' : 's'} edited`, n('run') && `${n('run')} run`].filter(Boolean);
  const key = `${id}:${ti}`, open = turnOpen.has(key) && turnOpen.get(key);
  // As in Codex: steps fold away once done. While the turn runs, only the step it
  // is on shows under the line; the rest are one click away.
  const j = list.length - 1, now = liveTurn && !open && j >= 0 && isLive(j) ? `<div class="steps now">${stepRow(list[j], `${id}:${ti}:${j}:now`, true, stepOpen.get(`${id}:${ti}:${j}:now`))}</div>` : '';
  const head = liveTurn ? `Working <span class="tm" data-t0="${T.t0}">${clockOf(T.t0)}</span>` : `Worked ${took(T.took ?? 0) || '—'}`;
  return `<div class="sum${open ? ' open' : ''}" data-t="${key}" role="button" tabindex="0" aria-expanded="${open}"><span class="sw"><b>${head}</b>${bits.map(b => `<span class="dot">·</span>${b}`).join('')}</span>${CARET}</div><div class="steps">${rows.join('')}</div>${now}`;
}
// What a finished turn changed, file by file.
function changesHTML(T) {
  const by = new Map();
  for (const x of T.steps.map(stepOf)) {
    if (x.k !== 'edit' || !x.name || !(x.add || x.del)) continue;
    const dir = (x.dir || '').split('/').pop(), k = (dir ? dir + '/' : '') + x.name, v = by.get(k) || [0, 0, [x.dir, x.name].filter(Boolean).join('/')];
    v[0] += x.add || 0; v[1] += x.del || 0; by.set(k, v);
  }
  if (!by.size) return '';
  const all = [...by.values()], a = all.reduce((n, v) => n + v[0], 0), d = all.reduce((n, v) => n + v[1], 0);
  const pm = (a, d) => `${a ? `<span class="a">+${a}</span>` : ''}${d ? `<span class="d">−${d}</span>` : ''}`;
  return `<div class="chg"><div class="ch">${by.size} file${by.size === 1 ? '' : 's'} changed <span>+${a} −${d}</span></div>`
    + [...by].map(([k, v]) => `<div class="fr" title="${esc(v[2])}"><span>${esc(k)}</span><i>${pm(v[0], v[1])}</i></div>`).join('') + '</div>';
}
// What a turn cost: "0.09 credits", and "<0.01 credits" for less.
const credits = n => { const s = n.toFixed(2); return n < 0.005 ? '<0.01 credits' : `${s} credit${s === '1.00' ? '' : 's'}`; };
function renderDrawer() {
  const s = cur(); if (!s) return;
  const T = last(s), st = T.stage, x = toolOf(s), hide = x.hideSteps;
  $('#dAv').innerHTML = `<span class="lg ${TOOLS[s.tool] ? s.tool : 'kiro'}" role="img" aria-label="${esc(x.name)}" title="${esc(x.name)}">${logo(s.tool)}</span>`;
  $('#dTitle').textContent = s.title; $('#dTitle').title = s.title;
  const f = $('#dFolder'); f.innerHTML = FOLDER + `<span>${esc(short(s.folder) || s.folder || '')}</span>`; f.title = s.folder || ''; f.hidden = !s.folder;
  const acc = $('#dAccess'), ac = ACCESS[s.access]; acc.hidden = !ac;
  if (ac) { acc.innerHTML = SHIELD + esc(ac[0]); acc.title = accessNote(s.access, s.tool); acc.className = 'chip ' + s.access; }
  const ctx = $('#ctx'); ctx.hidden = s.ctx == null;
  if (s.ctx != null) { const tip = `${s.ctx}% of the context window used`; ctx.dataset.tip = tip; ctx.setAttribute('aria-label', tip); $('#ctxRing').style.setProperty('--p', s.ctx); $('#ctxPct').textContent = s.ctx + '%'; }
  const th = $('#thread'), keep = th.scrollHeight - th.scrollTop - th.clientHeight < 40;
  // A typed answer to a question keeps its box's focus through the redraw.
  const typing = th.contains(document.activeElement) ? document.activeElement.dataset?.qt : undefined;
  let fresh = false;
  th.innerHTML = s.turns.map((T, i) => {
    const lastTurn = T === last(s), liveTurn = lastTurn && (st === 'working' || st === 'waiting');
    const steps = hide ? '' : stepsHTML(s, T, i, liveTurn);
    const now = lastTurn && st === 'waiting' && s.ask && !s.archived ? askHTML(s, 'chat') : '';
    if (T.answer && T.fresh) { fresh = true; T.fresh = false; }
    const ans = T.answer ? `<div><div class="who2">${badge(s.tool)}<b>${esc(s.b.name)}</b>${T.took != null ? `<span>· ${took(T.took)}</span>` : ''}</div><div class="ans md ${T.stage === 'failed' ? 'err' : ''}${fresh && lastTurn ? ' fresh' : ''}">${md(T.answer, s)}</div></div>` : '';
    const chg = T.answer && !hide ? changesHTML(T) : '';
    // What the turn cost, under its answer, when the tool says (Kiro does).
    const use = T.answer && T.credits != null ? `<div class="use" title="Kiro credits this turn used">${esc(credits(T.credits))}</div>` : '';
    const pics = T.images?.length ? `<div class="pics">${T.images.map(u => `<img src="${esc(u)}" alt="Attached image" loading="lazy">`).join('')}</div>` : '';
    return `<div class="me">${pics}${esc(T.prompt)}${T.queued ? '<span class="q">Queued · sends when this run ends</span>' : ''}${T.t0 ? `<span class="when">${hm(T.t0)}</span>` : ''}</div>${steps}${now}${ans}${chg}${use}`;
  }).join('');
  // A code block in an answer gets its language and a Copy button.
  for (const pre of th.querySelectorAll('.ans pre')) {
    const box = document.createElement('div'); box.className = 'cb';
    box.innerHTML = `<div class="ch">${esc(pre.dataset.lang || 'code')}<button type="button" data-copy>Copy</button></div>`;
    pre.replaceWith(box); box.appendChild(pre);
  }
  if (keep || fresh) th.scrollTop = 1e9;
  if (typing != null) { const box = th.querySelector(`[data-qt="${typing}"]`); if (box) { box.focus(); box.setSelectionRange(box.value.length, box.value.length); } }
  const b = busy(s);
  $('#input').placeholder = s.archived ? `Reply to wake ${x.name} and carry on…` : s.ask ? `Or tell ${s.b.name} what to do instead…` : b ? `Reply. ${s.b.name} reads it when this run ends` : `Reply to ${s.b.name}…`;
  $('#dDel').hidden = false; renderPill('#dModel', s.tool); syncSend();
}
// Over the composer while a run goes: what the agent does now, for how long, and
// Stop. Amber while it waits for the user.
const day = ms => { const d = new Date(ms), n = new Date(); const k = (n - new Date(d.getFullYear(), d.getMonth(), d.getDate())) / 864e5; return k < 1 ? 'Today' : k < 2 ? 'Yesterday' : k < 7 ? 'This week' : d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' }); };
// A history row's date, beside the day heading over it: the time today and
// yesterday, the weekday this week, and the date before that (with the year when it
// isn't this one).
const stamp = ms => {
  const d = new Date(ms), n = new Date(), k = Math.round((new Date(n.getFullYear(), n.getMonth(), n.getDate()) - new Date(d.getFullYear(), d.getMonth(), d.getDate())) / 864e5);
  return k < 2 ? d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) : k < 7 ? d.toLocaleDateString(undefined, { weekday: 'short' })
    : d.toLocaleDateString(undefined, d.getFullYear() === n.getFullYear() ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
};
let historyFind = '';
function renderPanel() {
  if (!panel) return;
  const body = $('#pBody');
  if (panel === 'history') {
    $('#pTitle').textContent = 'Session history';
    $('#pSub').textContent = `${history.length} session${history.length === 1 ? '' : 's'}, kept until you delete them`;
    const q = historyFind.toLowerCase(), list = history.filter(h => !q || (h.title + ' ' + h.folder + ' ' + h.tool).toLowerCase().includes(q));
    let at = '';
    const rows = list.map(h => {
      const head = day(h.at) !== at ? `<h4 class="sh">${esc(at = day(h.at))}</h4>` : '';
      const desk = sessions.some(s => s.key === h.key), tool = TOOLS[h.tool] ? h.tool : 'kiro';
      const meta = [`<span class="hs st-${h.stage}"><i></i>${esc(WORD[h.stage] || h.stage)}</span>`, `${h.turns} turn${h.turns === 1 ? '' : 's'}`, `<span class="hf" title="${esc(h.folder)}">${esc(short(h.folder))}</span>`];
      const when = new Date(h.at).toLocaleString(undefined, { dateStyle: 'full', timeStyle: 'short' });
      return head + `<div class="hrow"><button class="hr" data-key="${h.key}" title="${esc(h.title)}"><span class="lg ${tool}" role="img" aria-label="${TOOLS[tool][0]}">${logo(tool)}</span>`
        + `<span class="ht"><span class="h1"><b>${esc(h.title)}</b><time datetime="${new Date(h.at).toISOString()}" title="${esc(when)}">${esc(stamp(h.at))}</time></span><span class="hm">${meta.join('<span class="dot">·</span>')}${desk ? '<span class="hat">At a desk</span>' : ''}</span></span></button>`
        + `<button class="hx" data-del="${h.key}" aria-label="Delete ${esc(h.title)}" title="Delete"><svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/></svg></button></div>`;
    }).join('');
    const find = body.querySelector('#hFind'), had = document.activeElement === find;
    body.innerHTML = `<label class="hfind"><svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg><input id="hFind" type="search" placeholder="Find a session" aria-label="Find a session" value="${esc(historyFind)}"></label>${rows || `<p class="none">${history.length ? 'Nothing matches.' : 'Sessions you start are kept here. Open one to read it, reply to carry on.'}</p>`}`;
    if (had) { const f = body.querySelector('#hFind'); f.focus(); f.setSelectionRange(f.value.length, f.value.length); }
    return;
  }
  if (panel === 'board') {
    $('#pTitle').textContent = 'Session board';
    $('#pSub').textContent = `${sessions.length} of ${DESKS.length} desks in use`;
    body.innerHTML = `<div class="kanban">${COLS.map(([h, c, st]) => {
      const list = sessions.filter(s => st.includes(last(s).stage));
      return `<div class="col"><h4 style="--k:${c}">${h[0] + h.slice(1).toLowerCase()}<span>${list.length}</span></h4>${list.map(s => { const T = last(s);
        return `<button class="card st-${T.stage}" data-open="${s.id}">${LOGO(s.b.css)}<span class="ct"><b>${esc(s.b.name)}${badge(s.tool)} · ${WORD[T.stage]}</b><span>${esc(s.title)}</span><em>${ago(T.t0)}${T.steps.length ? ` · ${T.steps.length} step${T.steps.length === 1 ? '' : 's'}` : ''}</em></span></button>`; }).join('') || '<p class="none">Nobody here</p>'}</div>`;
    }).join('')}</div>`;
  } else {
    $('#pTitle').textContent = 'Office overview';
    $('#pSub').textContent = `Up to ${maxRunning === 1 ? 'one task runs' : maxRunning + ' tasks run'} at once, across Kiro, Codex, Cursor and OpenCode`;
    const stats = [['Working', count(['waking', 'working', 'waiting']), 'var(--li)'], ['Done', count(['done']), 'var(--ok)'], ['Failed', count(['failed']), 'var(--bad)'], ['Stopped', count(['stopped']), 'var(--stop)']];
    body.innerHTML = `<div class="stats">${stats.map(([l, v, c]) => `<div style="--k:${c}"><b>${v}</b><span>${l}</span></div>`).join('')}</div>
      <h4 class="sh">Context used</h4>${sessions.map(s => `<button class="meter" data-open="${s.id}">${LOGO(s.b.css)}<span class="mt"><b>${esc(s.b.name)}</b><span>${esc(s.title)}</span><i><u style="width:${s.ctx ?? 0}%"></u></i></span><em>${s.ctx == null ? '—' : s.ctx + '%'}</em></button>`).join('') || '<p class="none">No sessions yet. Press + to give an agent a task.</p>'}`;
  }
}
function changed(s) {
  drawBoard(); renderPanel(); renderAsks();
  if (s && s.id === sel && drawerOpen) renderDrawer();
}

// ── What an agent asks before it acts ───────────────────────────────────
// The question, as a card: over the bot's head in the room, or at the end of its
// chat. Run (or Allow edit, Delete) allows it once, Trust allows it again for the rest
// of the session, Deny turns it down and the agent carries on without it.
function askHTML(s, where) {
  const a = s.ask; if (!a) return '';
  if (a.questions?.length) return questionHTML(s, a, where);
  const body = a.command ? `<span class="pr">$</span>${esc(a.command)}`
    : a.preview ? (a.path ? `<span class="pth">${esc(a.path)}</span>\n` : '') + a.preview.split('\n').map(l => `<span class="${l[0] === '+' ? 'a' : l[0] === '-' ? 'd' : ''}">${esc(l)}</span>`).join('\n')
    : esc(a.path || a.title);
  return `<div class="askc ${where}${a.danger ? ' dz' : ''}" data-sid="${s.id}" data-ask="${esc(a.id)}" role="group" aria-label="${esc(a.title)}">`
    + `<div class="at">${badge(s.tool)}<b>${esc(a.title)}</b>${a.more ? `<em>+${a.more} more</em>` : ''}</div>`
    + `<pre>${body}</pre>${a.reason ? `<div class="ar"><i></i>${esc(a.reason)}</div>` : ''}`
    + `<div class="ab"><button data-ans="deny">Deny</button><span class="sp"></span><button data-ans="trust" title="Allow this again for the rest of the session">Trust</button>`
    + `<button data-ans="allow" class="${a.danger ? 'dz' : 'pri'}">${esc(a.allow || 'Allow')}</button></div></div>`;
}
function answer(s, id, how) {
  if (host) { host.postMessage({ type: 'answer', id: s.id, ask: id, answer: how }); return; }
  // The demo: the bot gets on with it, or finds another way.
  const T = last(s); s.ask = null;
  if (how === 'deny') { T.stage = 'working'; T.act = 'Thinking'; changed(s); timers[s.id] = setTimeout(() => play(s, T, 5), 2500); return; }
  T.stage = 'working'; T.act = 'Running'; changed(s); play(s, T, 4);
}
function renderAsks() {
  for (const s of sessions) {
    const slot = s.tag?.querySelector('.ask-slot'); if (!slot) continue;
    const key = last(s).stage === 'waiting' && s.ask ? s.ask.id + (s.ask.more || 0) : '';
    if (slot.dataset.key === key) continue;
    slot.dataset.key = key; slot.innerHTML = key ? askHTML(s, 'over') : '';
  }
}
addEventListener('click', e => {
  const b = e.target.closest('[data-ans]'); if (!b) return;
  const card = b.closest('.askc'), s = sessions.find(x => x.id === +card.dataset.sid);
  if (s?.ask && s.ask.id === card.dataset.ask) answer(s, s.ask.id, b.dataset.ans);
});

// ── What an agent asks the user (OpenCode's question tool) ──────────────
// Its choices are buttons that toggle (one, or several where it allows), with a box
// for an answer of one's own where it allows; the picks survive a redraw. Answer
// sends them, Skip turns the question down, and the agent is told either way.
const qPick = new Map();
function picksOf(a) {
  let p = qPick.get(a.id);
  if (!p) { p = { sel: a.questions.map(() => []), text: a.questions.map(() => '') }; qPick.set(a.id, p); if (qPick.size > 20) qPick.delete(qPick.keys().next().value); }
  return p;
}
function questionHTML(s, a, where) {
  const p = picksOf(a);
  // Over the head it stays small; the choices are in the chat, which Answer… opens.
  if (where === 'over') return `<div class="askc over" data-sid="${s.id}" data-ask="${esc(a.id)}" role="group" aria-label="${esc(a.title)}">`
    + `<div class="at">${badge(s.tool)}<b>${esc(a.title)}</b>${a.more ? `<em>+${a.more} more</em>` : ''}</div>`
    + `<div class="qq"><div class="qh">${esc(a.questions[0].header)}</div><p class="qline">${esc(a.questions[0].question)}</p></div>`
    + `<div class="ab"><button data-ans="deny">Skip</button><span class="sp"></span><button data-qopen class="pri">Answer…</button></div></div>`;
  const qs = a.questions.map((q, qi) => `<div class="qq"><div class="qh">${esc(q.header)}</div><p>${esc(q.question)}</p>`
    + `<div class="qo" role="group" aria-label="${esc(q.header)}${q.multiple ? ', pick any' : ', pick one'}">`
    + q.options.map(o => `<button type="button" data-q="${qi}" data-opt="${esc(o.label)}" aria-pressed="${p.sel[qi].includes(o.label)}">${esc(o.label)}${o.description ? `<small>${esc(o.description)}</small>` : ''}</button>`).join('')
    + (q.custom ? `<input type="text" data-qt="${qi}" maxlength="2000" placeholder="Or type your own answer" aria-label="Your own answer: ${esc(q.header)}" value="${esc(p.text[qi])}">` : '')
    + '</div></div>').join('');
  return `<div class="askc ${where}" data-sid="${s.id}" data-ask="${esc(a.id)}" role="group" aria-label="${esc(a.title)}">`
    + `<div class="at">${badge(s.tool)}<b>${esc(a.title)}</b>${a.more ? `<em>+${a.more} more</em>` : ''}</div>${qs}`
    + `<div class="ab"><button data-ans="deny" title="Don’t answer. ${esc(s.b?.name || 'The agent')} is told you skipped it.">Skip</button><span class="sp"></span><button data-qsend class="pri">Answer</button></div></div>`;
}
function sendAnswers(s, a) {
  const p = picksOf(a), answers = a.questions.map((_, i) => [...p.sel[i], ...(p.text[i].trim() ? [p.text[i].trim()] : [])]);
  if (answers.some(x => !x.length)) return toast(a.questions.length > 1 ? 'Answer each question first.' : 'Pick an answer first.');
  qPick.delete(a.id);
  if (host) { host.postMessage({ type: 'answer', id: s.id, ask: a.id, answers }); return; }
  s.ask = null; const T = last(s); T.stage = 'working'; T.act = 'Thinking'; changed(s); play(s, T, 2);
}
addEventListener('click', e => {
  const qo = e.target.closest('[data-qopen]'); if (qo) { const card = qo.closest('.askc'); return openSession(+card.dataset.sid); }
  const opt = e.target.closest('[data-opt]'), go = e.target.closest('[data-qsend]'); if (!opt && !go) return;
  const card = e.target.closest('.askc'), s = sessions.find(x => x.id === +card.dataset.sid), a = s?.ask;
  if (!a?.questions || a.id !== card.dataset.ask) return;
  if (go) return sendAnswers(s, a);
  const qi = +opt.dataset.q, label = opt.dataset.opt, p = picksOf(a), sel = p.sel[qi];
  p.sel[qi] = sel.includes(label) ? sel.filter(x => x !== label) : a.questions[qi].multiple ? [...sel, label] : [label];
  // The same question shows over the bot's head and in its chat: both follow.
  for (const b of document.querySelectorAll(`.askc[data-ask="${CSS.escape(a.id)}"] [data-q="${qi}"]`)) b.setAttribute('aria-pressed', p.sel[qi].includes(b.dataset.opt));
});
addEventListener('input', e => {
  const t = e.target.closest?.('[data-qt]'); if (!t) return;
  const card = t.closest('.askc'), s = sessions.find(x => x.id === +card.dataset.sid);
  if (s?.ask?.questions && s.ask.id === card.dataset.ask) picksOf(s.ask).text[+t.dataset.qt] = t.value;
});
addEventListener('keydown', e => {
  const t = e.target.closest?.('[data-qt]'); if (!t || e.key !== 'Enter') return;
  e.preventDefault(); t.closest('.askc').querySelector('[data-qsend]')?.click();
});

const clockOf = t0 => { const n = Math.max(0, Math.floor((Date.now() - t0) / 1000)); return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')}`; };
setInterval(() => { for (const lt of document.querySelectorAll('#thread .sum .tm')) lt.textContent = clockOf(+lt.dataset.t0); }, 1000);
function openSession(id) {
  closePanel(true); fold();
  viewing = null; sel = id; drawerOpen = true; view.classList.add('open'); $('#drawer').classList.add('open'); $('#drawer').setAttribute('aria-hidden', 'false');
  host?.postMessage({ type: 'open', id });
  renderDrawer(); drawTV(clockT);
}
function closeDrawer() { if (drawerOpen && !viewing) host?.postMessage({ type: 'close' }); viewing = null; drawerOpen = false; view.classList.remove('open'); $('#drawer').classList.remove('open'); $('#drawer').setAttribute('aria-hidden', 'true'); }
function openPanel(kind) {
  if (drawerOpen) closeDrawer();
  fold();
  panel = kind; view.classList.add('open'); $('#panel').classList.add('open'); $('#panel').setAttribute('aria-hidden', 'false'); renderPanel();
}
function closePanel(keepOpen) { if (!panel) return; panel = null; $('#panel').classList.remove('open'); $('#panel').setAttribute('aria-hidden', 'true'); if (!keepOpen) view.classList.remove('open'); }
let toastTimer;
function toast(text) { const t = $('#toast'); t.textContent = text; t.classList.add('show'); clearTimeout(toastTimer); toastTimer = setTimeout(() => t.classList.remove('show'), 2800); }

// ── Moving around: drag to pan, wheel to zoom, double-click to reset ────
// Where the user left the camera, kept across the page being dropped and made again.
const userView = (() => { try { const v = JSON.parse(localStorage.getItem('office.view')); if (v && [v.x, v.z, v.zoom].every(Number.isFinite)) return v; } catch {} return { x: 0, z: 0, zoom: 1 }; })();
let viewSave;
Object.assign(cam, { x: userView.x, z: userView.z, zoom: userView.zoom });
const FWD = new THREE.Vector3(-1, 0, -1).normalize(), SIN_E = ISO.y / ISO.length();
const clampN = (v, a, b) => Math.min(b, Math.max(a, v));
// A move on screen (px, y up) as a move over the floor.
function overFloor(dx, dy, zoom) { const w = 2 * halfWidth(zoom) / W; return { x: (RIGHT.x * dx + FWD.x * dy / SIN_E) * w, z: (RIGHT.z * dx + FWD.z * dy / SIN_E) * w }; }
function zoomBy(k, dx = 0, dy = 0) {
  const old = userView.zoom, nz = clampN(old * k, 0.85, 2.8); if (nz === old) return;
  const a = overFloor(dx, dy, old), b = overFloor(dx, dy, nz);
  userView.x += a.x - b.x; userView.z += a.z - b.z; userView.zoom = nz; clampView();
}
function clampView() { userView.x = clampN(userView.x, -6, 6); userView.z = clampN(userView.z, -5, 5); clearTimeout(viewSave); viewSave = setTimeout(() => localStorage.setItem('office.view', JSON.stringify(userView)), 400); }
function resetView() { Object.assign(userView, { x: 0, z: 0, zoom: 1 }); clampView(); }
canvas.addEventListener('wheel', e => {
  e.preventDefault(); if (drawerOpen || panel) return;
  const r = canvas.getBoundingClientRect(); zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left - W / 2, -(e.clientY - r.top - H / 2));
}, { passive: false });


// ── Pointer and keys ────────────────────────────────────────────────────
const ray = new THREE.Raycaster(), ndc = new THREE.Vector2();
let pointer = null, hovered = null, down = null, dragging = false, px0 = 0, py0 = 0;
canvas.addEventListener('pointermove', e => {
  const r = canvas.getBoundingClientRect(); pointer = [(e.clientX - r.left) / r.width * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1, e.clientX - r.left, e.clientY - r.top];
  if (down && !drawerOpen && !panel) {
    if (!dragging && Math.hypot(e.clientX - down[0], e.clientY - down[1]) > 6) { dragging = true; canvas.setPointerCapture(e.pointerId); canvas.classList.add('drag'); }
    if (dragging) { const g = overFloor(e.clientX - px0, -(e.clientY - py0), userView.zoom); userView.x -= g.x; userView.z -= g.z; clampView(); }
  }
  px0 = e.clientX; py0 = e.clientY;
});
canvas.addEventListener('pointerleave', () => { pointer = null; });
canvas.addEventListener('pointerdown', e => { down = [e.clientX, e.clientY]; px0 = e.clientX; py0 = e.clientY; dragging = false; });
canvas.addEventListener('pointerup', e => {
  const was = dragging; dragging = false; down = null; canvas.classList.remove('drag');
  if (was) return;
  if (hovered?.bot) { const s = sessions.find(x => x.b === hovered.bot); if (s) openSession(s.id); }
  else if (hovered?.prop) hovered.prop.go();
  else if (fab !== 'rest') fold();
  else if (drawerOpen) closeDrawer(); else if (panel) closePanel();
});
canvas.addEventListener('dblclick', () => { if (!hovered && !drawerOpen && !panel) resetView(); });
function pick() {
  let hit = null;
  if (pointer && !dragging) { ndc.set(pointer[0], pointer[1]); ray.setFromCamera(ndc, camera); const o = ray.intersectObjects(hits, false)[0]?.object;
    if (o?.userData.bot && sessions.some(s => s.b === o.userData.bot)) hit = { bot: o.userData.bot }; else if (o?.userData.prop) hit = { prop: o.userData.prop }; }
  const same = hit?.bot === hovered?.bot && hit?.prop === hovered?.prop;
  if (!same) { hovered = hit; canvas.style.cursor = hit ? 'pointer' : ''; }
  for (const s of sessions) { s.b.hot = s.b === hovered?.bot || (drawerOpen && s.id === sel); s.tag.classList.toggle('hot', s.b.hot); }
  for (const p of PROPS) p.pane?.material.color.setScalar(p === hovered?.prop ? 1.35 : 1);
  const tip = $('#tip');
  if (hovered?.prop && pointer) { tip.textContent = hovered.prop.hint(); tip.style.transform = `translate(${pointer[2] + 14}px,${pointer[3] + 16}px)`; tip.hidden = false; }
  else tip.hidden = true;
}
$('#pBody').addEventListener('click', e => {
  const b = e.target.closest('[data-open]'); if (b) return openSession(+b.dataset.open);
  const k = e.target.closest('[data-key]'); if (k) return openHistory(k.dataset.key);
  const d = e.target.closest('[data-del]');
  if (d) { const h = history.find(x => x.key === d.dataset.del); if (h) askDelete({ ...sessions.find(s => s.key === h.key), ...h, archived: !sessions.some(s => s.key === h.key) }); }
});
$('#pBody').addEventListener('input', e => { if (e.target.id === 'hFind') { historyFind = e.target.value; renderPanel(); } });
$('#dClose').onclick = closeDrawer; $('#pClose').onclick = () => closePanel();
// A step with a change or output opens and closes, and stays as the user left it.
function toggleStep(row) {
  const open = !row.classList.contains('open'), map = row.dataset.t ? turnOpen : stepOpen;
  row.classList.toggle('open', open); row.setAttribute('aria-expanded', open); map.set(row.dataset.t || row.dataset.s, open);
}
$('#thread').addEventListener('click', e => {
  const row = e.target.closest('.s.exp,.sum'); if (row) return toggleStep(row);
  const c = e.target.closest('[data-copy]'); if (c) return copyText(c.closest('.cb').querySelector('pre').textContent, c);
}, true);
$('#thread').addEventListener('keydown', e => { const row = e.target.closest?.('.s.exp,.sum'); if (row && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggleStep(row); } });
function copyText(text, btn) {
  const done = () => { btn.textContent = 'Copied'; setTimeout(() => { btn.textContent = 'Copy'; }, 1400); };
  const old = () => { const t = document.createElement('textarea'); t.value = text; document.body.appendChild(t); t.select(); document.execCommand('copy'); t.remove(); done(); };
  navigator.clipboard?.writeText(text).then(done, old) ?? old();
}
// Links open in the browser, through Hover.
$('#thread').addEventListener('click', e => { const a = e.target.closest('a[href]'); if (!a) return; e.preventDefault(); if (host) host.postMessage({ type: 'link', url: a.href }); else open(a.href, '_blank', 'noopener'); });
$('#dDel').onclick = () => { const s = cur(); if (s) askDelete(s); };
const input = $('#input');
function autosize(el) { el.style.height = '28px'; el.style.height = Math.min(112, Math.max(28, el.scrollHeight)) + 'px'; }

// Pasted, dropped or picked images, kept as data: URLs until sent. Big ones are
// scaled to 2000 px on the long side, which is plenty for Kiro to read them.
const MAX_PICS = 4, attached = { reply: [], new: [] };
function shrink(file) {
  return new Promise(res => {
    const url = URL.createObjectURL(file), img = new Image();
    img.onload = () => {
      const k = Math.min(1, 2000 / Math.max(img.width, img.height));
      if (k === 1 && file.size < 3e6 && /png|jpeg|webp|gif/.test(file.type)) { const r = new FileReader(); r.onload = () => res(r.result); r.readAsDataURL(file); URL.revokeObjectURL(url); return; }
      const c = document.createElement('canvas'); c.width = Math.round(img.width * k); c.height = Math.round(img.height * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height); URL.revokeObjectURL(url);
      res(c.toDataURL('image/jpeg', 0.9));
    };
    img.onerror = () => { URL.revokeObjectURL(url); res(null); };
    img.src = url;
  });
}
async function addPics(files, which) {
  const list = [...files].filter(f => f.type.startsWith('image/'));
  if (!list.length) return false;
  for (const f of list) {
    if (attached[which].length >= MAX_PICS) { toast(`Up to ${MAX_PICS} images at a time.`); break; }
    const d = await shrink(f); if (d) attached[which].push(d);
  }
  renderPics(which); return true;
}
function renderPics(which) {
  const box = $(which === 'reply' ? '#shots' : '#nShots'), list = attached[which];
  box.hidden = !list.length;
  box.innerHTML = list.map((d, i) => `<div class="shot"><img src="${d}" alt="Image ${i + 1}"><button data-rm="${i}" aria-label="Remove image ${i + 1}"><svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg></button></div>`).join('');
  if (which === 'reply') syncSend(); else renderNew();
}
for (const [which, el, strip] of [['reply', input, '#shots'], ['new', $('#nInput'), '#nShots']]) {
  el.addEventListener('paste', e => { const files = [...(e.clipboardData?.items || [])].filter(i => i.kind === 'file').map(i => i.getAsFile()).filter(Boolean); if (files.length) { e.preventDefault(); addPics(files, which); } });
  $(strip).addEventListener('click', e => { const b = e.target.closest('[data-rm]'); if (b) { attached[which].splice(+b.dataset.rm, 1); renderPics(which); } });
}
const composer = $('#composer');
composer.addEventListener('dragover', e => { if ([...e.dataTransfer.items].some(i => i.kind === 'file')) { e.preventDefault(); composer.classList.add('over'); } });
composer.addEventListener('dragleave', () => composer.classList.remove('over'));
composer.addEventListener('drop', e => { composer.classList.remove('over'); if (e.dataTransfer.files.length) { e.preventDefault(); addPics(e.dataTransfer.files, 'reply'); } });
$('#attach').onclick = () => $('#pick').click();
$('#pick').onchange = e => { addPics(e.target.files, 'reply'); e.target.value = ''; input.focus(); };

function syncSend() {
  const s = cur(), empty = !input.value.trim() && !attached.reply.length, b = $('#send');
  // While a run goes, an empty box makes this the Stop button; words in it queue a reply.
  const running = !!s && !s.archived && busy(s), stop = running && empty;
  b.disabled = empty && !stop; b.classList.toggle('stop', stop);
  const label = stop ? 'Stop this run' : running ? 'Queue this reply' : 'Send';
  b.setAttribute('aria-label', label); b.title = stop ? label : label + ' (Enter)';
}
function send() { const v = input.value.trim(), pics = attached.reply.splice(0); if (!v && !pics.length) return; input.value = ''; autosize(input); renderPics('reply'); doReply(v, pics); syncSend(); }
input.addEventListener('input', () => { autosize(input); syncSend(); });
input.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } });
composer.addEventListener('mousedown', e => { if (!e.target.closest('button,textarea,img')) { e.preventDefault(); input.focus(); } });
$('#send').onclick = () => { const s = cur(); if ($('#send').classList.contains('stop')) { if (s) doStop(s); } else send(); };

// New task: the circle, the agents' logos, then the box for the one picked. The
// folder and the model are the tool's own; each session keeps its folder.
function renderTools() {
  $('#fabTools').innerHTML = tools.map((x, i) => `<button class="lg ${x.id}${x.ready ? '' : ' off'}" role="menuitem" data-tool="${x.id}" style="--i:${i}" tabindex="${fab === 'pick' ? 0 : -1}" aria-label="${esc(x.ready ? x.name : `${x.name}: ${x.hint}`)}"><span class="tip">${esc(x.name)}${x.ready ? '' : ' <em>· ' + esc(x.hint) + '</em>'}</span>${logo(x.id)}</button>`).join('');
}
function renderNew() {
  const full = sessions.length >= DESKS.length && !sessions.some(s => !busy(s));
  const tool = tools.find(x => x.id === newTool) || tools[0];
  const who = $('#nWho'); who.className = 'lg ' + tool.id; who.innerHTML = logo(tool.id); who.title = `${tool.name} · change agent`; who.setAttribute('aria-label', `${tool.name}. Change agent`);
  $('#nFolderText').textContent = newFolder ? short(newFolder) : 'Choose a folder';
  $('#nFolder').title = newFolder ? `${newFolder}\nClick to change` : `Pick the folder ${tool.name} works in`;
  $('#nFolder').classList.toggle('none', !newFolder);
  $('#nInput').placeholder = `What should ${tool.name} do? Paste an image to show it.`;
  renderPill('#nModel', tool.id);
  // What this task may do on its own: the tool's setting until the box picks another.
  const acc = newAccessOf(tool);
  $('#nAccessText').textContent = ACCESS[acc][0];
  $('#nAccess').classList.toggle('full', acc === 'full');
  $('#nAccess').title = `${ACCESS[acc][0]}: ${accessNote(acc, tool.id)} Click to change.`;
  $('#nAccess').setAttribute('aria-label', `Tool access: ${ACCESS[acc][0]}. Change`);
  // Said only when something stops the task from starting.
  const why = !tool.ready ? tool.hint : !canStart ? `${maxRunning === 1 ? '1 task is' : maxRunning + ' tasks are'} running. Start another when one is done.` : full ? 'All six desks are busy. Stop or remove a session first.' : '';
  $('#nNote').textContent = why; $('#nNote').hidden = !why;
  const draft = !!$('#nInput').value.trim() || attached.new.length > 0;
  $('#nGo').disabled = !tool.ready || !canStart || full || !newFolder || !draft;
  $('#nGo').title = !newFolder ? 'Choose a folder first' : `Start (Enter). ${tool.name} starts with ${ACCESS[acc][0].toLowerCase()}.`;
  $('#fab').classList.toggle('draft', draft);
}
function setFab(to) {
  if (to === fab) return;
  fab = to; const f = $('#fab');
  f.className = 's-' + to; $('#fabMain').setAttribute('aria-expanded', to === 'pick');
  $('#fabMain').setAttribute('aria-label', to === 'pick' ? 'Close' : 'New task');
  $('#newtask').setAttribute('aria-hidden', to !== 'open');
  renderTools(); renderNew();
  if (to === 'pick') $('#fabTools .lg:not(.off)')?.focus();
  else if (to === 'open') setTimeout(() => $('#nInput').focus(), 60);
}
// Back to the circle; what was typed stays for next time.
function fold() { if (fab === 'rest') return; const had = fab === 'open' || document.activeElement?.closest('#fab'); closeMenu(); setFab('rest'); if (had) $('#fabMain').focus({ preventScroll: true }); }
function openNew() { if (drawerOpen) closeDrawer(); closePanel(); setFab('pick'); }
$('#fabMain').onclick = () => fab === 'pick' ? fold() : openNew();
$('#fabTools').onclick = e => {
  const b = e.target.closest('[data-tool]'); if (!b) return;
  const x = tools.find(t => t.id === b.dataset.tool);
  if (!x.ready) return toast(x.hint);
  newTool = x.id; toolPicked = true; setFab('open');
};
$('#fabTools').addEventListener('keydown', e => {
  const items = [...$('#fabTools').querySelectorAll('.lg')], i = items.indexOf(document.activeElement);
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { e.preventDefault(); items[(i + (e.key === 'ArrowRight' ? 1 : -1) + items.length) % items.length]?.focus(); }
});
$('#nWho').onclick = () => { closeMenu(); setFab('pick'); };
$('#nFold').onclick = fold;
$('#nAttach').onclick = () => $('#nPick').click();
$('#nPick').onchange = e => { addPics(e.target.files, 'new'); e.target.value = ''; $('#nInput').focus(); };
{ const c = $('#newtask');
  c.addEventListener('dragover', e => { if ([...e.dataTransfer.items].some(i => i.kind === 'file')) { e.preventDefault(); c.classList.add('over'); } });
  c.addEventListener('dragleave', () => c.classList.remove('over'));
  c.addEventListener('drop', e => { c.classList.remove('over'); if (e.dataTransfer.files.length) { e.preventDefault(); addPics(e.dataTransfer.files, 'new'); } }); }

// ── The model pill and its menu ─────────────────────────────────────────
// The pick is the tool's default from then on, as in Settings, from its next turn.
const EFFORT = e => e === 'xhigh' ? 'X-High' : e ? e[0].toUpperCase() + e.slice(1) : '';
// The efforts for the tool's picked model: its own levels (OpenCode's variants), or
// the tool's list when models don't carry any.
const effortsOf = x => { const m = (x.models || []).find(m => m.id === (x.model || '')); return m?.levels || (x.models || []).some(m => m.levels) ? m?.levels || [] : x.efforts || []; };
function renderPill(sel, toolId) {
  const x = tools.find(t => t.id === toolId) || tools[0], el = $(sel);
  const m = (x.models || []).find(m => m.id === (x.model || '')) || (x.models || [])[0];
  const eff = effortsOf(x).includes(x.effort) ? x.effort : null;
  el.dataset.tool = x.id;
  el.innerHTML = `<span>${esc(m?.name || 'Default')}</span>${eff ? `<em>${esc(EFFORT(eff))}</em>` : ''}<svg viewBox="0 0 24 24"><path d="m6 9 6 6 6-6"/></svg>`;
  el.setAttribute('aria-label', `Model: ${m?.name || 'Default'}${eff ? ', ' + EFFORT(eff) : ''}. Change`);
  el.hidden = !(x.models || []).length;
}
let menuFor = null;
function openMenu(pill) {
  const x = tools.find(t => t.id === pill.dataset.tool); if (!x) return;
  const menu = $('#mMenu'); menuFor = pill; pill.setAttribute('aria-expanded', 'true');
  const cur = x.model || (x.models[0]?.id ?? ''), efforts = effortsOf(x);
  menu.innerHTML = `<h5>${esc(x.name)} model</h5>${x.models.map(m => `<button role="menuitemradio" aria-checked="${m.id === cur}" data-model="${esc(m.id)}">${esc(m.name)}</button>`).join('')}`
    + (efforts.length ? `<h5>${esc(x.effortLabel || 'Effort')}</h5><div class="eff" role="group">${efforts.map(e => `<button role="menuitemradio" aria-checked="${e === x.effort}" data-effort="${esc(e)}">${esc(EFFORT(e))}</button>`).join('')}</div>` : '')
    + `<p>Used by ${esc(x.name)} from its next turn.</p>`;
  menu.hidden = false;
  const r = pill.getBoundingClientRect(), o = view.getBoundingClientRect(), mh = Math.min(menu.scrollHeight, o.height - 20);
  menu.style.left = Math.max(8, Math.min(r.left - o.left, o.width - 258)) + 'px';
  menu.style.top = Math.max(8, r.top - o.top - mh - 6) + 'px';
  (menu.querySelector('[aria-checked="true"]') || menu.querySelector('button'))?.focus();
}
function closeMenu() { if (!menuFor) return; $('#mMenu').hidden = true; menuFor.setAttribute('aria-expanded', 'false'); menuFor.focus(); menuFor = null; }
for (const id of ['#dModel', '#nModel']) $(id).onclick = e => { e.stopPropagation(); menuFor === $(id) ? closeMenu() : (closeMenu(), openMenu($(id))); };
// The new task's tool access: Trust all never asks; the rest ask more, or change nothing.
function openAccess(pill) {
  const x = tools.find(t => t.id === newTool) || tools[0], menu = $('#mMenu'), now = newAccessOf(x);
  menuFor = pill; pill.setAttribute('aria-expanded', 'true');
  menu.innerHTML = `<h5>${esc(x.name)} may</h5>` + Object.keys(ACCESS).filter(a => a !== 'read' || x.readOnly)
    .map(a => `<button role="menuitemradio" class="acc-opt" aria-checked="${a === now}" data-access="${a}"><span><b>${esc(ACCESS[a][0])}</b><em>${esc(accessNote(a, x.id))}</em></span></button>`).join('')
    + '<p>For this task only. Settings keeps the default.</p>';
  menu.hidden = false;
  const r = pill.getBoundingClientRect(), o = view.getBoundingClientRect(), mh = Math.min(menu.scrollHeight, o.height - 20);
  menu.style.left = Math.max(8, Math.min(r.left - o.left, o.width - 258)) + 'px';
  menu.style.top = Math.max(8, r.top - o.top - mh - 6) + 'px';
  (menu.querySelector('[aria-checked="true"]') || menu.querySelector('button'))?.focus();
}
$('#nAccess').onclick = e => { e.stopPropagation(); menuFor === $('#nAccess') ? closeMenu() : (closeMenu(), openAccess($('#nAccess'))); };
$('#mMenu').addEventListener('click', e => {
  const b = e.target.closest('button'); if (!b || !menuFor) return;
  if (b.dataset.access) { newAccess[newTool] = b.dataset.access; closeMenu(); renderNew(); return; }
  const x = tools.find(t => t.id === menuFor.dataset.tool);
  if (b.dataset.model != null) x.model = b.dataset.model; else if (b.dataset.effort) x.effort = b.dataset.effort;
  host?.postMessage({ type: 'setModel', tool: x.id, model: x.model, effort: x.effort || null });
  const pill = menuFor; renderPill('#' + pill.id, x.id);
  if (b.dataset.model != null) closeMenu(); else openMenu(pill);
});
$('#mMenu').addEventListener('keydown', e => {
  const items = [...$('#mMenu').querySelectorAll('button')], i = items.indexOf(document.activeElement);
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); items[(i + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus(); }
  else if (e.key === 'Escape') { e.stopPropagation(); closeMenu(); }
});
addEventListener('pointerdown', e => { if (menuFor && !e.target.closest('#mMenu,.mpill,#nAccess')) closeMenu(); });

// ── Deleting a session, after asking ────────────────────────────────────
let confirmAction = null;
function askDelete(s) {
  $('#cfTitle').textContent = 'Delete this session?';
  $('#cfText').textContent = `“${s.title}” goes from the office and the history${busy(s) ? ', and its run is stopped' : ''}. This can’t be undone.`;
  confirmAction = () => {
    if (host) host.postMessage({ type: 'delete', ...(s.archived ? { key: s.key } : { id: s.id }) });
    else if (sessions.includes(s)) retire(s);
    history = history.filter(h => h.key !== s.key);
    if (cur() === s) closeDrawer();
    renderPanel();
  };
  $('#confirm').hidden = false; $('#cfNo').focus();
}
function closeConfirm() { $('#confirm').hidden = true; confirmAction = null; }
$('#cfNo').onclick = closeConfirm;
$('#cfYes').onclick = () => { const a = confirmAction; closeConfirm(); a?.(); };
$('#confirm').addEventListener('keydown', e => { if (e.key === 'Escape') { e.stopPropagation(); closeConfirm(); } });

// ── History, on the bookshelf ───────────────────────────────────────────
function openHistory(key) {
  const here = sessions.find(s => s.key === key);
  if (here) return openSession(here.id);
  if (host) host.postMessage({ type: 'history', key });
}
// A saved session, as the chat shows it: no desk, a bot in its tool's colour.
function showTranscript(h) {
  const s = { ...h, archived: true, turns: h.turns.map(t => ({ ...t, t0: t.t0 || Date.now() })) };
  s.b = { name: 'History', css: (TOOLS[s.tool] || TOOLS.kiro)[1] };
  closePanel(true); fold();
  viewing = s; sel = null; drawerOpen = true; view.classList.add('open'); $('#drawer').classList.add('open'); $('#drawer').setAttribute('aria-hidden', 'false');
  renderDrawer();
}
// The menu: time of day, music, the history and Settings.
function closeHud() { $('#hudMenu').hidden = true; $('#menuBtn').setAttribute('aria-expanded', 'false'); }
$('#menuBtn').onclick = e => { e.stopPropagation(); const m = $('#hudMenu'), open = m.hidden; m.hidden = !open; $('#menuBtn').setAttribute('aria-expanded', open); if (open) (m.querySelector('.tseg .on') || m.querySelector('button')).focus(); };
addEventListener('pointerdown', e => { if (!e.target.closest('#hudMenu,#menuBtn')) closeHud(); });
$('#hudMenu').addEventListener('keydown', e => { if (e.key === 'Escape') { e.stopPropagation(); closeHud(); $('#menuBtn').focus(); } });
$('#histBtn').onclick = () => { closeHud(); panel === 'history' ? closePanel() : openPanel('history'); };
$('#setBtn').onclick = () => { closeHud(); if (host) host.postMessage({ type: 'settings' }); else toast('In Hover this opens Settings.'); };
$('#nInput').addEventListener('input', renderNew);
$('#nInput').addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); $('#nGo').click(); } });
$('#nFolder').onclick = () => { if (host) host.postMessage({ type: 'pickFolder', folder: newFolder }); else toast('In Hover this opens a folder picker.'); };
$('#nGo').onclick = () => { const v = $('#nInput').value.trim(); if ($('#nGo').disabled) return; if (doNew(v, newFolder, attached.new.slice())) { attached.new.length = 0; renderPics('new'); $('#nInput').value = ''; fold(); } };
addEventListener('keydown', e => {
  if (e.key === 'Escape') { if (fab !== 'rest') fold(); else if (drawerOpen) closeDrawer(); else if (panel) closePanel(); else host?.postMessage({ type: 'fold' }); return; }
  if (e.target.closest('textarea,input')) return;
  if (e.key === '+' || e.key === '=') zoomBy(1.25); else if (e.key === '-') zoomBy(0.8); else if (e.key === '0') resetView();
});
host?.addEventListener('message', e => {
  const m = e.data; if (!m || typeof m !== 'object') return;
  if (m.type === 'state') fromHost(m);
  else if (m.type === 'toast') toast(m.text);
  else if (m.type === 'transcript') showTranscript(m.session);
  else if (m.type === 'visible') { paused = !m.on; beats.follow(m.on); }
  else if (m.type === 'folder') { newFolder = m.text; renderNew(); $('#nInput').focus(); }
  // The notch's Review on a question: its chat opens here.
  else if (m.type === 'reveal') { if (sessions.some(s => s.id === m.id)) openSession(m.id); }
});

// Chill beats: a CC0 lofi loop (omfgdude, opengameart.org/node/94031), off until
// switched on, remembered, faded in and out, and silent while the page is hidden.
const beats = (() => {
  const btn = $('#beats'); let audio = null, want = localStorage.getItem('office.beats') === 'on', seen = true, fade = 0;
  const ramp = to => { cancelAnimationFrame(fade); const step = () => { audio.volume = Math.max(0, Math.min(0.32, audio.volume + (to > audio.volume ? 0.02 : -0.03))); if (Math.abs(audio.volume - to) > 0.02) fade = requestAnimationFrame(step); else { audio.volume = to; if (!to) audio.pause(); } }; step(); };
  function sync() {
    btn.classList.toggle('on', want); btn.setAttribute('aria-checked', want);
    if (want && seen) {
      if (!audio) { audio = new Audio('office-beats.ogg'); audio.loop = true; audio.volume = 0; audio.preload = 'auto'; }
      audio.play().then(() => ramp(0.32)).catch(() => { want = false; btn.classList.remove('on'); });
    } else if (audio && !audio.paused) ramp(0);
  }
  btn.onclick = () => { want = !want; localStorage.setItem('office.beats', want ? 'on' : 'off'); sync(); };
  // Played only after a click on the page: WebView2, like any browser, won't autoplay.
  addEventListener('pointerdown', () => { if (want && audio?.paused !== false) sync(); }, { once: true });
  return { follow(on) { seen = on; sync(); } };
})();

// Time of day: follows the PC's clock (day from 7 to 19) until the user picks one.
let manualTime = localStorage.getItem('office.time') || '';
function setTime(t) { manualTime = t; if (t) localStorage.setItem('office.time', t); else localStorage.removeItem('office.time'); applyTime(t || autoTime()); shadowDirty = true; poke(); document.querySelectorAll('[data-time]').forEach(b => { b.classList.toggle('on', b.dataset.time === (t || 'auto')); b.setAttribute('aria-checked', b.dataset.time === (t || 'auto')); }); }
const autoTime = () => { const h = new Date().getHours(); return h >= 7 && h < 19 ? 'day' : 'night'; };
document.querySelectorAll('[data-time]').forEach(b => b.onclick = () => setTime(b.dataset.time === 'auto' ? '' : b.dataset.time));
setInterval(() => { if (!manualTime && autoTime() !== time) applyTime(autoTime()); }, 60e3);

// Demo only: force the open (or first) session into a stage, or play a run.
if (!host) {
  const FORCE = [['Waking up', 'waking'], ['Thinking', 'working', 'Thinking'], ['Reading', 'working', 'Reading'], ['Editing', 'working', 'Editing'], ['Running', 'working', 'Running'], ['Done', 'done'], ['Couldn’t finish', 'failed'], ['Stopped', 'stopped']];
  $('#force').insertAdjacentHTML('beforeend', FORCE.map(([l], i) => `<button data-f="${i}">${l}</button>`).join('') + '<button data-play class="play">▶ Play a full run</button>');
  $('#force').addEventListener('click', e => {
    const b = e.target.closest('[data-f],[data-play]'); if (!b) return;
    const s = cur() || sessions[0]; if (!s) return; const T = last(s);
    if (b.hasAttribute('data-play')) { play(s, T); return; }
    const [, stage, act] = FORCE[+b.dataset.f];
    clearTimeout(timers[s.id]); T.stage = stage; T.act = act || null; T.file = T.file || 'src/auth/refresh.ts';
    if (stage === 'waking') { T.t0 = Date.now(); s.b.sinceSeat = 0; }
    T.took ||= 97e3; T.woke ||= 2.2;
    if (stage === 'done' && !T.answer) T.answer = T.final || DONE_TEXT;
    if (stage === 'failed') T.answer = 'I couldn\'t finish. dotnet test failed: 2 tests in AuthTests expect tokens that never expire.';
    if (stage === 'stopped') T.answer = 'Stopped. Nothing after the last step above was changed.';
    changed(s);
  });
}

// ── Frame loop ──────────────────────────────────────────────────────────
// Capped at 30 fps. requestAnimationFrame stops by itself when the page is hidden.
let prev = 0, acc = 0, frames = 0, clockT = 0, tvAt = -1, clockAt = -1, paused = false;
let lively = true, pokedAt = 0, shadowAt = -1, shadowDirty = true;
// Anything the user does (or Hover sends) wakes the room to full speed for a moment.
function poke() { pokedAt = performance.now(); }
for (const e of ['pointermove', 'pointerdown', 'wheel', 'keydown', 'resize', 'message']) addEventListener(e, poke, { passive: true });
host?.addEventListener('message', poke);
const v3 = new THREE.Vector3();
const FOCUS = { board: [X0 + 1.2, 2.1, -0.7, 2.6], tv: [5.2, 1.6, Z0 + 1.6, 1.35], history: [X0 + 1.4, 1.4, -3.5, 1.35] };
function frame(now) {
  requestAnimationFrame(frame);
  // Hover says when the page is out of sight; nothing is drawn until it's back.
  if (paused) { prev = now; return; }
  const dt = Math.min(0.1, (now - prev) / 1000 || 0); prev = now; acc += dt;
  // 30 fps while something happens (a bot walks or works, the camera moves, the
  // pointer is about); otherwise the room only idles, at 10 fps, or not at all when
  // motion is off.
  const calm = !lively && now - pokedAt > 1500;
  if (acc < (calm ? (still ? 1 : 1 / 10) : 1 / 31)) return;
  const step = acc; acc = 0; clockT += step;
  for (const s of sessions) { s.b.sync(last(s).stage, poseOf(last(s))); s.b.step(step); }
  for (const l of leaving) l.b.step(step);
  // The door swings open while a bot is near it.
  const near = [...sessions.map(s => s.b), ...leaving.map(l => l.b)].some(b => Math.hypot(b.x - DOOR.x, b.z - Z0) < 1.5);
  door.rotation.y = ease(door.rotation.y, near ? -1.3 : 0, 6, step);
  // Screen light on each bot's face, by stage.
  deskGlows.forEach((g, i) => { const s = sessions.find(x => x.desk === i && x.b.seated && !x.b.path.length); const [c, o] = s ? SCREEN[last(s).stage] || [0, 0] : [0, 0];
    g.material.color.set(c || 0x000000); g.material.opacity = ease(g.material.opacity, o * (s && last(s).act === 'Running' && Math.sin(clockT * 11) > 0 ? 1.3 : 1), 8, step); });
  if (!still) {
    const a = clockT * 0.21; vac.position.set(0.6 + Math.sin(a) * 3, 0, 4.3 + Math.sin(a * 2.3) * 0.7);
    vac.rotation.y = Math.atan2(Math.cos(a) * 3, Math.cos(a * 2.3) * 0.7 * 2.3);
    steam.forEach((s, i) => { const f = (clockT * 0.5 + i / 3) % 1; s.position.set(-4.3 + Math.sin(f * 6 + i) * 0.04, 1.5 + f * 0.6, Z0 + 0.3); s.material.opacity = 0.3 * (1 - f); s.scale.setScalar(0.15 + f * 0.25); });
    dust.rotation.y = Math.sin(clockT * 0.1) * 0.02; dust.position.y = Math.sin(clockT * 0.4) * 0.05;
    exitGlow.material.opacity = 0.5 + Math.sin(clockT * 2) * 0.05;
  }
  // Camera: where the user put it; or close on the open session's bot, or on the
  // board or the TV, left of the side panel.
  const s = drawerOpen && !viewing && cur(), side = W < 700 ? 0 : Math.min(424, W * 0.42) / 2;
  const aim = (x, y, z, zoom) => { const off = side * 2 * halfWidth(zoom) / W; Object.assign(camTo, { x: x + RIGHT.x * off, y, z: z + RIGHT.z * off, zoom }); };
  if (s) aim(s.b.x, 0.9, s.b.z, W < 700 ? 1.3 : 1.45);
  else if (panel) aim(...FOCUS[panel]);
  else Object.assign(camTo, { x: userView.x, y: 1.7, z: userView.z, zoom: userView.zoom });
  for (const k of ['x', 'y', 'z', 'zoom']) cam[k] = ease(cam[k], camTo[k], still || dragging ? 60 : 5, step);
  placeCamera();
  pick();
  // Tags follow the bots' heads; bubbles type out new text.
  for (const s of sessions) {
    s.b.head3(v3).project(camera);
    s.tag.style.transform = `translate(${((v3.x + 1) / 2 * W).toFixed(1)}px,${((1 - v3.y) / 2 * H).toFixed(1)}px)`;
    const want = bubbleFor(s);
    if (want !== s.tagText) { s.tagText = want; s.tagShown = 0; }
    s.tag.className = `tag st-${last(s).stage}${s.b.hot ? ' hot' : ''}`;
    const n = Math.min(want.length, (s.tagShown += step * 45) | 0);
    const span = s.tag.querySelector('.bub span'), text = want.slice(0, still ? want.length : n);
    if (span.textContent !== text) span.textContent = text;
    s.tag.querySelector('.bub').hidden = !want;
  }
  if ((clockT * 4 | 0) !== tvAt) { tvAt = clockT * 4 | 0; drawTV(clockT); }
  if ((clockT * 2 | 0) !== clockAt) { clockAt = clockT * 2 | 0; drawClock(clockT); }
  // The shadow map is the costly pass: every frame while a bot walks, ten times a
  // second otherwise (a bot's bob, the vacuum), and at once after a change of light.
  const walking = leaving.length > 0 || sessions.some(s => s.b.path.length);
  if (walking || shadowDirty || clockT - shadowAt >= 0.1) { renderer.shadowMap.needsUpdate = true; shadowAt = clockT; shadowDirty = false; }
  renderer.render(scene, camera);
  lively = walking || sessions.some(s => busy(s) || s.b.since < 2 || s.tagShown < (s.tagText?.length ?? 0)) || dragging ||
    Math.abs(door.rotation.y - (near ? -1.3 : 0)) > 0.01 || ['x', 'y', 'z', 'zoom'].some(k => Math.abs(cam[k] - camTo[k]) > 0.002);
  window.FRAMES = ++frames;
}

// ── Start ───────────────────────────────────────────────────────────────
resize(); addEventListener('resize', resize);
for (const s of sessions) spawn(s, !!s.arrive);
setTime(Q.get('time') ?? manualTime);
drawBoard(); drawClock(0); clampView(); renderTools(); renderNew();
if (!host) { play(sessions[0], last(sessions[0]), 3); play(sessions[2], last(sessions[2])); }
if (Q.has('open')) openSession(+Q.get('open'));
if (Q.has('panel')) openPanel(Q.get('panel'));
if (Q.has('f')) document.querySelector(`#force [data-f="${Q.get('f')}"]`)?.click();
document.fonts?.ready.then(() => { drawBoard(); drawClock(clockT); drawTV(clockT); });
requestAnimationFrame(frame);
host?.postMessage({ type: 'ready' });
window.READY = true;
