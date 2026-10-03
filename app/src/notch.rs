//! Owl/Notch.cs's NotchHost and NotchManager: one full-size window at the top centre
//! of the main display; the shape grows from its resting size (pill, alert, or
//! nothing) to the office by one openness value; hover, click and the shortcut open
//! it. What the platform does (placing, focus, click-through) is behind `Plat`.

use crate::ui::NotchWindow;
use hover_notch::{frame_way, open_size, outline, placement, Action, Hover, OfficeSize, Openness, Pointer, Rect, Rest, RestAnim, State, PAD};
use slint::ComponentHandle;
use std::time::Instant;

/// DropShadowEffect's reach around the shape, for the pointer's hit test.
pub const SHADOW_BLUR: f64 = 24.0;
pub const SHADOW_DEPTH: f64 = 4.0;

/// The camera housing on a Mac (NSScreen's safe-area insets and the two strips of menu bar
/// either side of it), in points; or, with `real` false, the notch-sized pill a screen
/// without one gets. The office's strip beside it and the island's wings are laid out
/// from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HwNotch { pub width: f64, pub height: f64, pub real: bool }

/// What differs per platform: Win32 (win.rs), X11 (x11.rs), or nothing (a plain window).
pub trait Plat {
    /// The primary display's work area in device pixels, and its scale.
    fn primary(&self) -> (Rect, f64);
    /// Changes when the displays do (checked every 2 s).
    fn signature(&self) -> String;
    fn cursor(&self) -> (i32, i32);
    fn buttons(&self) -> bool;
    fn place(&self, r: Rect);
    fn raise(&self);
    /// The open office needs the keyboard; the resting notch must never take it.
    fn set_accepts_keys(&self, on: bool);
    fn remember_foreground(&self);
    /// Hand the keyboard back to whatever had it before the notch took it.
    fn restore_foreground(&self);
    fn focus(&self);
    /// Everything outside the shape passes the pointer through: `hit` is the part
    /// that takes it, in window DIPs, and whether the pointer is over it now.
    fn set_hit(&self, over: bool, shape: (f64, f64, f64, f64), scale: f64);
    /// Focus went to something that isn't ours (the click-away rule).
    fn foreground_is_ours(&self) -> bool;
    /// The hardware notch under the window, on a Mac; none elsewhere.
    fn hardware_notch(&self) -> Option<HwNotch> { None }
}

pub struct Notch {
    pub plat: Box<dyn Plat>,
    pub hover: Hover,
    pub open: Openness,
    /// The resting size, springing to each new one (SetSizes).
    pub rest_anim: RestAnim,
    /// With animations off, sizes change at once.
    pub still: bool,
    /// A question waits (or its card is open): hovering doesn't open the office.
    pub asking: bool,
    pub rest_kind: i32,
    pub open_size: (f64, f64),
    pub size: OfficeSize,
    pub scale: f64,
    pub work: Rect,
    pub win: Rect,
    pub t0: Instant,
    pub over: bool,
    pub signature: String,
    pub last_display_check: Instant,
    pub anim: bool,
    pub hover_opens: bool,
    pub popover: bool,
    /// The camera housing under the window (macOS), as of the last layout.
    pub hw: Option<HwNotch>,
}

impl Notch {
    pub fn new(plat: Box<dyn Plat>) -> Notch {
        Notch {
            plat, hover: Hover::default(), open: Openness::default(), rest_anim: RestAnim::default(), still: false, asking: false, rest_kind: 0,
            open_size: (1120.0, 440.0), size: OfficeSize::Default, scale: 1.0, work: Rect { left: 0, top: 0, right: 1920, bottom: 1080 },
            win: Rect { left: 0, top: 0, right: 1, bottom: 1 }, t0: Instant::now(), over: false, signature: String::new(),
            last_display_check: Instant::now(), anim: false, hover_opens: true, popover: false, hw: None,
        }
    }

    pub fn now(&self) -> f64 { self.t0.elapsed().as_secs_f64() * 1000.0 }

    pub fn openness(&self) -> f64 { self.open.value(self.now()) }

    pub fn animating(&self) -> bool { self.open.animating(self.now()) || self.rest_anim.animating(self.now()) }

    /// The resting shape as it is drawn now, and the size it is going to (where its
    /// content is laid out).
    pub fn rest(&self) -> (f64, f64) { self.rest_anim.value(self.now()) }
    pub fn rest_target(&self) -> (f64, f64) { self.rest_anim.target() }
    pub fn set_rest(&mut self, to: (f64, f64)) { let (now, still) = (self.now(), self.still); self.rest_anim.go(to, now, still); }
}

