//! What Hover asks of AppKit that isn't the notch's window, the status item or the hot
//! keys: the screens, the pointer, the folder and file pickers, notifications, opening a
//! link, and the app's own activation. Main thread only (each function takes the marker
//! or runs where a window already does).

use super::geometry::{Area, ScreenInfo};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent, NSOpenPanel, NSScreen, NSWorkspace};
use objc2_foundation::{ns_string, NSArray, NSBundle, NSNumber, NSRect, NSString, NSURL};
// Deprecated for UserNotifications, which needs a bundle to speak as; this one works for
// both a bundled Hover.app and the bare binary (which uses AppleScript instead).
#[allow(deprecated)]
use objc2_foundation::{NSUserNotification, NSUserNotificationCenter};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// boolean_t: nonzero for the built-in display.
    fn CGDisplayIsBuiltin(display: u32) -> i32;
}

fn area(r: NSRect) -> Area { Area { x: r.origin.x, y: r.origin.y, w: r.size.width, h: r.size.height } }

/// The strip of menu bar either side of the camera housing, or none when the system
/// reports an empty rectangle (a screen without one).
fn strip(r: NSRect) -> Option<Area> {
    let a = area(r);
    (a.w > 0.0 && a.h > 0.0).then_some(a)
}

/// NSScreen.screens, as the notch's geometry reads them. The first is the primary (the one
/// with the menu bar), whose height every conversion needs.
pub fn screens(mtm: MainThreadMarker) -> Vec<ScreenInfo> {
    NSScreen::screens(mtm).iter().map(|s| {
        let id = s.deviceDescription().objectForKey(ns_string!("NSScreenNumber"))
            .and_then(|o| o.downcast::<NSNumber>().ok()).map_or(0, |n| n.unsignedIntValue());
        ScreenInfo {
            id,
            frame: area(s.frame()),
            visible: area(s.visibleFrame()),
            scale: s.backingScaleFactor(),
            builtin: unsafe { CGDisplayIsBuiltin(id) } != 0,
            safe_top: s.safeAreaInsets().top,
            left: strip(s.auxiliaryTopLeftArea()),
            right: strip(s.auxiliaryTopRightArea()),
        }
    }).collect()
}

/// NSEvent.mouseLocation: points, y up from the bottom of the primary screen.
pub fn pointer() -> (f64, f64) {
    let p = NSEvent::mouseLocation();
    (p.x, p.y)
}

/// Whether a mouse button is down anywhere (NSEvent.pressedMouseButtons).
pub fn button_down() -> bool { NSEvent::pressedMouseButtons() != 0 }

/// No Dock icon and no menu bar of its own (`.accessory`); winit is asked for the same when
/// it makes its event loop, this is the belt for a loop made by something else.
pub fn accessory(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Bring Hover forward, for a dialog or a window the user asked for.
#[allow(deprecated)]
pub fn activate(mtm: MainThreadMarker) { NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true); }

/// The folder (or file) picker: the system's open panel. `types` are file extensions, for
/// a file pick; empty is any file. None when cancelled.
#[allow(deprecated)]
pub fn pick(mtm: MainThreadMarker, folder: bool, message: &str, types: &[&str]) -> Option<String> {
    activate(mtm);
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(folder);
    panel.setCanChooseFiles(!folder);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str(message)));
    if !types.is_empty() {
        let list: Vec<Retained<NSString>> = types.iter().map(|t| NSString::from_str(t)).collect();
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&list)));
    }
    // NSModalResponseOK
    if panel.runModal() != 1 { return None; }
    let url = panel.URL()?;
    url.path().map(|p| p.to_string())
}

/// A link in the chat opens in the browser (http and https only: hover-md makes sure).
pub fn open_url(url: &str) -> bool {
    NSURL::URLWithString(&NSString::from_str(url)).is_some_and(|u| NSWorkspace::sharedWorkspace().openURL(&u))
}

/// A notification in the system's centre. One from a bundled Hover.app carries its icon;
/// the bare binary (cargo run) has no bundle to speak as, so AppleScript posts it.
#[allow(deprecated)]
pub fn notify(title: &str, body: &str) {
    if NSBundle::mainBundle().bundleIdentifier().is_some() {
        let n = NSUserNotification::new();
        n.setTitle(Some(&NSString::from_str(title)));
        n.setInformativeText(Some(&NSString::from_str(body)));
        NSUserNotificationCenter::defaultUserNotificationCenter().deliverNotification(&n);
        return;
    }
    let script = super::applescript_notification(title, body);
    let _ = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).spawn();
}
