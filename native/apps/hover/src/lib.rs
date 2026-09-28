//! Hover's product shell, the parts with no window: the shared state (OwlApp), what
//! the resting notch shows, the tray's menu and the office's music. `main.rs` puts
//! them on screen.

pub mod app;
pub mod keys;
pub mod music;
pub mod pages;
pub mod rest;
#[cfg(not(windows))]
pub mod sni;
