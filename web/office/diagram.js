// Mermaid flowcharts (graph / flowchart TD, TB, BT, LR, RL) drawn as SVG, with no
// library. Nodes: id, id[box], id(round), id([stadium]), id((circle)), id{diamond},
// id{{hexagon}}, id[[sub]], id[(db)], id>flag]. Edges: -->, ---, -.->, ==>, with
// labels (-->|text| or -- text -->), chains (a --> b --> c) and groups (a & b --> c).
// Subgraphs are drawn flat; styles and classes are ignored. Anything else (sequence,
// class, gantt...) returns null, and the answer shows the diagram's source instead.
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

const SHAPES = [
  ['([', '])', 'stadium'], ['((', '))', 'circle'], ['[[', ']]', 'sub'], ['[(', ')]', 'db'], ['{{', '}}', 'hex'],
  ['[', ']', 'box'], ['(', ')', 'round'], ['{', '}', 'diamond'], ['>', ']', 'flag'],
];
const ARROW = /^\s*(-\.+->|-\.+-|={2,}>|={3,}|-{2,}>|-{3,}|--[ox])\s*(?:\|([^|]*)\|)?\s*/;

function unquote(t) { t = t.trim(); return t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1) : t; }

// One node at the start of s: [id, label, shape, rest] or null.
function node(s, nodes) {
  const m = s.match(/^\s*([A-Za-z0-9_\u00C0-\uFFFF]+)/);
  if (!m) return null;
  const id = m[1];
  let rest = s.slice(m[0].length), label = null, shape = null;
  for (const [a, b, kind] of SHAPES) {
    if (!rest.startsWith(a)) continue;
    const end = rest.indexOf(b, a.length);
    if (end < 0) return null;
    label = unquote(rest.slice(a.length, end)); shape = kind; rest = rest.slice(end + b.length);
    break;
  }
  const n = nodes.get(id) || { id, label: id, shape: 'box' };
  if (label !== null) { n.label = label.replace(/<br\s*\/?>/gi, '\n'); n.shape = shape; }
  nodes.set(id, n);
  return [id, rest];
}

// A group of nodes joined by &.
function group(s, nodes) {
  const ids = [];
  let r = node(s, nodes);
  if (!r) return null;
  ids.push(r[0]); s = r[1];
  while (/^\s*&/.test(s)) { r = node(s.replace(/^\s*&/, ''), nodes); if (!r) return null; ids.push(r[0]); s = r[1]; }
  return [ids, s];
}

