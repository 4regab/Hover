//! main.js's "Helpers: a session's subagents". While a session has subagents out, each is
//! a small bot around its desk doing paperwork: writing on a clipboard, stamping it,
//! turning the page and handing a sheet in to the desk's tray. They hop out of the
//! session's bot and back into it when done. Each is the session bot's colour turned
//! round the hue wheel, with its own eye colour and a cap, so they read as its team and
//! not as sessions of their own (they can't be clicked; the desk and the bot still can).

use crate::js::{ang_to, ease, Rng};
use crate::m::{v3, Rgb, V3};
use crate::scene::{Graph, Mat, DESKS, ROOT};
use std::f64::consts::{FRAC_PI_4, PI};

pub const MINI: f64 = 0.46;
const MINI_HUE: [f64; 4] = [0.5, 0.17, -0.17, 0.33];
const MINI_EYE: [u32; 4] = [0xffe08a, 0xaaf6ff, 0xc8ffb0, 0xffc8ea];
/// Where they stand, from the desk's centre: on the camera's sides of it, clear of the
/// chair and of the walk between the rows.
pub const MINI_SPOTS: [(f64, f64); 4] = [(0.72, -0.36), (0.72, 0.42), (0.06, 1.08), (-0.62, 1.02)];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Duty { Write, Stamp, Flip, File }
/// What a helper does, in turn, and for how long (s). Each starts at a different point.
const DUTIES: [(Duty, f64); 6] = [(Duty::Write, 2.6), (Duty::Stamp, 1.6), (Duty::Write, 2.2), (Duty::Flip, 0.9), (Duty::Write, 1.8), (Duty::File, 1.5)];

fn smooth(f: f64) -> f64 { f * f * (3.0 - 2.0 * f) }

/// The three body colours of the helper in `slot` for a session bot of colour `bot`:
/// the bot's hue turned, a little lighter.
pub fn palette(bot: Rgb, slot: usize) -> [Rgb; 3] {
    let (h, s, l) = bot.hsl();
    let main = Rgb::from_hsl((h + MINI_HUE[slot] + 1.0) % 1.0, (s * 0.9).min(1.0), (l + 0.06).min(0.7));
    [main, main.mul(0.5), main.lerp(Rgb(1.0, 1.0, 1.0), 0.45)]
}

/// Where the helper in `slot` stands at `desk`.
pub fn spot(desk: usize, slot: usize) -> (f64, f64) {
    let d = DESKS[desk];
    (d.0 + MINI_SPOTS[slot].0, d.1 + MINI_SPOTS[slot].1)
}

#[derive(Clone, Copy)]
enum Tint { Main, Dark, Pale, Eye }

#[derive(Clone, Copy, Default)]
struct Pose { lean: f64, hx: f64, hy: f64, al: f64, ar: f64, sl: f64, sr: f64, leg: f64 }

pub struct Mini {
    /// The session it works for.
    pub sid: i64,
    pub desk: usize,
    pub slot: usize,
    t: f64,
    /// 0 in the bot, 1 at its post: it hops between.
    pub out: f64,
    pub leaving: bool,
    pub gone: bool,
    pub duty: usize,
    duty_t: f64,
    tossed: bool,
    p: Pose,
    pub yaw: f64,
    /// Where its bot was last seen, for the hop back when the bot is gone.
    bot_at: (f64, f64),
    pub root: usize,
    legs: [usize; 2], upper: usize, head: usize, arms: [usize; 2],
    pencil: usize, stamp: usize, board: usize, page: usize, mark: usize,
    tints: Vec<(usize, Tint)>,
    /// The main colour, as the office's tags and the desk card show it.
    pub color: Rgb,
}

