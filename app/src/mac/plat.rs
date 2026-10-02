//! The notch on macOS (Notch.swift's panel): one borderless, transparent window with no
//! shadow, above the menu bar at the status-window level, on every Space and over
//! full-screen apps, placed over the top centre of the built-in display, where the camera
//! housing is (a notch-sized pill at the top centre of a screen without one). winit makes
//! the window; this gives it the rest.
//!
//! - It never takes the keyboard while it rests. AppKit's own answer is a non-activating
//!   NSPanel; winit's window is an NSWindow, so its class is given a subclass whose
//!   `canBecomeKeyWindow` is false until the office opens (`set_accepts_keys`): a click on
//!   the resting notch then neither activates Hover nor takes the focus from what had it.
//! - Everything outside the shape passes the pointer through: `ignoresMouseEvents` is on
//!   except while the pointer is over the shape (`set_hit`, told every poll).
//! - The pointer is polled, as on the other OSes: NSEvent.mouseLocation every 50 ms.
//!
//! Coordinates: hover-notch thinks in device pixels, y down from the top of the primary
//! screen; AppKit in points, y up (`hover_app::mac::geometry` converts).

use crate::notch::{HwNotch, Plat};
use hover_app::mac::{cocoa, geometry};
use hover_notch::Rect;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWorkspace};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the notch's window may become key. Off while the notch rests.
static KEYABLE: AtomicBool = AtomicBool::new(false);

extern "C" fn can_become_key(_this: &AnyObject, _sel: Sel) -> Bool { Bool::new(KEYABLE.load(Ordering::SeqCst)) }
extern "C" fn can_become_main(_this: &AnyObject, _sel: Sel) -> Bool { Bool::NO }

/// winit's window class with those two answers changed. Adds no instance variable, so an
/// object of the class winit made can take it on (the way key-value observing does).
fn notch_class(base: &AnyClass) -> Option<&'static AnyClass> {
    if let Some(c) = AnyClass::get(c"HoverNotchWindow") { return Some(c); }
    let mut b = ClassBuilder::new(c"HoverNotchWindow", base)?;
    unsafe {
        b.add_method(sel!(canBecomeKeyWindow), can_become_key as extern "C" fn(_, _) -> _);
        b.add_method(sel!(canBecomeMainWindow), can_become_main as extern "C" fn(_, _) -> _);
    }
    Some(b.register())
}

/// NSStatusWindowLevel: above the menu bar, where the notch's own strip is.
const STATUS_LEVEL: isize = 25;

/// The NSWindow behind a Slint window (winit's), once winit has made it.
pub fn window_of(w: &slint::Window) -> Option<Retained<NSWindow>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|ww| match ww.window_handle().ok()?.as_raw() {
        RawWindowHandle::AppKit(h) => {
            let view = unsafe { h.ns_view.cast::<objc2_app_kit::NSView>().as_ref() };
            view.window()
        }
        _ => None,
    }).flatten()
}

/// The notch's window as Notch.swift makes its panel.
pub fn configure(win: &NSWindow) {
    win.setOpaque(false);
    win.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
    win.setHasShadow(false);
    // Above the menu bar (Notchy uses the same level), on every Space and over full-screen apps.
    win.setLevel(STATUS_LEVEL);
    win.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::IgnoresCycle);
    win.setHidesOnDeactivate(false);
    win.setAnimationBehavior(NSWindowAnimationBehavior::None);
    win.setMovable(false);
    win.setAcceptsMouseMovedEvents(true);
    win.setIgnoresMouseEvents(true);
    let ptr = win as *const NSWindow as *mut AnyObject;
    let base = unsafe { &*ptr }.class();
    match notch_class(base) {
        Some(c) => { unsafe { objc2::ffi::object_setClass(ptr, c) }; }
        None => hover_core::log::line("notch: the window keeps winit's class (it may take the keyboard when clicked)"),
    }
    win.orderFrontRegardless();
}

/// The screen the notch is on, the primary screen's height, and the notch there.
type Where = (geometry::ScreenInfo, f64, geometry::Geometry);

