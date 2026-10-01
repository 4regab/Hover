//! Paints the visible part of a thread into a pixel buffer: shapes, selection, glyphs
//! (swash outlines, unhinted like Chromium's DirectWrite path), decorations, images and
//! diagrams (resvg with page.html's `.flow` rules). Only the viewport is painted, and
//! glyph masks, decoded images and rasterised diagrams are cached, so a scroll or a
//! streamed chunk costs one viewport of drawing.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parley::PositionedLayoutItem;
use resvg::tiny_skia::{self, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use resvg::usvg;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};

use crate::doc::{Shape, TextBox, Thread};
use crate::scroll::{Bar, BarId, THICK};
use crate::theme::{self, Rgba};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    size: u32,
    sub: u8,
    skew: bool,
    bold: bool,
}

struct Mask {
    left: i32,
    top: i32,
    w: u32,
    h: u32,
    data: Vec<u8>,
}

pub struct Painter {
    scaler: ScaleContext,
    glyphs: HashMap<GlyphKey, Option<Mask>>,
    /// Shared with the thread (it lays images out by their size).
    pub images: crate::images::Shared,
    /// Chromium's broken-image icon at 100 % and 200 %.
    broken: [Pixmap; 2],
    /// Rasterised SVGs by their text and drawn width. Keyed by the text, not the Rc's
    /// address: a laid-out-again thread frees its strings and the next can land at the
    /// same address, which drew one step's icon for another (a run step with an edit's
    /// pencil) and added entries at every layout.
    svgs: HashMap<(Rc<str>, u32), Option<Pixmap>>,
    /// The painter's own fonts, and the database made from them and the system's the
    /// first time a flowchart is drawn (most chats have none).
    font_files: Vec<Vec<u8>>,
    fontdb: Option<Arc<usvg::fontdb::Database>>,
    /// Painted frames, for the benchmark.
    pub frames: u64,
    /// Seconds, for the live step's shimmer (a 2 s loop, as `@keyframes flow`).
    pub time: f32,
    /// The scrollbar whose thumb is under the pointer (it darkens).
    pub hover: Option<BarId>,
}

/// The system's fonts, looked up once per process. load_system_fonts reads every font
/// file there is to list its faces (over 130 files, some tens of MB each, on Windows):
/// done for every chat opened, that was hundreds of MB read and a peak the size of the
/// largest file each time.
fn system_fonts() -> &'static usvg::fontdb::Database {
    static DB: std::sync::OnceLock<usvg::fontdb::Database> = std::sync::OnceLock::new();
    DB.get_or_init(|| { let mut db = usvg::fontdb::Database::new(); db.load_system_fonts(); db })
}

// page.html's `.flow` rules, with its CSS variables resolved (usvg reads no `:not()`).
const FLOW_CSS: &str = ".flow{font-family:Inter,'Segoe UI',sans-serif;font-size:12px}\
.n>rect,.n>path,.n>circle{fill:rgba(144,70,255,.18);stroke:#C4A2FF;stroke-width:1.2}\
.n.diamond>path{fill:rgba(255,154,74,.14);stroke:#ffb36b}\
text{fill:#f6f2ff;text-anchor:middle}.e{fill:none;stroke:rgba(246,242,255,.62);stroke-width:1.4}\
.e.dot{stroke-dasharray:4 3}.e.thick{stroke-width:2.6}marker path{fill:rgba(246,242,255,.62)}\
.el rect{fill:#1a1220;stroke:rgba(255,255,255,.09)}.el text{fill:rgba(246,242,255,.62);font-size:11px}";

impl Painter {
    pub fn new(font_files: &[Vec<u8>], images: crate::images::Shared) -> Self {
        let icon = |b: &[u8]| Pixmap::decode_png(b).expect("broken_image.png");
        let broken = [icon(include_bytes!("../assets/broken_image_100.png")), icon(include_bytes!("../assets/broken_image_200.png"))];
        Painter { scaler: ScaleContext::new(), glyphs: HashMap::new(), images, broken, svgs: HashMap::new(), font_files: font_files.to_vec(), fontdb: None, frames: 0, time: 0.0, hover: None }
    }