impl Mini {
    /// `new Mini(s, slot)`: the helper's nodes, hidden until it hops out of its bot.
    pub fn new(g: &mut Graph, r: &mut Rng, sid: i64, desk: usize, slot: usize, bot: Rgb) -> Mini {
        let [c_main, c_dark, c_pale] = palette(bot, slot);
        let sd = |c: Rgb, rough: f64| Mat::std(c, rough, 0.0);
        let (main, dark, pale) = (sd(c_main, 0.5), sd(c_dark, 0.6), sd(c_pale, 0.45));
        let eye = Mat::basic(MINI_EYE[slot], false);
        let visor = Mat::std(Rgb::hex(0x111018), 0.22, 0.35);
        let paper = || Mat::std(Rgb::hex(0xf4efe4), 0.9, 0.0);
        let ink = || Mat::basic(0xd8443a, true);
        let mut tints: Vec<(usize, Tint)> = vec![];
        // A box that takes the body's colour: remembered, to be coloured again when the
        // helper is used for another session.
        macro_rules! bx { ($p:expr, $w:expr, $h:expr, $d:expr, $x:expr, $y:expr, $z:expr, $t:expr, $m:expr, $s:expr) => {{
            let n = g.boxm($p, $w, $h, $d, $x, $y, $z, $m.clone(), $s);
            tints.push((n, $t));
            n
        }}; }
        let root = g.add(ROOT, V3::default());
        g.nodes[root].s = v3(0.001, 0.001, 0.001);
        g.nodes[root].visible = false;
        let hips = g.pivot(root, 0.0, 0.24, 0.0);
        let legs = [-1.0, 1.0].map(|k| {
            let p = g.pivot(hips, k * 0.1, 0.0, 0.0);
            bx!(p, 0.13, 0.2, 0.15, 0.0, -0.1, 0.0, Tint::Dark, dark, true);
            bx!(p, 0.15, 0.06, 0.21, 0.0, -0.21, 0.03, Tint::Pale, pale, true);
            p
        });
        let upper = g.pivot(hips, 0.0, 0.0, 0.0);
        bx!(upper, 0.42, 0.3, 0.3, 0.0, 0.15, 0.0, Tint::Dark, dark, true);
        bx!(upper, 0.22, 0.13, 0.02, 0.0, 0.17, 0.155, Tint::Pale, pale, true);
        let head = g.pivot(upper, 0.0, 0.3, 0.0);
        bx!(head, 0.58, 0.44, 0.48, 0.0, 0.22, 0.0, Tint::Main, main, true);
        g.boxm(head, 0.46, 0.28, 0.02, 0.0, 0.21, 0.245, visor, false);
        for k in [-1.0, 1.0] {
            bx!(head, 0.07, 0.2, 0.22, k * 0.315, 0.22, 0.0, Tint::Dark, dark, true);
            bx!(head, 0.075, 0.11, 0.01, k * 0.1, 0.21, 0.258, Tint::Eye, eye, false);
        }
        // The cap and its brim.
        bx!(head, 0.62, 0.07, 0.52, 0.0, 0.47, 0.0, Tint::Pale, pale, true);
        bx!(head, 0.46, 0.03, 0.16, 0.0, 0.455, 0.32, Tint::Pale, pale, true);
        bx!(head, 0.03, 0.12, 0.03, -0.14, 0.56, -0.08, Tint::Dark, dark, true);
        bx!(head, 0.08, 0.08, 0.08, -0.14, 0.65, -0.08, Tint::Eye, eye, false);
        let arms = [-1.0, 1.0].map(|k| {
            let p = g.pivot(upper, k * 0.27, 0.27, 0.0);
            bx!(p, 0.1, 0.22, 0.12, 0.0, -0.1, 0.0, Tint::Main, main, true);
            bx!(p, 0.11, 0.07, 0.13, 0.0, -0.23, 0.0, Tint::Pale, pale, true);
            p
        });
        let pencil = g.boxm(arms[1], 0.035, 0.035, 0.18, 0.0, -0.26, 0.07, Mat::std(Rgb::hex(0xf2c14a), 0.6, 0.0), true);
        let stamp = g.pivot(arms[1], 0.0, -0.29, 0.02);
        bx!(stamp, 0.05, 0.1, 0.05, 0.0, 0.0, 0.0, Tint::Dark, dark, true);
        g.boxm(stamp, 0.12, 0.04, 0.12, 0.0, -0.06, 0.0, ink(), true);
        // The clipboard is held against the chest, tilted up towards the face. Its top
        // sheet takes the stamp's mark and turns over the clip, leaving a clean one.
        let board = g.pivot(upper, 0.0, 0.13, 0.27);
        g.nodes[board].r.x = -0.55;
        g.boxm(board, 0.34, 0.02, 0.42, 0.0, 0.0, 0.0, Mat::std(Rgb::hex(0x8a5a34), 0.8, 0.0), true);
        g.boxm(board, 0.3, 0.012, 0.36, 0.0, 0.016, -0.01, paper(), true);
        g.boxm(board, 0.13, 0.035, 0.05, 0.0, 0.026, 0.19, Mat::std(Rgb::hex(0x9aa0aa), 0.35, 0.6), true);
        let page = g.pivot(board, 0.0, 0.025, 0.17);
        g.boxm(page, 0.3, 0.006, 0.36, 0.0, 0.0, -0.18, paper(), true);
        let mark = g.boxm(page, 0.09, 0.004, 0.07, 0.05, 0.005, -0.24, ink(), false);
        g.nodes[mark].visible = false;
        let mut m = Mini {
            sid, desk, slot, t: 0.0, out: 0.0, leaving: false, gone: false, duty: 0, duty_t: 0.0, tossed: false, p: Pose::default(), yaw: 0.0,
            bot_at: (0.0, 0.0), root, legs, upper, head, arms, pencil, stamp, board, page, mark, tints, color: c_main,
        };
        m.renew(g, r, sid, desk, slot, bot);
        m
    }

