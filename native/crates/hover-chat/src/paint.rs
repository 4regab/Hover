//! Paints the visible part of a thread into a pixel buffer: shapes, selection, glyphs
//! (swash outlines, unhinted like Chromium's DirectWrite path), decorations, images and
//! diagrams (resvg with page.html's `.flow` rules). Only the viewport is painted, and
//! glyph masks, decoded images and rasterised diagrams are cached, so a scroll or a
//! streamed chunk costs one viewport of drawing.

use std::collections::HashMap;
use std::sync::Arc;

use parley::PositionedLayoutItem;
use resvg::tiny_skia::{self, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use resvg::usvg;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};

use crate::doc::{Shape, TextBox, Thread};
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

/// Fetches an image's bytes for a URL the image rule allowed (web or session files host).
pub type Loader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

pub struct Painter {
    scaler: ScaleContext,
    glyphs: HashMap<GlyphKey, Option<Mask>>,
    images: HashMap<String, Option<Pixmap>>,
    svgs: HashMap<(usize, u32), Option<Pixmap>>,
    fontdb: Arc<usvg::fontdb::Database>,
    pub loader: Loader,
    /// Painted frames, for the benchmark.
    pub frames: u64,
}

// page.html's `.flow` rules, with its CSS variables resolved (usvg reads no `:not()`).
const FLOW_CSS: &str = ".flow{font-family:Inter;font-size:12px}\
.n>rect,.n>path,.n>circle{fill:rgba(144,70,255,.18);stroke:#C4A2FF;stroke-width:1.2}\
.n.diamond>path{fill:rgba(255,154,74,.14);stroke:#ffb36b}\
text{fill:#f6f2ff;text-anchor:middle}.e{fill:none;stroke:rgba(246,242,255,.62);stroke-width:1.4}\
.e.dot{stroke-dasharray:4 3}.e.thick{stroke-width:2.6}marker path{fill:rgba(246,242,255,.62)}\
.el rect{fill:#1a1220;stroke:rgba(255,255,255,.09)}.el text{fill:rgba(246,242,255,.62);font-size:11px}";

impl Painter {
    pub fn new(font_files: &[Vec<u8>], loader: Loader) -> Self {
        let mut db = usvg::fontdb::Database::new();
        for f in font_files { db.load_font_data(f.clone()); }
        db.load_system_fonts();
        Painter { scaler: ScaleContext::new(), glyphs: HashMap::new(), images: HashMap::new(), svgs: HashMap::new(), fontdb: Arc::new(db), loader, frames: 0 }
    }

    /// The natural size of an image, once it has been loaded.
    pub fn image_size(&mut self, src: &str) -> Option<(f32, f32)> {
        self.image(src).map(|p| (p.width() as f32, p.height() as f32))
    }

