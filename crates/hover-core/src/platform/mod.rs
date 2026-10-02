//! What differs between Windows, Linux and macOS, behind the same few names: the data
//! folder's base, full paths, the key's guard, launch at login and the desktop's look.
//! The logic that uses them is shared (see port/README.md, "Platform layout").

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

/// Compiled everywhere so that its pure parts (the LaunchAgent's text, `defaults`'
/// output, the Keychain guard over a stand-in) are tested on every OS; only macOS
/// takes its names as `platform::*`.
pub mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

/// Settings.LaunchAtLogin, per platform: HKCU\…\Run on Windows, an XDG autostart
/// entry on Linux, a LaunchAgent on macOS.
pub trait Autostart {
    fn enabled(&self) -> bool;
    fn set(&self, on: bool) -> Result<(), String>;
}