    /// A helper that was done, used again for another: its nodes are coloured again and it
    /// starts over inside its bot, so the scene doesn't grow with each subagent the office
    /// sees. The random draw is the one `new` makes.
    pub fn renew(&mut self, g: &mut Graph, r: &mut Rng, sid: i64, desk: usize, slot: usize, bot: Rgb) {
        self.sid = sid;
        self.desk = desk;
        self.slot = slot;
        self.t = r.next() * 10.0;
        self.out = 0.0;
        self.leaving = false;
        self.gone = false;
        self.duty = slot * 2 % DUTIES.len();
        self.duty_t = 0.0;
        self.tossed = false;
        self.p = Pose::default();
        self.yaw = 0.0;
        let [main, dark, pale] = palette(bot, slot);
        self.color = main;
        for &(n, t) in &self.tints {
            let c = match t { Tint::Main => main, Tint::Dark => dark, Tint::Pale => pale, Tint::Eye => Rgb::hex(MINI_EYE[slot]) };
            match g.mat_mut(n) { Mat::Std { color, .. } | Mat::Basic { color, .. } => *color = c, _ => {} }
        }
        g.nodes[self.page].r.x = 0.0;
        g.nodes[self.mark].visible = false;
        g.nodes[self.root].s = v3(0.001, 0.001, 0.001);
        g.nodes[self.root].visible = true;
    }

    /// dispose(): out of the scene; the nodes stay, for `renew`.
    pub fn dispose(&self, g: &mut Graph) { g.nodes[self.root].visible = false; }

    /// Where it stands.
    pub fn spot(&self) -> (f64, f64) { spot(self.desk, self.slot) }

    /// Where it is now.
    pub fn at(&self, g: &Graph) -> V3 { g.nodes[self.root].p }

    pub fn duty_now(&self) -> Duty { DUTIES[self.duty].0 }

