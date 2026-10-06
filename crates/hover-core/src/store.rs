//! One sealed JSON document on disk: what the orchestration records, schedules, watches,
//! custom agents and the like are kept in. Sealed with Hover's key as the history is
//! (crypto.rs), written to a temporary file and then renamed over the old one, so a crash
//! mid-write never leaves half a document. A file that can't be read is set aside, not
//! written over. Blocking: call it off the UI thread.

use crate::crypto::Crypto;
use crate::json::{self, Json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Sealed { file: PathBuf, crypto: Arc<Crypto> }

impl Sealed {
    pub fn new(file: PathBuf, crypto: Arc<Crypto>) -> Sealed { Sealed { file, crypto } }

    /// The document under `name` in `dir` (`<dir>/<name>.dat`).
    pub fn in_dir(dir: &Path, name: &str, crypto: Arc<Crypto>) -> Sealed { Sealed::new(dir.join(format!("{name}.dat")), crypto) }

    pub fn file(&self) -> &Path { &self.file }

    /// The document, or none when there is none yet or it can't be read (then the file is
    /// kept beside it as `.bad`, and the log says why).
    pub fn read(&self) -> Option<Json> {
        let bytes = std::fs::read(&self.file).ok()?;
        match json::parse(&self.crypto.open(&bytes)) {
            Ok(v) if !v.is_null() => Some(v),
            r => {
                let why = r.err().map_or("empty".to_owned(), |e| e.to_string());
                crate::log::line(&format!("store: {} unreadable - {why}", self.file.display()));
                let mut bad = self.file.as_os_str().to_owned();
                bad.push(format!(".{}.bad", crate::guid_n()));
                let _ = std::fs::rename(&self.file, bad);
                None
            }
        }
    }

    pub fn write(&self, v: &Json) -> std::io::Result<()> {
        if let Some(dir) = self.file.parent() { std::fs::create_dir_all(dir)?; }
        let mut tmp = self.file.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, self.crypto.seal(&v.compact()))?;
        std::fs::rename(&tmp, &self.file)
    }

    pub fn remove(&self) { let _ = std::fs::remove_file(&self.file); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_document_is_sealed_and_comes_back_whole() {
        let d = dir("seal");
        let s = Sealed::in_dir(&d, "jobs", Arc::new(Crypto::with_key([5; 32])));
        assert!(s.read().is_none());
        s.write(&Json::obj(vec![("Secret", Json::str("hunter2"))])).unwrap();
        let raw = std::fs::read(s.file()).unwrap();
        assert!(!raw.windows(7).any(|w| w == b"hunter2"), "sealed, not plain text");
        assert_eq!(s.read().unwrap().get("Secret").and_then(Json::as_str), Some("hunter2"));
        // Another key opens nothing and the file is set aside, not written over.
        let other = Sealed::in_dir(&d, "jobs", Arc::new(Crypto::with_key([6; 32])));
        assert!(other.read().is_none());
        assert!(!d.join("jobs.dat").exists());
        assert!(std::fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".bad")));
    }
}
