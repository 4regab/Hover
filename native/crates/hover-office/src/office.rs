//! The page's model (main.js from "Sessions" to "Frame loop"): the sessions Hover's
//! `state` message lists, a bot each at its desk (walking in and out), the time of day,
//! the camera and its user view, picking, the name tags, the wall pictures, and the
//! frame pacing. The page's DOM parts (drawer, panels, HUD) are the app's (Slint); what
//! they need is here as data.

use crate::bot::{Arrive, Bot, Stage, BOTS};
use crate::canvas::{inter, pixel, Align, Baseline, Canvas};
use crate::js::{ease, floor_i, Rng};
use crate::m::{v3, Rgb, M4, V3};
use crate::scene::{self, seat, Graph, Hit, Mat, Room, DESKS, DOOR, X0, Z0};
use hover_core::json::Json;

#[derive(Clone, Debug, PartialEq)]
pub struct Turn { pub prompt: String, pub stage: Stage, pub act: Option<String>, pub file: String, pub steps: usize, pub queued: bool, pub t0: f64, pub took: Option<f64> }

pub struct Session {
    pub id: i64,
    pub key: String,
    pub tool: String,
    pub bot: usize,
    pub desk: usize,
    pub title: String,
    pub folder: String,
    pub ctx: Option<f64>,
    pub turns: Vec<Turn>,
    pub act: Option<String>,
    pub pose: Option<String>,
    pub file: String,
    pub b: Bot,
    pub tag_text: String,
    pub tag_shown: f64,
}

