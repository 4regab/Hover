//! API keys the user gives Hover (Groq, the cleanup service): sealed with Hover's own
//! key (note.key, kept by DPAPI or the Secret Service) in secrets.dat, never in
//! settings.json, the history or the log. Without that key this run, a key is kept in
//! memory only until Hover quits, and the caller says so: never written in the clear.

use crate::crypto::Crypto;
use crate::json::{self, Json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where a key that was set now lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored { Saved, ThisRunOnly }

pub struct Secrets {
    file: PathBuf,
    crypto: Option<Arc<Crypto>>,
    /// Every key by name, read once.
    map: Mutex<Option<Vec<(String, String)>>>,
}

impl Secrets {
    pub fn new(file: PathBuf, crypto: Option<Arc<Crypto>>) -> Secrets { Secrets { file, crypto, map: Mutex::new(None) } }

    /// The real one: secrets.dat beside settings.json, sealed with Hover's key.
    pub fn system() -> Secrets { Secrets::new(crate::paths::support().join("secrets.dat"), crate::crypto::global()) }

    /// Keys set now are kept across restarts.
    pub fn persistent(&self) -> bool { self.crypto.is_some() }

    fn with<R>(&self, f: impl FnOnce(&mut Vec<(String, String)>) -> R) -> R {
        let mut g = self.map.lock().unwrap();
        if g.is_none() {
            let mut list = vec![];
            if let (Some(c), Ok(bytes)) = (&self.crypto, std::fs::read(&self.file)) {
                // Unsealed by this key or not at all: a file that won't open reads as none.
                if let Ok(Some(m)) = json::parse(&c.open(&bytes)).and_then(|v| v.opt_map(|x| Ok(x.opt_str()?.unwrap_or_default()))) { list = m; }
            }
            *g = Some(list);
        }
        f(g.as_mut().unwrap())
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.with(|m| m.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())).filter(|v| !v.is_empty())
    }

    pub fn has(&self, name: &str) -> bool { self.get(name).is_some() }

    /// Sets (or with None or blank, forgets) a key. Err when it couldn't be written; the
    /// message never holds the key.
    pub fn set(&self, name: &str, value: Option<&str>) -> Result<Stored, String> {
        let v = value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
        let text = self.with(|m| {
            m.retain(|(k, _)| k != name);
            if let Some(v) = v { m.push((name.to_owned(), v)); }
            Json::Obj(m.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect()).compact()
        });
        let Some(c) = &self.crypto else { return Ok(Stored::ThisRunOnly) };
        let tmp = self.file.with_extension("dat.tmp");
        crate::crypto::write_private(&tmp, &c.seal(&text)).and_then(|_| std::fs::rename(&tmp, &self.file))
            .map_err(|e| format!("Hover couldn’t save the key: {}", e.kind()))?;
        Ok(Stored::Saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_sealed_on_disk_and_without_a_key_live_only_in_memory() {
        let d = std::env::temp_dir().join(format!("hover-secrets-{}", crate::guid_n()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("secrets.dat");
        let c = Arc::new(Crypto::with_key([3; 32]));
        let s = Secrets::new(f.clone(), Some(c.clone()));
        assert_eq!(s.set("voice.groq", Some("  gsk_SECRETVALUE ")).unwrap(), Stored::Saved);
        let raw = std::fs::read(&f).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("SECRETVALUE"), "sealed, not plain");
        assert_eq!(Secrets::new(f.clone(), Some(c.clone())).get("voice.groq").as_deref(), Some("gsk_SECRETVALUE"));
        assert_eq!(Secrets::new(f.clone(), Some(Arc::new(Crypto::with_key([4; 32])))).get("voice.groq"), None, "another key opens nothing");
        s.set("voice.groq", Some(" ")).unwrap();
        assert!(!Secrets::new(f.clone(), Some(c)).has("voice.groq"));
        let mem = Secrets::new(d.join("none.dat"), None);
        assert_eq!(mem.set("voice.groq", Some("k")).unwrap(), Stored::ThisRunOnly);
        assert_eq!(mem.get("voice.groq").as_deref(), Some("k"));
        assert!(!d.join("none.dat").exists(), "nothing written without Hover's key");
        let _ = std::fs::remove_dir_all(&d);
    }
}