pub struct Mac {
    mtm: MainThreadMarker,
    /// Set by main.rs once winit has made the window.
    pub win: Rc<RefCell<Option<Retained<NSWindow>>>>,
    previous: RefCell<Option<Retained<NSRunningApplication>>>,
    hit: Cell<Option<bool>>,
    /// The displays as last read: the pointer is polled 20 times a second, and the displays
    /// are looked at afresh every two seconds (`signature`).
    seen: RefCell<Option<Where>>,
}

impl Mac {
    pub fn new(mtm: MainThreadMarker) -> Mac {
        Mac { mtm, win: Rc::new(RefCell::new(None)), previous: RefCell::new(None), hit: Cell::new(None), seen: RefCell::new(None) }
    }

    /// Reads the displays now.
    fn read(&self) -> Option<Where> {
        let screens = cocoa::screens(self.mtm);
        let s = screens.get(geometry::choose(&screens)?)?.clone();
        let g = geometry::geometry(&s);
        Some((s, screens.first()?.frame.h, g))
    }

    /// The displays as last read (read once if never).
    fn screen(&self) -> Option<Where> {
        if self.seen.borrow().is_none() { *self.seen.borrow_mut() = self.read(); }
        self.seen.borrow().clone()
    }

    fn window(&self) -> Option<Retained<NSWindow>> { self.win.borrow().clone() }
}

impl Plat for Mac {
    fn primary(&self) -> (Rect, f64) {
        // The whole frame, not the visible one: the notch sits over the menu bar.
        self.screen().map_or((Rect { left: 0, top: 0, right: 1512, bottom: 982 }, 1.0), |(s, ph, _)| (geometry::device_rect(&s, ph), s.scale))
    }

    /// Reads the displays afresh (this is the two-second look), and keeps what it finds.
    fn signature(&self) -> String {
        *self.seen.borrow_mut() = self.read();
        self.screen().map_or_else(String::new, |(s, _, g)| geometry::signature(&s, &g))
    }

    fn hardware_notch(&self) -> Option<HwNotch> {
        self.screen().map(|(_, _, g)| HwNotch { width: g.width, height: g.height, real: g.has_notch })
    }

    fn cursor(&self) -> (i32, i32) {
        let (x, y) = cocoa::pointer();
        self.screen().map_or((-1, -1), |(s, ph, _)| geometry::pointer_px(x, y, s.scale, ph))
    }

    fn buttons(&self) -> bool { cocoa::button_down() }

    fn place(&self, r: Rect) {
        let (Some(w), Some((s, ph, _))) = (self.window(), self.screen()) else { return };
        let a = geometry::appkit_frame(r, s.scale, ph);
        w.setFrame_display(NSRect::new(NSPoint::new(a.x, a.y), NSSize::new(a.w, a.h)), false);
    }

    fn raise(&self) { if let Some(w) = self.window() { w.orderFrontRegardless(); } }

    fn set_accepts_keys(&self, on: bool) { KEYABLE.store(on, Ordering::SeqCst); }

    fn remember_foreground(&self) {
        let front = NSWorkspace::sharedWorkspace().frontmostApplication();
        let ours = NSRunningApplication::currentApplication().processIdentifier();
        if let Some(f) = front.filter(|f| f.processIdentifier() != ours) { *self.previous.borrow_mut() = Some(f); }
    }

    fn restore_foreground(&self) {
        // Only if Hover still has the keyboard: the user may have clicked elsewhere.
        if !self.foreground_is_ours() { return; }
        if let Some(p) = self.previous.borrow_mut().take() { p.activateWithOptions(NSApplicationActivationOptions::empty()); }
    }

    fn focus(&self) {
        let Some(w) = self.window() else { return };
        cocoa::activate(self.mtm);
        w.makeKeyAndOrderFront(None);
    }

    /// Only the shape (and its shadow's reach) takes the pointer.
    fn set_hit(&self, over: bool, _shape: (f64, f64, f64, f64), _scale: f64) {
        if self.hit.get() == Some(over) { return; }
        self.hit.set(Some(over));
        if let Some(w) = self.window() { w.setIgnoresMouseEvents(!over); }
    }

    fn foreground_is_ours(&self) -> bool { self.window().is_some_and(|w| w.isKeyWindow()) }
}