impl Session {
    /// last(s): the newest turn that isn't waiting in the queue.
    pub fn last(&self) -> &Turn { self.turns.iter().rev().find(|t| !t.queued).unwrap_or(&self.turns[0]) }
    pub fn busy(&self) -> bool { self.last().stage.busy() }
    fn pose_of(&self) -> Option<&str> { self.pose.as_deref().or(self.act.as_deref()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Time { Night, Day }

struct Times { hemi: (u32, u32, f64), sun: (u32, f64), fill: (u32, f64), lamp: f64, exposure: f64, patch: (u32, f64), beam: (u32, f64), dust: f64, shade: u32 }

const NIGHT: Times = Times { hemi: (0x8a78b8, 0x2a1812, 1.05), sun: (0x8fa2ff, 0.6), fill: (0xffc8a0, 0.5), lamp: 3.4, exposure: 1.3, patch: (0x6f86ff, 0.1), beam: (0x6f86ff, 0.05), dust: 0.0, shade: 0xffc27a };
const DAY: Times = Times { hemi: (0xfff1de, 0x6a4a3a, 1.5), sun: (0xffdcaa, 3.2), fill: (0xfff0e0, 0.9), lamp: 0.0, exposure: 1.0, patch: (0xffc070, 0.42), beam: (0xffd79a, 0.13), dust: 0.8, shade: 0x8a7a66 };

/// The lights as the renderer takes them.
#[derive(Clone, Debug)]
pub struct Lights {
    pub hemi_sky: Rgb, pub hemi_ground: Rgb,
    pub sun_dir: V3, pub sun: Rgb, pub sun_view: M4,
    pub fill_dir: V3, pub fill: Rgb,
    /// Position, colour × intensity, distance, decay.
    pub points: Vec<(V3, Rgb, f64, f64)>,
    pub exposure: f64,
}

/// The six props that can be clicked (PROPS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop { Tv, Board, Clock, Window, Door, Shelf }

pub const PROPS: [(Prop, [f64; 6]); 6] = [
    (Prop::Tv, [5.4, 2.38, Z0 + 0.2, 2.6, 1.55, 0.35]),
    (Prop::Board, [X0 + 0.2, 2.33, -0.7, 0.35, 1.75, 3.0]),
    (Prop::Clock, [X0 + 0.2, 2.72, 1.8, 0.35, 0.62, 1.2]),
    (Prop::Window, [1.5, 2.3, Z0 + 0.2, 2.9, 2.1, 0.35]),
    (Prop::Door, [DOOR.0, 1.2, Z0 + 0.3, 1.3, 2.45, 0.5]),
    (Prop::Shelf, [X0 + 0.25, 1.21, -3.53, 0.5, 2.42, 1.6]),
];

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hover { Bot(i64), Prop(Prop) }

/// What a click asks the host (the page's postMessage) or the page itself to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Click { Open(i64), Panel(&'static str), Toast(String), NewTask, Time(Time), Fold, Nothing }

pub struct Office {
    pub g: Graph,
    pub room: Room,
    r: Rng,
    pub sessions: Vec<Session>,
    leaving: Vec<(Bot, usize)>,
    first_state: bool,
    pub time: Time,
    pub manual_time: Option<Time>,
    pub w: f64, pub h: f64, aspect: f64,
    pub cam: [f64; 4], cam_to: [f64; 4],
    pub user: [f64; 3],
    /// The drawer's open session, or a panel ('board', 'tv', 'history'): the camera aims there.
    pub sel: Option<i64>,
    pub drawer_open: bool,
    pub viewing: bool,
    pub panel: Option<&'static str>,
    pub dragging: bool,
    pub pointer: Option<(f64, f64)>,
    pub hovered: Option<Hover>,
    pub still: bool,
    pub clock_t: f64,
    tv_at: i64, clock_at: i64,
    pub lively: bool,
    poked: f64,
    now_ms: f64,
    shadow_at: f64,
    pub shadow_dirty: bool,
    /// Which canvases changed since the renderer last took them.
    pub dirty: [bool; 6],
    pub canvases: Vec<Canvas>,
    pub frames: u64,
    acc: f64,
    /// The local clock (ms since the epoch, and the offset in minutes) for the LED clock.
    pub wall_clock: Box<dyn Fn() -> (f64, i64) + Send>,
}

pub const ISO: V3 = v3(0.5932, 0.5102, 0.5932);
const RIGHT: V3 = v3(std::f64::consts::FRAC_1_SQRT_2, 0.0, -std::f64::consts::FRAC_1_SQRT_2);
const FWD: V3 = v3(-std::f64::consts::FRAC_1_SQRT_2, 0.0, -std::f64::consts::FRAC_1_SQRT_2);

fn iso() -> V3 { v3(1.0, 0.86, 1.0).norm().mul(40.0) }

impl Office {
    pub fn new(w: f64, h: f64, still: bool) -> Office {
        let mut g = Graph::new();
        let mut r = Rng::new(11);
        let room = scene::build(&mut g, &mut r);
        for (i, (_, at)) in PROPS.iter().enumerate() {
            let n = g.add(scene::ROOT, v3(at[0], at[1], at[2]));
            g.nodes[n].s = v3(at[3], at[4], at[5]);
            g.nodes[n].hit = Some(Hit::Prop(i));
        }
        let canvases = vec![Canvas::new(128, 96), Canvas::new(208, 118), Canvas::new(480, 280), Canvas::new(96, 44), Canvas::new(4, 64), Canvas::new(64, 64)];
        let mut o = Office {
            g, room, r, sessions: vec![], leaving: vec![], first_state: true, time: Time::Night, manual_time: None,
            w, h, aspect: w / h, cam: [0.0, 1.7, 0.0, 1.0], cam_to: [0.0, 1.7, 0.0, 1.0], user: [0.0, 0.0, 1.0],
            sel: None, drawer_open: false, viewing: false, panel: None, dragging: false, pointer: None, hovered: None, still,
            clock_t: 0.0, tv_at: -1, clock_at: -1, lively: true, poked: 0.0, now_ms: 0.0, shadow_at: -1.0, shadow_dirty: true,
            dirty: [true; 6], canvases, frames: 0, acc: 0.0,
            wall_clock: Box::new(|| {
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
                let t = hover_core::time::Stamp::now();
                (ms, hover_core::time::local_offset_min(t.ticks))
            }),
        };
        o.draw_textures();
        o
    }

    pub fn resize(&mut self, w: f64, h: f64) { self.w = w; self.h = h; self.aspect = w / h; }

    fn half_width(&self, zoom: f64) -> f64 { (6.2 * self.aspect).max(9.2) / zoom }

    // MARK: Time of day

    pub fn auto_time(hour: i64) -> Time { if (7..19).contains(&hour) { Time::Day } else { Time::Night } }

    pub fn apply_time(&mut self, t: Time) {
        self.time = t;
        let tt = self.times();
        for &s in self.room.shades.iter().chain([&self.room.floor_shade]) {
            if let Mat::Basic { color, .. } = self.g.mat_mut(s) { *color = Rgb::hex(tt.shade); }
        }
        if let Mat::Basic { color, opacity, .. } = self.g.mat_mut(self.room.patch) { *color = Rgb::hex(tt.patch.0); *opacity = tt.patch.1; }
        if let Mat::Basic { color, opacity, .. } = self.g.mat_mut(self.room.beam) { *color = Rgb::hex(tt.beam.0); *opacity = tt.beam.1; }
        if let Mat::Points { opacity, .. } = self.g.mat_mut(self.room.dust) { *opacity = tt.dust; }
        self.g.nodes[self.room.dust].visible = tt.dust > 0.0;
        self.draw_sky();
        self.draw_tv(0.0);
        self.shadow_dirty = true;
    }

    fn times(&self) -> &'static Times { if self.time == Time::Day { &DAY } else { &NIGHT } }

    pub fn lights(&self) -> Lights {
        let t = self.times();
        let sun_pos = v3(-1.5, 10.0, -12.0);
        let target = v3(1.5, 0.0, 1.5);
        let mut points: Vec<(V3, Rgb, f64, f64)> = self.room.lamps.iter().map(|(p, _)| (*p, Rgb::hex(0xffa860).mul(t.lamp), 4.2, 1.6)).collect();
        points.push((v3(-6.5, 1.6, 2.8), Rgb::hex(0xffa860).mul(t.lamp * 0.9), 5.0, 1.5));
        Lights {
            hemi_sky: Rgb::hex(t.hemi.0).mul(t.hemi.2), hemi_ground: Rgb::hex(t.hemi.1).mul(t.hemi.2),
            sun_dir: sun_pos.sub(target).norm(), sun: Rgb::hex(t.sun.0).mul(t.sun.1),
            sun_view: M4::look_at(sun_pos, target, v3(0.0, 1.0, 0.0)).rigid_inverse(),
            fill_dir: v3(8.0, 6.0, 10.0).norm(), fill: Rgb::hex(t.fill.0).mul(t.fill.1),
            points, exposure: t.exposure,
        }
    }

    // MARK: Sessions from Hover's state

    fn path_in(d: usize) -> Vec<(f64, f64)> { vec![(DOOR.0, 0.25), (seat(d) + 0.05, 0.25), (seat(d) + 0.05, DESKS[d].1)] }
    fn path_out(d: usize) -> Vec<(f64, f64)> { vec![(seat(d) + 0.05, 0.25), (DOOR.0, 0.25), (DOOR.0, Z0 + 0.1)] }

    /// fromHost: sessions added (walking in when new after the first state), updated, retired.
    pub fn state(&mut self, m: &Json) {
        let Some(list) = m.get("sessions").and_then(|x| x.items().ok()) else { return };
        let mut seen = vec![];
        for h in list {
            let id = h.get("id").and_then(|x| x.i64().ok()).unwrap_or(0);
            seen.push(id);
            let s = |k: &str| h.get(k).and_then(Json::as_str).unwrap_or("").to_owned();
            let now = (self.wall_clock)().0;
            let turns: Vec<Turn> = h.get("turns").and_then(|x| x.items().ok()).unwrap_or(&[]).iter().map(|t| Turn {
                prompt: t.get("prompt").and_then(Json::as_str).unwrap_or("").into(),
                stage: Stage::parse(t.get("stage").and_then(Json::as_str).unwrap_or("waking")),
                act: None,
                file: String::new(),
                steps: t.get("steps").and_then(|x| x.items().ok()).map_or(0, |v| v.len()),
                queued: t.get("queued").is_some_and(|q| *q == Json::Bool(true)),
                t0: t.get("t0").and_then(|x| x.f64().ok()).filter(|v| *v != 0.0).unwrap_or(now),
                took: t.get("took").and_then(|x| x.f64().ok()),
            }).collect();
            if turns.is_empty() { continue; }
            let (act, pose, file) = (h.get("act").and_then(Json::as_str).map(str::to_owned), h.get("pose").and_then(Json::as_str).map(str::to_owned), s("file"));
            let ctx = h.get("ctx").and_then(|x| x.f64().ok());
            if let Some(ex) = self.sessions.iter_mut().find(|x| x.id == id) {
                let was_len = ex.turns.len();
                let waking = ex.last().stage == Stage::Waking;
                if waking && was_len != turns.len() && ex.b.seated { ex.b.since_seat = 0.0; }
                ex.turns = turns;
                ex.title = s("title");
                ex.folder = s("folder");
                ex.ctx = ctx;
                ex.act = act;
                ex.pose = pose;
                ex.file = file;
            } else {
                let bot = h.get("bot").and_then(|x| x.i64().ok()).unwrap_or(0) as usize;
                let desk = h.get("seat").and_then(|x| x.i64().ok()).unwrap_or(0) as usize % DESKS.len();
                let (name, color) = BOTS[bot % BOTS.len()];
                let mut b = Bot::new(&mut self.g, &mut self.r, name, color, self.sessions.len());
                let walk_in = !self.first_state && turns.iter().rev().find(|t| !t.queued).unwrap_or(&turns[0]).stage == Stage::Waking;
                if walk_in { b.place(DOOR.0, Z0 + 0.1, false); b.go(Self::path_in(desk), Arrive::Sit); } else { b.place(seat(desk) + 0.05, DESKS[desk].1, true); }
                let sess = Session { id, key: s("key"), tool: s("tool"), bot, desk, title: s("title"), folder: s("folder"), ctx, turns, act, pose, file, b, tag_text: String::new(), tag_shown: 0.0 };
                let (stage, p) = (sess.last().stage, sess.pose_of().map(str::to_owned));
                let mut sess = sess;
                sess.b.sync(stage, p.as_deref());
                self.sessions.push(sess);
            }
        }
        let gone: Vec<i64> = self.sessions.iter().filter(|s| !seen.contains(&s.id)).map(|s| s.id).collect();
        for id in gone { self.retire(id); }
        self.first_state = false;
        self.draw_board();
        self.poke();
    }

    /// retire: the bot walks out and is gone; its desk is free once it has left.
    fn retire(&mut self, id: i64) {
        let Some(i) = self.sessions.iter().position(|s| s.id == id) else { return };
        let s = self.sessions.remove(i);
        let mut b = s.b;
        b.sync(Stage::Done, None);
        b.hot = false;
        b.go(Self::path_out(s.desk), Arrive::Gone);
        self.leaving.push((b, s.desk));
        if self.sel == Some(id) { self.drawer_open = false; self.sel = None; }
    }

    pub fn poke(&mut self) { self.poked = self.now_ms; }

    // MARK: The camera and the user's view

    pub fn camera(&self) -> (M4, M4) {
        let t = v3(self.cam[0], self.cam[1], self.cam[2]);
        let world = M4::look_at(t.add(iso()), t, v3(0.0, 1.0, 0.0));
        let w = self.half_width(self.cam[3]);
        let h = w / self.aspect;
        (world.rigid_inverse(), M4::ortho(-w, w, h, -h, 1.0, 90.0))
    }

    /// A move on screen (px, y up) as a move over the floor.
    fn over_floor(&self, dx: f64, dy: f64, zoom: f64) -> (f64, f64) {
        let w = 2.0 * self.half_width(zoom) / self.w;
        let sin_e = iso().y / iso().len();
        ((RIGHT.x * dx + FWD.x * dy / sin_e) * w, (RIGHT.z * dx + FWD.z * dy / sin_e) * w)
    }

    pub fn zoom_by(&mut self, k: f64, dx: f64, dy: f64) {
        let old = self.user[2];
        let nz = (old * k).clamp(0.85, 2.8);
        if nz == old { return; }
        let (a, b) = (self.over_floor(dx, dy, old), self.over_floor(dx, dy, nz));
        self.user[0] += a.0 - b.0;
        self.user[1] += a.1 - b.1;
        self.user[2] = nz;
        self.clamp_view();
    }

    pub fn clamp_view(&mut self) { self.user[0] = self.user[0].clamp(-6.0, 6.0); self.user[1] = self.user[1].clamp(-5.0, 5.0); }
    pub fn reset_view(&mut self) { self.user = [0.0, 0.0, 1.0]; }

    /// A drag by (dx, dy) screen px: the floor follows the pointer.
    pub fn drag(&mut self, dx: f64, dy: f64) {
        let g = self.over_floor(dx, -dy, self.user[2]);
        self.user[0] -= g.0;
        self.user[1] -= g.1;
        self.clamp_view();
    }

    // MARK: Picking

    /// Raycaster.setFromCamera for an orthographic camera, then the nearest hit box.
    pub fn pick(&mut self) {
        let hit = match (self.pointer, self.dragging) {
            (Some((px, py)), false) => self.ray(px, py),
            _ => None,
        };
        self.hovered = hit;
        let (drawer, sel) = (self.drawer_open, self.sel);
        for s in &mut self.sessions { s.b.hot = hit == Some(Hover::Bot(s.id)) || (drawer && Some(s.id) == sel); }
        let panes = [self.room.tv, self.room.board, self.room.clock, self.room.sky];
        for (k, p) in [Prop::Tv, Prop::Board, Prop::Clock, Prop::Window].iter().enumerate() {
            let on = hit == Some(Hover::Prop(*p));
            if let Mat::Basic { color, .. } = self.g.mat_mut(panes[k]) { let v = if on { 1.35 } else { 1.0 }; *color = Rgb(v, v, v); }
        }
    }

    fn ray(&self, px: f64, py: f64) -> Option<Hover> {
        let (view, proj) = self.camera();
        let inv = proj.mul(&view).inverse();
        let (nx, ny) = (px / self.w * 2.0 - 1.0, -(py / self.h) * 2.0 + 1.0);
        let a = inv.point(v3(nx, ny, 0.0));
        let b = inv.point(v3(nx, ny, 1.0));
        let dir = b.sub(a).norm();
        let world = self.g.world();
        let shown = self.g.shown();
        let mut best: Option<(f64, Hover)> = None;
        for (i, n) in self.g.nodes.iter().enumerate() {
            let Some(h) = n.hit else { continue };
            if !shown[i] { continue; }
            let m = world[i].inverse();
            let (o, d) = (m.point(a), m.dir(dir));
            // The unit box, -0.5..0.5.
            let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
            for (oo, dd) in [(o.x, d.x), (o.y, d.y), (o.z, d.z)] {
                if dd.abs() < 1e-12 { if oo.abs() > 0.5 { t1 = -1.0; } continue; }
                let (u, v) = ((-0.5 - oo) / dd, (0.5 - oo) / dd);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
            }
            if t1 < t0.max(0.0) { continue; }
            let hv = match h {
                Hit::Prop(k) => Hover::Prop(PROPS[k].0),
                Hit::Bot(_) => match self.sessions.iter().find(|s| s.b.hit == i) { Some(s) => Hover::Bot(s.id), None => continue },
            };
            if best.is_none_or(|(bt, _)| t0 < bt) { best = Some((t0, hv)); }
        }
        best.map(|b| b.1)
    }

    /// pointerup without a drag: what the page does.
    pub fn click(&mut self) -> Click {
        match self.hovered {
            Some(Hover::Bot(id)) => Click::Open(id),
            Some(Hover::Prop(Prop::Tv)) => Click::Panel("tv"),
            Some(Hover::Prop(Prop::Board)) => Click::Panel("board"),
            Some(Hover::Prop(Prop::Shelf)) => Click::Panel("history"),
            Some(Hover::Prop(Prop::Door)) => Click::NewTask,
            Some(Hover::Prop(Prop::Clock)) => Click::Toast(String::new()),
            Some(Hover::Prop(Prop::Window)) => Click::Time(if self.time == Time::Day { Time::Night } else { Time::Day }),
            None => Click::Nothing,
        }
    }

    /// The prop's tooltip ("Office overview", "Make it day"…); the clock's is the date.
    pub fn hint(&self, p: Prop) -> &'static str {
        match p {
            Prop::Tv => "Office overview", Prop::Board => "Session board", Prop::Clock => "", Prop::Door => "New task", Prop::Shelf => "Session history",
            Prop::Window => if self.time == Time::Day { "Make it night" } else { "Make it day" },
        }
    }

    // MARK: The frame

    /// bubbleFor: what the bot says over its head.
    pub fn bubble(s: &Session) -> String {
        let t = s.last();
        if s.b.walking() { return "On my way…".into(); }
        match t.stage {
            Stage::Waking => "Waking up…".into(),
            Stage::Working => match s.act.as_deref() {
                Some("Thinking") => "Thinking…".into(),
                Some("Writing") => "Writing it up…".into(),
                a if !s.file.is_empty() => format!("{} {}", a.unwrap_or(""), short(&s.file)),
                a => format!("{}…", a.unwrap_or("Working")),
            },
            Stage::Done => if s.b.since < 6.0 { "Done! ✓".into() } else { String::new() },
            Stage::Failed => "Couldn’t finish".into(),
            Stage::Stopped => "z z z".into(),
            // The question shows over the head in place of the bubble.
            Stage::Waiting => String::new(),
        }
    }

    /// One animation frame at `now_ms`. False when the page's pacing skips it (30 fps
    /// while lively, 10 idle, 1 with reduced motion), so nothing needs drawing.
    pub fn frame(&mut self, now_ms: f64, dt_ms: f64) -> bool {
        self.now_ms = now_ms;
        let dt = (dt_ms / 1000.0).clamp(0.0, 0.1);
        self.acc += dt;
        let calm = !self.lively && now_ms - self.poked > 1500.0;
        let need = if calm { if self.still { 1.0 } else { 0.1 } } else { 1.0 / 31.0 };
        if self.acc < need { return false; }
        let step = self.acc;
        self.acc = 0.0;
        self.clock_t += step;
        let still = self.still;
        for s in &mut self.sessions {
            let (stage, pose) = (s.last().stage, s.pose_of().map(str::to_owned));
            s.b.sync(stage, pose.as_deref());
            s.b.step(&mut self.g, step, still);
        }
        let mut gone = vec![];
        for (i, (b, _)) in self.leaving.iter_mut().enumerate() {
            if b.step(&mut self.g, step, still) == Some(Arrive::Gone) { b.dispose(&mut self.g); gone.push(i); }
        }
        for i in gone.into_iter().rev() { self.leaving.remove(i); }
        // The door swings open while a bot is near it.
        let near = self.sessions.iter().map(|s| &s.b).chain(self.leaving.iter().map(|l| &l.0)).any(|b| (b.x - DOOR.0).hypot(b.z - Z0) < 1.5);
        let dr = self.g.nodes[self.room.door].r.y;
        self.g.nodes[self.room.door].r.y = ease(dr, if near { -1.3 } else { 0.0 }, 6.0, step);
        // Screen light on each bot's face, by stage.
        for i in 0..DESKS.len() {
            let s = self.sessions.iter().find(|x| x.desk == i && x.b.seated && !x.b.walking());
            let (c, o) = s.map_or((0, 0.0), |s| s.last().stage.screen());
            let running = s.is_some_and(|s| s.act.as_deref() == Some("Running")) && (self.clock_t * 11.0).sin() > 0.0;
            if let Mat::Glow { color, opacity } = self.g.mat_mut(self.room.desk_glows[i]) {
                *color = Rgb::hex(c);
                *opacity = ease(*opacity, o * if running { 1.3 } else { 1.0 }, 8.0, step);
            }
        }
        if !still {
            let a = self.clock_t * 0.21;
            self.g.nodes[self.room.vac].p = v3(0.6 + a.sin() * 3.0, 0.0, 4.3 + (a * 2.3).sin() * 0.7);
            self.g.nodes[self.room.vac].r.y = (a.cos() * 3.0).atan2((a * 2.3).cos() * 0.7 * 2.3);
            for (i, &n) in self.room.steam.iter().enumerate() {
                let f = (self.clock_t * 0.5 + i as f64 / 3.0) % 1.0;
                self.g.nodes[n].p = v3(-4.3 + (f * 6.0 + i as f64).sin() * 0.04, 1.5 + f * 0.6, Z0 + 0.3);
                let sc = 0.15 + f * 0.25;
                self.g.nodes[n].s = v3(sc, sc, sc);
                if let Mat::Glow { opacity, .. } = self.g.mat_mut(n) { *opacity = 0.3 * (1.0 - f); }
            }
            self.g.nodes[self.room.dust].r.y = (self.clock_t * 0.1).sin() * 0.02;
            self.g.nodes[self.room.dust].p.y = (self.clock_t * 0.4).sin() * 0.05;
            if let Mat::Glow { opacity, .. } = self.g.mat_mut(self.room.exit_glow) { *opacity = 0.5 + (self.clock_t * 2.0).sin() * 0.05; }
        }
        // Camera: the user's view; or close on the open session's bot, or on a panel's prop.
        let side = if self.w < 700.0 { 0.0 } else { (424f64).min(self.w * 0.42) / 2.0 };
        let focus = if self.drawer_open && !self.viewing { self.sel.and_then(|id| self.sessions.iter().find(|s| s.id == id)).map(|s| (s.b.x, 0.9, s.b.z, if self.w < 700.0 { 1.3 } else { 1.45 })) } else { None };
        let focus = focus.or_else(|| self.panel.map(|p| match p { "board" => (X0 + 1.2, 2.1, -0.7, 2.6), "tv" => (5.2, 1.6, Z0 + 1.6, 1.35), _ => (X0 + 1.4, 1.4, -3.5, 1.35) }));
        self.cam_to = match focus {
            Some((x, y, z, zoom)) => { let off = side * 2.0 * self.half_width(zoom) / self.w; [x + RIGHT.x * off, y, z + RIGHT.z * off, zoom] }
            None => [self.user[0], 1.7, self.user[1], self.user[2]],
        };
        let k = if still || self.dragging { 60.0 } else { 5.0 };
        for i in 0..4 { self.cam[i] = ease(self.cam[i], self.cam_to[i], k, step); }
        self.pick();
        // Bubbles type out new text, 45 characters a second.
        for s in &mut self.sessions {
            let want = Self::bubble(s);
            if want != s.tag_text { s.tag_text = want; s.tag_shown = 0.0; }
            s.tag_shown += step * 45.0;
        }
        if floor_i(self.clock_t * 4.0) != self.tv_at { self.tv_at = floor_i(self.clock_t * 4.0); let t = self.clock_t; self.draw_tv(t); }
        if floor_i(self.clock_t * 2.0) != self.clock_at { self.clock_at = floor_i(self.clock_t * 2.0); self.draw_clock(); }
        let walking = !self.leaving.is_empty() || self.sessions.iter().any(|s| s.b.walking());
        if walking || self.shadow_dirty || self.clock_t - self.shadow_at >= 0.1 { self.shadow_at = self.clock_t; self.shadow_dirty = true; }
        let door_moving = (self.g.nodes[self.room.door].r.y - if near { -1.3 } else { 0.0 }).abs() > 0.01;
        self.lively = walking || self.dragging || door_moving
            || self.sessions.iter().any(|s| s.busy() || s.b.since < 2.0 || s.tag_shown < s.tag_text.chars().count() as f64)
            || (0..4).any(|i| (self.cam[i] - self.cam_to[i]).abs() > 0.002);
        self.frames += 1;
        true
    }

    /// Where each tag sits (the bot's head projected), its words so far, its stage.
    pub fn tags(&self) -> Vec<Tag> {
        let (view, proj) = self.camera();
        let vp = proj.mul(&view);
        self.sessions.iter().map(|s| {
            let p = vp.point(s.b.head3(&self.g));
            let n = if self.still { usize::MAX } else { s.tag_shown as usize };
            Tag { id: s.id, x: (p.x + 1.0) / 2.0 * self.w, y: (1.0 - p.y) / 2.0 * self.h, name: s.b.name, color: s.b.css, tool: s.tool.clone(),
                text: s.tag_text.chars().take(n).collect(), stage: s.last().stage, hot: s.b.hot }
        }).collect()
    }

    // MARK: The wall canvases

    fn count(&self, st: &[Stage]) -> usize { self.sessions.iter().filter(|s| st.contains(&s.last().stage)).count() }

    fn draw_textures(&mut self) {
        // glowTex (the sprites' own texture is made by the renderer), beamTex, patchTex.
        let c = &mut self.canvases[scene::TEX_BEAM];
        c.gradient_v(0.0, 64.0, &[(0.0, [1.0, 1.0, 1.0, 0.9]), (1.0, [1.0, 1.0, 1.0, 0.0])], 0.0, 0.0, 4.0, 64.0);
        let c = &mut self.canvases[scene::TEX_PATCH];
        c.blur = 2.0;
        c.style("#fff");
        for (px, py) in [(4.0, 4.0), (34.0, 4.0), (4.0, 34.0), (34.0, 34.0)] { c.rect(px, py, 26.0, 26.0); }
        self.apply_time(Time::Night);
        self.draw_board();
        self.draw_clock();
    }

    pub fn draw_sky(&mut self) {
        let night = self.time == Time::Night;
        let x = &mut self.canvases[scene::TEX_SKY];
        *x = Canvas::new(128, 96);
        let mut r = Rng::new(3);
        let stops = if night { [(0.0, crate::canvas::css("#070a24")), (1.0, crate::canvas::css("#2a2458"))] } else { [(0.0, crate::canvas::css("#5eb0ff")), (1.0, crate::canvas::css("#cfe8ff"))] };
        x.gradient_v(0.0, 96.0, &stops, 0.0, 0.0, 128.0, 96.0);
        if night {
            for _ in 0..40 {
                let c = if r.next() < 0.3 { "#fff" } else { "#9aa6ff" };
                x.style(c);
                let (a, b) = ((r.next() * 128.0).trunc(), (r.next() * 55.0).trunc());
                x.rect(a, b, 1.0, 1.0);
            }
            x.style("#fff2cc"); x.rect(92.0, 12.0, 12.0, 12.0); x.rect(90.0, 14.0, 16.0, 8.0);
            x.style("#e6d6a8"); x.rect(96.0, 16.0, 3.0, 3.0); x.rect(100.0, 20.0, 2.0, 2.0);
        } else {
            x.style("#fff6d8"); x.rect(96.0, 10.0, 12.0, 12.0);
            x.style("#fff");
            for (cx, cy, w) in [(14.0, 20.0, 26.0), (58.0, 12.0, 20.0), (70.0, 34.0, 30.0)] { x.rect(cx, cy, w, 5.0); x.rect(cx + 4.0, cy - 3.0, w - 10.0, 3.0); }
        }
        let mut bx = 0.0;
        while bx < 128.0 {
            let w = 8.0 + (r.next() * 14.0).trunc();
            let h = 18.0 + (r.next() * 36.0).trunc();
            x.style(if night { "#120e2a" } else { "#8fb2d6" });
            x.rect(bx, 96.0 - h, w, h);
            let mut wy = 96.0 - h + 3.0;
            while wy < 94.0 {
                let mut wx = bx + 2.0;
                while wx < bx + w - 2.0 {
                    if r.next() < if night { 0.35 } else { 0.2 } {
                        let c = if night { if r.next() < 0.8 { "#ffd27a" } else { "#b99bff" } } else { "#dbe9f8" };
                        x.style(c);
                        x.rect(wx, wy, 1.0, 2.0);
                    }
                    wx += 3.0;
                }
                wy += 4.0;
            }
            bx += w + 1.0;
        }
        self.dirty[scene::TEX_SKY] = true;
    }

    /// The session the TV follows: the open one, else the first at work.
    fn tv_session(&self) -> Option<&Session> {
        self.sel.filter(|_| self.drawer_open).and_then(|id| self.sessions.iter().find(|s| s.id == id)).or_else(|| self.sessions.iter().find(|s| s.busy()))
    }

    pub fn draw_tv(&mut self, t: f64) {
        let rows = [("Working", self.count(&[Stage::Waking, Stage::Working, Stage::Waiting]), "#c4a2ff"), ("Done", self.count(&[Stage::Done]), "#4ade80"),
            ("Failed", self.count(&[Stage::Failed]), "#ff6b62"), ("Stopped", self.count(&[Stage::Stopped]), "#8a8fa0")];
        let cur = self.tv_session().map(|s| {
            let what = if s.last().stage == Stage::Working { format!("{} {}", s.act.as_deref().unwrap_or(""), short(&s.file)).trim().to_owned() } else { s.last().stage.word().to_owned() };
            (s.b.name, s.b.css, what, s.ctx)
        });
        let x = &mut self.canvases[scene::TEX_TV];
        *x = Canvas::new(208, 118);
        x.style("#061022"); x.rect(0.0, 0.0, 208.0, 118.0);
        x.style("#0b1b36");
        let mut y = 0.0;
        while y < 118.0 { x.rect(0.0, y, 208.0, 1.0); y += 3.0; }
        x.font = pixel(11.0, true);
        x.baseline = Baseline::Top;
        x.style("#9ad2ff"); x.text("AGENT OFFICE", 10.0, 8.0);
        x.style("#2f5a8a"); x.rect(10.0, 22.0, 188.0, 1.0);
        for (i, (l, v, c)) in rows.iter().enumerate() {
            let yy = 30.0 + i as f64 * 14.0;
            x.style("#6fa8d8"); x.text(l, 10.0, yy);
            x.style(c); x.text(&v.to_string(), 70.0, yy);
            for k in 0..*v { x.rect(86.0 + k as f64 * 8.0, 33.0 + i as f64 * 14.0, 6.0, 6.0); }
        }
        match cur {
            Some((name, css, what, ctx)) => {
                x.style(&format!("#{:02x}{:02x}{:02x}", css[0], css[1], css[2])); x.text(name, 10.0, 90.0);
                x.style("#cfe6ff");
                let n = what.encode_utf16().count();
                let shown = if n > 24 { String::from_utf16_lossy(&what.encode_utf16().take(23).collect::<Vec<_>>()) + "…" } else { what };
                x.text(&shown, 46.0, 90.0);
                if let Some(c) = ctx {
                    x.style("#1c3458"); x.rect(10.0, 105.0, 150.0, 5.0);
                    x.style("#9ad2ff"); x.rect(10.0, 105.0, 1.5 * c, 5.0);
                    x.style("#6fa8d8"); x.text(&format!("{}%", hover_core::json::dotnet_double(c)), 166.0, 101.0);
                }
            }
            None => { x.style("#6fa8d8"); x.text("No sessions yet", 10.0, 90.0); }
        }
        if floor_i(t * 2.0) % 2 != 0 { x.style("#9ad2ff"); x.rect(190.0, 8.0, 6.0, 10.0); }
        self.dirty[scene::TEX_TV] = true;
    }

    pub fn draw_board(&mut self) {
        const COLS: [(&str, &str, &[Stage]); 3] = [("WAKING", "#f5b83d", &[Stage::Waking]), ("DOING", "#9b6bff", &[Stage::Working, Stage::Waiting]), ("FINISHED", "#2fae66", &[Stage::Done, Stage::Failed, Stage::Stopped])];
        let notes: Vec<(Stage, &'static str, [u8; 3], String)> = self.sessions.iter().map(|s| (s.last().stage, s.b.name, s.b.css, s.title.clone())).collect();
        let x = &mut self.canvases[scene::TEX_BOARD];
        *x = Canvas::new(480, 280);
        x.scale = 2.0;
        x.style("#e9e3d6"); x.rect(0.0, 0.0, 240.0, 140.0);
        x.style("#d6cebd"); x.rect(0.0, 132.0, 240.0, 8.0);
        x.baseline = Baseline::Top;
        for (i, (h, c, st)) in COLS.iter().enumerate() {
            let cx = 8.0 + i as f64 * 78.0;
            x.font = pixel(11.0, true);
            x.style(c); x.rect(cx, 7.0, 70.0, 14.0);
            x.style("#fff"); x.text(h, cx + 5.0, 8.0);
            if i > 0 && !notes.is_empty() { x.style("#cfc6b3"); x.rect(cx - 5.0, 8.0, 1.0, 118.0); }
            let list: Vec<_> = notes.iter().filter(|n| st.contains(&n.0)).collect();
            let room = if list.len() > 3 { 2 } else { 3 };
            for (k, (stage, name, css, title)) in list.iter().take(room).enumerate() {
                let ny = 27.0 + k as f64 * 34.0;
                x.style("rgba(0,0,0,.14)"); x.rect(cx + 1.5, ny + 1.5, 68.0, 31.0);
                x.style("#fbf8f1"); x.rect(cx, ny, 68.0, 31.0);
                x.style(&format!("#{:02x}{:02x}{:02x}", css[0], css[1], css[2])); x.rect(cx, ny, 3.0, 31.0);
                x.font = pixel(8.0, true);
                x.style("#2a2233"); x.text(name, cx + 6.0, ny + 3.0);
                let mark = match stage { Stage::Failed => Some("#ff453a"), Stage::Stopped => Some("#8e8a96"), Stage::Done => Some("#2fae66"), _ => None };
                if let Some(m) = mark { x.style(m); x.rect(cx + 60.0, ny + 4.0, 5.0, 5.0); }
                // The title on up to two lines.
                x.font = inter(7.0);
                x.style("#5a5263");
                let words: Vec<&str> = title.split_whitespace().collect();
                let (mut line, mut row) = (String::new(), 0);
                let mut j = 0;
                while j < words.len() && row < 2 {
                    let next = if line.is_empty() { words[j].to_owned() } else { format!("{line} {}", words[j]) };
                    if x.measure(&next) <= 58.0 || line.is_empty() { line = next; j += 1; continue; }
                    if row == 1 { line = next; break; }
                    let f = fit(x, &line, 58.0);
                    x.text(&f, cx + 6.0, ny + 13.0);
                    row = 1;
                    line = words[j].to_owned();
                    j += 1;
                }
                if !line.is_empty() { let f = fit(x, &line, 58.0); x.text(&f, cx + 6.0, ny + 13.0 + row as f64 * 8.5); }
            }
            if list.len() > room { x.font = inter(7.0); x.style("#7a7282"); x.text(&format!("+{} more", list.len() - room), cx + 3.0, 29.0 + room as f64 * 34.0); }
        }
        if notes.is_empty() {
            x.align = Align::Center;
            x.style("#3a3044"); x.font = pixel(16.0, true); x.text("The office is quiet", 120.0, 46.0);
            x.style("#5e5666"); x.font = inter(10.0);
            x.text("Give Kiro, Codex, Cursor or OpenCode", 120.0, 72.0);
            x.text("a task, and a bot walks in to do it.", 120.0, 86.0);
            x.align = Align::Start;
        }
        self.dirty[scene::TEX_BOARD] = true;
    }

    pub fn draw_clock(&mut self) {
        let (ms, off) = (self.wall_clock)();
        let local = (ms / 1000.0).floor() as i64 + off * 60;
        let (h, m, s) = (local.rem_euclid(86400) / 3600, local.rem_euclid(3600) / 60, local.rem_euclid(60));
        let x = &mut self.canvases[scene::TEX_CLOCK];
        *x = Canvas::new(96, 44);
        x.style("#0f0d12"); x.rect(0.0, 0.0, 96.0, 44.0);
        x.font = pixel(30.0, true);
        x.baseline = Baseline::Middle;
        x.align = Align::Center;
        x.style("#3a1a0c"); x.text("88 88", 48.0, 23.0);
        // The colon blinks with the real seconds.
        x.style("#ff8a3a"); x.text(&format!("{h:02}{}{m:02}", if s % 2 != 0 { ' ' } else { ':' }), 48.0, 23.0);
        self.dirty[scene::TEX_CLOCK] = true;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag { pub id: i64, pub x: f64, pub y: f64, pub name: &'static str, pub color: [u8; 3], pub tool: String, pub text: String, pub stage: Stage, pub hot: bool }

/// short(f): the last part of a path.
pub fn short(f: &str) -> String { f.rsplit(['\\', '/']).next().unwrap_or("").to_owned() }

/// fit(): cut with an ellipsis to fit the width.
fn fit(x: &mut Canvas, text: &str, w: f64) -> String {
    if x.measure(text) <= w { return text.to_owned(); }
    let mut t: Vec<char> = text.chars().collect();
    while !t.is_empty() && x.measure(&(t.iter().collect::<String>() + "…")) > w { t.pop(); }
    t.iter().collect::<String>().trim_end().to_owned() + "…"
}
