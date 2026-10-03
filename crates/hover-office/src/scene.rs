//! main.js's scene: a tree of nodes (Object3D), each maybe drawing something, and the
//! room built into it line by line in the page's order (the order matters: every
//! jittered box draws from the same generator, R).

use crate::js::{Rng, TAU};
use crate::m::{v3, Rgb, M4, V3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend { Normal, Additive }

#[derive(Clone, Debug, PartialEq)]
pub enum Mat {
    /// MeshStandardMaterial: a colour (or the vertices'), roughness, metalness.
    Std { color: Rgb, rough: f64, metal: f64, vertex: bool },
    /// MeshBasicMaterial: unlit; tone mapped unless said.
    Basic { color: Rgb, tone: bool, opacity: f64, blend: Blend, depth_write: bool, tex: Option<usize>, double: bool },
    /// SpriteMaterial with the radial glow texture, additive (tone mapped by default).
    Glow { color: Rgb, opacity: f64 },
    /// PointsMaterial: 1 px points (size 0.05, not attenuated by an orthographic camera).
    Points { color: Rgb, opacity: f64 },
}

impl Mat {
    pub fn basic(hex: u32, tone: bool) -> Mat { Mat::Basic { color: Rgb::hex(hex), tone, opacity: 1.0, blend: Blend::Normal, depth_write: true, tex: None, double: false } }
    pub fn std(color: Rgb, rough: f64, metal: f64) -> Mat { Mat::Std { color, rough, metal, vertex: false } }
    pub fn transparent(&self) -> bool {
        match self { Mat::Basic { opacity, blend, .. } => *opacity < 1.0 || *blend == Blend::Additive, Mat::Glow { .. } | Mat::Points { .. } => true, _ => false }
    }
}

/// A voxel box of a merged mesh: its low corner, size and (linear) colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VBox { pub x: f64, pub y: f64, pub z: f64, pub w: f64, pub h: f64, pub d: f64, pub c: Rgb }

#[derive(Clone, Debug, PartialEq)]
pub enum Geo {
    /// BoxGeometry(1, 1, 1).
    Unit,
    /// Vox.mesh(): boxes merged, a colour per box.
    Merged(usize),
    Cylinder { top: f64, bottom: f64, h: f64, seg: u32 },
    Ring { inner: f64, outer: f64, seg: u32 },
    Plane { w: f64, h: f64 },
    /// Four points, uv (0,1)(1,1)(1,0)(0,0), indices 0 3 1 · 1 3 2.
    Quad([V3; 4]),
    Sprite,
    Points(Vec<V3>),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<usize>,
    pub p: V3,
    pub r: V3,
    pub s: V3,
    pub visible: bool,
    pub draw: Option<(Geo, Mat)>,
    pub cast: bool,
    pub receive: bool,
    /// hitMat: in the raycast, never drawn.
    pub hit: Option<Hit>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit { Bot(usize), Prop(usize), Desk(usize) }

#[derive(Default)]
pub struct Graph { pub nodes: Vec<Node>, pub merged: Vec<Vec<VBox>> }

pub const ROOT: usize = 0;

impl Graph {
    pub fn new() -> Graph {
        let mut g = Graph::default();
        g.nodes.push(Node { parent: None, p: V3::default(), r: V3::default(), s: v3(1.0, 1.0, 1.0), visible: true, draw: None, cast: false, receive: false, hit: None });
        g
    }

    pub fn add(&mut self, parent: usize, p: V3) -> usize {
        self.nodes.push(Node { parent: Some(parent), p, r: V3::default(), s: v3(1.0, 1.0, 1.0), visible: true, draw: None, cast: false, receive: false, hit: None });
        self.nodes.len() - 1
    }

    /// `pivot(parent, x, y, z)`.
    pub fn pivot(&mut self, parent: usize, x: f64, y: f64, z: f64) -> usize { self.add(parent, v3(x, y, z)) }

    /// `box(parent, w, h, d, x, y, z, mat, shadow)`: the unit box scaled, centred at x, y, z.
    pub fn boxm(&mut self, parent: usize, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64, mat: Mat, shadow: bool) -> usize {
        let n = self.add(parent, v3(x, y, z));
        let node = &mut self.nodes[n];
        node.s = v3(w, h, d);
        node.draw = Some((Geo::Unit, mat));
        node.cast = shadow;
        node.receive = true;
        n
    }

    pub fn drawing(&mut self, parent: usize, geo: Geo, mat: Mat) -> usize {
        let n = self.add(parent, V3::default());
        self.nodes[n].draw = Some((geo, mat));
        n
    }

    pub fn local(&self, i: usize) -> M4 { let n = &self.nodes[i]; M4::trs(n.p, n.r, n.s) }

    /// matrixWorld of one node (getWorldPosition's matrix), without working out the rest.
    pub fn world_at(&self, i: usize) -> M4 {
        let mut m = self.local(i);
        let mut up = self.nodes[i].parent;
        while let Some(p) = up { m = self.local(p).mul(&m); up = self.nodes[p].parent; }
        m
    }

    /// matrixWorld of every node, parents before children (they are added so).
    pub fn world(&self) -> Vec<M4> {
        let mut w: Vec<M4> = Vec::with_capacity(self.nodes.len());
        for (i, n) in self.nodes.iter().enumerate() {
            let l = self.local(i);
            w.push(match n.parent { Some(p) => w[p].mul(&l), None => l });
        }
        w
    }

    /// Drawn: visible itself and all the way up.
    pub fn shown(&self) -> Vec<bool> {
        let mut v: Vec<bool> = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes { v.push(n.visible && n.parent.is_none_or(|p| v[p])); }
        v
    }

    pub fn mat_mut(&mut self, i: usize) -> &mut Mat { &mut self.nodes[i].draw.as_mut().expect("a drawing node").1 }
}

/// `class Vox`: boxes gathered into one mesh with a colour per box.
pub struct Vox<'r> { pub b: Vec<VBox>, r: &'r mut Rng }

impl<'r> Vox<'r> {
    pub fn new(r: &'r mut Rng) -> Vox<'r> { Vox { b: vec![], r } }

    /// `box(x, y, z, w, h, d, c, j = 0.04)`: the colour jittered by ±j (R is drawn only when j).
    pub fn bx(&mut self, x: f64, y: f64, z: f64, w: f64, h: f64, d: f64, c: u32, j: f64) -> &mut Self {
        let mut col = Rgb::hex(c);
        if j != 0.0 { col = col.mul(1.0 + (self.r.next() - 0.5) * j * 2.0); }
        self.b.push(VBox { x, y, z, w, h, d, c: col });
        self
    }

    /// With the default jitter.
    pub fn b(&mut self, x: f64, y: f64, z: f64, w: f64, h: f64, d: f64, c: u32) -> &mut Self { self.bx(x, y, z, w, h, d, c, 0.04) }
}

pub const RW: f64 = 14.0;
pub const RD: f64 = 11.0;
pub const WH: f64 = 4.0;
pub const X0: f64 = -7.0;
pub const Z0: f64 = -5.5;
pub const DOOR: (f64, f64) = (-5.65, Z0 + 0.35);

/// DESKS: two rows of three; a bot sits on the -x side facing +x.
pub const DESKS: [(f64, f64); 6] = [(-3.2, -1.6), (0.6, -1.6), (4.4, -1.6), (-3.2, 2.1), (0.6, 2.1), (4.4, 2.1)];
pub fn seat(d: usize) -> f64 { DESKS[d].0 - 0.67 }

/// The room's handles: what main.js keeps in variables to change later.
pub struct Room {
    pub door: usize,
    pub exit_glow: usize,
    pub sky: usize, pub tv: usize, pub board: usize, pub clock: usize,
    pub tv_glow: usize, pub clock_glow: usize, pub coffee_led: usize,
    pub steam: [usize; 3],
    pub shades: Vec<usize>, pub floor_shade: usize,
    pub lamps: Vec<(V3, f64)>,
    pub desk_glows: Vec<usize>,
    pub vac: usize,
    pub patch: usize, pub beam: usize, pub dust: usize,
}

/// Canvas texture ids, in the order the renderer makes them.
pub const TEX_SKY: usize = 0;
pub const TEX_TV: usize = 1;
pub const TEX_BOARD: usize = 2;
pub const TEX_CLOCK: usize = 3;
pub const TEX_BEAM: usize = 4;
pub const TEX_PATCH: usize = 5;

fn glow(g: &mut Graph, parent: usize, hex: u32, size: f64, opacity: f64, p: V3) -> usize {
    let n = g.drawing(parent, Geo::Sprite, Mat::Glow { color: Rgb::hex(hex), opacity });
    g.nodes[n].s = v3(size, size, size);
    g.nodes[n].p = p;
    n
}

fn screen(g: &mut Graph, tex: usize, w: f64, h: f64, p: V3, ry: f64) -> usize {
    let n = g.drawing(ROOT, Geo::Plane { w, h }, Mat::Basic { color: Rgb(1.0, 1.0, 1.0), tone: false, opacity: 1.0, blend: Blend::Normal, depth_write: true, tex: Some(tex), double: false });
    g.nodes[n].p = p;
    g.nodes[n].r = v3(0.0, ry, 0.0);
    n
}

fn quad(g: &mut Graph, pts: [V3; 4], tex: usize) -> usize {
    g.drawing(ROOT, Geo::Quad(pts), Mat::Basic { color: Rgb(1.0, 1.0, 1.0), tone: false, opacity: 1.0, blend: Blend::Additive, depth_write: false, tex: Some(tex), double: true })
}

fn plant(v: &mut Vox, x: f64, z: f64, s: f64, y: f64, seed: i32) {
    let mut r = Rng::new(seed);
    v.b(x - 0.2 * s, y, z - 0.2 * s, 0.4 * s, 0.34 * s, 0.4 * s, 0xa4552e).b(x - 0.23 * s, y + 0.3 * s, z - 0.23 * s, 0.46 * s, 0.07 * s, 0.46 * s, 0xb8653a)
        .b(x - 0.03 * s, y + 0.34 * s, z - 0.03 * s, 0.06 * s, 0.55 * s, 0.06 * s, 0x4a3a22);
    for i in 0..11 {
        let a = r.next() * TAU;
        let rr = r.next() * 0.3 * s;
        let h = (0.45 + r.next() * 0.6) * s;
        let w = (0.14 + r.next() * 0.16) * s;
        v.bx(x + a.cos() * rr - w / 2.0, y + h, z + a.sin() * rr - w / 2.0, w, w * 0.7, w, [0x4f7a3a, 0x3e6630, 0x6a9a45, 0x5a8a3a][i % 4], 0.08);
    }
}

/// The room, as main.js builds it from "The room" to the dust.
pub fn build(g: &mut Graph, r: &mut Rng) -> Room {
    let mut walls: Vec<VBox>;
    let mut room: Vec<VBox>;
    // Two Voxes share R; the page interleaves their calls, so this does too.
    macro_rules! w { ($($t:tt)*) => {{ let mut v = Vox { b: std::mem::take(&mut walls), r: &mut *r }; v$($t)*; walls = v.b; }}; }
    macro_rules! o { ($($t:tt)*) => {{ let mut v = Vox { b: std::mem::take(&mut room), r: &mut *r }; v$($t)*; room = v.b; }}; }
    walls = vec![];
    room = vec![];
    o!(.bx(X0 - 0.3, -0.7, Z0 - 0.3, RW + 0.3, 0.6, RD + 0.3, 0x1c1215, 0.0));
    for i in 0..RW as i32 { for k in 0..RD as i32 { o!(.bx(X0 + i as f64, -0.1, Z0 + k as f64, 1.0, 0.1, 1.0, if (i + k) % 2 != 0 { 0x5a3c34 } else { 0x48302b }, 0.05)); } }
    o!(.bx(X0 - 0.3, -0.7, Z0 - 0.3 + RD + 0.3 - 0.02, RW + 0.3, 0.6, 0.02, 0x140c0f, 0.0));
    let mut x = 0.0;
    while x < RW { w!(.bx(X0 + x, -0.7, Z0 - 0.3, 0.5, WH + 0.7, 0.3, if (x * 2.0) % 2.0 != 0.0 { 0x6e4643 } else { 0x684240 }, 0.03)); x += 0.5; }
    let mut z = 0.0;
    while z < RD { w!(.bx(X0 - 0.3, -0.7, Z0 + z, 0.3, WH + 0.7, 0.5, if (z * 2.0) % 2.0 != 0.0 { 0x633e3c } else { 0x5e3a38 }, 0.03)); z += 0.5; }
    w!(.bx(X0 - 0.3, -0.7, Z0 - 0.3, 0.3, WH + 0.7, 0.3, 0x5e3a38, 0.0));
    w!(.bx(X0, 0.0, Z0, RW, 1.15, 0.035, 0x4b2f2c, 0.02).bx(X0, 0.0, Z0, 0.035, 1.15, RD, 0x462b29, 0.02));
    w!(.bx(X0, 1.15, Z0, RW, 0.07, 0.06, 0x80564d, 0.0).bx(X0, 1.15, Z0, 0.06, 0.07, RD, 0x7a524a, 0.0));
    w!(.bx(X0, 0.0, Z0, RW, 0.16, 0.07, 0x33201d, 0.0).bx(X0, 0.0, Z0, 0.07, 0.16, RD, 0x301e1b, 0.0));
    w!(.bx(X0 - 0.3, WH, Z0 - 0.3, RW + 0.3, 0.1, 0.3, 0x8d5f57, 0.0).bx(X0 - 0.3, WH, Z0 - 0.3, 0.3, 0.1, RD + 0.3, 0x86594f, 0.0));
    w!(.bx(X0 - 0.3, -0.7, Z0 + RD - 0.02, 0.3, WH + 0.8, 0.02, 0x3a2422, 0.0).bx(X0 + RW - 0.02, -0.7, Z0 - 0.3, 0.02, WH + 0.8, 0.3, 0x3a2422, 0.0));

    // Door on the back wall; the panel swings.
    w!(.bx(-6.3, 0.0, Z0, 1.3, 2.42, 0.08, 0x2c1b17, 0.0).bx(-6.2, 0.0, Z0 + 0.01, 1.1, 2.3, 0.08, 0x0b0708, 0.0));
    let mut door_v = Vox::new(r);
    door_v.bx(0.0, 0.0, 0.0, 1.1, 2.3, 0.07, 0x5c3b2b, 0.02).bx(0.14, 1.28, 0.07, 0.82, 0.82, 0.02, 0x6b4633, 0.0).bx(0.14, 0.24, 0.07, 0.82, 0.86, 0.02, 0x6b4633, 0.0).bx(0.9, 1.05, 0.07, 0.08, 0.08, 0.06, 0xe0ab4c, 0.0);
    let door_boxes = door_v.b;
    let door = g.pivot(ROOT, -6.2, 0.0, Z0 + 0.02);
    g.merged.push(door_boxes);
    let dm = g.drawing(door, Geo::Merged(g.merged.len() - 1), Mat::Std { color: Rgb(1.0, 1.0, 1.0), rough: 0.88, metal: 0.0, vertex: true });
    g.nodes[dm].cast = true;
    g.nodes[dm].receive = true;
    o!(.bx(-6.35, 0.0, Z0 + 0.12, 1.4, 0.02, 0.75, 0x6f3b2a, 0.02).bx(-6.2, 0.02, Z0 + 0.22, 1.1, 0.005, 0.55, 0x8a4c34, 0.0));
    g.boxm(ROOT, 0.34, 0.12, 0.08, DOOR.0, 2.62, Z0 + 0.05, Mat::basic(0xffb35c, false), false);
    let exit_glow = glow(g, ROOT, 0xffa24a, 1.4, 0.55, v3(DOOR.0, 2.62, Z0 + 0.2));

    // Window with curtains; the sky behind it.
    w!(.bx(0.1, 3.25, Z0, 2.8, 0.12, 0.12, 0x3a2620, 0.0).bx(-0.05, 1.22, Z0, 3.1, 0.12, 0.26, 0x4a3026, 0.0)
        .bx(0.1, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0.0).bx(2.78, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0.0)
        .bx(1.46, 1.34, Z0, 0.08, 1.92, 0.1, 0x3a2620, 0.0).bx(0.2, 2.26, Z0, 2.6, 0.07, 0.1, 0x3a2620, 0.0)
        .bx(-0.5, 3.52, Z0 + 0.1, 4.0, 0.05, 0.05, 0x241612, 0.0));
    for (cx, _dir) in [(-0.45, 1.0), (2.9, -1.0)] {
        for i in 0..4 { w!(.bx(cx + i as f64 * 0.13, 1.0, Z0 + 0.06 + (i % 2) as f64 * 0.05, 0.14, 2.52, 0.1, if i % 2 != 0 { 0x9b5230 } else { 0x8a4629 }, 0.0)); }
    }
    let sky = screen(g, TEX_SKY, 2.56, 1.9, v3(1.5, 2.3, Z0 + 0.012), 0.0);

    // Wall TV and the cabinet under it.
    w!(.bx(4.1, 1.62, Z0, 2.6, 1.52, 0.1, 0x0c0b10, 0.0));
    let tv = screen(g, TEX_TV, 2.44, 1.38, v3(5.4, 2.38, Z0 + 0.105), 0.0);
    let tv_glow = glow(g, ROOT, 0x5aa8ff, 3.6, 0.22, v3(5.4, 2.3, Z0 + 0.6));
    o!(.b(4.3, 0.0, Z0 + 0.02, 2.2, 0.55, 0.5, 0x4c3028).bx(4.25, 0.55, Z0 + 0.02, 2.3, 0.05, 0.54, 0x5e3c30, 0.0)
        .bx(4.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0.0).bx(5.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0.0)
        .b(4.45, 0.6, Z0 + 0.12, 0.26, 0.42, 0.26, 0x22212a).b(6.1, 0.6, Z0 + 0.1, 0.24, 0.1, 0.3, 0xc8a24a).b(6.12, 0.7, Z0 + 0.1, 0.2, 0.08, 0.3, 0x5a7aa0));

    // Coffee counter.
    o!(.b(-4.6, 0.0, Z0 + 0.02, 1.9, 0.86, 0.62, 0x5a3a2c).bx(-4.65, 0.86, Z0 + 0.02, 2.0, 0.06, 0.66, 0xd8cdbf, 0.02)
        .bx(-4.5, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0.0).bx(-3.6, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0.0)
        .b(-4.45, 0.92, Z0 + 0.1, 0.42, 0.56, 0.4, 0x26252d).b(-4.4, 1.2, Z0 + 0.5, 0.32, 0.18, 0.02, 0x121118)
        .b(-3.8, 0.92, Z0 + 0.2, 0.12, 0.14, 0.12, 0xeeeeee).b(-3.6, 0.92, Z0 + 0.25, 0.12, 0.14, 0.12, 0x9b6bff)
        .b(-3.3, 0.92, Z0 + 0.12, 0.34, 0.4, 0.3, 0x7a8a95).bx(-4.6, 1.85, Z0, 1.9, 0.05, 0.3, 0x4a2f24, 0.0));
    for i in 0..5 { o!(.b(-4.5 + i as f64 * 0.36, 1.9, Z0 + 0.06, 0.2, 0.22 + (i % 2) as f64 * 0.08, 0.18, [0xc8a24a, 0x9a4f2e, 0x7a9a6a, 0xdcd2c4, 0xb46a3a][i])); }
    let coffee_led = glow(g, ROOT, 0x7ee0ff, 0.35, 0.9, v3(-4.24, 1.29, Z0 + 0.53));
    let steam = [0, 1, 2].map(|_| glow(g, ROOT, 0xffffff, 0.2, 0.25, V3::default()));

    // Left wall: bookcase, board, clock, painting.
    o!(.b(X0, 0.0, -4.3, 0.46, 2.42, 0.06, 0x4a2e24).b(X0, 0.0, -2.76, 0.46, 2.42, 0.06, 0x4a2e24).bx(X0, 0.0, -4.3, 0.05, 2.42, 1.6, 0x3a241c, 0.0));
    const BOOKS: [u32; 8] = [0x8a3b2e, 0xc9a24a, 0x4a6b4a, 0x7b5aa6, 0xd07a3a, 0x3a5a8a, 0xb8b0a0, 0x9a4a5a];
    for y in [0.0, 0.6, 1.2, 1.8, 2.36] {
        o!(.bx(X0, y, -4.26, 0.46, 0.06, 1.52, 0x55352a, 0.02));
        if y > 2.0 { continue; }
        let mut z = -4.2;
        while z < -2.9 {
            let w = 0.06 + r.next() * 0.07;
            let h = 0.26 + r.next() * 0.2;
            if z + w > -2.82 { break; }
            let depth = 0.32 + r.next() * 0.06;
            let book = BOOKS[(r.next() * BOOKS.len() as f64) as usize];
            o!(.bx(X0 + 0.06, y + 0.06, z, depth, h.min(0.5), w, book, 0.06));
            z += w + if r.next() < 0.12 { 0.08 } else { 0.006 };
        }
    }
    w!(.bx(X0, 1.46, -2.2, 0.07, 1.74, 3.0, 0x3a2620, 0.0));
    let board = screen(g, TEX_BOARD, 2.84, 1.6, v3(X0 + 0.075, 2.33, -0.7), std::f64::consts::FRAC_PI_2);
    w!(.bx(X0, 2.42, 1.22, 0.09, 0.6, 1.16, 0x0f0d12, 0.0));
    let clock = screen(g, TEX_CLOCK, 1.04, 0.48, v3(X0 + 0.095, 2.72, 1.8), std::f64::consts::FRAC_PI_2);
    let clock_glow = glow(g, ROOT, 0xff7a2a, 1.6, 0.35, v3(X0 + 0.3, 2.72, 1.8));
    w!(.bx(X0, 1.6, 3.55, 0.06, 1.02, 1.42, 0x2e1c18, 0.0).bx(X0 + 0.06, 1.68, 3.63, 0.01, 0.86, 1.26, 0x41628f, 0.0)
        .bx(X0 + 0.07, 1.68, 3.63, 0.01, 0.3, 1.26, 0x3f6a44, 0.0).bx(X0 + 0.075, 1.9, 3.75, 0.01, 0.3, 0.5, 0x5a7a5a, 0.0)
        .bx(X0 + 0.075, 1.95, 4.2, 0.01, 0.42, 0.55, 0x6a8a6a, 0.0).bx(X0 + 0.08, 2.24, 4.45, 0.01, 0.13, 0.13, 0xffd070, 0.0)
        .bx(X0 + 0.075, 2.28, 4.3, 0.01, 0.09, 0.3, 0xe8f0ff, 0.0));

    // Lounge.
    o!(.bx(-6.8, 0.0, 2.95, 2.8, 0.02, 2.5, 0x6f4430, 0.02).bx(-6.5, 0.02, 3.25, 2.2, 0.01, 1.9, 0x8a5a3c, 0.02));
    o!(.b(X0 + 0.05, 0.0, 3.2, 0.95, 0.42, 2.1, 0x5b3a6a).b(X0 + 0.05, 0.42, 3.2, 0.26, 0.55, 2.1, 0x4f3160)
        .b(X0 + 0.05, 0.42, 3.02, 0.95, 0.24, 0.2, 0x553565).b(X0 + 0.05, 0.42, 5.28, 0.95, 0.24, 0.2, 0x553565)
        .b(X0 + 0.32, 0.42, 3.24, 0.66, 0.1, 0.98, 0x6b4a7a).b(X0 + 0.32, 0.42, 4.28, 0.66, 0.1, 0.98, 0x6b4a7a)
        .b(X0 + 0.33, 0.52, 3.4, 0.14, 0.34, 0.42, 0xd9a64a).b(X0 + 0.33, 0.52, 4.8, 0.14, 0.3, 0.36, 0x5aa8a0));
    o!(.b(-5.55, 0.32, 3.7, 0.8, 0.06, 1.25, 0x6b4a36).b(-5.5, 0.0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).b(-4.85, 0.0, 3.75, 0.06, 0.32, 0.06, 0x3a261c)
        .b(-5.5, 0.0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).b(-4.85, 0.0, 4.84, 0.06, 0.32, 0.06, 0x3a261c)
        .b(-5.3, 0.38, 3.9, 0.3, 0.05, 0.4, 0x3a5a8a).b(-5.0, 0.38, 4.5, 0.12, 0.14, 0.12, 0xeeeeee));
    o!(.b(-6.75, 0.0, 2.55, 0.26, 0.04, 0.26, 0x241a16).b(-6.64, 0.04, 2.66, 0.04, 1.6, 0.04, 0x241a16));
    let floor_shade = g.boxm(ROOT, 0.44, 0.3, 0.44, -6.62, 1.78, 2.68, Mat::basic(0xffc27a, false), false);

    // Beanbags.
    o!(.b(4.7, 0.0, 3.8, 0.9, 0.3, 0.9, 0x7a4a9a).b(4.8, 0.3, 3.9, 0.7, 0.14, 0.7, 0x8a5aaa).b(4.75, 0.3, 3.82, 0.2, 0.34, 0.84, 0x6a3a8a)
        .b(5.9, 0.0, 4.4, 0.8, 0.28, 0.8, 0x2f8a7a).b(6.0, 0.28, 4.5, 0.6, 0.12, 0.6, 0x3a9a8a));

    {
        let mut v = Vox { b: std::mem::take(&mut room), r: &mut *r };
        plant(&mut v, 6.4, Z0 + 0.55, 1.6, 0.0, 3);
        plant(&mut v, -0.6, Z0 + 0.5, 1.2, 0.0, 5);
        plant(&mut v, 6.4, 5.0, 1.5, 0.0, 7);
        plant(&mut v, -3.3, 5.0, 1.0, 0.0, 9);
        plant(&mut v, -3.95, Z0 + 0.3, 0.55, 0.92, 4);
        room = v.b;
    }

    // Desks.
    for z in [-1.6, 2.1] {
        o!(.bx(-4.9, 0.0, z - 1.15, 10.4, 0.015, 2.3, if z < 0.0 { 0x3f4a3a } else { 0x6e4a2a }, 0.02).bx(-4.7, 0.015, z - 0.95, 10.0, 0.006, 1.9, if z < 0.0 { 0x4a5846 } else { 0x7e5634 }, 0.02));
    }
    let (mut shades, mut lamps, mut desk_glows) = (vec![], vec![], vec![]);
    for (di, &(x, z)) in DESKS.iter().enumerate() {
        o!(.bx(x - 0.42, 0.64, z - 0.78, 0.84, 0.07, 1.56, 0x6e4c37, 0.02));
        for (lx, lz) in [(-0.38, -0.74), (0.3, -0.74), (-0.38, 0.68), (0.3, 0.68)] { o!(.bx(x + lx, 0.0, z + lz, 0.07, 0.64, 0.07, 0x3e2a1f, 0.0)); }
        o!(.b(x - 0.3, 0.12, z + 0.3, 0.66, 0.5, 0.42, 0x5e4030).bx(x - 0.31, 0.38, z + 0.36, 0.01, 0.04, 0.3, 0xc8a24a, 0.0));
        o!(.b(x + 0.02, 0.71, z - 0.14, 0.24, 0.03, 0.28, 0x1c1c24).b(x + 0.1, 0.74, z - 0.04, 0.06, 0.2, 0.08, 0x1c1c24)
            .b(x + 0.02, 0.88, z - 0.4, 0.09, 0.46, 0.8, 0x1a1d28).bx(x + 0.11, 0.92, z - 0.12, 0.02, 0.26, 0.24, 0x2a2f3e, 0.0)
            .b(x - 0.36, 0.71, z - 0.26, 0.17, 0.025, 0.52, 0x2c3040).bx(x - 0.34, 0.735, z - 0.24, 0.13, 0.008, 0.48, 0x454a5e, 0.0)
            .b(x - 0.34, 0.71, z + 0.36, 0.1, 0.03, 0.07, 0x2c3040).bx(x - 0.1, 0.71, z + 0.3, 0.26, 0.04, 0.34, 0xece6da, 0.02)
            .b(x + 0.12, 0.71, z + 0.55, 0.11, 0.13, 0.11, [0xeeeeee, 0x9b6bff, 0xff9a4a][di % 3]));
        o!(.b(x + 0.14, 0.71, z - 0.65, 0.18, 0.03, 0.18, 0x2a2a30).b(x + 0.21, 0.74, z - 0.58, 0.04, 0.4, 0.04, 0x2a2a30));
        shades.push(g.boxm(ROOT, 0.26, 0.14, 0.26, x + 0.23, 1.16, z - 0.56, Mat::basic(0xffc27a, false), false));
        lamps.push((v3(x - 0.1, 1.1, z - 0.3), 0.0));
        let s = x - 0.67;
        o!(.b(s - 0.24, 0.38, z - 0.24, 0.48, 0.07, 0.48, 0x3b2d4c).b(s - 0.29, 0.45, z - 0.22, 0.07, 0.54, 0.44, 0x33263f)
            .b(s - 0.03, 0.08, z - 0.03, 0.06, 0.3, 0.06, 0x1c1c22).b(s - 0.22, 0.04, z - 0.03, 0.44, 0.04, 0.06, 0x1c1c22).b(s - 0.03, 0.04, z - 0.22, 0.06, 0.04, 0.44, 0x1c1c22));
        desk_glows.push(glow(g, ROOT, 0x7fb8ff, 1.3, 0.0, v3(x - 0.22, 1.02, z)));
    }

    // scene.add(walls.mesh(false), room.mesh(true)).
    g.merged.push(walls);
    let wm = g.drawing(ROOT, Geo::Merged(g.merged.len() - 1), Mat::Std { color: Rgb(1.0, 1.0, 1.0), rough: 0.88, metal: 0.0, vertex: true });
    g.nodes[wm].receive = true;
    g.merged.push(room);
    let rm = g.drawing(ROOT, Geo::Merged(g.merged.len() - 1), Mat::Std { color: Rgb(1.0, 1.0, 1.0), rough: 0.88, metal: 0.0, vertex: true });
    g.nodes[rm].receive = true;
    g.nodes[rm].cast = true;

    // The vacuum.
    let vac = g.add(ROOT, V3::default());
    let a = g.drawing(vac, Geo::Cylinder { top: 0.27, bottom: 0.28, h: 0.08, seg: 24 }, Mat::std(Rgb::hex(0x2a2a33), 0.5, 0.0));
    g.nodes[a].p.y = 0.05;
    g.nodes[a].cast = true;
    g.nodes[a].receive = true;
    let b = g.drawing(vac, Geo::Cylinder { top: 0.17, bottom: 0.17, h: 0.02, seg: 20 }, Mat::std(Rgb::hex(0x4a4a58), 0.4, 0.0));
    g.nodes[b].p.y = 0.1;
    g.nodes[b].receive = true;
    glow(g, vac, 0x4ade80, 0.22, 0.9, v3(0.0, 0.13, 0.2));

    // Light through the window.
    let patch = quad(g, [v3(0.3, 0.02, Z0 + 1.2), v3(2.9, 0.02, Z0 + 1.2), v3(3.9, 0.02, Z0 + 3.7), v3(1.3, 0.02, Z0 + 3.7)], TEX_PATCH);
    let beam = quad(g, [v3(0.2, 3.25, Z0 + 0.02), v3(2.8, 3.25, Z0 + 0.02), v3(3.9, 0.02, Z0 + 3.7), v3(1.3, 0.02, Z0 + 3.7)], TEX_BEAM);
    let mut dr = Rng::new(4);
    let pts: Vec<V3> = (0..46).map(|_| {
        let f = dr.next();
        let x = 0.4 + dr.next() * 2.4 + f * 1.1;
        let y = 3.1 * (1.0 - f) + dr.next() * 0.3;
        let z = Z0 + 0.3 + f * 3.2;
        v3(x, y, z)
    }).collect();
    let dust = g.drawing(ROOT, Geo::Points(pts), Mat::Points { color: Rgb::hex(0xffe2a8), opacity: 0.8 });

    Room { door, exit_glow, sky, tv, board, clock, tv_glow, clock_glow, coffee_led, steam, shades, floor_shade, lamps, desk_glows, vac, patch, beam, dust }
}