    /// One step. `bot` is where its session's bot sits, if the session is still there.
    /// Returns a sheet it hands in this step: where from, and to the desk's tray.
    pub fn step(&mut self, g: &mut Graph, dt: f64, still: bool, bot: Option<(f64, f64)>) -> Option<(V3, V3)> {
        if let Some(b) = bot { self.bot_at = b; }
        self.t += dt;
        let t = self.t;
        self.out = if still { if self.leaving { 0.0 } else { 1.0 } } else { (self.out + dt / 0.6 * if self.leaving { -1.0 } else { 1.0 }).clamp(0.0, 1.0) };
        if self.leaving && self.out <= 0.0 { self.gone = true; return None; }
        let (px, pz) = self.spot();
        let d = DESKS[self.desk];
        let (bx, bz) = (self.bot_at.0 + 0.32, self.bot_at.1);
        let (f, hop) = (smooth(self.out), self.out < 1.0);
        g.nodes[self.root].p = v3(bx + (px - bx) * f, (PI * self.out).sin() * 0.42, bz + (pz - bz) * f);
        let sc = MINI * (self.out * 1.8).clamp(0.001, 1.0);
        g.nodes[self.root].s = v3(sc, sc, sc);
        // At work it turns three-quarters to the room (the camera looks from +x, +z), so
        // its clipboard, pencil and stamp show rather than its back.
        let work = FRAC_PI_4 + if self.slot % 2 != 0 { 0.55 } else { -0.55 };
        let face = if !hop { work } else if self.leaving { (bx - px).atan2(bz - pz) } else { (px - bx).atan2(pz - bz) };
        self.yaw = if self.out < 0.05 && !self.leaving { face } else { ang_to(self.yaw, face, 10.0, dt) };

        let (mut lean, mut hx, mut hy, mut al, mut sl, mut sr) = (0.0, 0.28, 0.0, -1.0, 0.35, -0.3);
        // Every branch below sets these two.
        let (ar, leg);
        let mut toss = None;
        let mut duty = Duty::Write;
        if hop {
            al = -2.7; ar = -2.7; sl = -0.3; sr = 0.3; hx = -0.15; leg = -0.6 * (PI * self.out).sin();
        } else {
            self.duty_t += dt;
            let (mut n, len) = DUTIES[self.duty];
            if self.duty_t >= len {
                // A new page after the turn, and a fresh hand-in after a file.
                if n == Duty::Flip { g.nodes[self.page].r.x = 0.0; g.nodes[self.mark].visible = false; }
                self.duty = (self.duty + 1) % DUTIES.len();
                self.duty_t = 0.0;
                self.tossed = false;
                n = DUTIES[self.duty].0;
            }
            let len = DUTIES[self.duty].1;
            duty = n;
            let (u, w) = (self.duty_t, if still { 0.0 } else { 1.0 });
            match n {
                Duty::Write => {
                    ar = -1.15 + (t * 16.0).sin() * 0.07 * w;
                    sr = -0.32 + (t * 6.5).sin() * 0.09 * w;
                    hy = (t * 0.8).sin() * 0.08 * w;
                }
                Duty::Stamp => {
                    // Up, down hard, a beat on the paper; twice.
                    let c = (u / 0.8) % 1.0;
                    ar = if c < 0.55 { -1.1 - smooth(c / 0.55) * 1.1 } else if c < 0.68 { -2.2 + (c - 0.55) / 0.13 * 1.15 } else { -1.05 };
                    sr = -0.34;
                    lean = if (0.68..0.8).contains(&c) { 0.06 } else { 0.0 };
                    hx = 0.34;
                    if c >= 0.68 { g.nodes[self.mark].visible = true; }
                }
                Duty::Flip => {
                    ar = -1.7; sr = -0.4; hx = 0.18;
                    g.nodes[self.page].r.x = smooth((u / (len * 0.75)).min(1.0)) * PI;
                }
                Duty::File => {
                    // Lifts the board, sends the top sheet to the desk's tray, and bows a little.
                    al = -1.45; ar = -1.6; hx = 0.05;
                    lean = if u > 0.5 && u < 1.1 { -0.08 } else { 0.0 };
                    if !self.tossed && u > 0.35 {
                        self.tossed = true;
                        if !still {
                            // The pose is set below; the board is where last step left it.
                            toss = Some((g.world_at(self.board).point(V3::default()), v3(d.0 + 0.03, 0.77, d.1 + 0.47)));
                        }
                    }
                }
            }
            leg = (t * 2.2 + self.slot as f64).sin().max(0.0) * 0.12 * w;
        }
        let k = if still { 30.0 } else { 14.0 };
        let p = &mut self.p;
        for (v, to) in [(&mut p.lean, lean), (&mut p.hx, hx), (&mut p.hy, hy), (&mut p.al, al), (&mut p.ar, ar), (&mut p.sl, sl), (&mut p.sr, sr), (&mut p.leg, leg)] { *v = ease(*v, to, k, dt); }
        g.nodes[self.root].r.y = self.yaw;
        if !hop && !still { g.nodes[self.root].p.y += (t * 3.0 + self.slot as f64).sin() * 0.008; }
        g.nodes[self.legs[0]].r.x = p.leg;
        g.nodes[self.legs[1]].r.x = if hop { p.leg } else { -p.leg * 0.3 };
        g.nodes[self.upper].r.x = p.lean;
        g.nodes[self.head].r = v3(p.hx, p.hy, 0.0);
        g.nodes[self.arms[0]].r = v3(p.al, 0.0, p.sl);
        g.nodes[self.arms[1]].r = v3(p.ar, 0.0, p.sr);
        // The clipboard comes out once it has landed, and goes away before it hops back.
        g.nodes[self.board].visible = !hop;
        g.nodes[self.pencil].visible = !hop && duty != Duty::Stamp;
        g.nodes[self.stamp].visible = !hop && duty == Duty::Stamp;
        let blink = if t % 4.1 < 0.12 { 0.3 } else { 1.0 };
        for &(n, tint) in &self.tints {
            if let (Tint::Eye, Mat::Basic { color, .. }) = (tint, g.mat_mut(n)) { *color = Rgb::hex(MINI_EYE[self.slot]).mul(blink); }
        }
        toss
    }
}

/// A sheet on its way from a clipboard to the desk's tray, in an arc.
struct Sheet { node: usize, from: V3, to: V3, t: f64 }