    /// The fonts a flowchart's text is drawn with: the painter's own, then the system's,
    /// made the first time one is drawn.
    fn fontdb(&mut self) -> Arc<usvg::fontdb::Database> {
        if let Some(db) = &self.fontdb { return db.clone(); }
        let mut db = usvg::fontdb::Database::new();
        for f in &self.font_files { db.load_font_data(f.clone()); }
        for face in system_fonts().faces() { db.push_face_info(face.clone()); }
        // `sans-serif` as the browser resolves it on each system.
        for fam in ["Segoe UI", "Noto Sans", "DejaVu Sans"] {
            if db.faces().any(|f| f.families.iter().any(|(n, _)| n == fam)) {
                db.set_sans_serif_family(fam);
                break;
            }
        }
        let db = Arc::new(db);
        self.fontdb = Some(db.clone());
        db
    }

    /// Paints `th` from `scroll` (thread px) into a w x h device-pixel buffer.
    pub fn paint(&mut self, th: &Thread, scroll: f32, w: u32, h: u32, scale: f32, bg: Rgba) -> Pixmap {
        self.frames += 1;
        let mut px = Pixmap::new(w.max(1), h.max(1)).unwrap();
        px.fill(color(bg));
        let view = (scroll, scroll + h as f32 / scale);
        let ox = theme::THREAD_PAD[3];
        // .ans.fresh: @keyframes rise { from { opacity: 0; transform: translateY(4px) } },
        // .35s ease-out. The answer is drawn on a layer, then put down faded and lowered.
        let fade = th.fresh.and_then(|(si, t0)| {
            let p = (self.time - t0) / 0.35;
            (p < 1.0 && th.sections.get(si).is_some_and(|s| s.answer_at.is_some())).then(|| (si, crate::scroll::ease_out(p.max(0.0))))
        });
        let mut layer = fade.map(|_| Pixmap::new(px.width(), px.height()).unwrap());
        for (si, s) in th.sections.iter().enumerate() {
            if s.y + s.h + 10.0 < view.0 || s.y - 10.0 > view.1 { continue; }
            let dy = s.y - scroll;
            let (t_at, s_at, k_at) = match (fade, s.answer_at) { (Some((f, _)), Some(a)) if f == si => a, _ => (usize::MAX, usize::MAX, usize::MAX) };
            for (i, sh) in s.frag.shapes.iter().enumerate() {
                let to = if i >= s_at { layer.as_mut().unwrap() } else { &mut px };
                self.shape(to, sh, ox, dy, scale);
            }
            // What scrolls sideways with a box, cut to it (only square fills do).
            for (k, sc) in s.frag.scrollers.iter().enumerate() {
                let to = if k >= k_at { layer.as_mut().unwrap() } else { &mut px };
                let off = th.hscroll.get(&(si, k)).copied().unwrap_or(0.0);
                let [cx, _, cw, _] = sc.clip;
                for sh in &sc.shapes {
                    if let Shape::Rect { x, y, w, h, fill: Some(f), .. } = sh {
                        let (a, b) = ((x - off).max(cx), (x - off + w).min(cx + cw));
                        if b > a { fill_rect(to, (ox + a) * scale, (dy + y) * scale, (b - a) * scale, h * scale, 0.0, *f); }
                    }
                }
            }
            for (ti, t) in s.frag.texts.iter().enumerate() {
                let top = s.y + t.y;
                if top > view.1 || top + t.layout.height() < view.0 { continue; }
                let to = if ti >= t_at { layer.as_mut().unwrap() } else { &mut px };
                let off = th.offset(si, t);
                for (x0, y0, x1, y1) in th.selection_rects(si, ti) {
                    let (mut a, mut b) = (ox + t.x - off + x0, ox + t.x - off + x1);
                    if let Some(c) = t.clip { a = a.max(ox + c[0]); b = b.min(ox + c[0] + c[2]); }
                    if b > a { fill_rect(to, a * scale, (dy + t.y + y0) * scale, (b - a) * scale, (y1 - y0) * scale, 0.0, theme::SELECTION); }
                }
                self.text(to, t, ox + t.x - off, dy + t.y, scale, off);
            }
        }
        // The boxes' own scrollbars, inside their rounded bottom corners.
        for (id, b) in th.hbars() {
            if b.y > view.1 || b.y + THICK < view.0 { continue; }
            let to = match (fade, &mut layer) { (Some((f, _)), Some(l)) if f == id.0 => l, _ => &mut px };
            self.bar(to, &Bar { y: b.y - scroll, ..b }, scale, self.hover == Some(BarId::Box(id.0, id.1)), [0.0, 0.0, 9.0, 9.0]);
        }
        if let (Some((_, e)), Some(l)) = (fade, layer) {
            let paint = tiny_skia::PixmapPaint { opacity: e, quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
            px.draw_pixmap(0, 0, l.as_ref(), &paint, Transform::from_translate(0.0, 4.0 * (1.0 - e) * scale), None);
        }
        px
    }

    /// Whether a frame drawn now would differ from the last because of time alone (the
    /// fresh answer's fade).
    pub fn fading(&self, th: &Thread) -> bool {
        th.fresh.is_some_and(|(_, t0)| self.time - t0 < 0.35)
    }

    /// Draws a scrollbar (in CSS px of the buffer): the track, the arrow buttons and the
    /// 6 px thumb. `radius` rounds the track's corners (top left, top right, bottom
    /// right, bottom left) where the box's border does.
    pub fn bar(&mut self, px: &mut Pixmap, b: &Bar, k: f32, hover: bool, radius: [f32; 4]) {
        let (w, h) = if b.vertical { (THICK, b.len) } else { (b.len, THICK) };
        if let Some(path) = rounded(b.x * k, b.y * k, w * k, h * k, radius.map(|r| r * k)) {
            let mut p = Paint::default();
            p.set_color(color(theme::SCROLL_TRACK));
            p.anti_alias = true;
            px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
        // Along the bar a, across it c, to the buffer's x, y.
        let at = |a: f32, c: f32| if b.vertical { ((b.x + c) * k, (b.y + a) * k) } else { ((b.x + a) * k, (b.y + c) * k) };
        let mut arrow = |tip: f32, base: f32| {
            let mut pb = PathBuilder::new();
            let (x, y) = at(tip, 5.0);
            pb.move_to(x, y);
            let (x, y) = at(base, 2.0);
            pb.line_to(x, y);
            let (x, y) = at(base, 8.0);
            pb.line_to(x, y);
            pb.close();
            if let Some(path) = pb.finish() {
                let mut p = Paint::default();
                p.set_color(color(theme::SCROLL_THUMB));
                p.anti_alias = true;
                px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
            }
        };
        arrow(3.5, 7.0);
        arrow(b.len - 3.5, b.len - 7.0);
        let (t0, tl) = b.thumb();
        let (x, y) = at(t0, 2.0);
        let (tw, th) = if b.vertical { (6.0, tl) } else { (tl, 6.0) };
        fill_rect(px, x, y, tw * k, th * k, 3.0 * k, if hover { theme::SCROLL_THUMB_HOVER } else { theme::SCROLL_THUMB });
    }

    fn shape(&mut self, px: &mut Pixmap, sh: &Shape, ox: f32, dy: f32, k: f32) {
        match sh {
            Shape::Rect { x, y, w, h, radius, fill, stroke } => {
                let (x, y, w, h) = ((ox + x) * k, (dy + y) * k, w * k, h * k);
                let r = radius.map(|r| r * k);
                if let Some(f) = fill {
                    if let Some(path) = rounded(x, y, w, h, r) {
                        let mut p = Paint::default();
                        p.set_color(color(*f));
                        p.anti_alias = true;
                        px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
                    }
                }
                if let Some((c, sw)) = stroke {
                    let sw = sw * k;
                    if let Some(path) = rounded(x + sw / 2.0, y + sw / 2.0, w - sw, h - sw, r.map(|r| (r - sw / 2.0).max(0.0))) {
                        let mut p = Paint::default();
                        p.set_color(color(*c));
                        p.anti_alias = true;
                        px.stroke_path(&path, &p, &Stroke { width: sw, ..Default::default() }, Transform::identity(), None);
                    }
                }
            }
            Shape::Glow { x, y, r, color: c } => {
                let (cx, cy, r) = ((ox + x) * k, (dy + y) * k, r * k);
                let mut p = Paint::default();
                let mut c0 = *c;
                c0[3] = 150;
                let mut c1 = *c;
                c1[3] = 0;
                p.shader = tiny_skia::RadialGradient::new(tiny_skia::Point::from_xy(cx, cy), 0.0, tiny_skia::Point::from_xy(cx, cy), r,
                    vec![tiny_skia::GradientStop::new(0.0, color(c0)), tiny_skia::GradientStop::new(1.0, color(c1))],
                    tiny_skia::SpreadMode::Pad, Transform::identity()).unwrap_or(tiny_skia::Shader::SolidColor(color(c1)));
                if let Some(rect) = tiny_skia::Rect::from_xywh(cx - r, cy - r, r * 2.0, r * 2.0) {
                    px.fill_rect(rect, &p, Transform::identity(), None);
                }
            }
            Shape::Line { pts, color: c, width } => {
                let mut pb = PathBuilder::new();
                for (i, (px_, py_)) in pts.iter().enumerate() {
                    let (a, b) = ((ox + px_) * k, (dy + py_) * k);
                    if i == 0 { pb.move_to(a, b) } else { pb.line_to(a, b) }
                }
                if let Some(path) = pb.finish() {
                    let mut p = Paint::default();
                    p.set_color(color(*c));
                    p.anti_alias = true;
                    px.stroke_path(&path, &p, &Stroke { width: width * k, ..Default::default() }, Transform::identity(), None);
                }
            }
            Shape::Broken { x, y } => {
                // 14 x 16 at the top left of the box; the 200 % bitmap from 150 % up, as Chromium picks.
                let b = &self.broken[(k >= 1.5) as usize];
                let s = k * 14.0 / b.width() as f32;
                let (tx, ty) = (((ox + x) * k).round(), ((dy + y) * k).round());
                px.draw_pixmap(0, 0, b.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() },
                    Transform::from_row(s, 0.0, 0.0, s, tx, ty), None);
            }
            Shape::Image { x, y, w, h, radius, src, cover } => {
                let (x, y, w, h, r) = ((ox + x) * k, (dy + y) * k, w * k, h * k, radius * k);
                let mut images = self.images.borrow_mut();
                let Some(img) = images.pixmap(src) else {
                    // .md img { background: rgba(255,255,255,.04) } while it loads, or broken.
                    fill_rect(px, x, y, w, h, r, [255, 255, 255, 10]);
                    return;
                };
                let (iw, ih) = (img.width() as f32, img.height() as f32);
                // object-fit: cover scales to fill and centres; otherwise the box is the image's size.
                let (sx, sy) = if *cover { let s = (w / iw).max(h / ih); (s, s) } else { (w / iw, h / ih) };
                let (tx, ty) = (x + (w - iw * sx) / 2.0, y + (h - ih * sy) / 2.0);
                let mut p = Paint::default();
                p.anti_alias = true;
                p.shader = tiny_skia::Pattern::new(img.as_ref(), tiny_skia::SpreadMode::Pad, tiny_skia::FilterQuality::Bicubic, 1.0,
                    Transform::from_row(sx, 0.0, 0.0, sy, tx, ty));
                if let Some(path) = rounded(x, y, w, h, [r; 4]) {
                    px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
                }
            }
            Shape::Svg { x, y, w, h, svg } => {
                let key = (svg.clone(), (w * k).to_bits());
                if !self.svgs.contains_key(&key) {
                    let opts = usvg::Options { fontdb: self.fontdb(), font_family: "sans-serif".into(), ..Default::default() };
                    let src = svg.replacen("><defs>", &format!("><style>{FLOW_CSS}</style><defs>"), 1);
                    let r = usvg::Tree::from_str(&src, &opts).ok().and_then(|tree| {
                        let (pw, ph) = ((w * k).ceil() as u32, (h * k).ceil() as u32);
                        let mut p = Pixmap::new(pw.max(1), ph.max(1))?;
                        let s = tree.size();
                        resvg::render(&tree, Transform::from_scale(pw as f32 / s.width(), ph as f32 / s.height()), &mut p.as_mut());
                        Some(p)
                    });
                    self.svgs.insert(key.clone(), r);
                }
                if let Some(Some(p)) = self.svgs.get(&key) {
                    px.draw_pixmap(((ox + x) * k).round() as i32, ((dy + y) * k).round() as i32, p.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
                }
            }
        }
    }

    fn text(&mut self, px: &mut Pixmap, t: &TextBox, x: f32, y: f32, k: f32, off: f32) {
        // The clip is in the text box's parent coordinates, which don't scroll: x - t.x + off is that origin.
        let (ox, oy) = (x - t.x + off, y - t.y);
        let clip = t.clip.map(|c| [((ox + c[0]) * k) as i32, ((oy + c[1]) * k) as i32, ((ox + c[0] + c[2]) * k) as i32, ((oy + c[1] + c[3]) * k) as i32]);
        // .work .on span: linear-gradient(90deg, dim 30%, #fff 50%, dim 70%) at 200% width,
        // moved by -200% every 2 s, clipped to the text.
        let sw = t.layout.width().max(1.0);
        let phase = (self.time / 2.0).fract();
        let shimmer = |gx: f32| -> Rgba {
            let u = ((gx - 2.0 * sw * phase) / (2.0 * sw)).rem_euclid(1.0);
            let m = if u <= 0.3 || u >= 0.7 { 0.0 } else if u <= 0.5 { (u - 0.3) / 0.2 } else { (0.7 - u) / 0.2 };
            let d = theme::DIM;
            let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * m).round() as u8;
            [l(d[0], 255), l(d[1], 255), l(d[2], 255), l(d[3], 255)]
        };
        for item in Thread::items(t) {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let style = run.style();
            let r = run.run();
            let m = r.metrics();
            let base = y + run.baseline();
            if style.brush.code {
                // .md code { background; padding: 1px 5px; border-radius: 5px }
                let x0 = x + run.offset() - 5.0;
                fill_clipped(px, x0 * k, (base - m.ascent - 1.0) * k, (run.advance() + 10.0) * k, (m.ascent + m.descent + 2.0) * k, 5.0 * k, theme::CODE_BG, clip);
            }
            let font = r.font();
            let Some(fref) = swash::FontRef::from_index(font.data.as_ref(), font.index as usize) else { continue };
            let size = r.font_size() * k;
            let synth = r.synthesis();
            let skew = synth.skew().is_some();
            let bold = synth.embolden();
            let coords: Vec<i16> = r.normalized_coords().to_vec();
            let mut scaler = self.scaler.builder(fref).size(size).hint(false).normalized_coords(coords.iter()).build();
            let c = style.brush.color;
            for g in run.positioned_glyphs() {
                let gx = (x + g.x) * k;
                let gy = ((y + g.y) * k).round();
                let sub = ((gx.fract() * 4.0) as u8).min(3);
                let key = GlyphKey { font: font.data.id(), index: font.index, glyph: g.id, size: size.to_bits(), sub, skew, bold };
                let mask = self.glyphs.entry(key).or_insert_with(|| {
                    let mut rnd = Render::new(&[Source::Outline]);
                    rnd.format(Format::Alpha).offset(Vector::new(sub as f32 / 4.0, 0.0));
                    // Chromium's synthetic oblique for Inter, which has no italic here.
                    if skew { rnd.transform(Some(swash::zeno::Transform::skew(swash::zeno::Angle::from_degrees(14.0), swash::zeno::Angle::ZERO))); }
                    if bold { rnd.embolden(size / 24.0); }
                    rnd.render(&mut scaler, g.id as u16).map(|img| Mask { left: img.placement.left, top: img.placement.top, w: img.placement.width, h: img.placement.height, data: img.data })
                });
                if let Some(mk) = mask {
                    let c = if t.shimmer { shimmer(g.x + g.advance / 2.0) } else { c };
                    blit(px, gx.floor() as i32 + mk.left, gy as i32 - mk.top, mk, c, clip);
                }
            }
            for (deco, under) in [(&style.underline, true), (&style.strikethrough, false)] {
                if let Some(d) = deco {
                    let off = d.offset.unwrap_or(if under { m.underline_offset } else { m.strikethrough_offset });
                    let sz = d.size.unwrap_or(if under { m.underline_size } else { m.strikethrough_size }).max(1.0 / k);
                    // text-underline-offset: 2px on links.
                    let dy = if under { 2.0 } else { 0.0 };
                    fill_clipped(px, (x + run.offset()) * k, (base - off + dy) * k, run.advance() * k, sz * k, 0.0, d.brush.color, clip);
                }
            }
        }
    }
}

fn color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3])
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 { return None; }
    let lim = (w / 2.0).min(h / 2.0);
    let [tl, tr, br, bl] = r.map(|r| r.min(lim).max(0.0));
    const K: f32 = 0.447_715; // 1 - 0.5523: a quarter circle as a cubic
    let mut p = PathBuilder::new();
    p.move_to(x + tl, y);
    p.line_to(x + w - tr, y);
    p.cubic_to(x + w - tr * K, y, x + w, y + tr * K, x + w, y + tr);
    p.line_to(x + w, y + h - br);
    p.cubic_to(x + w, y + h - br * K, x + w - br * K, y + h, x + w - br, y + h);
    p.line_to(x + bl, y + h);
    p.cubic_to(x + bl * K, y + h, x, y + h - bl * K, x, y + h - bl);
    p.line_to(x, y + tl);
    p.cubic_to(x, y + tl * K, x + tl * K, y, x + tl, y);
    p.close();
    p.finish()
}

