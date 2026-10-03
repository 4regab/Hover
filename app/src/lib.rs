//! Hover's product shell, the parts with no window: the shared state (OwlApp), what
//! the resting notch shows, the tray's menu and the office's music. `main.rs` puts
//! them on screen. This is the Windows and Linux app; the Mac app is macos/ (Swift) on
//! the hover-backend crate.

#[cfg(target_os = "macos")]
compile_error!("The hover app is for Windows and Linux; on macOS build macos/ (Swift) with scripts/build-macos.sh, which runs on crates/hover-backend.");

pub mod app;
pub mod keys;
pub mod music;
pub mod pages;
pub mod phonon;
pub mod rest;
pub mod screen;
pub mod speech;
pub mod voice;
// The tray over D-Bus is Linux's.
#[cfg(target_os = "linux")]
pub mod sni;

/// pages.rs still names one note from the Mac port (its line for the quota switches, which
/// only prints on a Mac); this goes with that line.
pub mod mac {
    pub mod notes {
        pub const QUOTAS_IN_MENU_BAR: &str = "On a Mac the usage rings are in the menu bar.";
    }
}