/// NotchShell.Relayout: the shape, its fill and rim, and what shows through it.
pub fn shape(ui: &NotchWindow, n: &Notch, panel: slint::Color) {
    let t = n.openness();
    let f = frame_way(t, n.rest(), n.open_size, n.open.closing());
    let win_w = n.open_size.0 + 2.0 * PAD;
    let x0 = (win_w - f.w) / 2.0;
    ui.set_shape_commands(outline(f.w, f.h, f.r, f.ear, x0).into());
    ui.set_shape_x(x0 as f32);
    ui.set_shape_w(f.w as f32);
    ui.set_shape_h(f.h as f32);
    ui.set_shape_r(f.r as f32);
    // Slint keeps a blurred image of the shadow for each size it is drawn at (up to 16),
    // and in the notch each one was office-sized with a stencil of its own: drawn all
    // through the opening and closing, that was about 70 MB of the GPU's, kept for good.
    // So the shadow shows at rest and once fully open, not while the shape grows.
    ui.set_shadow_on(t <= 0.001 || !n.open.animating(n.now()));
    // Black while small, as a real notch is; the panel's own colour by the time the
    // cards are in.
    let k = f.fill_mix as f32;
    let (r, g, b) = (panel.red() as f32 * k, panel.green() as f32 * k, panel.blue() as f32 * k);
    ui.set_shape_fill(slint::Color::from_rgb_u8(r.round() as u8, g.round() as u8, b.round() as u8));
    ui.set_mini_opacity(f.mini_opacity as f32);
    ui.set_view_opacity(f.view_opacity as f32);
    ui.set_openness(t as f32);
    ui.set_view_visible(t > 0.001 || n.hover.state != State::Rest);
    let (tw, th) = n.rest_target();
    ui.set_rest_w(tw as f32);
    ui.set_rest_h(th as f32);
    n.plat.set_hit(n.over, (x0, 0.0, f.w, f.h + SHADOW_DEPTH), n.scale);
}

/// Layout: one window size for every state (resizing a layered window on each
/// transition makes it blink), at the size Settings → Office size asks for.
pub fn layout(ui: &NotchWindow, n: &mut Notch, panel: slint::Color) {
    let (work, scale) = n.plat.primary();
    n.work = work;
    n.scale = scale;
    n.hw = n.plat.hardware_notch();
    let work_dips = (work.width() as f64 / scale, work.height() as f64 / scale);
    n.open_size = open_size(n.size, work_dips);
    n.win = placement(work, scale, n.open_size);
    ui.set_open_w(n.open_size.0 as f32);
    ui.set_open_h(n.open_size.1 as f32);
    // A Mac's camera housing: the island's two sides, and the office below it.
    let hw = n.hw.filter(|h| h.real);
    ui.set_hw_w(hw.map_or(0.0, |h| h.width as f32));
    ui.set_hw_h(hw.map_or(0.0, |h| h.height as f32));
    crate::hold_gpu();
    ui.window().set_size(slint::PhysicalSize::new(n.win.width() as u32, n.win.height() as u32));
    n.plat.place(n.win);
    n.signature = n.plat.signature();
    shape(ui, n, panel);
}

/// The resting shape from what the pill or the alert measures (NotchHost.RestSize).
pub fn rest_of(ui: &NotchWindow, kind: i32) -> (f64, f64) {
    match kind {
        // The island's items start 4 in.
        1 => hover_notch::rest_size(Rest::Pill(4.0 + ui.get_pill_width() as f64)),
        2 => hover_notch::rest_size(Rest::Card(ui.get_card_w() as f64, ui.get_card_h() as f64)),
        // Voice's card, as it measures.
        3 => hover_notch::rest_size(Rest::Card(ui.get_voice_w() as f64, ui.get_voice_h() as f64)),
        _ => hover_notch::rest_size(Rest::None),
    }
}

/// Pointer bookkeeping for one poll, and what the hover rules say to do.
pub fn poll(n: &mut Notch) -> Option<Action> {
    let (x, y) = n.plat.cursor();
    let buttons = n.plat.buttons();
    let zone = hover_notch::zone(n.work, n.scale, n.rest());
    let panel = hover_notch::panel_zone(n.work, n.scale, n.open_size);
    let p = Pointer { in_zone: zone.contains(x, y), in_panel: panel.contains(x, y), buttons, popover: n.popover, hover_opens: n.hover_opens && !n.asking };
    let now = n.now() as u64;
    let act = n.hover.poll(now, &p);
    // The window takes the pointer only over the shape (and its shadow).
    let f = frame_way(n.openness(), n.rest(), n.open_size, n.open.closing());
    let (dx, dy) = ((x - n.win.left) as f64 / n.scale, (y - n.win.top) as f64 / n.scale);
    n.over = n.win.contains(x, y) && hover_notch::hittable(dx, dy, n.open_size.0 + 2.0 * PAD, &f, SHADOW_BLUR, SHADOW_DEPTH);
    act
}

/// Expand: the office comes in (560 ms, a touch past and back).
pub fn expand(n: &mut Notch, peek: bool, focus: bool) {
    if n.hover.state == State::Rest {
        n.plat.remember_foreground();
        n.plat.set_accepts_keys(true);
        n.plat.raise();
        let now = n.now();
        n.open.go(1.0, now);
    }
    n.hover.opened(peek);
    if focus { n.plat.focus(); }
}

pub fn collapse(n: &mut Notch) {
    if n.hover.state == State::Rest { return; }
    n.hover.collapsed();
    n.plat.restore_foreground();
    n.plat.set_accepts_keys(false);
    let now = n.now();
    n.open.go(0.0, now);
}
