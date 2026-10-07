//! Voice's aura: a Siri-style orb on the listening and working cards, in place of their
//! words. Slint has no shaders of its own, so it is drawn here on the CPU into a small
//! image (66 DIPs, about 17 000 pixels at 2x: a millisecond or two a frame).
//!
//! The orb is a dark glass ball with soft blobs of light swirling inside it, a bright rim
//! and a faint glow round it, as Siri's orb was from iOS 14 to 17. The blobs' colours are
//! made from the one picked in Settings → Voice (it, the hues either side of it and one
//! further round; a pale tint of it lights the glass), so the orb stays one family of colour. Listening, it swirls and the voice
//! swells it; working on what was said, it swirls slowly and breathes.
//!
//! The voice is read against the room: the quietest level heard is taken as the room's
//! noise and the loudest lately as full voice, so speech uses the whole range whatever the
//! microphone's gain.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::f32::consts::TAU;

/// What the card is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Recording: the voice's level swells the orb.
    Listening,
    /// Starting local speech, transcribing, cleaning up, finding the project.
    Working,
}

/// The blobs of light inside the orb.
const BLOBS: usize = 4;

/// One state's targets: how fast the blobs swirl (radians a second), how far the orb
/// breathes in and out (a fraction of its size) and over how many seconds, and how bright
/// it is.
struct Look { speed: f32, breath: f32, period: f32, bright: f32 }

fn look(mode: Mode) -> Look {
    match mode {
        Mode::Listening => Look { speed: 1.1, breath: 0.025, period: 2.4, bright: 1.0 },
        Mode::Working => Look { speed: 0.55, breath: 0.05, period: 2.0, bright: 0.85 },
    }
}

/// The aura between frames: its eased parameters and its phase.
pub struct Aura {
    last_t: Option<f32>,
    phase: f32,
    speed: f32,
    breath: f32,
    period: f32,
    bright: f32,
    level: f32,
    /// The room's noise and the loudest lately, in the microphone's own 0..1, once heard.
    floor: f32,
    peak: f32,
    heard: bool,
}

impl Default for Aura {
    fn default() -> Self {
        let l = look(Mode::Listening);
        Aura { last_t: None, phase: 0.0, speed: l.speed, breath: l.breath, period: l.period, bright: l.bright, level: 0.0, floor: 0.0, peak: 0.0, heard: false }
    }
}

/// Moves `v` toward `to` as an ease-out over about half a second.
fn ease(v: &mut f32, to: f32, k: f32) { *v += (to - *v) * k; }

