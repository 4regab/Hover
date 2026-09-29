//! What page.html lays around and over the canvas in host mode: #office's radial
//! background (night or day), the ::after vignette, the 16 px rounded corners and the
//! 1 px border, over the body's #07050a. CSS gradients are interpolated premultiplied
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
/// over the background, the vignette over both, the corners and border. RGB8.
pub fn compose(frame: &[u8], w: usize, h: usize, day: bool) -> Vec<u8> {
    let bg: [(f64, [f64; 4]); 3] = if day { [(0.0, css("#4a3530")), (0.6, css("#241815")), (1.0, css("#0e0a09"))] } else { [(0.0, css("#2a1824")), (0.55, css("#150c14")), (1.0, css("#07050a"))] };
    let vig = [(0.0, [0.0, 0.0, 0.0, 0.0]), (0.6, [0.0, 0.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 0.0, 0.45])];
    let body = css("#07050a");
    let border = css("rgba(255,190,150,.16)");
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
        // The 1 px border inside the 16 px rounded box, and the body outside it.
        let r = 16.0;
        let (qx, qy) = ((px - r).min(fw - r - px).min(0.0), (py - r).min(fh - r - py).min(0.0));
        let d = r - qx.hypot(qy);
        let edge = px.min(fw - px).min(py).min(fh - py);
        let inside = (d + 0.5).clamp(0.0, 1.0) * if qx < 0.0 && qy < 0.0 { 1.0 } else { (edge + 0.5).clamp(0.0, 1.0) };
        let dist = if qx < 0.0 && qy < 0.0 { d } else { edge };
        let on_border = (1.5 - dist).clamp(0.0, 1.0) * (dist + 0.5).clamp(0.0, 1.0);
        for k in 0..3 { c[k] = border[k] * border[3] * on_border + c[k] * (1.0 - border[3] * on_border); }
        for k in 0..3 { c[k] = c[k] * inside + body[k] * (1.0 - inside); }
        out.extend(c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
    } }
    out
}
