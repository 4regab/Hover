//! The notch's geometry and behaviour, from `src/Hover/Owl/Notch.cs`, with no windowing:
//! sizes and placement, the outline (square top, concave ears, round bottom corners),
//! the openness animation, the hover state machine and the hit region. The Windows layer
//! (`tools/notch-proto/src/win.rs`) applies it to a real window.

/// DIP padding around the open shape inside the window (the shadow lives there).
pub const PAD: f64 = 40.0;
pub const PILL_HEIGHT: f64 = 32.0;
/// The island's padding after its last item (11 before the first is in its content).
pub const PILL_PAD_RIGHT: f64 = 9.0;
pub const POLL_MS: u64 = 50;
pub const DWELL_MS: u64 = 120;
pub const LEAVE_GRACE_MS: u64 = 350;
pub const OPEN_MS: f64 = 560.0;
pub const CLOSE_MS: f64 = 340.0;
pub const OPEN_R: f64 = 32.0;
pub const OPEN_EAR: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OfficeSize { #[default] Default, Small, Large, ExtraLarge }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }

impl Rect {
    pub fn width(&self) -> i32 { self.right - self.left }
    pub fn height(&self) -> i32 { self.bottom - self.top }
    /// Half-open, as RECT.Contains in the C#.
    pub fn contains(&self, x: i32, y: i32) -> bool { x >= self.left && x < self.right && y >= self.top && y < self.bottom }
}

/// The open office's size in DIPs, capped by the work area (less 24 each way).
pub fn open_size(size: OfficeSize, work_dips: (f64, f64)) -> (f64, f64) {
    let (w, h): (f64, f64) = match size {
        OfficeSize::Small => (840.0, 340.0),
        OfficeSize::Default => (1120.0, 440.0),
        OfficeSize::Large => (1320.0, 520.0),
        OfficeSize::ExtraLarge => (1560.0, 600.0),
    };
    (w.min(work_dips.0 - 24.0), h.min(work_dips.1 - 24.0))
}

/// The window, in device pixels: open size plus the pad, centred on the work area's top.
pub fn placement(work: Rect, scale: f64, open: (f64, f64)) -> Rect {
    let w = ((open.0 + 2.0 * PAD) * scale).round() as i32;
    let h = ((open.1 + PAD) * scale).round() as i32;
    // Integer division, as the C# does it.
    let x = work.left + (work.width() - w) / 2;
    Rect { left: x, top: work.top, right: x + w, bottom: work.top + h }
}

/// What the resting notch shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rest {
    None,
    /// The island's content width in DIPs, the 11 before its first item included.
    Pill(f64),
    /// The question's card, as it measures.
    Card(f64, f64),
}

/// RestSize: the island rounded to 2 px (so it doesn't twitch as its clock ticks), with
/// 9 after its last item; the card as it measures.
pub fn rest_size(rest: Rest) -> (f64, f64) {
    match rest {
        Rest::None => (0.0, 0.0),
        Rest::Pill(content) => (((content + PILL_PAD_RIGHT) / 2.0).ceil() * 2.0, PILL_HEIGHT),
        Rest::Card(w, h) => (w.ceil(), h.ceil()),
    }
}

/// RestCorners: the island's round ends (r 16, ear 7); a card's 24 and 10.
pub fn rest_corners(h: f64) -> (f64, f64) {
    let r = if h > 60.0 { 24.0 } else { (h / 2.0).min(16.0) };
    (r, (if h > 60.0 { 10.0 } else { 7.0f64 }).min(h - r).max(0.0))
}

/// WPF's BackEase, EaseOut: a little past the end, then back.
pub fn back_ease_out(t: f64, amplitude: f64) -> f64 {
    let u = 1.0 - t;
    1.0 - (u * u * u - u * amplitude * (u * std::f64::consts::PI).sin())
}

/// SineEase, EaseInOut.
pub fn sine_in_out(t: f64) -> f64 { (1.0 - (t * std::f64::consts::PI).cos()) / 2.0 }

