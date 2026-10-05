//! Voice's aura: a pulsing ring of light on the listening and working cards, in place
//! of their words. Slint has no shaders of its own, so it is drawn here on the CPU into
//! a small image (66 DIPs, about 17 000 pixels at 2x: a millisecond or two a frame).
//!
//! How it moves in each state follows LiveKit Agents UI's Aura visualizer (the state
//! table in its use-agent-audio-visualizer-aura.ts, Apache-2.0): listening swirls
//! slowly with a gentle pulse, working swirls faster with a deep one, and the voice's
//! level swells it as an agent's does when it speaks. The drawing is Hover's own:
//! LiveKit's shader is Unicorn Studio's, under a non-resale licence, and isn't used.
//!
//! The voice is read against the room: the quietest level heard is taken as the room's
//! noise and the loudest lately as full voice, so speech uses the whole range whatever the
//! microphone's gain. Small dots orbit the ring and drift out and brighten as the voice
//! rises, and a thin ring spreads out from it on a sudden loud sound.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::f32::consts::{PI, TAU};

/// What the card is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Recording: the voice's level swells the ring.
    Listening,
    /// Starting local speech, transcribing, cleaning up, finding the project.
    Working,
}

/// The strands of light that make the ring, and the angle steps each is sampled at.
const STRANDS: usize = 12;
const BINS: usize = 360;
/// The dots that orbit it, and how long a ripple lasts (seconds) and how far it travels.
const DOTS: usize = 16;
const RIPPLE_LIFE: f32 = 0.9;
const RIPPLE_REACH: f32 = 0.08;

/// A number in 0..1 that is always the same for the same `i` and `k`: each dot's own
/// angle, speed, size and twinkle.
fn rnd(i: usize, k: u32) -> f32 {
    let mut x = (i as u32).wrapping_mul(0x9E37_79B1) ^ k.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / 16_777_216.0
}

/// One state's targets (LiveKit's numbers: speed, ring size, turbulence, its frequency,
/// and the brightness it pulses between, 0.35 s each way).
struct Look { speed: f32, scale: f32, amplitude: f32, frequency: f32, pulse: (f32, f32) }

fn look(mode: Mode) -> Look {
    match mode {
        Mode::Listening => Look { speed: 20.0, scale: 0.3, amplitude: 1.0, frequency: 0.7, pulse: (1.5, 2.0) },
        Mode::Working => Look { speed: 30.0, scale: 0.3, amplitude: 0.5, frequency: 1.0, pulse: (0.5, 2.5) },
    }
}

/// The aura between frames: its eased parameters, its phase, and the strands' radii.
pub struct Aura {
    last_t: Option<f32>,
    phase: f32,
    speed: f32,
    scale: f32,
    amplitude: f32,
    frequency: f32,
    lo: f32,
    hi: f32,
    level: f32,
    radii: Vec<f32>,
    /// The room's noise and the loudest lately, in the microphone's own 0..1, once heard.
    floor: f32,
    peak: f32,
    heard: bool,
    /// The voice (0..1 against the room) at the last frame, when a ripple last started, and
    /// when each one still going began.
    before: f32,
    rippled: f32,
    ripples: Vec<f32>,
}

impl Default for Aura {
    fn default() -> Self {
        let l = look(Mode::Listening);
        Aura {
            last_t: None, phase: 0.0, speed: l.speed, scale: l.scale, amplitude: l.amplitude, frequency: l.frequency,
            lo: l.pulse.0, hi: l.pulse.1, level: 0.0, radii: vec![0.0; STRANDS * BINS],
            floor: 0.0, peak: 0.0, heard: false, before: 0.0, rippled: f32::MIN, ripples: vec![],
        }
    }
}

/// Moves `v` toward `to` as an ease-out over about half a second.
fn ease(v: &mut f32, to: f32, k: f32) { *v += (to - *v) * k; }

