//! `web/office/diagram.js`, ported line for line: Mermaid flowcharts drawn as the same
//! SVG, or `None` for anything it can't read (the answer then shows the source as code).
//! The golden tests in `tests/golden.rs` compare the output with the JavaScript's own.

pub mod js;

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use fancy_regex::Regex;
use js::{esc, num, re, trim};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape { Box, Round, Stadium, Circle, Sub, Db, Hex, Diamond, Flag }

impl Shape {
    fn class(self) -> &'static str {
        match self {
            Shape::Box => "box", Shape::Round => "round", Shape::Stadium => "stadium", Shape::Circle => "circle",
            Shape::Sub => "sub", Shape::Db => "db", Shape::Hex => "hex", Shape::Diamond => "diamond", Shape::Flag => "flag",
        }
    }
}

const SHAPES: [(&str, &str, Shape); 9] = [
    ("([", "])", Shape::Stadium), ("((", "))", Shape::Circle), ("[[", "]]", Shape::Sub), ("[(", ")]", Shape::Db), ("{{", "}}", Shape::Hex),
    ("[", "]", Shape::Box), ("(", ")", Shape::Round), ("{", "}", Shape::Diamond), (">", "]", Shape::Flag),
];

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
    lines: Vec<String>,
    key: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Empty when there is none (the JS treats "" and null alike).
    pub label: String,
    pub kind: &'static str,
    pub head: bool,
}

#[derive(Clone, Debug)]
pub struct Graph {
    pub dir: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// The SVG's width and height (viewBox size), once laid out.
    pub size: (f64, f64),
}

static ID: LazyLock<Regex> = LazyLock::new(|| re(r"^[{S}]*([A-Za-z0-9_\x{C0}-\x{10FFFF}]+)"));
static AMP: LazyLock<Regex> = LazyLock::new(|| re(r"^[{S}]*&"));
static ARROW: LazyLock<Regex> = LazyLock::new(|| re(r"^[{S}]*(-\.+->|-\.+-|={2,}>|={3,}|-{2,}>|-{3,}|--[ox])[{S}]*(?:\|([^|]*)\|)?[{S}]*"));
static IN_TEXT: LazyLock<Regex> = LazyLock::new(|| re(r"^[{S}]*(--|-\.|==)[{S}]+([^-.=>][^>]*?)[{S}]+(-->|\.->|==>|---)[{S}]*"));
static HEAD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:graph|flowchart)(?:[{S}]+(TD|TB|BT|LR|RL))?[{S}]*$"));
static SKIP: LazyLock<Regex> = LazyLock::new(|| re(r"^(subgraph|end$|end[{S}]|classDef|class[{S}]|style[{S}]|click[{S}]|linkStyle|direction[{S}])"));
static COMMENT: LazyLock<Regex> = LazyLock::new(|| re(r"%%{DOT}*"));
static BR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<br[{S}]*/?>"));

fn unquote(t: &str) -> String {
    let t = trim(t);
    if t.starts_with('"') && t.ends_with('"') {
        if t.len() == 1 { String::new() } else { t[1..t.len() - 1].to_string() }
    } else {
        t.to_string()
    }
}

struct Nodes {
    list: Vec<Node>,
    index: HashMap<String, usize>,
}