/// SetSizes: the resting shape springs to each new size (BackEase 0.22), in 560 ms when
/// it grows into the card, else 500; the first size, or with animations off, is at once.
#[derive(Clone, Copy, Debug)]
pub struct RestAnim { from: (f64, f64), to: (f64, f64), start: f64, dur: f64, set: bool }

impl Default for RestAnim { fn default() -> Self { RestAnim { from: (0.0, 0.0), to: (0.0, 0.0), start: 0.0, dur: 1.0, set: false } } }

impl RestAnim {
    pub fn value(&self, now: f64) -> (f64, f64) {
        let k = ((now - self.start) / self.dur).clamp(0.0, 1.0);
        let e = back_ease_out(k, 0.22);
        (self.from.0 + (self.to.0 - self.from.0) * e, self.from.1 + (self.to.1 - self.from.1) * e)
    }
    pub fn target(&self) -> (f64, f64) { self.to }
    pub fn animating(&self, now: f64) -> bool { now - self.start < self.dur && self.from != self.to }
    pub fn go(&mut self, to: (f64, f64), now: f64, still: bool) {
        if to == self.to && self.set { return; }
        let cur = self.value(now);
        if !self.set || still { *self = RestAnim { from: to, to, start: now, dur: 1.0, set: true }; return; }
        self.dur = if to.1 > cur.1 + 40.0 { 560.0 } else { 500.0 };
        self.from = cur;
        self.to = to;
        self.start = now;
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }
fn clamp01(v: f64) -> f64 { v.clamp(0.0, 1.0) }
fn smoothstep(v: f64) -> f64 { v * v * (3.0 - 2.0 * v) }

pub fn ease_out_cubic(t: f64) -> f64 { 1.0 - (1.0 - t).powi(3) }
pub fn ease_in_cubic(t: f64) -> f64 { t * t * t }

/// One frame of the shape at openness t (0 resting, 1 open).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub w: f64,
    pub h: f64,
    pub r: f64,
    pub ear: f64,
    /// ViewHost opacity: clamp((t - 0.35) / 0.65).
    pub view_opacity: f64,
    /// The resting content: clamp(1 - 3t).
    pub mini_opacity: f64,
    /// Fill mix from black to the panel colour, and the rim's opacity.
    pub fill_mix: f64,
    /// The office takes input only once fully open.
    pub view_hit: bool,
}

pub fn frame(t: f64, rest: (f64, f64), open: (f64, f64)) -> Frame { frame_way(t, rest, open, false) }

/// frame, closing: the office goes first (clamp((t - 0.55) / 0.45)).
pub fn frame_way(t: f64, rest: (f64, f64), open: (f64, f64), closing: bool) -> Frame {
    let (rr, re) = rest_corners(rest.1);
    Frame {
        w: lerp(rest.0, open.0, t),
        h: lerp(rest.1, open.1, t),
        r: lerp(rr, OPEN_R, t),
        ear: lerp(re, OPEN_EAR, t),
        view_opacity: if closing { clamp01((t - 0.55) / 0.45) } else { clamp01((t - 0.35) / 0.65) },
        mini_opacity: clamp01(1.0 - 3.0 * t),
        fill_mix: smoothstep(clamp01((t - 0.2) / 0.6)),
        view_hit: t >= 0.999,
    }
}

/// The outline as SVG path commands, in DIPs, with the shape's left edge at `x0`:
/// the top edge flush with the screen, a concave ear on each side, round bottom corners.
pub fn outline(w: f64, h: f64, r: f64, ear: f64, x0: f64) -> String {
    if w < 1.0 || h < 1.0 {
        return String::new();
    }
    let r = r.min(w / 2.0).min(h).max(0.0);
    let ear = ear.min(h - r).max(0.0);
    let (x1, f) = (x0 + w, |v: f64| format!("{v:.3}"));
    format!(
        "M {} 0 A {e} {e} 0 0 1 {} {} L {} {} A {r} {r} 0 0 0 {} {} L {} {} A {r} {r} 0 0 0 {} {} L {} {} A {e} {e} 0 0 1 {} 0 Z",
        f(x0 - ear), f(x0), f(ear), f(x0), f(h - r), f(x0 + r), f(h), f(x1 - r), f(h), f(x1), f(h - r), f(x1), f(ear), f(x1 + ear),
        e = f(ear), r = f(r),
    )
}

