//! The little linear algebra the scene needs, column-major as three.js and WGSL keep it.

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V3 { pub x: f64, pub y: f64, pub z: f64 }

pub const fn v3(x: f64, y: f64, z: f64) -> V3 { V3 { x, y, z } }

impl V3 {
    pub fn add(self, o: V3) -> V3 { v3(self.x + o.x, self.y + o.y, self.z + o.z) }
    pub fn sub(self, o: V3) -> V3 { v3(self.x - o.x, self.y - o.y, self.z - o.z) }
    pub fn mul(self, k: f64) -> V3 { v3(self.x * k, self.y * k, self.z * k) }
    pub fn dot(self, o: V3) -> f64 { self.x * o.x + self.y * o.y + self.z * o.z }
    pub fn cross(self, o: V3) -> V3 { v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x) }
    pub fn len(self) -> f64 { self.dot(self).sqrt() }
    pub fn norm(self) -> V3 { let l = self.len(); if l == 0.0 { self } else { self.mul(1.0 / l) } }
}

/// A 4×4 matrix, column-major (`m[col*4 + row]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4(pub [f64; 16]);

impl M4 {
    pub const I: M4 = M4([1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]);

    pub fn mul(&self, o: &M4) -> M4 {
        let (a, b) = (&self.0, &o.0);
        let mut r = [0.0; 16];
        for c in 0..4 { for rw in 0..4 { r[c * 4 + rw] = (0..4).map(|k| a[k * 4 + rw] * b[c * 4 + k]).sum(); } }
        M4(r)
    }

    pub fn translate(x: f64, y: f64, z: f64) -> M4 { let mut m = M4::I; m.0[12] = x; m.0[13] = y; m.0[14] = z; m }
    pub fn scale(x: f64, y: f64, z: f64) -> M4 { let mut m = M4::I; m.0[0] = x; m.0[5] = y; m.0[10] = z; m }

    /// Euler XYZ, as Object3D.rotation applies it (R = Rx · Ry · Rz).
    pub fn euler(x: f64, y: f64, z: f64) -> M4 {
        let (a, b, c, d, e, f) = (x.cos(), x.sin(), y.cos(), y.sin(), z.cos(), z.sin());
        let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
        // Matrix4.makeRotationFromEuler, 'XYZ'.
        M4([c * e, af + be * d, bf - ae * d, 0.0, -c * f, ae - bf * d, be + af * d, 0.0, d, -b * c, a * c, 0.0, 0.0, 0.0, 0.0, 1.0])
    }

    /// position · rotation · scale, as Object3D.matrix composes them.
    pub fn trs(p: V3, r: V3, s: V3) -> M4 { M4::translate(p.x, p.y, p.z).mul(&M4::euler(r.x, r.y, r.z)).mul(&M4::scale(s.x, s.y, s.z)) }

