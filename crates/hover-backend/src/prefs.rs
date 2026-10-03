//! Settings.MaxRunning: how many tasks run at once (1 to 6, 3 unless changed), which the
//! Mac's Settings offers. hover-core's settings.json has no such key, so it is kept here,
//! in a file of the backend's own beside settings.json, and handed to the sessions
//! (KiroSessions::set_max_running) at start and on every change.

use hover_core::json::Json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const DEFAULT: usize = 3;
pub const MIN: usize = 1;
pub const MAX: usize = 6;

pub struct MaxRunning { file: PathBuf, n: AtomicUsize }

impl MaxRunning {
    /// Read from the file (3 when there is none, or it can't be read).
    pub fn load(file: PathBuf) -> MaxRunning {
        let n = std::fs::read(&file).ok()
            .and_then(|b| hover_core::json::parse(&hover_core::json::text_of(&b)).ok())
            .and_then(|v| match v.get("MaxRunning") { Some(Json::Num(n)) => n.parse::<i64>().ok(), _ => None })
            .map_or(DEFAULT, |n| n.clamp(MIN as i64, MAX as i64) as usize);
        MaxRunning { file, n: AtomicUsize::new(n) }
    }

    pub fn get(&self) -> usize { self.n.load(Ordering::SeqCst) }

    /// Math.Clamp(value, 1, 6), kept.
    pub fn set(&self, n: i32) {
        let n = n.clamp(MIN as i32, MAX as i32) as usize;
        if self.n.swap(n, Ordering::SeqCst) == n && self.file.exists() { return; }
        let text = Json::obj(vec![("MaxRunning", Json::int(n as i64))]).compact();
        let tmp = self.file.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &self.file)) {
            hover_core::log::line(&format!("backend settings save failed — {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_between_runs_and_clamped() {
        let d = std::env::temp_dir().join(format!("hover-backend-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("backend.json");
        let _ = std::fs::remove_file(&f);
        let m = MaxRunning::load(f.clone());
        assert_eq!(m.get(), 3);
        m.set(4);
        assert_eq!(MaxRunning::load(f.clone()).get(), 4);
        m.set(0);
        assert_eq!(m.get(), 1);
        m.set(99);
        assert_eq!(MaxRunning::load(f.clone()).get(), 6);
        std::fs::write(&f, "not json").unwrap();
        assert_eq!(MaxRunning::load(f).get(), 3);
    }
}