/// The rim: the outline without its top edge (the shape is flush with the screen).
pub fn rim(w: f64, h: f64, r: f64, ear: f64, x0: f64) -> String {
    let o = outline(w, h, r, ear, x0);
    o.strip_suffix(" Z").unwrap_or(&o).to_string()
}

/// Whether a point (DIPs, window coordinates) takes the pointer. In the C# the layered
/// window takes every pixel with alpha > 0, which includes the drop shadow; the shadow
/// is modelled as the shape grown by the blur and moved down by its depth.
pub fn hittable(x: f64, y: f64, win_w: f64, f: &Frame, shadow_blur: f64, shadow_depth: f64) -> bool {
    if f.w < 1.0 || f.h < 1.0 {
        return false;
    }
    let cx = win_w / 2.0;
    // Distance from the shape (rounded bottom corners), negative inside.
    let d = |px: f64, py: f64| -> f64 {
        let qx = (px - cx).abs() - (f.w / 2.0 - f.r);
        let qy = py - (f.h - f.r);
        if py < 0.0 { return f64::INFINITY; }
        if qy <= 0.0 { return (px - cx).abs() - f.w / 2.0; }
        if qx <= 0.0 { return py - f.h; }
        (qx * qx + qy * qy).sqrt() - f.r
    };
    d(x, y) <= 0.0 || d(x, y - shadow_depth) <= shadow_blur
}

/// The resting wake strip (device px): at least 220 x 6 DIPs even with nothing drawn.
pub fn zone(work: Rect, scale: f64, rest: (f64, f64)) -> Rect {
    let half = (rest.0 / 2.0).max(110.0) * scale;
    let h = rest.1.max(6.0) * scale;
    let cx = work.left as f64 + work.width() as f64 / 2.0;
    Rect { left: (cx - half) as i32, top: work.top, right: (cx + half) as i32, bottom: work.top + h.ceil() as i32 }
}

