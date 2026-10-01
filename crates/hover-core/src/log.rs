//! Core/Log.cs: one line per event, to hover.log in the data folder and to stderr (the
//! debugger's stand-in).

use std::io::Write;
use std::sync::Mutex;

static GATE: Mutex<()> = Mutex::new(());

pub fn line(message: &str) {
    let stamp = format!("{} hover: {message}", crate::time::local_clock());
    eprintln!("{stamp}");
    let _g = GATE.lock();
    // Logging must never take the app down.
    // Unit tests log beside their temporary files, never into the user's data folder.
    let file = if cfg!(test) { std::env::temp_dir().join("hover-core-test.log") } else { crate::paths::log() };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = write!(f, "{stamp}{}", crate::json::NEWLINE);
    }
}
