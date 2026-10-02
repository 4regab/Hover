//! Hover's product shell, the parts with no window: the shared state (OwlApp), what
//! the resting notch shows, the tray's menu and the office's music. `main.rs` puts
//! them on screen. `mac` is the macOS port's native pieces (and the pure parts of them).

pub mod app;
pub mod browser_host;
pub mod keys;
pub mod mac;
pub mod music;
pub mod pages;
pub mod phonon;
pub mod rest;
pub mod screen;
pub mod speech;
pub mod voice;
// The tray over D-Bus is Linux's; macOS has the menu bar (mac/status.rs).
#[cfg(target_os = "linux")]
pub mod sni;