    fn image(&mut self, src: &str) -> Option<&Pixmap> {
        if !self.images.contains_key(src) {
            let px = (self.loader)(src).and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
                let rgba = img.to_rgba8();
                let mut p = Pixmap::new(rgba.width(), rgba.height()).unwrap();
                for (d, s) in p.pixels_mut().iter_mut().zip(rgba.pixels()) {
                    *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
                }
                p
            });
            self.images.insert(src.to_string(), px);
        }
        self.images.get(src).and_then(|p| p.as_ref())
    }

    /// Paints `th` from `scroll` (thread px) into a w x h device-pixel buffer.
    pub fn paint(&mut self, th: &Thread, scroll: f32, w: u32, h: u32, scale: f32, bg: Rgba) -> Pixmap {
        self.frames += 1;
        let mut px = Pixmap::new(w.max(1), h.max(1)).unwrap();
        px.fill(color(bg));
        let view = (scroll, scroll + h as f32 / scale);
        let ox = theme::THREAD_PAD[3];
        for (si, s) in th.sections.iter().enumerate() {
            if s.y + s.h + 10.0 < view.0 || s.y - 10.0 > view.1 { continue; }
            let dy = s.y - scroll;
            for sh in &s.frag.shapes {
                self.shape(&mut px, sh, ox, dy, scale);
            }
            for (ti, t) in s.frag.texts.iter().enumerate() {
                let top = s.y + t.y;
                if top > view.1 || top + t.layout.height() < view.0 { continue; }
                for (x0, y0, x1, y1) in th.selection_rects(si, ti) {
                    fill_rect(&mut px, (ox + t.x + x0) * scale, (dy + t.y + y0) * scale, (x1 - x0) * scale, (y1 - y0) * scale, 0.0, theme::SELECTION);
                }
                self.text(&mut px, t, ox + t.x, dy + t.y, scale);
            }
        }
        px
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
            Shape::Image { x, y, w, h, radius, src } => {
                let (x, y, w, h, r) = ((ox + x) * k, (dy + y) * k, w * k, h * k, radius * k);
                let Some(img) = self.image(src) else {
                    // .md img { background: rgba(255,255,255,.04) } while it loads, or broken.
                    fill_rect(px, x, y, w, h, r, [255, 255, 255, 10]);
                    return;
                };
                let (iw, ih) = (img.width() as f32, img.height() as f32);
                let mut p = Paint::default();
                p.anti_alias = true;
                p.shader = tiny_skia::Pattern::new(img.as_ref(), tiny_skia::SpreadMode::Pad, tiny_skia::FilterQuality::Bicubic, 1.0,
                    Transform::from_row(w / iw, 0.0, 0.0, h / ih, x, y));
                if let Some(path) = rounded(x, y, w, h, [r; 4]) {
                    px.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
                }
            }
            Shape::Svg { x, y, w, h, svg } => {
                let key = (svg.as_ptr() as usize, (w * k).to_bits());
                if !self.svgs.contains_key(&key) {
                    let opts = usvg::Options { fontdb: self.fontdb.clone(), font_family: "Inter".into(), ..Default::default() };
                    let src = svg.replacen("><defs>", &format!("><style>{FLOW_CSS}</style><defs>"), 1);
                    let r = usvg::Tree::from_str(&src, &opts).ok().and_then(|tree| {
                        let (pw, ph) = ((w * k).ceil() as u32, (h * k).ceil() as u32);
                        let mut p = Pixmap::new(pw.max(1), ph.max(1))?;
                        let s = tree.size();
                        resvg::render(&tree, Transform::from_scale(pw as f32 / s.width(), ph as f32 / s.height()), &mut p.as_mut());
                        Some(p)
                    });
                    self.svgs.insert(key, r);
                }
                if let Some(Some(p)) = self.svgs.get(&key) {
                    px.draw_pixmap(((ox + x) * k).round() as i32, ((dy + y) * k).round() as i32, p.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
                }
            }
        }
    }

    fn text(&mut self, px: &mut Pixmap, t: &TextBox, x: f32, y: f32, k: f32) {
        // The clip is in the text box's parent coordinates: x - t.x is that origin.
        let clip = t.clip.map(|c| [((x - t.x + c[0]) * k) as i32, ((y - t.y + c[1]) * k) as i32, ((x - t.x + c[0] + c[2]) * k) as i32, ((y - t.y + c[1] + c[3]) * k) as i32]);
        for item in Thread::items(t) {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let style = run.style();
            let r = run.run();
            let m = r.metrics();
            let base = y + run.baseline();
            if style.brush.code {
                // .md code { background; padding: 1px 5px; border-radius: 5px }
                let x0 = x + run.offset() - 5.0;
                fill_rect(px, x0 * k, (base - m.ascent - 1.0) * k, (run.advance() + 10.0) * k, (m.ascent + m.descent + 2.0) * k, 5.0 * k, theme::CODE_BG);
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
                    blit(px, gx.floor() as i32 + mk.left, gy as i32 - mk.top, mk, c, clip);
                }
            }
            for (deco, under) in [(&style.underline, true), (&style.strikethrough, false)] {
                if let Some(d) = deco {
                    let off = d.offset.unwrap_or(if under { m.underline_offset } else { m.strikethrough_offset });
                    let sz = d.size.unwrap_or(if under { m.underline_size } else { m.strikethrough_size }).max(1.0 / k);
                    // text-underline-offset: 2px on links.
                    let dy = if under { 2.0 } else { 0.0 };
                    fill_rect(px, (x + run.offset()) * k, (base - off + dy) * k, run.advance() * k, sz * k, 0.0, d.brush.color);
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
