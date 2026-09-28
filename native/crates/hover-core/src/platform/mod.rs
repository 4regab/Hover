//! What differs between Windows and Linux, behind the same few names: the data
//! folder's base, full paths, the key's guard and launch at login. The logic that
//! uses them is shared (see port/README.md, "Platform layout").

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
pub use self::linux::*;

/// Settings.LaunchAtLogin, per platform: HKCU\…\Run on Windows, an XDG autostart
/// entry on Linux.
pub trait Autostart {
    fn enabled(&self) -> bool;
    fn set(&self, on: bool) -> Result<(), String>;
}
