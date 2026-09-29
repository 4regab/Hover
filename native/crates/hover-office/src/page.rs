//! What page.html lays around and over the canvas in host mode: #office's radial
//! background (night or day) and the ::after vignette. Since 639c01c the page drops
//! #office's rounded corners and border in Hover (body.host #office), so the office
//! fills its box edge to edge. CSS gradients are interpolated premultiplied
//! in sRGB; ellipse radii are percentages of the box.

use crate::canvas::css;

fn ramp(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    let mut i = 0;
    while i + 1 < stops.len() && stops[i + 1].0 < t { i += 1; }
    let (a, b) = (stops[i], stops[(i + 1).min(stops.len() - 1)]);
    let k = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
    let pa = [a.1[0] * a.1[3], a.1[1] * a.1[3], a.1[2] * a.1[3], a.1[3]];
    let pb = [b.1[0] * b.1[3], b.1[1] * b.1[3], b.1[2] * b.1[3], b.1[3]];
    std::array::from_fn(|j| pa[j] + (pb[j] - pa[j]) * k)
}

/// radial-gradient(rx% ry% at cx% cy%, stops): premultiplied RGBA at a pixel centre.
fn radial(x: f64, y: f64, w: f64, h: f64, r: (f64, f64), at: (f64, f64), stops: &[(f64, [f64; 4])]) -> [f64; 4] {
    let (dx, dy) = ((x - at.0 * w) / (r.0 * w), (y - at.1 * h) / (r.1 * h));
    ramp(stops, dx.hypot(dy))
}

/// The office as the page shows it: the frame (premultiplied RGBA8 from the renderer)
/// over the background, and the vignette over both. RGB8.
pub fn compose(frame: &[u8], w: usize, h: usize, day: bool) -> Vec<u8> {
    let bg: [(f64, [f64; 4]); 3] = if day { [(0.0, css("#4a3530")), (0.6, css("#241815")), (1.0, css("#0e0a09"))] } else { [(0.0, css("#2a1824")), (0.55, css("#150c14")), (1.0, css("#07050a"))] };
    let vig = [(0.0, [0.0, 0.0, 0.0, 0.0]), (0.6, [0.0, 0.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 0.0, 0.45])];
    let (fw, fh) = (w as f64, h as f64);
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h { for x in 0..w {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let b = radial(px, py, fw, fh, (1.2, 0.9), (0.5, 0.45), &bg);
        let f = &frame[(y * w + x) * 4..(y * w + x) * 4 + 4];
        let fa = f[3] as f64 / 255.0;
        let mut c: [f64; 3] = std::array::from_fn(|k| f[k] as f64 / 255.0 + b[k] * (1.0 - fa));
        let v = radial(px, py, fw, fh, (1.3, 1.0), (0.5, 0.5), &vig);
        for k in 0..3 { c[k] = v[k] + c[k] * (1.0 - v[3]); }
        out.extend(c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
    } }
    out
}

/// compose(), with what doesn't change from frame to frame (the background, the
/// vignette) worked out once per size and time of day.
#[derive(Default)]
pub struct Composer { key: (usize, usize, bool), under: Vec<[f32; 3]>, over: Vec<[f32; 4]> }

impl Composer {
    pub fn compose(&mut self, frame: &[u8], w: usize, h: usize, day: bool) -> Vec<u8> {
        if self.key != (w, h, day) || self.under.is_empty() {
            self.key = (w, h, day);
            // Two layers from compose() itself: the page with a clear frame (the
            // background), and with a white opaque one (what lies over the frame).
            let clear = compose(&vec![0u8; w * h * 4], w, h, day);
            let white = compose(&vec![255u8; w * h * 4], w, h, day);
            self.under = clear.chunks(3).map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect();
            // Over the frame everything is linear in it: out = frame * k + c; k from white − black-opaque.
            let black = compose(&[0u8, 0, 0, 255].repeat(w * h), w, h, day);
            self.over = white.chunks(3).zip(black.chunks(3)).map(|(a, b)| [(a[0] as f32 - b[0] as f32) / 255.0, b[0] as f32, b[1] as f32, b[2] as f32]).collect();
        }
        let mut out = Vec::with_capacity(w * h * 3);
        for (i, p) in frame.chunks(4).enumerate() {
            let a = p[3] as f32 / 255.0;
            let (u, o) = (self.under[i], self.over[i]);
            for k in 0..3 {
                // The frame over its background, then what lies over both; the part of the
                // background the frame lets through is under's, which already carries it.
                let v = p[k] as f32 * o[0] + o[k + 1] * a + u[k] * (1.0 - a);
                out.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        out
    }
}
