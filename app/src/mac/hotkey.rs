//! Global shortcuts on macOS: Carbon's `RegisterEventHotKey`, which needs no permission
//! and no event tap, as Hover.swift's registerHotKey does (Option-N opens the office,
//! Control-Option-Space is voice, held to talk).
//!
//! The window server hands keys straight to whichever of Hover's own windows has the
//! keyboard (the office in the notch, the app window), and there a hot key doesn't fire:
//! the chord reached the office as a key press and only beeped. So a local key monitor
//! catches the same chords in those windows too. A chord seen both ways is one press: a
//! hold is down once until it comes up, and a toggle ignores a second press within 150 ms.
//!
//! Everything here runs on the main thread (Carbon delivers hot keys to the application's
//! event target, which the run loop serves); the callbacks are `Send + Sync` closures that
//! hand over to the UI thread themselves, as the X11 and Windows ones do.

use super::keycodes::ns_chord;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

type OSStatus = i32;
type Ref = *mut c_void;

#[repr(C)]
struct EventTypeSpec { event_class: u32, event_kind: u32 }

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventHotKeyId { signature: u32, id: u32 }

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn InstallEventHandler(target: Ref, handler: extern "C" fn(Ref, Ref, *mut c_void) -> OSStatus, n: u32, list: *const EventTypeSpec, user: *mut c_void, out: *mut Ref) -> OSStatus;
    fn RegisterEventHotKey(code: u32, mods: u32, id: EventHotKeyId, target: Ref, options: u32, out: *mut Ref) -> OSStatus;
    fn UnregisterEventHotKey(key: Ref) -> OSStatus;
    fn GetEventParameter(event: Ref, name: u32, ty: u32, actual_type: *mut u32, size: usize, actual_size: *mut usize, data: *mut c_void) -> OSStatus;
    fn GetEventKind(event: Ref) -> u32;
}

/// 'keyb', 'hkid', '----', and the two kinds of hot key event (CarbonEvents.h).
const CLASS_KEYBOARD: u32 = 0x6B65_7962;
const TYPE_HOT_KEY_ID: u32 = 0x686B_6964;
const PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D;
const HOT_KEY_PRESSED: u32 = 5;
const HOT_KEY_RELEASED: u32 = 6;
/// Hover's signature on its hot keys ('HVR1').
const SIGNATURE: u32 = 0x4856_5231;
const DEBOUNCE: Duration = Duration::from_millis(150);

/// What a chord does: called with true on the press and, for a hold, false on the release.
pub type Callback = Arc<dyn Fn(bool) + Send + Sync>;

struct Slot {
    /// The EventHotKeyRef, as an address (raw pointers aren't Send).
    key: usize,
    code: u32,
    /// NSEvent's modifier bits, for the local monitor.
    ns_mods: usize,
    hold: bool,
    down: bool,
    last: Option<Instant>,
    cb: Callback,
}

#[derive(Default)]
struct State { slots: HashMap<u32, Slot>, monitor: Option<usize> }

static STATE: Mutex<Option<State>> = Mutex::new(None);
static INSTALL: Once = Once::new();

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// One press or release of chord `id`, from either source. Returns the callback to call
/// (outside the lock), if this is news.
fn event(id: u32, pressed: bool) -> Option<(Callback, bool)> {
    with(|s| {
        let slot = s.slots.get_mut(&id)?;
        if pressed {
            if slot.hold { if slot.down { return None; } slot.down = true; }
            else if slot.last.is_some_and(|t| t.elapsed() < DEBOUNCE) { return None; }
            slot.last = Some(Instant::now());
            Some((slot.cb.clone(), true))
        } else {
            if !slot.hold || !slot.down { return None; }
            slot.down = false;
            Some((slot.cb.clone(), false))
        }
    })
}

