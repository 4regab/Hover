//! Hover's product shell, the parts with no window: the shared state (OwlApp), what
//! the resting notch shows, the tray's menu and the office's music. `main.rs` puts
//! them on screen.

pub mod app;
pub mod keys;
pub mod music;
pub mod pages;
pub mod phonon;
pub mod rest;
pub mod speech;
pub mod voice;
#[cfg(not(windows))]
pub mod sni;