/// The open panel plus 16 DIPs of slack, which a peek stays open over.
pub fn panel_zone(work: Rect, scale: f64, open: (f64, f64)) -> Rect {
    let slack = 16.0 * scale;
    let half = open.0 / 2.0 * scale + slack;
    let cx = work.left as f64 + work.width() as f64 / 2.0;
    Rect { left: (cx - half) as i32, top: work.top - 2, right: (cx + half) as i32, bottom: work.top + (open.1 * scale + slack) as i32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Rest, Peek, Open }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { Peek, Collapse }

/// NotchManager's pointer rules, fed by the 50 ms poll. Times are in ms.
#[derive(Debug)]
pub struct Hover {
    pub state: State,
    armed: bool,
    zone_since: Option<u64>,
    leave_since: Option<u64>,
}

impl Default for Hover {
    fn default() -> Self { Hover { state: State::Rest, armed: true, zone_since: None, leave_since: None } }
}

pub struct Pointer {
    pub in_zone: bool,
    pub in_panel: bool,
    pub buttons: bool,
    pub popover: bool,
    pub hover_opens: bool,
}

impl Hover {
    pub fn poll(&mut self, now: u64, p: &Pointer) -> Option<Action> {
        match self.state {
            State::Rest => {
                if !p.in_zone {
                    self.zone_since = None;
                    self.armed = true;
                    return None;
                }
                if !self.armed || p.buttons || !p.hover_opens {
                    return None;
                }
                let since = *self.zone_since.get_or_insert(now);
                (now - since >= DWELL_MS).then_some(Action::Peek)
            }
            State::Peek => {
                if p.in_panel || p.buttons || p.popover {
                    self.leave_since = None;
                    return None;
                }
                let since = *self.leave_since.get_or_insert(now);
                (now - since >= LEAVE_GRACE_MS).then_some(Action::Collapse)
            }
            State::Open => None,
        }
    }

    pub fn opened(&mut self, peek: bool) {
        self.state = if peek && self.state != State::Open { State::Peek } else { State::Open };
        self.leave_since = None;
    }

    /// After a collapse the pointer must leave the strip before hover works again.
    pub fn collapsed(&mut self) {
        self.state = State::Rest;
        self.armed = false;
        self.zone_since = None;
        self.leave_since = None;
    }
}

/// The openness value animated between 0 and 1 (300 ms ease-out up, 220 ms ease-in down).
#[derive(Clone, Copy, Debug)]
pub struct Openness { from: f64, to: f64, start: f64, dur: f64 }

impl Default for Openness {
    fn default() -> Self { Openness { from: 0.0, to: 0.0, start: 0.0, dur: 1.0 } }
}

impl Openness {
    /// Standing still at a value (where an interrupted greeting had got to).
    pub fn at(v: f64) -> Openness { Openness { from: v, to: v, start: 0.0, dur: 1.0 } }

    pub fn value(&self, now_ms: f64) -> f64 {

        let k = ((now_ms - self.start) / self.dur).clamp(0.0, 1.0);
        // Opening overshoots a touch (BackEase 0.16); closing eases in and out.
        let e = if self.to > self.from { back_ease_out(k, 0.16) } else { sine_in_out(k) };
        self.from + (self.to - self.from) * e
    }
    pub fn animating(&self, now_ms: f64) -> bool { now_ms - self.start < self.dur && self.from != self.to }
    pub fn closing(&self) -> bool { self.to < self.from }
    pub fn go(&mut self, to: f64, now_ms: f64) {
        self.from = self.value(now_ms);
        self.to = to;
        self.start = now_ms;
        // A full run takes the whole duration; the C# DoubleAnimation keeps it fixed too.
        self.dur = if to > self.from { OPEN_MS } else { CLOSE_MS };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn placement_matches_the_csharp_layout() {
        assert_eq!(open_size(OfficeSize::Default, (1920.0, 1040.0)), (1120.0, 440.0));
        assert_eq!(open_size(OfficeSize::ExtraLarge, (1280.0, 600.0)), (1256.0, 576.0));
        let p = placement(WORK, 1.0, (1120.0, 440.0));
        assert_eq!(p, Rect { left: 360, top: 0, right: 1560, bottom: 480 });
        let p = placement(Rect { left: -2560, top: 0, right: 0, bottom: 1400 }, 1.5, (1120.0, 440.0));
        assert_eq!((p.width(), p.height(), p.left), (1800, 720, -2560 + 380));
    }

    #[test]
    fn resting_shapes_and_corners() {
        // 11 before the first item (in the content), 9 after the last, to 2 px.
        assert_eq!(rest_size(Rest::Pill(101.0)), (110.0, 32.0));
        assert_eq!(rest_size(Rest::Pill(100.0)), (110.0, 32.0));
        assert_eq!(rest_size(Rest::Card(500.0, 181.4)), (500.0, 182.0));
        assert_eq!(rest_corners(32.0), (16.0, 7.0));
        assert_eq!(rest_corners(182.0), (24.0, 10.0));
        let mut a = RestAnim::default();
        a.go((108.0, 32.0), 0.0, false);
        assert_eq!(a.value(0.0), (108.0, 32.0), "the first size is at once");
        a.go((500.0, 182.0), 100.0, false);
        assert!(a.value(100.0 + 280.0).0 > 108.0 && a.value(100.0 + 560.0) == (500.0, 182.0));
        assert!(a.value(100.0 + 450.0).0 > 500.0, "it springs a little past");
        let f = frame(1.0, (128.0, 24.0), (1120.0, 440.0));
        assert_eq!((f.w, f.h, f.r, f.ear, f.view_opacity, f.mini_opacity, f.view_hit), (1120.0, 440.0, 32.0, 10.0, 1.0, 0.0, true));
        let f = frame(0.5, (128.0, 24.0), (1120.0, 440.0));
        assert!(!f.view_hit && f.mini_opacity == 0.0 && (f.view_opacity - 0.15 / 0.65).abs() < 1e-9);
        assert!(outline(0.0, 0.0, 0.0, 0.0, 0.0).is_empty());
        assert!(outline(128.0, 24.0, 12.0, 5.0, 40.0).starts_with("M 35.000 0 A 5.000 5.000 0 0 1 40.000 5.000"));
        assert_eq!(frame_way(0.8, (108.0, 32.0), (1120.0, 440.0), true).view_opacity, (0.25f64 / 0.45).clamp(0.0, 1.0));
    }

    #[test]
    fn hover_opens_after_dwell_and_closes_after_grace_and_re_arms() {
        let mut h = Hover::default();
        let p = |z: bool, pn: bool| Pointer { in_zone: z, in_panel: pn, buttons: false, popover: false, hover_opens: true };
        assert_eq!(h.poll(0, &p(true, false)), None);
        assert_eq!(h.poll(100, &p(true, false)), None);
        assert_eq!(h.poll(150, &p(true, false)), Some(Action::Peek));
        h.opened(true);
        assert_eq!(h.poll(200, &p(false, true)), None);
        assert_eq!(h.poll(250, &p(false, false)), None);
        assert_eq!(h.poll(550, &p(false, false)), None);
        assert_eq!(h.poll(650, &p(false, false)), Some(Action::Collapse));
        h.collapsed();
        // Still in the strip: nothing until the pointer has left once.
        assert_eq!(h.poll(700, &p(true, false)), None);
        assert_eq!(h.poll(900, &p(true, false)), None);
        assert_eq!(h.poll(950, &p(false, false)), None);
        assert_eq!(h.poll(1000, &p(true, false)), None);
        assert_eq!(h.poll(1150, &p(true, false)), Some(Action::Peek));
        // Held buttons and the setting both stop it.
        let mut h = Hover::default();
        let held = Pointer { in_zone: true, in_panel: false, buttons: true, popover: false, hover_opens: true };
        assert_eq!(h.poll(0, &held), None);
        assert_eq!(h.poll(500, &held), None);
        // Open never closes on leave.
        h.opened(false);
        assert_eq!(h.poll(10_000, &p(false, false)), None);
    }

    #[test]
    fn zones_and_hit_region() {
        assert_eq!(zone(WORK, 1.0, (0.0, 0.0)), Rect { left: 850, top: 0, right: 1070, bottom: 6 });
        assert_eq!(panel_zone(WORK, 1.0, (1120.0, 440.0)), Rect { left: 384, top: -2, right: 1536, bottom: 456 });
        let f = frame(0.0, (128.0, 24.0), (1120.0, 440.0));
        let ww = 1200.0;
        assert!(hittable(600.0, 10.0, ww, &f, 24.0, 4.0), "on the pill");
        assert!(hittable(600.0 + 64.0 + 10.0, 10.0, ww, &f, 24.0, 4.0), "on its shadow");
        assert!(!hittable(600.0 + 64.0 + 40.0, 10.0, ww, &f, 24.0, 4.0), "beyond the shadow");
        assert!(!hittable(100.0, 300.0, ww, &f, 24.0, 4.0), "the empty window");
        let f = frame(0.0, (0.0, 0.0), (1120.0, 440.0));
        assert!(!hittable(600.0, 2.0, ww, &f, 24.0, 4.0), "nothing drawn: all click-through");
    }

    #[test]
    fn openness_eases_both_ways() {
        let mut o = Openness::default();
        // 560 ms up with BackEase 0.16, 340 down with SineEase in-out.
        o.go(1.0, 0.0);
        assert!((o.value(280.0) - back_ease_out(0.5, 0.16)).abs() < 1e-12);
        assert!(o.value(480.0) > 1.0, "it opens a touch past, then settles");
        assert_eq!(o.value(560.0), 1.0);
        o.go(0.0, 560.0);
        assert!(o.closing());
        assert!((o.value(730.0) - 0.5).abs() < 1e-12);
        assert!(!o.animating(900.0));
    }
}