/// `rgb` (0..1) with its hue turned by `deg` degrees, keeping its brightness and saturation.
fn turn_hue(rgb: [f32; 3], deg: f32) -> [f32; 3] {
    let (max, min) = (rgb[0].max(rgb[1]).max(rgb[2]), rgb[0].min(rgb[1]).min(rgb[2]));
    let c = max - min;
    // Grey has no hue to turn: white stays white.
    if c < 1e-4 { return rgb; }
    let h = if max == rgb[0] { ((rgb[1] - rgb[2]) / c).rem_euclid(6.0) } else if max == rgb[1] { (rgb[2] - rgb[0]) / c + 2.0 } else { (rgb[0] - rgb[1]) / c + 4.0 };
    let h = (h + deg / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
    [r + min, g + min, b + min]
}

impl Aura {
    /// Forget the last frame: the next one starts at its state's look, not eased into it.
    pub fn reset(&mut self) { self.last_t = None; self.heard = false; }

    /// The microphone's level against the room: 0 at the room's noise, 1 at the loudest
    /// lately. The noise falls to a quieter level at once and follows a louder room over
    /// about twelve seconds; the loudest falls back over about three.
    fn against_room(&mut self, raw: f32, dt: f32) -> f32 {
        if !self.heard {
            // Started mid-sentence, the room is taken as no louder than a quiet one.
            (self.floor, self.peak, self.heard) = (raw.min(0.3), raw.min(0.3) + 0.3, true);
        }
        let dt = dt.clamp(0.0, 0.5);
        if raw < self.floor { self.floor = raw; } else { self.floor += (raw - self.floor) * (1.0 - (-dt / 12.0).exp()); }
        if raw > self.peak { self.peak = raw; } else { self.peak += (self.floor + 0.3 - self.peak) * (1.0 - (-dt / 3.0).exp()); }
        // The gap is never under 0.2 (-12 dB), so a silent room doesn't blow its hiss up.
        let v = ((raw - self.floor) / (self.peak - self.floor).max(0.2)).clamp(0.0, 1.0);
        // Quiet speech counts for more than loud: the curve lifts the low end.
        v.powf(0.7)
    }

    /// One frame at `t` seconds (the notch's clock), `px` pixels square, in `color`
    /// (Settings → Voice → Aura colour). `level` is the microphone's (0 to 1, listening
    /// only). `snap`: no easing (animations off, shots).
    pub fn frame(&mut self, mode: Mode, t: f32, level: f32, px: u32, snap: bool, color: [u8; 3]) -> Image {
        let dt = self.last_t.map_or(-1.0, |l| t - l);
        self.last_t = Some(t);
        let to = look(mode);
        let level = if mode == Mode::Listening { self.against_room(level.clamp(0.0, 1.0), dt.max(0.0)) } else { 0.0 };
        if snap || !(0.0..=0.5).contains(&dt) {
            (self.speed, self.breath, self.period, self.bright, self.level) = (to.speed, to.breath, to.period, to.bright, level);
            self.phase = t * self.speed;
        } else {
            let k = 1.0 - (-dt / 0.15).exp();
            ease(&mut self.speed, to.speed, k);
            ease(&mut self.breath, to.breath, k);
            ease(&mut self.period, to.period, k);
            ease(&mut self.bright, to.bright, k);
            // The voice comes up quickly and falls away more slowly, as a meter does.
            let kl = 1.0 - (-dt / if level > self.level { 0.04 } else { 0.25 }).exp();
            ease(&mut self.level, level, kl);
            // The voice swirls it faster too.
            self.phase += dt * self.speed * (1.0 + 1.6 * self.level);
        }
        let breath = (t / self.period * TAU).sin();
        self.draw(px.max(8), breath, color.map(|c| c as f32 / 255.0))
    }

    /// `breath` is -1..1, where the orb is in its breathing.
    fn draw(&self, px: u32, breath: f32, color: [f32; 3]) -> Image {
        let (ph, lv) = (self.phase, self.level);
        // The orb's radius, in the picture's 0.5: the voice swells it, the breath moves it.
        let radius = 0.34 * (1.0 + 0.13 * lv) * (1.0 + self.breath * breath);
        // It brightens as it breathes in, and with the voice.
        let bright = self.bright * (1.0 + 0.12 * breath * self.breath / 0.05) * (1.0 + 0.5 * lv);
        // The family of colours: the picked one, its neighbours either side and one further
        // round for the blobs, and a pale tint for the glass.
        let pale = color.map(|c| c * 0.45 + 0.55);
        let cols = [color, turn_hue(color, 38.0), turn_hue(color, -38.0), turn_hue(color, 80.0)];
        // Each blob: centre, its long axis (along the way it moves), its two sizes, its colour
        // and strength. They circle the middle at their own rates, nearer and further in turn,
        // and spread out as the voice rises.
        let mut blobs = [((0.0f32, 0.0f32), (1.0f32, 0.0f32), 0.0f32, 0.0f32, [0.0f32; 3], 0.0f32); BLOBS];
        for (i, b) in blobs.iter_mut().enumerate() {
            let k = i as f32;
            let way = if i % 2 == 0 { 1.0 } else { -0.8 };
            let a = ph * way * (0.8 + 0.23 * k) + k * 1.7 + 0.3 * k * k;
            let r = radius * (0.44 + 0.14 * (ph * (0.6 + 0.17 * k) + 1.9 * k).sin()) * (1.0 + 0.35 * lv);
            let along = (-(a.sin()) * way, a.cos() * way);
            let long = radius * (0.58 + 0.10 * (ph * 0.9 + k).sin());
            let short = radius * (0.17 + 0.04 * (ph * 1.3 + 2.0 * k).cos());
            *b = ((a.cos() * r, a.sin() * r), along, long, short, cols[i], if i == 3 { 0.6 } else { 0.9 });
        }
        // The middle turns a little against the rim, which twists the blobs into ribbons.
        let swirl = 1.1 * (ph * 0.37).sin() + 0.6 * lv;
        let n = px as usize;
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(px, px);
        let pixels = buf.make_mut_slice();
        // One pixel, in the picture's units: the rim's edge is smoothed over it.
        let pix = 1.0 / n as f32;
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = ((x as f32 + 0.5) / n as f32 - 0.5, (y as f32 + 0.5) / n as f32 - 0.5);
                let rho = (dx * dx + dy * dy).sqrt();
                let q = rho / radius;
                // Outside the orb only its glow shows, fading out before the picture's edge.
                let edge_fade = ((0.5 - rho) / 0.1).clamp(0.0, 1.0);
                let halo = 0.22 * bright * (-((q - 1.0).max(0.0) / 0.3).powi(2)).exp() * edge_fade * edge_fade;
                let inside = ((radius - rho) / pix + 0.5).clamp(0.0, 1.0);
                let mut v = [0.0f32; 3];
                if inside > 0.0 {
                    let s = swirl * (1.0 - q).max(0.0).powi(2);
                    let (sn, cs) = s.sin_cos();
                    let (rx, ry) = (dx * cs - dy * sn, dx * sn + dy * cs);
                    for &((cx, cy), (ax, ay), long, short, col, strength) in &blobs {
                        let (ox, oy) = (rx - cx, ry - cy);
                        let (u, w) = ((ox * ax + oy * ay) / long, (-ox * ay + oy * ax) / short);
                        let g = strength * (-(u * u + w * w)).exp();
                        for c in 0..3 { v[c] += g * col[c]; }
                    }
                    // The glass: a rim in the pale tint that brightens toward the edge, and a soft
                    // white highlight up and to the left.
                    let rim = 0.45 * q.powi(12);
                    let (hx, hy) = (dx / radius + 0.36, dy / radius + 0.42);
                    let shine = 0.16 * (-(hx * hx / 0.07 + hy * hy / 0.035)).exp();
                    for c in 0..3 { v[c] = (v[c] * 1.25 + 0.07 * color[c] + rim * pale[c] + shine) * bright * inside; }
                }
                // Light adds up to white where it is brightest, the colour round it.
                let out = [0, 1, 2].map(|c| 1.0 - (-(v[c] + halo * (1.0 - inside) * color[c] * 1.4)).exp());
                // The ball itself is dark glass over whatever is behind (the notch is black).
                let a = out[0].max(out[1]).max(out[2]).max(0.92 * inside);
                let q8 = |f: f32| (f * 255.0).round().clamp(0.0, 255.0) as u8;
                pixels[y * n + x] = Rgba8Pixel { r: q8(out[0]), g: q8(out[1]), b: q8(out[2]), a: q8(a) };
            }
        }
        Image::from_rgba8_premultiplied(buf)
    }
}