/// A fill cut to a clip (device px); a rounded fill that is cut loses its rounding there,
/// which is only ever an inline code background at a scrolling box's edge.
fn fill_clipped(px: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba, clip: Option<[i32; 4]>) {
    let Some([x0, y0, x1, y1]) = clip.map(|c| c.map(|v| v as f32)) else { return fill_rect(px, x, y, w, h, r, c) };
    let (a, b, t, u) = (x.max(x0), (x + w).min(x1), y.max(y0), (y + h).min(y1));
    if b <= a || u <= t { return; }
    let cut = a > x || b < x + w || t > y || u < y + h;
    fill_rect(px, a, t, b - a, u - t, if cut { 0.0 } else { r }, c);
}

fn fill_rect(px: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
    if let Some(path) = rounded(x, y, w, h, [r; 4]) {
        let mut p = Paint::default();
        p.set_color(color(c));
        p.anti_alias = true;
        px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
    }
}

// Source-over of a coverage mask in one colour, on premultiplied pixels.
fn blit(px: &mut Pixmap, x: i32, y: i32, m: &Mask, c: Rgba, clip: Option<[i32; 4]>) {
    let (pw, ph) = (px.width() as i32, px.height() as i32);
    let [cx0, cy0, cx1, cy1] = clip.unwrap_or([0, 0, pw, ph]);
    let data = px.data_mut();
    for row in 0..m.h as i32 {
        let dy = y + row;
        if dy < cy0.max(0) || dy >= cy1.min(ph) { continue; }
        for col in 0..m.w as i32 {
            let dx = x + col;
            if dx < cx0.max(0) || dx >= cx1.min(pw) { continue; }
            let cov = m.data[(row * m.w as i32 + col) as usize] as u32;
            if cov == 0 { continue; }
            let a = cov * c[3] as u32 / 255;
            let i = ((dy * pw + dx) * 4) as usize;
            for ch in 0..3 {
                let s = c[ch] as u32 * a / 255;
                data[i + ch] = (s + data[i + ch] as u32 * (255 - a) / 255) as u8;
            }
            data[i + 3] = (a + data[i + 3] as u32 * (255 - a) / 255) as u8;
        }
    }
}