    pub fn point(&self, p: V3) -> V3 {
        let m = &self.0;
        let w = m[3] * p.x + m[7] * p.y + m[11] * p.z + m[15];
        v3((m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12]) / w, (m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13]) / w, (m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14]) / w)
    }

    pub fn dir(&self, d: V3) -> V3 { let m = &self.0; v3(m[0] * d.x + m[4] * d.y + m[8] * d.z, m[1] * d.x + m[5] * d.y + m[9] * d.z, m[2] * d.x + m[6] * d.y + m[10] * d.z) }

    /// Matrix4.lookAt for a camera (its -z towards the target), as a camera's world matrix.
    pub fn look_at(eye: V3, target: V3, up: V3) -> M4 {
        let z = eye.sub(target).norm();
        let mut x = up.cross(z);
        if x.len() == 0.0 { x = up.cross(v3(z.x + 1e-4, z.y, z.z)); }
        let x = x.norm();
        let y = z.cross(x);
        M4([x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, eye.x, eye.y, eye.z, 1.0])
    }

    /// The inverse of a rigid transform (rotation and translation only).
    pub fn rigid_inverse(&self) -> M4 {
        let m = &self.0;
        let (t, r) = (v3(m[12], m[13], m[14]), [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]]);
        let inv = [r[0], r[3], r[6], 0.0, r[1], r[4], r[7], 0.0, r[2], r[5], r[8], 0.0, 0.0, 0.0, 0.0, 1.0];
        let mut o = M4(inv);
        let tt = o.dir(t).mul(-1.0);
        o.0[12] = tt.x; o.0[13] = tt.y; o.0[14] = tt.z;
        o
    }

    /// The general inverse (for picking through scaled boxes).
    pub fn inverse(&self) -> M4 {
        let m = &self.0;
        let mut inv = [0.0; 16];
        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        let d = if det == 0.0 { 0.0 } else { 1.0 / det };
        M4(inv.map(|v| v * d))
    }

    /// OrthographicCamera.updateProjectionMatrix with WebGPU's 0..1 depth.
    pub fn ortho(l: f64, r: f64, t: f64, b: f64, near: f64, far: f64) -> M4 {
        let (w, h, p) = (1.0 / (r - l), 1.0 / (t - b), 1.0 / (far - near));
        M4([2.0 * w, 0.0, 0.0, 0.0, 0.0, 2.0 * h, 0.0, 0.0, 0.0, 0.0, -p, 0.0, -(r + l) * w, -(t + b) * h, -near * p, 1.0])
    }

    pub fn f32(&self) -> [f32; 16] { self.0.map(|v| v as f32) }
}

// MARK: Colour

/// THREE.Color from a hex: sRGB, turned into the linear working space as three does
/// with colour management on (the default since r152).
pub fn srgb_to_linear(c: f64) -> f64 { if c < 0.04045 { c * 0.0773993808 } else { (c * 0.9478672986 + 0.0521327014).powf(2.4) } }
pub fn linear_to_srgb(c: f64) -> f64 { if c < 0.0031308 { c * 12.92 } else { 1.055 * c.powf(0.41666) - 0.055 } }

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgb(pub f64, pub f64, pub f64);

impl Rgb {
    pub fn hex(h: u32) -> Rgb {
        let f = |s: u32| srgb_to_linear(((h >> s) & 0xFF) as f64 / 255.0);
        Rgb(f(16), f(8), f(0))
    }
    pub fn mul(self, k: f64) -> Rgb { Rgb(self.0 * k, self.1 * k, self.2 * k) }
    pub fn lerp(self, o: Rgb, k: f64) -> Rgb { Rgb(self.0 + (o.0 - self.0) * k, self.1 + (o.1 - self.1) * k, self.2 + (o.2 - self.2) * k) }
    /// Color.getHexString: back to sRGB bytes, rounded.
    pub fn css(self) -> [u8; 3] {
        let b = |c: f64| (linear_to_srgb(c).clamp(0.0, 1.0) * 255.0).round() as u8;
        [b(self.0), b(self.1), b(self.2)]
    }
    pub fn f32(self) -> [f32; 3] { [self.0 as f32, self.1 as f32, self.2 as f32] }

    /// Color.getHSL, in the linear working space (three.js does not convert for it).
    pub fn hsl(self) -> (f64, f64, f64) {
        let (r, g, b) = (self.0, self.1, self.2);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (min + max) / 2.0;
        if min == max { return (0.0, 0.0, l); }
        let d = max - min;
        let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
        let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        (h / 6.0, s, l)
    }

    /// Color.setHSL (linear): the hue wraps, the saturation and lightness are clamped.
    pub fn from_hsl(h: f64, s: f64, l: f64) -> Rgb {
        let (h, s, l) = (h.rem_euclid(1.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
        if s == 0.0 { return Rgb(l, l, l); }
        let p = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let q = 2.0 * l - p;
        let f = |mut t: f64| {
            if t < 0.0 { t += 1.0; }
            if t > 1.0 { t -= 1.0; }
            if t < 1.0 / 6.0 { q + (p - q) * 6.0 * t } else if t < 0.5 { p } else if t < 2.0 / 3.0 { q + (p - q) * 6.0 * (2.0 / 3.0 - t) } else { q }
        };
        Rgb(f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
    }
}
