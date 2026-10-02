//! Hover on macOS (macos/Sources/*.swift, in Rust): the notch over the MacBook's camera
//! housing, the menu bar's usage rings and menu, the global shortcuts, the login shell's
//! environment, and the pickers and notifications the system gives.
//!
//! What can be computed without AppKit lives in modules compiled on every OS, so the
//! tests run on Windows and Linux too: the notch's size from the screen's numbers
//! (`geometry`), the key codes (`keycodes`), the menu's items (`menu`), the rings as
//! pixels (`bar`) and the shell probe's text (`shell_env`). What talks to AppKit is
//! behind `target_os = "macos"`: `hotkey` (Carbon), `status` (NSStatusItem), `cocoa`
//! (the pointer, pickers, notifications, Spaces). The notch window itself is `plat.rs`,
//! which main.rs includes, since it implements the binary's `notch::Plat`.

pub mod bar;
pub mod browser_js;
pub mod geometry;
pub mod keycodes;
pub mod marks;
pub mod menu;
pub mod shell_env;

#[cfg(target_os = "macos")]
pub mod cocoa;
#[cfg(target_os = "macos")]
pub mod hotkey;
#[cfg(target_os = "macos")]
pub mod status;

/// What Settings shows beside a thing this OS can't do, or none where it can.
pub mod notes {
    /// Local speech has no macOS build of its runtime pinned yet (phonon.rs).
    pub const LOCAL_SPEECH: &str = "Local speech isn’t available on macOS yet; use Cloud (Groq).";
    /// The usage rings live in the menu bar on a Mac, not in the notch.
    pub const QUOTAS_IN_MENU_BAR: &str = "On a Mac the usage rings are in the menu bar.";
    /// Hover's own browser is a WKWebView.
    pub const BROWSER: &str = "Agent browser needs macOS.";
    /// The screen panel's live picture needs the system's Screen Recording grant.
    pub const SCREEN_ACCESS: &str = "Allow Screen Recording for Hover to see the agent’s apps live (System Settings → Privacy & Security).";
}

/// The AppleScript that posts a notification, with the two texts quoted so nothing in
/// them can end the string (they come from an agent's words).
pub fn applescript_notification(title: &str, body: &str) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " "));
    format!("display notification {} with title {}", q(body), q(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notification_cannot_break_out_of_its_script() {
        assert_eq!(applescript_notification("Done", "All good"), "display notification \"All good\" with title \"Done\"");
        let s = applescript_notification("a\" & (do shell script \"x\") & \"", "line\nbreak \\ \"q\"");
        assert_eq!(s, "display notification \"line break \\\\ \\\"q\\\"\" with title \"a\\\" & (do shell script \\\"x\\\") & \\\"\"");
    }
}
