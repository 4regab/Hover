//! `scrollbar-width: thin` as Chromium draws it (the Fluent scrollbar WebView2 has on
//! Windows 11), for the thread and for code blocks and tables that overflow sideways.
//! The geometry was measured in Chromium with its scrollbars shown (Playwright hides
//! them by default, which is also why the Phase 0 captures show none): a 10 px bar that
//! takes its room from the content, a 10 px arrow button at each end, and a 6 px round
//! thumb set 2 px into the track, never shorter than 11 px.

/// Thickness, and each arrow button's length along the bar.
pub const THICK: f32 = 10.0;
const INSET: f32 = 2.0;
const MIN_THUMB: f32 = 11.0;
/// An arrow button scrolls by 40 px (Chromium's line step); the track by 87.5 % of a page.
pub const LINE: f32 = 40.0;
const PAGE: f32 = 0.875;

/// Which scrollbar: the thread's, or a box's by (section, scroller).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarId { Thread, Box(usize, usize) }

/// One scrollbar: where it is (x, y along the top or left edge, and its length) and
/// what it scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub vertical: bool,
    pub x: f32,
    pub y: f32,
    pub len: f32,
    /// The content's length, how much of it shows, and how far it is scrolled.
    pub content: f32,
    pub view: f32,
    pub pos: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part { Back, Forward, TrackBack, TrackForward, Thumb }

impl Bar {
    pub fn max(&self) -> f32 { (self.content - self.view).max(0.0) }

    /// The thumb's start and length, along the bar from its start.
    pub fn thumb(&self) -> (f32, f32) {
        let track = (self.len - 2.0 * THICK - 2.0 * INSET).max(0.0);
        let len = (track * self.view / self.content.max(1.0)).clamp(MIN_THUMB.min(track), track);
        let at = if self.max() > 0.0 { (track - len) * self.pos / self.max() } else { 0.0 };
        (THICK + INSET + at, len)
    }

    /// The point's distance along the bar, or None when it is off the bar.
    pub fn along(&self, px: f32, py: f32) -> Option<f32> {
        let (a, c) = if self.vertical { (py - self.y, px - self.x) } else { (px - self.x, py - self.y) };
        (a >= 0.0 && a < self.len && (0.0..THICK).contains(&c)).then_some(a)
    }

    pub fn part(&self, along: f32) -> Part {
        let (t0, tl) = self.thumb();
        if along < THICK { Part::Back } else if along >= self.len - THICK { Part::Forward }
        else if along < t0 { Part::TrackBack } else if along >= t0 + tl { Part::TrackForward } else { Part::Thumb }
    }

    /// Where a press on `part` scrolls to, one step. A track press stops once the thumb
    /// reaches the pointer (at `along`), as Chromium's repeating track press does.
    pub fn step(&self, part: Part, along: f32) -> f32 {
        let (t0, tl) = self.thumb();
        let pos = match part {
            Part::Back => self.pos - LINE,
            Part::Forward => self.pos + LINE,
            Part::TrackBack if along < t0 => self.pos - self.view * PAGE,
            Part::TrackForward if along >= t0 + tl => self.pos + self.view * PAGE,
            _ => self.pos,
        };
        pos.clamp(0.0, self.max())
    }

    /// The scroll position for a thumb dragged so that the point `grab` px into it is at `along`.
    pub fn drag(&self, grab: f32, along: f32) -> f32 {
        let (_, tl) = self.thumb();
        let track = self.len - 2.0 * THICK - 2.0 * INSET;
        let free = track - tl;
        if free <= 0.0 { return self.pos; }
        ((along - grab - THICK - INSET) / free * self.max()).clamp(0.0, self.max())
    }
}

/// A cubic Bézier timing function from (0,0) to (1,1), as gfx::CubicBezier.
#[derive(Clone, Copy, Debug)]
struct Bezier { x1: f64, y1: f64, x2: f64, y2: f64 }

impl Bezier {
    fn at(a: f64, b: f64, t: f64) -> f64 { 3.0 * a * t * (1.0 - t) * (1.0 - t) + 3.0 * b * t * t * (1.0 - t) + t * t * t }
    fn d(a: f64, b: f64, t: f64) -> f64 { 3.0 * a * (1.0 - t) * (1.0 - t) + 6.0 * (b - a) * t * (1.0 - t) + 3.0 * (1.0 - b) * t * t }
    /// The curve's parameter for progress x (x is monotonic for x1, x2 in [0, 1]).
    fn t_for(&self, x: f64) -> f64 {
        let mut t = x;
        for _ in 0..8 {
            let e = Self::at(self.x1, self.x2, t) - x;
            let d = Self::d(self.x1, self.x2, t);
            if e.abs() < 1e-7 { return t; }
            if d.abs() < 1e-6 { break; }
            t -= e / d;
        }
        let (mut lo, mut hi) = (0.0, 1.0);
        t = x;
        for _ in 0..40 {
            if Self::at(self.x1, self.x2, t) < x { lo = t } else { hi = t }
            t = (lo + hi) / 2.0;
        }
        t
    }
    fn value(&self, x: f64) -> f64 { if x <= 0.0 { 0.0 } else if x >= 1.0 { 1.0 } else { Self::at(self.y1, self.y2, self.t_for(x)) } }
    fn slope(&self, x: f64) -> f64 {
        let t = self.t_for(x.clamp(0.0, 1.0));
        let dx = Self::d(self.x1, self.x2, t);
        if dx.abs() < 1e-9 { 0.0 } else { Self::d(self.y1, self.y2, t) / dx }
    }
}