/// Every helper of the office, and the sheets they hand in. Helpers and sheets that are
/// done go to a spare list and are used again, so the scene stops growing once the
/// busiest moment has had its nodes.
#[derive(Default)]
pub struct Crew {
    pub minis: Vec<Mini>,
    spare: Vec<Mini>,
    sheets: Vec<Sheet>,
    spare_sheets: Vec<usize>,
}

impl Crew {
    /// syncMinis: the session `sid` has `want` subagents out. The newest helpers past that
    /// hop back into their bot; missing ones hop out, each to the first free place.
    pub fn sync(&mut self, g: &mut Graph, r: &mut Rng, sid: i64, desk: usize, bot: Rgb, want: usize) {
        let want = want.min(MINI_SPOTS.len());
        let mine: Vec<usize> = self.minis.iter().enumerate().filter(|(_, m)| m.sid == sid && !m.leaving).map(|(i, _)| i).collect();
        for &i in mine.iter().skip(want) { self.minis[i].leaving = true; }
        for _ in mine.len()..want {
            let used: Vec<usize> = self.minis.iter().filter(|m| m.sid == sid).map(|m| m.slot).collect();
            let Some(slot) = (0..MINI_SPOTS.len()).find(|i| !used.contains(i)) else { break };
            let m = match self.spare.pop() {
                Some(mut m) => { m.renew(g, r, sid, desk, slot, bot); m }
                None => Mini::new(g, r, sid, desk, slot, bot),
            };
            self.minis.push(m);
        }
    }

    /// The session is gone: its helpers go back into its bot.
    pub fn leave(&mut self, sid: i64) { for m in self.minis.iter_mut().filter(|m| m.sid == sid) { m.leaving = true; } }

    /// The frame loop's part: every helper one step, the ones that are back in their bot
    /// put away, the sheets handed in. `bot_at` says where a session's bot sits.
    pub fn step(&mut self, g: &mut Graph, dt: f64, still: bool, bot_at: impl Fn(i64) -> Option<(f64, f64)>) {
        let mut tosses = vec![];
        for m in &mut self.minis { if let Some(t) = m.step(g, dt, still, bot_at(m.sid)) { tosses.push(t); } }
        for i in (0..self.minis.len()).rev() {
            if self.minis[i].gone { let m = self.minis.remove(i); m.dispose(g); self.spare.push(m); }
        }
        for (from, to) in tosses { self.toss(g, from, to); }
        self.step_sheets(g, dt);
    }

    fn toss(&mut self, g: &mut Graph, from: V3, to: V3) {
        let node = match self.spare_sheets.pop() {
            Some(n) => n,
            None => {
                let n = g.boxm(ROOT, 0.13, 0.006, 0.16, 0.0, 0.0, 0.0, Mat::std(Rgb::hex(0xf4efe4), 0.9, 0.0), true);
                g.nodes[n].receive = false;
                n
            }
        };
        g.nodes[node].visible = true;
        g.nodes[node].p = from;
        self.sheets.push(Sheet { node, from, to, t: 0.0 });
    }

    fn step_sheets(&mut self, g: &mut Graph, dt: f64) {
        for i in (0..self.sheets.len()).rev() {
            let s = &mut self.sheets[i];
            s.t += dt / 0.7;
            let f = s.t.min(1.0);
            let n = &mut g.nodes[s.node];
            n.p = s.from.add(s.to.sub(s.from).mul(f));
            n.p.y += (PI * f).sin() * 0.35;
            n.r = v3((f * 9.0).sin() * 0.4 * (1.0 - f), f * 4.0, 0.0);
            // It lies on the pile a moment, then is part of it.
            if s.t >= 1.4 { n.visible = false; self.spare_sheets.push(s.node); self.sheets.remove(i); }
        }
    }

    /// Something is hopping or in the air: the shadows are redrawn every frame.
    pub fn moving(&self) -> bool { !self.sheets.is_empty() || self.minis.iter().any(|m| m.out < 1.0) }

    /// Sheets on their way to a tray (or lying on it for a moment).
    pub fn sheets_in_air(&self) -> usize { self.sheets.len() }

    /// Any helper at all: they bob and write, so the office keeps its lively pace.
    pub fn any(&self) -> bool { !self.minis.is_empty() }

    /// The main colours of the helpers a session has out (those not on their way back).
    pub fn colors(&self, sid: i64) -> Vec<[u8; 3]> { self.minis.iter().filter(|m| m.sid == sid && !m.leaving).map(|m| m.color.css()).collect() }
}