export function parse(src) {
  const lines = src.replace(/%%.*$/gm, '').split(/\n|;/).map(l => l.trim()).filter(Boolean);
  const head = lines.shift()?.match(/^(?:graph|flowchart)(?:\s+(TD|TB|BT|LR|RL))?\s*$/i);
  if (!head) return null;
  const dir = (head[1] || 'TD').toUpperCase();
  const nodes = new Map(), edges = [];
  for (const line of lines) {
    if (/^(subgraph|end$|end\s|classDef|class\s|style\s|click\s|linkStyle|direction\s)/.test(line)) {
      const sub = line.match(/^subgraph\s+(.*)$/);
      if (sub && /\[|\(/.test(sub[1])) node(sub[1], new Map());
      continue;
    }
    let g = group(line, nodes);
    if (!g) return null;
    let [from, s] = g;
    while (s.trim()) {
      // -- text --> and -. text .-> put the label inside the arrow.
      let label = null;
      const inText = s.match(/^\s*(--|-\.|==)\s+([^-.=>][^>]*?)\s+(-->|\.->|==>|---)\s*/);
      let a;
      if (inText) { label = inText[2]; a = [inText[0], inText[3]]; }
      else { a = s.match(ARROW); if (!a) return null; label = a[2] ?? null; }
      const kind = a[1].includes('.') ? 'dot' : a[1].includes('=') ? 'thick' : 'line';
      const head = /[>ox]$/.test(a[1]);
      s = s.slice(a[0].length);
      g = group(s, nodes);
      if (!g) return null;
      const [to, rest] = g;
      for (const f of from) for (const t of to) edges.push({ from: f, to: t, label: label && unquote(label), kind, head });
      from = to; s = rest;
    }
  }
  return nodes.size ? { dir, nodes: [...nodes.values()], edges } : null;
}

// Lines of a label, wrapped near 26 characters.
function wrap(text) {
  const out = [];
  for (const para of String(text).split('\n')) {
    let line = '';
    for (const w of para.split(/\s+/)) {
      if (line && (line + ' ' + w).length > 26) { out.push(line); line = w; } else line = line ? line + ' ' + w : w;
    }
    out.push(line);
  }
  return out.slice(0, 6);
}

/// SVG for a flowchart, or null when it isn't one this can read.
export function flowchart(src) {
  let g;
  try { g = parse(src); } catch { return null; }
  if (!g || g.nodes.length > 80) return null;
  const across = g.dir === 'LR' || g.dir === 'RL', flip = g.dir === 'BT' || g.dir === 'RL';
  // Layers: the longest path from the start. A line that goes back (a loop) is left
  // out of the layering, found by walking from the nodes in the order they were written.
  const out = new Map(g.nodes.map(n => [n.id, []]));
  for (const e of g.edges) out.get(e.from).push(e);
  const back = new Set(), seen = new Map();
  const walk = id => {
    seen.set(id, 1);
    for (const e of out.get(id)) {
      if (seen.get(e.to) === 1) back.add(e);
      else if (!seen.has(e.to)) walk(e.to);
    }
    seen.set(id, 2);
  };
  g.nodes.forEach(n => { if (!seen.has(n.id)) walk(n.id); });
  const incoming = new Map(g.nodes.map(n => [n.id, []]));
  for (const e of g.edges) if (!back.has(e) && e.from !== e.to) incoming.get(e.to).push(e.from);
  const rank = new Map();
  const visit = id => {
    if (rank.has(id)) return rank.get(id);
    rank.set(id, 0);
    let r = 0;
    for (const p of incoming.get(id)) r = Math.max(r, visit(p) + 1);
    rank.set(id, r);
    return r;
  };
  g.nodes.forEach(n => visit(n.id));
  const layers = [];
  for (const n of g.nodes) (layers[rank.get(n.id)] ||= []).push(n);
  // Order inside a layer by where their parents sit, so lines cross less.
  const pos = new Map();
  layers.forEach(layer => {
    layer.forEach((n, i) => { const ps = incoming.get(n.id).filter(p => pos.has(p)); n.key = ps.length ? ps.reduce((a, p) => a + pos.get(p), 0) / ps.length : i; });
    layer.sort((a, b) => a.key - b.key).forEach((n, i) => pos.set(n.id, i));
  });
  // Sizes.
  const CH = 6.6, LH = 15;
  for (const n of g.nodes) {
    n.lines = wrap(n.label);
    const w = Math.max(...n.lines.map(l => l.length)) * CH + 24, h = n.lines.length * LH + 16;
    n.w = n.shape === 'diamond' ? Math.max(w * 1.45, 64) : n.shape === 'circle' ? Math.max(w, h) : Math.max(w, 48);
    n.h = n.shape === 'diamond' ? Math.max(h * 1.5, 48) : n.shape === 'circle' ? n.w : h;
  }
  // Place: layers along the flow, nodes side by side across it, each layer centred.
  // Across, an edge's label sits between two layers, so the gap fits the longest.
  const GAP = 26, STEP = across ? Math.max(58, ...g.edges.map(e => e.label ? e.label.length * CH + 34 : 0)) : 58;
  const thick = layers.map(l => Math.max(...l.map(n => across ? n.w : n.h)));
  const spans = layers.map(l => l.reduce((a, n) => a + (across ? n.h : n.w), 0) + GAP * (l.length - 1));
  const width = Math.max(...spans);
  let along = 0;
  layers.forEach((l, k) => {
    let at = (width - spans[k]) / 2;
    for (const n of l) {
      const size = across ? n.h : n.w;
      const c = at + size / 2, a = along + thick[k] / 2;
      [n.x, n.y] = across ? [a, c] : [c, a];
      at += size + GAP;
    }
    along += thick[k] + STEP;
  });
  along -= STEP;
  const W = (across ? along : width) + 16, H = (across ? width : along) + 16;
  if (flip) for (const n of g.nodes) { if (across) n.x = along - n.x; else n.y = along - n.y; }
  const byId = new Map(g.nodes.map(n => [n.id, n]));
  // Where a line leaves or meets a node's edge.
  const edgePoint = (n, dx, dy) => {
    if (n.shape === 'circle') { const r = n.w / 2, d = Math.hypot(dx, dy) || 1; return [n.x + dx / d * r, n.y + dy / d * r]; }
    const hw = n.w / 2, hh = n.h / 2;
    const t = n.shape === 'diamond' ? 1 / (Math.abs(dx) / hw + Math.abs(dy) / hh || 1) : Math.min(hw / Math.abs(dx || 1e-9), hh / Math.abs(dy || 1e-9));
    return [n.x + dx * t, n.y + dy * t];
  };
  const parts = [];
  for (const e of g.edges) {
    const a = byId.get(e.from), b = byId.get(e.to);
    if (a === b) continue;
    const [x1, y1] = edgePoint(a, b.x - a.x, b.y - a.y), [x2, y2] = edgePoint(b, a.x - b.x, a.y - b.y);
    const mx = (x1 + x2) / 2, my = (y1 + y2) / 2;
    const path = across ? `M${x1} ${y1}C${mx} ${y1} ${mx} ${y2} ${x2} ${y2}` : `M${x1} ${y1}C${x1} ${my} ${x2} ${my} ${x2} ${y2}`;
    parts.push(`<path class="e ${e.kind}" d="${path}"${e.head ? ' marker-end="url(#ah)"' : ''}/>`);
    if (e.label) {
      const w = e.label.length * CH + 10;
      parts.push(`<g class="el"><rect x="${mx - w / 2}" y="${my - 9}" width="${w}" height="18" rx="4"/><text x="${mx}" y="${my + 4}">${esc(e.label)}</text></g>`);
    }
  }
  for (const n of g.nodes) {
    const { x, y, w, h } = n, l = x - w / 2, t = y - h / 2;
    const shape = {
      diamond: `<path d="M${x} ${t}L${x + w / 2} ${y}L${x} ${t + h}L${l} ${y}Z"/>`,
      circle: `<circle cx="${x}" cy="${y}" r="${w / 2}"/>`,
      hex: `<path d="M${l + 12} ${t}H${l + w - 12}L${l + w} ${y}L${l + w - 12} ${t + h}H${l + 12}L${l} ${y}Z"/>`,
      flag: `<path d="M${l} ${t}H${l + w}V${t + h}H${l}L${l + 12} ${y}Z"/>`,
      stadium: `<rect x="${l}" y="${t}" width="${w}" height="${h}" rx="${h / 2}"/>`,
      round: `<rect x="${l}" y="${t}" width="${w}" height="${h}" rx="10"/>`,
      db: `<rect x="${l}" y="${t}" width="${w}" height="${h}" rx="${Math.min(w / 2, 14)}" ry="8"/>`,
      sub: `<rect x="${l}" y="${t}" width="${w}" height="${h}" rx="3"/><path d="M${l + 7} ${t}V${t + h}M${l + w - 7} ${t}V${t + h}"/>`,
    }[n.shape] || `<rect x="${l}" y="${t}" width="${w}" height="${h}" rx="5"/>`;
    const text = n.lines.map((s, k) => `<tspan x="${x}" y="${y + (k - (n.lines.length - 1) / 2) * LH + 4}">${esc(s)}</tspan>`).join('');
    parts.push(`<g class="n ${n.shape}">${shape}<text>${text}</text></g>`);
  }
  return `<svg class="flow" viewBox="-8 -8 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="Diagram"><defs><marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10Z"/></marker></defs>${parts.join('')}</svg>`;
}