/// A user scroll's animation, as cc's ScrollOffsetAnimationCurve animates a wheel,
/// arrow or track scroll: ease-in-out (0.42, 0, 0.58, 1) over 6 to 12 frames at 60 Hz,
/// shorter the further it goes (kInverseDelta: 12 frames up to 120 px, 6 from 480 px).
/// A new scroll during one retargets it, keeping its speed (UpdateTarget). Times are
/// seconds from any fixed start.
#[derive(Clone, Debug)]
pub struct Smooth {
    from: f64,
    to: f64,
    start: f64,
    end: f64,
    curve: Bezier,
}

/// CSS `ease-out`, cubic-bezier(0, 0, 0.58, 1).
pub fn ease_out(x: f32) -> f32 {
    Bezier { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 }.value(x as f64) as f32
}

const EASE: Bezier = Bezier { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 };

fn inverse_delta(delta: f64) -> f64 {
    let (a, b, min, max) = (120.0, 480.0, 6.0, 12.0);
    let slope = (min - max) / (b - a);
    let offset = max - a * slope;
    (offset + delta.abs() * slope).clamp(min, max) / 60.0
}

impl Smooth {
    pub fn new(from: f32, to: f32, now: f64) -> Self {
        let (from, to) = (from as f64, to as f64);
        Smooth { from, to, start: now, end: now + inverse_delta(to - from), curve: EASE }
    }

    pub fn value(&self, now: f64) -> f32 {
        let d = self.end - self.start;
        if d <= 0.0 || now >= self.end { return self.to as f32; }
        (self.from + (self.to - self.from) * self.curve.value((now - self.start) / d)) as f32
    }

    pub fn done(&self, now: f64) -> bool { now >= self.end }
    pub fn target(&self) -> f32 { self.to as f32 }

    fn velocity(&self, now: f64) -> f64 {
        let d = self.end - self.start;
        if d <= 0.0 || now >= self.end { return 0.0; }
        self.curve.slope((now - self.start) / d) * (self.to - self.from) / d
    }

    /// Heads for a new target from where the scroll is now, at the speed it has.
    pub fn retarget(&mut self, to: f32, now: f64) {
        let to = to as f64;
        if (to - self.to).abs() < 0.01 { return; }
        let cur = self.value(now) as f64;
        let delta = to - cur;
        if delta.abs() < 0.01 || self.done(now) { *self = Smooth::new(cur as f32, to as f32, now); return; }
        let v = self.velocity(now);
        // The velocity bound: no longer than the present speed takes, with a fudge for the ease out.
        let bound = if v.abs() < 0.01 { f64::MAX } else { let b = delta / v * 2.5; if b < 0.0 { f64::MAX } else { b } };
        let dur = inverse_delta(delta).min(bound);
        let slope = (v * dur / delta).clamp(-1000.0, 1000.0);
        *self = Smooth { from: cur, to, start: now, end: now + dur, curve: Bezier { y1: EASE.x1 * slope, ..EASE } };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_matches_chromium() {
        // The rich session in the page: 271 px of a 961 px thread, scrolled to 300.
        let b = Bar { vertical: true, x: 0.0, y: 0.0, len: 271.0, content: 961.0, view: 271.0, pos: 300.0 };
        let (at, len) = b.thumb();
        assert!((at - 89.1).abs() < 1.0 && (len - 69.6).abs() < 1.0, "{at} {len}");
        // A 100000 px list in 271 px: the thumb stops shrinking at 11 px, 2 px into the track.
        let b = Bar { content: 100000.0, pos: 0.0, ..b };
        assert_eq!(b.thumb(), (12.0, 11.0));
        assert_eq!(b.part(5.0), Part::Back);
        assert_eq!(b.part(15.0), Part::Thumb);
        assert_eq!(b.part(100.0), Part::TrackForward);
        assert_eq!(b.step(Part::TrackForward, 100.0), 271.0 * 0.875);
        assert_eq!(b.drag(0.0, 12.0), 0.0);
        assert_eq!(b.drag(0.0, 271.0 - 12.0 - 11.0), b.max());
    }

    #[test]
    fn smooth_scrolls_ease_over_chromiums_durations_and_retarget_at_speed() {
        // 100 px (a wheel notch on Windows): 12 frames; 600 px: 6.
        let s = Smooth::new(0.0, 100.0, 0.0);
        assert!((s.end - 0.2).abs() < 1e-9);
        assert!((Smooth::new(0.0, 600.0, 0.0).end - 0.1).abs() < 1e-9);
        assert_eq!(s.value(0.0), 0.0);
        assert!((s.value(0.1) - 50.0).abs() < 0.01, "ease-in-out is half way at half time");
        assert!(s.value(0.05) < 25.0, "it eases in");
        assert_eq!(s.value(0.3), 100.0);
        // A second notch half way: from where it is, at the speed it has, to 200.
        let mut r = s.clone();
        let (p, v) = (s.value(0.1), s.velocity(0.1));
        r.retarget(200.0, 0.1);
        assert!((r.value(0.1) - p).abs() < 0.01);
        assert!((r.velocity(0.1001) - v).abs() / v < 0.02, "{} vs {v}", r.velocity(0.1001));
        assert_eq!(r.value(1.0), 200.0);
    }
}