// One node at the start of s: its index and the rest, or None.
fn node<'a>(s: &'a str, nodes: &mut Nodes) -> Option<(usize, &'a str)> {
    let m = ID.captures(s).ok()??;
    let id = m.get(1).unwrap().as_str().to_string();
    let mut rest = &s[m.get(0).unwrap().end()..];
    let mut found: Option<(String, Shape)> = None;
    for (a, b, kind) in SHAPES {
        if !rest.starts_with(a) {
            continue;
        }
        let end = rest[a.len()..].find(b)? + a.len();
        found = Some((unquote(&rest[a.len()..end]), kind));
        rest = &rest[end + b.len()..];
        break;
    }
    let i = match nodes.index.get(&id) {
        Some(&i) => i,
        None => {
            nodes.list.push(Node { id: id.clone(), label: id.clone(), shape: Shape::Box, lines: vec![], key: 0.0, x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
            nodes.index.insert(id, nodes.list.len() - 1);
            nodes.list.len() - 1
        }
    };
    if let Some((label, shape)) = found {
        nodes.list[i].label = BR.replace_all(&label, "\n").into_owned();
        nodes.list[i].shape = shape;
    }
    Some((i, rest))
}

// A group of nodes joined by &.
fn group<'a>(s: &'a str, nodes: &mut Nodes) -> Option<(Vec<usize>, &'a str)> {
    let (first, mut s) = node(s, nodes)?;
    let mut ids = vec![first];
    while let Ok(Some(m)) = AMP.find(s) {
        let (i, rest) = node(&s[m.end()..], nodes)?;
        ids.push(i);
        s = rest;
    }
    Some((ids, s))
}

pub fn parse(src: &str) -> Option<Graph> {
    let cleaned = COMMENT.replace_all(src, "");
    let mut lines = cleaned.split(['\n', ';']).map(trim).filter(|l| !l.is_empty());
    let head = HEAD.captures(lines.next()?).ok()??;
    let dir = head.get(1).map_or("TD".to_string(), |m| m.as_str().to_uppercase());
    let mut nodes = Nodes { list: vec![], index: HashMap::new() };
    let mut edges = vec![];
    for line in lines {
        if SKIP.is_match(line).unwrap_or(false) {
            // A subgraph's own [label] node is read into a throwaway map in the JS: nothing kept.
            continue;
        }
        let (mut from, mut s) = group(line, &mut nodes)?;
        while !trim(s).is_empty() {
            let (label, arrow, taken) = if let Ok(Some(m)) = IN_TEXT.captures(s) {
                (m.get(2).unwrap().as_str().to_string(), m.get(3).unwrap().as_str().to_string(), m.get(0).unwrap().end())
            } else {
                let m = ARROW.captures(s).ok()??;
                (m.get(2).map_or(String::new(), |g| g.as_str().to_string()), m.get(1).unwrap().as_str().to_string(), m.get(0).unwrap().end())
            };
            let kind = if arrow.contains('.') { "dot" } else if arrow.contains('=') { "thick" } else { "line" };
            let head = arrow.ends_with(['>', 'o', 'x']);
            s = &s[taken..];
            let (to, rest) = group(s, &mut nodes)?;
            let label = if label.is_empty() { label } else { unquote(&label) };
            for &f in &from {
                for &t in &to {
                    edges.push(Edge { from: f, to: t, label: label.clone(), kind, head });
                }
            }
            from = to;
            s = rest;
        }
    }
    if nodes.list.is_empty() { None } else { Some(Graph { dir, nodes: nodes.list, edges, size: (0.0, 0.0) }) }
}

// JS `str.split(/\s+/)`: runs of whitespace, keeping the empty ends.
fn split_ws(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    let mut in_ws = false;
    let mut ws_start = 0;
    for (i, c) in s.char_indices() {
        if js::is_ws(c) {
            if !in_ws { in_ws = true; ws_start = i; }
        } else if in_ws {
            out.push(&s[start..ws_start]);
            start = i;
            in_ws = false;
        }
    }
    out.push(if in_ws { &s[start..ws_start] } else { &s[start..] });
    if in_ws { out.push(""); }
    out
}

// Lines of a label, wrapped near 26 characters.
fn wrap(text: &str) -> Vec<String> {
    let mut out = vec![];
    for para in text.split('\n') {
        let mut line = String::new();
        for w in split_ws(para) {
            if !line.is_empty() && js::len(&line) + 1 + js::len(w) > 26 {
                out.push(std::mem::replace(&mut line, w.to_string()));
            } else if line.is_empty() {
                line = w.to_string();
            } else {
                line = format!("{line} {w}");
            }
        }
        out.push(line);
    }
    out.truncate(6);
    out
}

/// V8's Math.hypot (a scaled, compensated sum), so edge points land on the same bits.
fn hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut comp) = (0.0f64, 0.0f64);
    for x in [a, b] {
        let n = x / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

fn max_of(it: impl IntoIterator<Item = f64>) -> f64 {
    it.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

/// The flowchart laid out (positions filled in), or None as `flowchart()` returns null.
pub fn layout(src: &str) -> Option<Graph> {
    let mut g = parse(src)?;
    if g.nodes.len() > 80 {
        return None;
    }
    let across = g.dir == "LR" || g.dir == "RL";
    let flip = g.dir == "BT" || g.dir == "RL";
    let n = g.nodes.len();
    // Layers: the longest path from the start, loops set aside.
    let mut out: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, e) in g.edges.iter().enumerate() {
        out[e.from].push(i);
    }
    let mut back = HashSet::new();
    let mut seen = vec![0u8; n];
    fn walk(id: usize, g: &Graph, out: &[Vec<usize>], seen: &mut [u8], back: &mut HashSet<usize>) {
        seen[id] = 1;
        for &e in &out[id] {
            let to = g.edges[e].to;
            if seen[to] == 1 { back.insert(e); } else if seen[to] == 0 { walk(to, g, out, seen, back); }
        }
        seen[id] = 2;
    }
    for i in 0..n {
        if seen[i] == 0 { walk(i, &g, &out, &mut seen, &mut back); }
    }
    let mut incoming: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, e) in g.edges.iter().enumerate() {
        if !back.contains(&i) && e.from != e.to { incoming[e.to].push(e.from); }
    }
    let mut rank: Vec<Option<usize>> = vec![None; n];
    fn visit(id: usize, incoming: &[Vec<usize>], rank: &mut [Option<usize>]) -> usize {
        if let Some(r) = rank[id] { return r; }
        rank[id] = Some(0);
        let mut r = 0;
        for &p in &incoming[id] { r = r.max(visit(p, incoming, rank) + 1); }
        rank[id] = Some(r);
        r
    }
    for i in 0..n { visit(i, &incoming, &mut rank); }
    let mut layers: Vec<Vec<usize>> = vec![];
    for i in 0..n {
        let r = rank[i].unwrap();
        if layers.len() <= r { layers.resize(r + 1, vec![]); }
        layers[r].push(i);
    }
    // Order inside a layer by where the parents sit, so lines cross less.
    let mut pos: Vec<Option<usize>> = vec![None; n];
    for layer in layers.iter_mut() {
        for (i, &id) in layer.iter().enumerate() {
            let ps: Vec<usize> = incoming[id].iter().copied().filter(|&p| pos[p].is_some()).collect();
            g.nodes[id].key = if ps.is_empty() { i as f64 } else { ps.iter().fold(0.0, |a, &p| a + pos[p].unwrap() as f64) / ps.len() as f64 };
        }
        layer.sort_by(|&a, &b| g.nodes[a].key.partial_cmp(&g.nodes[b].key).unwrap());
        for (i, &id) in layer.iter().enumerate() { pos[id] = Some(i); }
    }
    const CH: f64 = 6.6;
    const LH: f64 = 15.0;
    for nd in g.nodes.iter_mut() {
        nd.lines = wrap(&nd.label);
        let w = max_of(nd.lines.iter().map(|l| js::len(l) as f64)) * CH + 24.0;
        let h = nd.lines.len() as f64 * LH + 16.0;
        nd.w = match nd.shape { Shape::Diamond => (w * 1.45).max(64.0), Shape::Circle => w.max(h), _ => w.max(48.0) };
        nd.h = match nd.shape { Shape::Diamond => (h * 1.5).max(48.0), Shape::Circle => nd.w, _ => h };
    }
    const GAP: f64 = 26.0;
    let step = if across {
        max_of(std::iter::once(58.0).chain(g.edges.iter().map(|e| if e.label.is_empty() { 0.0 } else { js::len(&e.label) as f64 * CH + 34.0 })))
    } else { 58.0 };
    let thick: Vec<f64> = layers.iter().map(|l| max_of(l.iter().map(|&i| if across { g.nodes[i].w } else { g.nodes[i].h }))).collect();
    let spans: Vec<f64> = layers.iter().map(|l| l.iter().fold(0.0, |a, &i| a + if across { g.nodes[i].h } else { g.nodes[i].w }) + GAP * (l.len() as f64 - 1.0)).collect();
    let width = max_of(spans.iter().copied());
    let mut along = 0.0;
    for (k, l) in layers.iter().enumerate() {
        let mut at = (width - spans[k]) / 2.0;
        for &i in l {
            let nd = &mut g.nodes[i];
            let size = if across { nd.h } else { nd.w };
            let c = at + size / 2.0;
            let a = along + thick[k] / 2.0;
            (nd.x, nd.y) = if across { (a, c) } else { (c, a) };
            at += size + GAP;
        }
        along += thick[k] + step;
    }
    along -= step;
    if flip {
        for nd in g.nodes.iter_mut() {
            if across { nd.x = along - nd.x } else { nd.y = along - nd.y }
        }
    }
    g.size = ((if across { along } else { width }) + 16.0, (if across { width } else { along }) + 16.0);
    Some(g)
}