extern "C" fn handler(_next: Ref, ev: Ref, _user: *mut c_void) -> OSStatus {
    const EVENT_NOT_HANDLED: OSStatus = -9874;
    let mut key = EventHotKeyId::default();
    let got = unsafe {
        GetEventParameter(ev, PARAM_DIRECT_OBJECT, TYPE_HOT_KEY_ID, std::ptr::null_mut(), std::mem::size_of::<EventHotKeyId>(), std::ptr::null_mut(), (&mut key as *mut EventHotKeyId).cast())
    };
    if got != 0 || key.signature != SIGNATURE { return EVENT_NOT_HANDLED; }
    let pressed = unsafe { GetEventKind(ev) } == HOT_KEY_PRESSED;
    if let Some((cb, down)) = event(key.id, pressed) { cb(down); }
    0
}

fn install() {
    INSTALL.call_once(|| {
        let specs = [EventTypeSpec { event_class: CLASS_KEYBOARD, event_kind: HOT_KEY_PRESSED }, EventTypeSpec { event_class: CLASS_KEYBOARD, event_kind: HOT_KEY_RELEASED }];
        let mut out: Ref = std::ptr::null_mut();
        let r = unsafe { InstallEventHandler(GetApplicationEventTarget(), handler, 2, specs.as_ptr(), std::ptr::null_mut(), &mut out) };
        if r != 0 { hover_core::log::line(&format!("hotkeys: the event handler wasn't installed ({r})")); }
        install_monitor();
    });
}

/// The local monitor: a key event meant for one of Hover's own windows that is a chord of
/// ours is taken (returned as null) and handled as the hot key would have been.
fn install_monitor() {
    let block = RcBlock::new(|e: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { e.as_ref() };
        let (code, chord) = (ev.keyCode() as u32, ns_chord(ev.modifierFlags().0 as usize));
        let kind = ev.r#type();
        let hit = with(|s| {
            s.slots.iter().find(|(_, sl)| sl.code == code && (sl.ns_mods == chord || (kind == NSEventType::KeyUp && sl.down))).map(|(id, _)| *id)
        });
        let Some(id) = hit else { return e.as_ptr() };
        if kind == NSEventType::KeyDown {
            if !ev.isARepeat() { if let Some((cb, down)) = event(id, true) { cb(down); } }
        } else if let Some((cb, down)) = event(id, false) { cb(down); }
        std::ptr::null_mut()
    });
    let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown | NSEventMask::KeyUp, &block) };
    with(|s| s.monitor = token.map(|t| Retained::into_raw(t) as usize));
}

/// Takes the chord (`code` is a virtual key code, `mods` Carbon's mask, `ns_mods` the same
/// chord as NSEvent reports it). `hold` chords call back on the release as well. An error
/// is Carbon's status (-9878 eventHotKeyExistsErr: another app has it, -9868 it is Hover's
/// own already).
pub fn register(id: u32, code: u32, mods: u32, ns_mods: usize, hold: bool, cb: Callback) -> Result<(), i32> {
    install();
    unregister(id);
    let mut key: Ref = std::ptr::null_mut();
    let r = unsafe { RegisterEventHotKey(code, mods, EventHotKeyId { signature: SIGNATURE, id }, GetApplicationEventTarget(), 0, &mut key) };
    if r != 0 { return Err(r); }
    with(|s| { s.slots.insert(id, Slot { key: key as usize, code, ns_mods, hold, down: false, last: None, cb }); });
    Ok(())
}

/// Lets go of a chord. One held now is released first, so no recording is left running.
pub fn unregister(id: u32) {
    let slot = with(|s| s.slots.remove(&id));
    if let Some(sl) = slot {
        unsafe { UnregisterEventHotKey(sl.key as Ref) };
        if sl.hold && sl.down { (sl.cb)(false); }
    }
}

/// Carbon's refusal in words.
pub fn refusal(status: i32, label: &str) -> String {
    match status {
        -9878 => format!("{label} is already in use by another app."),
        -9868 => format!("{label} is already one of Hover’s shortcuts."),
        s => format!("macOS wouldn’t register {label} (status {s})."),
    }
}

/// The monitor is not needed any more (Hover quits).
pub fn stop() {
    let ids: Vec<u32> = with(|s| s.slots.keys().copied().collect());
    for id in ids { unregister(id); }
    if let Some(m) = with(|s| s.monitor.take()) {
        unsafe {
            let obj = m as *mut AnyObject;
            if let Some(o) = Retained::from_raw(obj) { NSEvent::removeMonitor(&o); }
        }
    }
}