impl Aura {
    /// Forget the last frame: the next one starts at its state's look, not eased into it.
    pub fn reset(&mut self) { self.last_t = None; self.heard = false; self.ripples.clear(); }

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
            (self.speed, self.scale, self.amplitude, self.frequency, self.lo, self.hi, self.level) =
                (to.speed, to.scale, to.amplitude, to.frequency, to.pulse.0, to.pulse.1, level);
            self.phase = t * 0.05 * self.speed;
        } else {
            let k = 1.0 - (-dt / 0.15).exp();
            ease(&mut self.speed, to.speed, k);
            ease(&mut self.scale, to.scale, k);
            ease(&mut self.amplitude, to.amplitude, k);
            ease(&mut self.frequency, to.frequency, k);
            ease(&mut self.lo, to.pulse.0, k);
            ease(&mut self.hi, to.pulse.1, k);
            // The voice comes up quickly and falls away more slowly, as a meter does.
            let kl = 1.0 - (-dt / if level > self.level { 0.04 } else { 0.25 }).exp();
            ease(&mut self.level, level, kl);
            // The voice turns the ring faster too.
            self.phase += dt * 0.05 * (self.speed + 45.0 * self.level);
        }
        // A sudden loud sound sends a ripple out (not more than one in 0.35 s, three at most).
        self.ripples.retain(|b| t - b < RIPPLE_LIFE);
        if mode == Mode::Listening && !snap && self.level > 0.5 && self.level - self.before > 0.06 && t - self.rippled > 0.35 && self.ripples.len() < 3 {
            self.ripples.push(t);
            self.rippled = t;
        }
        self.before = self.level;
        // Brightness swings lo → hi → lo, 0.35 s each way, eased out.
        let m = (t / 0.35).rem_euclid(2.0);
        let u = if m < 1.0 { m } else { 2.0 - m };
        let bright = (self.lo + (self.hi - self.lo) * (1.0 - (1.0 - u) * (1.0 - u))) * (1.0 + 0.6 * self.level);
        self.draw(px.max(8), t, bright, color.map(|c| c as f32 / 255.0))
    }

    fn draw(&mut self, px: u32, t: f32, bright: f32, color: [f32; 3]) -> Image {
        let (scale, amp) = (self.scale * (0.82 + 0.3 * self.level), self.amplitude * (1.0 + 1.0 * self.level));
        // Higher frequency: more of the finer ripples.
        let f = 0.4 + self.frequency;
        let (ph, rot) = (self.phase, self.phase * 0.15);
        // Each strand's ring: a few whole waves round the circle (so it closes), each
        // moving at its own rate, the strands a little out of step so they fan apart
        // where the waves are steep and braid where they cross.
        for s in 0..STRANDS {
            let u = s as f32 / (STRANDS - 1) as f32 - 0.5;
            for b in 0..BINS {
                let th = b as f32 / BINS as f32 * TAU - PI + rot;
                let w = 0.10 * (2.0 * th + 1.0 * ph + 1.2 * u).sin()
                    + 0.06 * f * (3.0 * th - 1.3 * ph + 2.0 * u + 1.7).sin()
                    + 0.04 * f * (5.0 * th + 1.7 * ph + 2.8 * u + 4.1).sin();
                self.radii[s * BINS + b] = scale * (1.0 + 0.8 * amp * w) + u * 0.02 * amp;
            }
        }
        // The dots: each on its own slow orbit, out from the ring by its own way, more as the
        // voice rises, and twinkling. (x, y, size, brightness), in the picture's 0.5 radius.
        let unit = px as f32 / 88.0;
        let mut dots = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); DOTS];
        for (i, d) in dots.iter_mut().enumerate() {
            let turn = (rnd(i, 1) - 0.5) * 2.0 * (0.25 + 0.75 * rnd(i, 2));
            let a = rnd(i, 3) * TAU + turn * ph * 0.25 + rot;
            let bin = (((a + PI).rem_euclid(TAU)) / TAU * BINS as f32) as usize % BINS;
            let ring = self.radii[STRANDS / 2 * BINS + bin];
            let way = (rnd(i, 4) - 0.5) * 2.0;
            let r = ring + way * (0.02 + 0.07 * self.level) + 0.008 * (t * (0.8 + rnd(i, 5)) * 2.0 + rnd(i, 6) * TAU).sin();
            let twinkle = 0.55 + 0.45 * (t * (1.5 + 2.5 * rnd(i, 7)) + rnd(i, 8) * TAU).sin();
            *d = (a.cos() * r, a.sin() * r, (1.2 + 1.1 * rnd(i, 9)) * unit / px as f32, (0.25 + 0.75 * self.level) * twinkle);
        }
        // The ripples: (radius, strength) of each, fading as it spreads.
        let ripples: Vec<(f32, f32)> = self.ripples.iter().map(|b| {
            let age = ((t - b) / RIPPLE_LIFE).clamp(0.0, 1.0);
            (scale + RIPPLE_REACH * (1.0 - (1.0 - age) * (1.0 - age)), 0.32 * (1.0 - age) * (1.0 - age))
        }).collect();
        let n = px as usize;
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(px, px);
        let pixels = buf.make_mut_slice();
        // In pixels of an 88 px aura, scaled to this one: a thin filament,
        // its glow, and a faint haze further out.
        let (core, glow, haze) = (0.9 * unit / n as f32, 3.5 * unit / n as f32, 9.0 * unit / n as f32);
        let gain = bright / 1.5;
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = ((x as f32 + 0.5) / n as f32 - 0.5, (y as f32 + 0.5) / n as f32 - 0.5);
                let rho = (dx * dx + dy * dy).sqrt();
                let fade = ((0.5 - rho) / 0.12).clamp(0.0, 1.0);
                if fade <= 0.0 { pixels[y * n + x] = Rgba8Pixel { r: 0, g: 0, b: 0, a: 0 }; continue; }
                let bin = (((dy.atan2(dx) + PI) / TAU * BINS as f32) as usize).min(BINS - 1);
                let mut i = 0.0;
                for s in 0..STRANDS {
                    let d = rho - self.radii[s * BINS + bin];
                    let (c, g, h) = (d / core, d / glow, d / haze);
                    i += 0.10 * (-c * c).exp() + 0.025 * (-g * g).exp() + 0.006 / (1.0 + h * h);
                }
                for &(dotx, doty, size, lit) in &dots {
                    let d2 = (dx - dotx) * (dx - dotx) + (dy - doty) * (dy - doty);
                    let q = d2 / (size * size);
                    if q < 36.0 { i += lit * (0.55 * (-q).exp() + 0.08 * (-q / 9.0).exp()); }
                }
                for &(rr, strength) in &ripples {
                    let w = (rho - rr) / (glow * 1.3);
                    i += strength * (-w * w).exp();
                }
                let i = i * gain * fade * fade;
                // Light adds up to white where it is brightest, the colour round it.
                let c = |k: f32| 1.0 - (-i * (k * 2.5 + 0.15)).exp();
                let (r, g, b) = (c(color[0]), c(color[1]), c(color[2]));
                let a = r.max(g).max(b);
                let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                pixels[y * n + x] = Rgba8Pixel { r: q(r), g: q(g), b: q(b), a: q(a) };
            }
        }
        Image::from_rgba8_premultiplied(buf)
    }
}
