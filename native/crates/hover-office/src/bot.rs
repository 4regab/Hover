//! `class Bot`: the boxy mascot, its walk along a path, sitting, and a pose per stage
//! and act, eased as the page eases them.

use crate::js::{ang_to, ease, Rng};
use crate::m::{v3, Rgb, V3};
use crate::scene::{Blend, Geo, Graph, Hit, Mat, ROOT};

pub const BOTS: [(&str, u32); 6] = [("Pip", 0x9b6bff), ("Juno", 0x2fc9b0), ("Moss", 0xff9a4a), ("Nova", 0xff6fae), ("Ada", 0x5aa8ff), ("Rue", 0xb4e04a)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Waiting: the agent asks the user first (its hand up, the bulb blinking amber).
pub enum Stage { Waking, Working, Done, Failed, Stopped, Waiting }

impl Stage {
    pub fn parse(s: &str) -> Stage {
        match s { "working" => Stage::Working, "done" => Stage::Done, "failed" => Stage::Failed, "stopped" => Stage::Stopped, "waiting" => Stage::Waiting, _ => Stage::Waking }
    }
    pub fn bulb(self) -> u32 { [0xffd24a, 0xc4a2ff, 0x4ade80, 0xff5b52, 0x55505f, 0xffb340][self as usize] }
    /// SCREEN: the desk screen's light on the bot's face.
    pub fn screen(self) -> (u32, f64) { [(0x7fb8ff, 0.25), (0x7fb8ff, 0.6), (0x4ade80, 0.42), (0xff5b52, 0.5), (0, 0.0), (0xffb340, 0.6)][self as usize] }
    pub fn word(self) -> &'static str { ["Waking up", "Working", "Done", "Couldn’t finish", "Stopped", "Waiting for you"][self as usize] }
    pub fn busy(self) -> bool { matches!(self, Stage::Waking | Stage::Working | Stage::Waiting) }
}

#[derive(Clone, Copy, Default)]
struct Pose { lean: f64, hx: f64, hy: f64, hz: f64, al: f64, ar: f64, sl: f64, sr: f64, lx: f64, ly: f64 }

struct Eye { g: usize, open: usize, happy: usize, shut: usize, s: f64 }

pub struct Bot {
    pub name: &'static str,
    pub color: Rgb,
    pub css: [u8; 3],
    pub t: f64,
    pub since: f64,
    pub since_seat: f64,
    pub stage: Stage,
    pub act: Option<String>,
    pub x: f64, pub z: f64, face: f64, pub yaw: f64,
    pub path: Vec<(f64, f64)>,
    pub seated: bool,
    /// Seat on arrival (the page's `arrive` callbacks: sit down, or be gone).
    pub arrive: Option<Arrive>,
    sit: f64, walk: f64, phase: f64,
    pub hot: bool,
    p: Pose,
    pub root: usize, legs: [usize; 2], upper: usize, head: usize, arms: [usize; 2], eyes: Vec<Eye>,
    bulb: usize, halo: usize, ring: usize, ring_opacity: f64,
    pub hit: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrive { Sit, Gone }

fn std(c: Rgb, rough: f64) -> Mat { Mat::std(c, rough, 0.0) }

impl Bot {
    pub fn new(g: &mut Graph, r: &mut Rng, name: &'static str, color: u32, index: usize) -> Bot {
        let c = Rgb::hex(color);
        let t = r.next() * 10.0;
        let main = std(c, 0.5);
        let dark = std(c.mul(0.5), 0.6);
        let pale = std(c.lerp(Rgb(1.0, 1.0, 1.0), 0.4), 0.45);
        let visor = Mat::std(Rgb::hex(0x111018), 0.22, 0.35);
        let eye = Mat::basic(0xaaf6ff, false);
        let root = g.add(ROOT, V3::default());
        let hips = g.pivot(root, 0.0, 0.24, 0.0);
        let legs = [-1.0, 1.0].map(|s| {
            let p = g.pivot(hips, s * 0.1, 0.0, 0.0);
            g.boxm(p, 0.13, 0.2, 0.15, 0.0, -0.1, 0.0, dark.clone(), true);
            g.boxm(p, 0.15, 0.06, 0.21, 0.0, -0.21, 0.03, pale.clone(), true);
            p
        });
        let upper = g.pivot(hips, 0.0, 0.0, 0.0);
        g.boxm(upper, 0.42, 0.3, 0.3, 0.0, 0.15, 0.0, dark.clone(), true);
        g.boxm(upper, 0.22, 0.13, 0.02, 0.0, 0.17, 0.155, pale.clone(), true);
        let head = g.pivot(upper, 0.0, 0.3, 0.0);
        g.boxm(head, 0.58, 0.44, 0.48, 0.0, 0.22, 0.0, main.clone(), true);
        g.boxm(head, 0.5, 0.05, 0.4, 0.0, 0.465, 0.0, pale.clone(), true);
        let b = g.boxm(head, 0.5, 0.36, 0.4, 0.0, 0.22, -0.02, pale.clone(), true);
        g.nodes[b].s = v3(0.5, 0.36, 0.49);
        g.boxm(head, 0.46, 0.28, 0.02, 0.0, 0.21, 0.245, visor, false);
        for s in [-1.0, 1.0] {
            g.boxm(head, 0.07, 0.2, 0.22, s * 0.315, 0.22, 0.0, dark.clone(), true);
            g.boxm(head, 0.02, 0.1, 0.1, s * 0.355, 0.22, 0.0, pale.clone(), true);
        }
        g.boxm(head, 0.03, 0.14, 0.03, 0.14, 0.53, -0.08, dark.clone(), true);
        let bulb = g.boxm(head, 0.1, 0.1, 0.1, 0.14, 0.64, -0.08, Mat::basic(Stage::Waking.bulb(), false), false);
        let halo = g.drawing(head, Geo::Sprite, Mat::Glow { color: Rgb::hex(Stage::Waking.bulb()), opacity: 0.8 });
        g.nodes[halo].s = v3(0.55, 0.55, 0.55);
        g.nodes[halo].p = v3(0.14, 0.64, -0.08);
        let eyes = [-1.0, 1.0].map(|s| {
            let eg = g.pivot(head, s * 0.1, 0.21, 0.258);
            let open = g.boxm(eg, 0.075, 0.11, 0.01, 0.0, 0.0, 0.0, eye.clone(), false);
            let happy = g.add(eg, V3::default());
            let h1 = g.boxm(happy, 0.055, 0.024, 0.01, -0.018, 0.0, 0.0, eye.clone(), false);
            g.nodes[h1].r.z = 0.75;
            let h2 = g.boxm(happy, 0.055, 0.024, 0.01, 0.018, 0.0, 0.0, eye.clone(), false);
            g.nodes[h2].r.z = -0.75;
            let shut = g.boxm(eg, 0.085, 0.022, 0.01, 0.0, -0.02, 0.0, eye.clone(), false);
            Eye { g: eg, open, happy, shut, s }
        });
        let arms = [-1.0, 1.0].map(|s| {
            let p = g.pivot(upper, s * 0.27, 0.27, 0.0);
            g.boxm(p, 0.1, 0.22, 0.12, 0.0, -0.1, 0.0, main.clone(), true);
            g.boxm(p, 0.11, 0.07, 0.13, 0.0, -0.23, 0.0, pale.clone(), true);
            p
        });
        let hit = g.add(root, v3(0.0, 0.65, 0.0));
        g.nodes[hit].s = v3(0.8, 1.3, 0.8);
        g.nodes[hit].hit = Some(Hit::Bot(index));
        let ring = g.drawing(root, Geo::Ring { inner: 0.42, outer: 0.52, seg: 40 },
            Mat::Basic { color: c, tone: false, opacity: 0.0, blend: Blend::Normal, depth_write: false, tex: None, double: false });
        g.nodes[ring].r.x = -std::f64::consts::FRAC_PI_2;
        Bot {
            name, color: c, css: c.css(), t, since: 0.0, since_seat: 99.0, stage: Stage::Waking, act: None,
            x: 0.0, z: 0.0, face: 0.0, yaw: 0.0, path: vec![], seated: false, arrive: None, sit: 0.0, walk: 0.0, phase: 0.0, hot: false,
            p: Pose::default(), root, legs, upper, head, arms, eyes: eyes.into_iter().collect(), bulb, halo, ring, ring_opacity: 0.0, hit,
        }
    }

    pub fn place(&mut self, x: f64, z: f64, seated: bool) {
        self.x = x;
        self.z = z;
        self.seated = seated;
        self.sit = if seated { 1.0 } else { 0.0 };
        self.yaw = if seated { std::f64::consts::FRAC_PI_2 } else { std::f64::consts::PI };
        self.face = self.yaw;
        self.since_seat = 99.0;
    }

    pub fn go(&mut self, path: Vec<(f64, f64)>, arrive: Arrive) { self.path = path; self.seated = false; self.arrive = Some(arrive); }

    pub fn sync(&mut self, stage: Stage, act: Option<&str>) {
        if stage != self.stage { self.since = 0.0; }
        self.stage = stage;
        self.act = act.map(str::to_owned);
    }

    /// head3: where the name tag sits.
    pub fn head3(&self, g: &Graph) -> V3 { v3(self.x, g.nodes[self.root].p.y + if self.seated { 1.28 } else { 1.22 }, self.z) }

    /// One step. Returns the arrival reached this step, if any (the caller acts on it).
    pub fn step(&mut self, g: &mut Graph, dt: f64, still: bool) -> Option<Arrive> {
        use std::f64::consts::{FRAC_PI_2, PI};
        self.t += dt;
        self.since += dt;
        self.since_seat += dt;
        let t = self.t;
        let mut walking = false;
        let mut arrived = None;
        if !self.path.is_empty() && self.sit < 0.05 {
            let (px, pz) = self.path[0];
            let (dx, dz) = (px - self.x, pz - self.z);
            let d = dx.hypot(dz);
            let sp = 1.7 * dt;
            if d <= sp {
                self.x = px;
                self.z = pz;
                self.path.remove(0);
                if self.path.is_empty() { arrived = self.arrive.take(); }
            } else {
                self.x += dx / d * sp;
                self.z += dz / d * sp;
                self.face = dx.atan2(dz);
                walking = true;
            }
        }
        if arrived == Some(Arrive::Sit) { self.seated = true; self.since_seat = 0.0; }
        self.walk = ease(self.walk, if walking { 1.0 } else { 0.0 }, 10.0, dt);
        if walking { self.phase += dt * 11.0; }
        let seat_now = self.seated && self.path.is_empty();
        if seat_now { self.face = FRAC_PI_2; }
        self.sit = ease(self.sit, if seat_now { 1.0 } else { 0.0 }, 7.0, dt);
        self.yaw = ang_to(self.yaw, self.face, 9.0, dt);

        let mut q = Pose::default();
        let (mut eyes, mut blink_bulb, mut halo) = ("open", false, 0.8);
        let sw = self.phase.sin();
        let bulb = self.stage.bulb();
        if !seat_now {
            q.al = sw * 0.6 * self.walk;
            q.ar = -sw * 0.6 * self.walk;
            q.hx = 0.05;
            if self.stage == Stage::Done { eyes = "happy"; }
        } else {
            match self.stage {
                Stage::Waking => {
                    let w = self.since_seat;
                    if w < 1.6 { q.al = -2.9; q.ar = -2.9; q.sl = -0.35; q.sr = 0.35; q.lean = -0.14; q.hx = -0.25; eyes = if w < 0.6 { "shut" } else { "happy" }; }
                    else { q.al = -1.35; q.ar = -1.35; q.hx = 0.08; q.lx = (t * 1.3).sin() * 0.02; }
                    blink_bulb = true;
                }
                Stage::Working => {
                    halo = 0.5 + 0.35 * (t * 3.0).sin();
                    q.al = -1.4; q.ar = -1.4; q.lean = 0.08; q.hx = 0.12;
                    match self.act.as_deref() {
                        Some("Thinking") => { q.ar = -2.25; q.sr = 0.55; q.hx = -0.2; q.hz = (t * 0.9).sin() * 0.14; q.ly = 0.02; q.lx = 0.02; }
                        Some("Reading") => { q.hy = (t * 1.5).sin() * 0.14; q.lx = (t * 1.5).sin() * 0.022; q.ly = -0.012; }
                        Some("Editing") => { q.al = -1.4 + (t * 22.0).sin() * 0.14; q.ar = -1.4 + (t * 22.0 + 2.0).sin() * 0.14; q.hx = 0.16; }
                        Some("Running") => { q.al = -1.15; q.ar = -1.15; q.lean = 0.2; q.hx = 0.05; halo = if (t * 11.0).sin() > 0.0 { 0.9 } else { 0.35 }; }
                        _ => {}
                    }
                }
                Stage::Done => {
                    eyes = "happy";
                    if self.since < 1.8 { q.al = -3.0 + (t * 13.0).sin() * 0.3; q.ar = -3.0 - (t * 13.0).sin() * 0.3; q.sl = -0.3; q.sr = 0.3; q.lean = -0.1; q.hx = -0.2; }
                    else { q.al = -2.75; q.ar = -2.75; q.sl = 0.6; q.sr = -0.6; q.lean = -0.2; q.hx = -0.12; q.hz = (t * 0.7).sin() * 0.06; }
                }
                // Asking the user: a hand up and waving, the bulb blinking amber.
                Stage::Waiting => { blink_bulb = true; q.al = -1.4; q.ar = -2.95 + (t * 6.0).sin() * 0.22; q.sr = 0.35 + (t * 6.0).sin() * 0.12; q.lean = -0.06; q.hx = -0.12; }
                Stage::Failed => { eyes = "sad"; blink_bulb = true; q.al = -1.5; q.ar = -1.5; q.lean = 0.25; q.hx = 0.35; }
                Stage::Stopped => { eyes = "shut"; halo = 0.0; q.al = -1.55; q.ar = -1.55; q.lean = 0.45 + (t * 1.6).sin() * 0.02; q.hx = 0.42; q.hz = 0.1; }
            }
        }
        if eyes == "open" && t % 3.7 < 0.12 { eyes = "shut"; }
        let k = if still { 30.0 } else { 14.0 };
        let p = &mut self.p;
        for (v, to) in [(&mut p.lean, q.lean), (&mut p.hx, q.hx), (&mut p.hy, q.hy), (&mut p.hz, q.hz), (&mut p.al, q.al), (&mut p.ar, q.ar),
            (&mut p.sl, q.sl), (&mut p.sr, q.sr), (&mut p.lx, q.lx), (&mut p.ly, q.ly)] { *v = ease(*v, to, k, dt); }

        let bob = if seat_now { (t * 2.0).sin() * 0.006 } else { sw.abs() * 0.035 * self.walk };
        let ry = self.sit * 0.21 + bob;
        g.nodes[self.root].p = v3(self.x, ry, self.z);
        g.nodes[self.root].r.y = self.yaw;
        let leg_w = sw * 0.6 * self.walk;
        g.nodes[self.legs[0]].r.x = -PI / 2.0 * self.sit + leg_w;
        g.nodes[self.legs[1]].r.x = -PI / 2.0 * self.sit - leg_w;
        g.nodes[self.upper].r.x = p.lean;
        g.nodes[self.head].r = v3(p.hx, p.hy, p.hz);
        g.nodes[self.arms[0]].r = v3(p.al, 0.0, p.sl);
        g.nodes[self.arms[1]].r = v3(p.ar, 0.0, p.sr);
        for e in &self.eyes {
            g.nodes[e.g].p.x = e.s * 0.1 + p.lx;
            g.nodes[e.g].p.y = 0.21 + p.ly;
            g.nodes[e.open].visible = eyes == "open" || eyes == "sad";
            g.nodes[e.open].s.y = if eyes == "sad" { 0.065 } else { 0.11 };
            g.nodes[e.open].r.z = if eyes == "sad" { -e.s * 0.45 } else { 0.0 };
            g.nodes[e.happy].visible = eyes == "happy";
            g.nodes[e.shut].visible = eyes == "shut";
        }
        let on = !blink_bulb || (t * 9.0).sin() > -0.2;
        if let Mat::Basic { color, .. } = g.mat_mut(self.bulb) { *color = Rgb::hex(bulb).mul(if on { 1.0 } else { 0.35 }); }
        if let Mat::Glow { color, opacity } = g.mat_mut(self.halo) { *color = Rgb::hex(bulb); *opacity = if on { halo } else { 0.05 }; }
        g.nodes[self.ring].p.y = 0.02 - ry;
        self.ring_opacity = ease(self.ring_opacity, if self.hot { 0.95 } else { 0.0 }, 12.0, dt);
        if let Mat::Basic { opacity, .. } = g.mat_mut(self.ring) { *opacity = self.ring_opacity; }
        g.nodes[self.ring].visible = self.ring_opacity > 0.02;
        arrived
    }

    pub fn walking(&self) -> bool { !self.path.is_empty() }

    /// dispose(): hidden and out of the raycast; the nodes stay (a new bot makes new ones).
    pub fn dispose(&self, g: &mut Graph) {
        g.nodes[self.root].visible = false;
        g.nodes[self.hit].hit = None;
    }
}