// Where a line leaves or meets a node's edge.
fn edge_point(n: &Node, dx: f64, dy: f64) -> (f64, f64) {
    if n.shape == Shape::Circle {
        let r = n.w / 2.0;
        let d = hypot(dx, dy);
        let d = if d == 0.0 { 1.0 } else { d };
        return (n.x + dx / d * r, n.y + dy / d * r);
    }
    let (hw, hh) = (n.w / 2.0, n.h / 2.0);
    let or = |v: f64| if v == 0.0 { 1e-9 } else { v };
    let t = if n.shape == Shape::Diamond {
        let s = dx.abs() / hw + dy.abs() / hh;
        1.0 / if s == 0.0 || s.is_nan() { 1.0 } else { s }
    } else {
        (hw / or(dx).abs()).min(hh / or(dy).abs())
    };
    (n.x + dx * t, n.y + dy * t)
}

/// SVG for a flowchart, or None when it isn't one this can read.
pub fn flowchart(src: &str) -> Option<String> {
    let g = layout(src)?;
    let (w_all, h_all) = (num(g.size.0), num(g.size.1));
    let across = g.dir == "LR" || g.dir == "RL";
    const CH: f64 = 6.6;
    const LH: f64 = 15.0;
    let mut parts = String::new();
    for e in &g.edges {
        if e.from == e.to {
            continue;
        }
        let (a, b) = (&g.nodes[e.from], &g.nodes[e.to]);
        let (x1, y1) = edge_point(a, b.x - a.x, b.y - a.y);
        let (x2, y2) = edge_point(b, a.x - b.x, a.y - b.y);
        let (mx, my) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
        let path = if across {
            format!("M{} {}C{} {} {} {} {} {}", num(x1), num(y1), num(mx), num(y1), num(mx), num(y2), num(x2), num(y2))
        } else {
            format!("M{} {}C{} {} {} {} {} {}", num(x1), num(y1), num(x1), num(my), num(x2), num(my), num(x2), num(y2))
        };
        parts += &format!("<path class=\"e {}\" d=\"{path}\"{}/>", e.kind, if e.head { " marker-end=\"url(#ah)\"" } else { "" });
        if !e.label.is_empty() {
            let w = js::len(&e.label) as f64 * CH + 10.0;
            parts += &format!("<g class=\"el\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"18\" rx=\"4\"/><text x=\"{}\" y=\"{}\">{}</text></g>",
                num(mx - w / 2.0), num(my - 9.0), num(w), num(mx), num(my + 4.0), esc(&e.label));
        }
    }
    for n in &g.nodes {
        let (x, y, w, h) = (n.x, n.y, n.w, n.h);
        let (l, t) = (x - w / 2.0, y - h / 2.0);
        let shape = match n.shape {
            Shape::Diamond => format!("<path d=\"M{} {}L{} {}L{} {}L{} {}Z\"/>", num(x), num(t), num(x + w / 2.0), num(y), num(x), num(t + h), num(l), num(y)),
            Shape::Circle => format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>", num(x), num(y), num(w / 2.0)),
            Shape::Hex => format!("<path d=\"M{} {}H{}L{} {}L{} {}H{}L{} {}Z\"/>", num(l + 12.0), num(t), num(l + w - 12.0), num(l + w), num(y), num(l + w - 12.0), num(t + h), num(l + 12.0), num(l), num(y)),
            Shape::Flag => format!("<path d=\"M{} {}H{}V{}H{}L{} {}Z\"/>", num(l), num(t), num(l + w), num(t + h), num(l), num(l + 12.0), num(y)),
            Shape::Stadium => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>", num(l), num(t), num(w), num(h), num(h / 2.0)),
            Shape::Round => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"10\"/>", num(l), num(t), num(w), num(h)),
            Shape::Db => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" ry=\"8\"/>", num(l), num(t), num(w), num(h), num((w / 2.0).min(14.0))),
            Shape::Sub => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"3\"/><path d=\"M{} {}V{}M{} {}V{}\"/>", num(l), num(t), num(w), num(h), num(l + 7.0), num(t), num(t + h), num(l + w - 7.0), num(t), num(t + h)),
            Shape::Box => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"5\"/>", num(l), num(t), num(w), num(h)),
        };
        let count = n.lines.len() as f64;
        let text: String = n.lines.iter().enumerate()
            .map(|(k, s)| format!("<tspan x=\"{}\" y=\"{}\">{}</tspan>", num(x), num(y + (k as f64 - (count - 1.0) / 2.0) * LH + 4.0), esc(s)))
            .collect();
        parts += &format!("<g class=\"n {}\">{shape}<text>{text}</text></g>", n.shape.class());
    }
    Some(format!("<svg class=\"flow\" viewBox=\"-8 -8 {w_all} {h_all}\" width=\"{w_all}\" height=\"{h_all}\" role=\"img\" aria-label=\"Diagram\"><defs><marker id=\"ah\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0 0L10 5L0 10Z\"/></marker></defs>{parts}</svg>"))
}
