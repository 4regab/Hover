//! The 2D canvases main.js draws on (the sky, the TV, the session board, the clock, and
//! the glow, beam and light-patch textures): the part of CanvasRenderingContext2D they
//! use, premultiplied as Chromium keeps a canvas, with text shaped and rasterised by
//! swash. Baselines follow Chromium: 'top' and 'middle' are taken from the em square
//! of the font's own ascent and descent.

use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::zeno::{Format, Vector};
use swash::FontRef;

pub const PIXELIFY: &[u8] = include_bytes!("../../../apps/hover/assets/PixelifySans.ttf");
pub const INTER: &[u8] = include_bytes!("../../../apps/hover/assets/Inter-Regular.ttf");

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Face { Pixel, Inter }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font { pub face: Face, pub px: f64, pub bold: bool }

pub fn pixel(px: f64, bold: bool) -> Font { Font { face: Face::Pixel, px, bold } }
pub fn inter(px: f64) -> Font { Font { face: Face::Inter, px, bold: false } }

#[derive(Clone, Copy, PartialEq)]
pub enum Baseline { Top, Middle }
#[derive(Clone, Copy, PartialEq)]
pub enum Align { Start, Center }

/// A CSS colour as the canvas takes it: #rgb, #rrggbb or rgba(r,g,b,a).
pub fn css(s: &str) -> [f64; 4] {
    if let Some(h) = s.strip_prefix('#') {
        let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_owned() };
        let v = u32::from_str_radix(&h, 16).unwrap_or(0);
        return [((v >> 16) & 255) as f64 / 255.0, ((v >> 8) & 255) as f64 / 255.0, (v & 255) as f64 / 255.0, 1.0];
    }
    let inner = s.trim_start_matches("rgba(").trim_start_matches("rgb(").trim_end_matches(')');
    let p: Vec<f64> = inner.split(',').map(|x| x.trim().parse().unwrap_or(0.0)).collect();
    [p[0] / 255.0, p[1] / 255.0, p[2] / 255.0, *p.get(3).unwrap_or(&1.0)]
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    /// Premultiplied RGBA, 0..1.
    px: Vec<[f64; 4]>,
    pub fill: [f64; 4],
    pub scale: f64,
    pub baseline: Baseline,
    pub align: Align,
    pub font: Font,
    /// ctx.filter = 'blur(n px)' (only the patch texture uses it).
    pub blur: f64,
    scaler: ScaleContext,
    shaper: ShapeContext,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![[0.0; 4]; w * h], fill: [0.0, 0.0, 0.0, 1.0], scale: 1.0, baseline: Baseline::Top, align: Align::Start,
            font: pixel(10.0, false), blur: 0.0, scaler: ScaleContext::new(), shaper: ShapeContext::new() }
    }

    pub fn style(&mut self, s: &str) { self.fill = css(s); }

    fn blend(&mut self, x: usize, y: usize, c: [f64; 4], cover: f64) {
        let a = c[3] * cover;
        if a <= 0.0 { return; }
        let d = &mut self.px[y * self.w + x];
        for k in 0..3 { d[k] = c[k] * a + d[k] * (1.0 - a); }
        d[3] = a + d[3] * (1.0 - a);
    }

    /// fillRect, with coverage for fractional edges (the canvas antialiases them).
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let (x0, y0, x1, y1) = (x * self.scale, y * self.scale, (x + w) * self.scale, (y + h) * self.scale);
        let c = self.fill;
        if self.blur > 0.0 { return self.blurred_rect(x0, y0, x1, y1, c); }
        let (ix0, iy0) = (x0.floor().max(0.0) as usize, y0.floor().max(0.0) as usize);
        let (ix1, iy1) = ((x1.ceil() as usize).min(self.w), (y1.ceil() as usize).min(self.h));
        for py in iy0..iy1 {
            let cy = ((py as f64 + 1.0).min(y1) - (py as f64).max(y0)).max(0.0);
            for px in ix0..ix1 {
                let cx = ((px as f64 + 1.0).min(x1) - (px as f64).max(x0)).max(0.0);
                self.blend(px, py, c, cx * cy);
            }
        }
    }

    fn blurred_rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: [f64; 4]) {
        // The rect's coverage blurred by a Gaussian of the filter's sigma, separably:
        // the coverage of a box under a Gaussian is a difference of error functions.
        let s = self.blur;
        let erf = |v: f64| { let t = 1.0 / (1.0 + 0.3275911 * v.abs()); let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * (-v * v).exp(); if v < 0.0 { -y } else { y } };
        let cov = |p: f64, a: f64, b: f64| 0.5 * (erf((b - p) / (s * std::f64::consts::SQRT_2)) - erf((a - p) / (s * std::f64::consts::SQRT_2)));
        for py in 0..self.h { let cy = cov(py as f64 + 0.5, y0, y1); if cy < 1e-4 { continue; }
            for px in 0..self.w { let cx = cov(px as f64 + 0.5, x0, x1); self.blend(px, py, c, cx * cy); } }
    }

    /// A vertical linear gradient over a rect: stops of (offset, rgba), interpolated
    /// premultiplied, as Chromium does.
    pub fn gradient_v(&mut self, y0: f64, y1: f64, stops: &[(f64, [f64; 4])], x: f64, y: f64, w: f64, h: f64) {
        for py in (y.max(0.0) as usize)..((y + h) as usize).min(self.h) {
            let t = ((py as f64 + 0.5 - y0) / (y1 - y0)).clamp(0.0, 1.0);
            let c = sample(stops, t);
            for px in (x.max(0.0) as usize)..((x + w) as usize).min(self.w) { self.blend(px, py, c, 1.0); }
        }
    }

    /// The radial gradient of glowTex, centred, radius r.
    pub fn gradient_r(&mut self, cx: f64, cy: f64, r: f64, stops: &[(f64, [f64; 4])]) {
        for py in 0..self.h { for px in 0..self.w {
            let t = ((px as f64 + 0.5 - cx).hypot(py as f64 + 0.5 - cy) / r).clamp(0.0, 1.0);
            let c = sample(stops, t);
            self.blend(px, py, c, 1.0);
        } }
    }

    fn font_ref(&self) -> FontRef<'static> {
        FontRef::from_index(match self.font.face { Face::Pixel => PIXELIFY, Face::Inter => INTER }, 0).expect("the font parses")
    }

    fn shape(&mut self, text: &str) -> Vec<(swash::GlyphId, f64, f64)> {
        let f = self.font_ref();
        let wght = if self.font.bold { 700.0 } else { 400.0 };
        let size = (self.font.px * self.scale) as f32;
        let mut s = self.shaper.builder(f).size(size).variations(&[("wght", wght)]).features(&[("liga", 0), ("clig", 0)]).build();
        s.add_str(text);
        let mut out = vec![];
        let mut x = 0.0;
        s.shape_with(|c| for g in c.glyphs { out.push((g.id, x + g.x as f64, g.y as f64)); x += g.advance as f64; });
        out.push((0, x, 0.0));
        out
    }

    /// measureText(text).width, in canvas units.
    pub fn measure(&mut self, text: &str) -> f64 { self.shape(text).last().map_or(0.0, |g| g.1) / self.scale }

    /// fillText.
    pub fn text(&mut self, text: &str, x: f64, y: f64) {
        let glyphs = self.shape(text);
        let width = glyphs.last().map_or(0.0, |g| g.1);
        let f = self.font_ref();
        let m = f.metrics(&[]);
        let size = self.font.px * self.scale;
        let (asc, desc) = (m.ascent as f64, m.descent.abs() as f64);
        let em_asc = size * asc / (asc + desc);
        let em_desc = size * desc / (asc + desc);
        let base = y * self.scale + match self.baseline { Baseline::Top => em_asc, Baseline::Middle => (em_asc - em_desc) / 2.0 };
        let left = x * self.scale - if self.align == Align::Center { width / 2.0 } else { 0.0 };
        let wght = if self.font.bold { 700.0 } else { 400.0 };
        let c = self.fill;
        let mut sc = self.scaler.builder(f).size(size as f32).variations(&[("wght", wght)]).hint(false).build();
        let mut jobs = vec![];
        for &(id, gx, gy) in &glyphs[..glyphs.len() - 1] {
            let (ox, oy) = (left + gx, base - gy);
            let (fx, fy) = (ox.floor(), oy.floor());
            let img = Render::new(&[Source::ColorOutline(0), Source::ColorBitmap(StrikeWith::BestFit), Source::Outline])
                .format(Format::Alpha).offset(Vector::new((ox - fx) as f32, (oy - fy) as f32)).render(&mut sc, id);
            if let Some(img) = img { jobs.push((img, fx as i64, fy as i64)); }
        }
        for (img, fx, fy) in jobs {
            let p = img.placement;
            for row in 0..p.height as i64 { for col in 0..p.width as i64 {
                let (px, py) = (fx + p.left as i64 + col, fy - p.top as i64 + row);
                if px < 0 || py < 0 || px >= self.w as i64 || py >= self.h as i64 { continue; }
                let a = img.data[(row * p.width as i64 + col) as usize] as f64 / 255.0;
                self.blend(px as usize, py as usize, c, a);
            } }
        }
    }

    /// The pixels as a texture takes them: RGBA8, not premultiplied (three uploads
    /// canvases with premultiplyAlpha off), rounded as Chromium stores its canvas.
    pub fn rgba(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(self.w * self.h * 4);
        for p in &self.px {
            let a = p[3];
            let un = |v: f64| if a > 0.0 { (v / a * 255.0).round().clamp(0.0, 255.0) as u8 } else { 0 };
            o.extend([un(p[0]), un(p[1]), un(p[2]), (a * 255.0).round() as u8]);
        }
        o
    }
}

fn sample(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    let pm = |c: [f64; 4]| [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
    let mut i = 0;
    while i + 1 < stops.len() && stops[i + 1].0 < t { i += 1; }
    let (a, b) = (stops[i], stops[(i + 1).min(stops.len() - 1)]);
    let k = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
    let (pa, pb) = (pm(a.1), pm(b.1));
    let p: [f64; 4] = std::array::from_fn(|j| pa[j] + (pb[j] - pa[j]) * k);
    // Back to straight colour for blend(), which premultiplies.
    if p[3] > 0.0 { [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]] } else { [0.0; 4] }
}
