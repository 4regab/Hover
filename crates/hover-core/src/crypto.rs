//! Core/Crypto.cs: AES-256-GCM over what Hover seals, framed nonce ‖ ciphertext ‖ tag
//! (12 + n + 16 bytes, no associated data). The key is 32 random bytes kept in
//! note.key, wrapped by the platform's KeyGuard: DPAPI (current user) on Windows,
//! the Secret Service or a 0600 file on Linux, the login Keychain or a 0600 file on
//! macOS.

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, Tag};
use std::path::Path;
use std::sync::{Arc, OnceLock};

pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

/// Why a stored key couldn't be read, and whether it can be later.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyError {
    pub reason: String,
    /// It may read next time (the keyring isn't running or stayed locked): the file
    /// is left alone and nothing is sealed this run.
    pub transient: bool,
}

impl KeyError {
    pub fn never(reason: impl Into<String>) -> KeyError { KeyError { reason: reason.into(), transient: false } }
    pub fn not_now(reason: impl Into<String>) -> KeyError { KeyError { reason: reason.into(), transient: true } }
}

/// How note.key keeps the key from anyone else: what the file holds for a key, and
/// the key back from what the file holds.
pub trait KeyGuard {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String>;
    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, KeyError>;
    /// The key an earlier build left in the platform's own store, for a history that
    /// has no note.key beside it (macOS: the first native build kept it in the Keychain
    /// itself). Ok(None) when there is none; an Err that is transient when the store
    /// can't be asked now.
    fn inherited(&self) -> Result<Option<Vec<u8>>, KeyError> { Ok(None) }
}

pub struct Crypto { key: [u8; 32] }

impl Crypto {
    pub fn with_key(key: [u8; 32]) -> Crypto { Crypto { key } }

    /// LoadOrCreateKey, except that a key is never destroyed (decided, 2026-09-28): the
    /// stored key when it unwraps to 32 bytes. One that can never be read (DPAPI
    /// refuses, the keyring item is gone, the file is foreign) is moved aside as
    /// note.key.unreadable-<yyyyMMddHHmmss> for recovery by hand, and a new key made.
    /// One that can't be read now (the keyring isn't running) is left as it is, and
    /// there is no key this run: None, so nothing is sealed that the next run couldn't
    /// open. None too when a new key can't be stored (C# sealed with it anyway, and
    /// that history was lost on the next start).
    pub fn load_or_create(file: &Path, guard: &dyn KeyGuard) -> Option<Crypto> {
        if file.exists() {
            let got = std::fs::read(file).map_err(|e| KeyError::not_now(e.to_string())).and_then(|s| guard.unwrap(&s));
            match got {
                Ok(plain) if plain.len() == 32 => return Some(Crypto { key: plain.try_into().unwrap() }),
                Ok(_) => set_aside(file, "it doesn't hold a 32-byte key")?,
                Err(e) if e.transient => {
                    crate::log::line(&format!("key unwrap failed — {}; no history this run, trying again next start", e.reason));
                    return None;
                }
                Err(e) => set_aside(file, &e.reason)?,
            }
        }
        // A history with no note.key beside it was sealed by a key an earlier build kept
        // elsewhere: a new one would leave that history unreadable (and never destroy it).
        let mut key = [0u8; 32];
        let history = file.parent().is_some_and(|d| d.join("agents").join("index.dat").exists());
        match if history { guard.inherited() } else { Ok(None) } {
            Ok(Some(k)) if k.len() == 32 => {
                key.copy_from_slice(&k);
                crate::log::line("note.key is missing; carrying on with the key the history was sealed with");
            }
            Ok(_) => getrandom::fill(&mut key).expect("the system has no randomness"),
            Err(e) => {
                crate::log::line(&format!("key lookup failed — {}; no history this run, trying again next start", e.reason));
                return None;
            }
        }
        match guard.wrap(&key).and_then(|w| write_private(file, &w).map_err(|e| e.to_string())) {
            Ok(()) => Some(Crypto { key }),
            Err(e) => { crate::log::line(&format!("key write failed — {e}; no history this run")); None }
        }
    }

    /// Crypto.Seal: the text's UTF-8 under a fresh nonce.
    pub fn seal(&self, text: &str) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_SIZE];
        getrandom::fill(&mut nonce).expect("the system has no randomness");
        self.seal_with(text, nonce)
    }

    pub fn seal_with(&self, text: &str, nonce: [u8; NONCE_SIZE]) -> Vec<u8> {
        let gcm = Aes256Gcm::new_from_slice(&self.key).unwrap();
        let mut out = Vec::with_capacity(NONCE_SIZE + text.len() + TAG_SIZE);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(text.as_bytes());
        let tag = gcm.encrypt_in_place_detached(Nonce::from_slice(&nonce), b"", &mut out[NONCE_SIZE..]).expect("AES-GCM seals any length Hover writes");
        out.extend_from_slice(&tag);
        out
    }

    /// Crypto.Open: the text, or "" when the data is missing, short or not this key's.
    pub fn open(&self, data: &[u8]) -> String {
        if data.len() < NONCE_SIZE + TAG_SIZE { return String::new(); }
        let (nonce, rest) = data.split_at(NONCE_SIZE);
        let (cipher, tag) = rest.split_at(rest.len() - TAG_SIZE);
        let mut plain = cipher.to_vec();
        let gcm = Aes256Gcm::new_from_slice(&self.key).unwrap();
        match gcm.decrypt_in_place_detached(Nonce::from_slice(nonce), b"", &mut plain, Tag::from_slice(tag)) {
            // Encoding.UTF8.GetString: bad sequences become U+FFFD.
            Ok(()) => String::from_utf8_lossy(&plain).into_owned(),
            Err(_) => {
                crate::log::line("unseal failed — The computed authentication tag did not match the input authentication tag.");
                String::new()
            }
        }
    }
}

/// Written for this user only: 0600 on Unix (the file may hold the key itself there),
/// as File.WriteAllBytes on Windows, where DPAPI does the protecting.
pub fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(file)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(bytes)
    }
    #[cfg(not(unix))]
    { std::fs::write(file, bytes) }
}

/// Moves an unreadable key out of the way (the planner's naming), or gives up (None)
/// when it can't: a key is never overwritten.
fn set_aside(file: &Path, why: &str) -> Option<()> {
    let mut to = file.as_os_str().to_owned();
    to.push(format!(".unreadable-{}", crate::time::local_compact().replace('-', "")));
    match std::fs::rename(file, &to) {
        Ok(()) => { crate::log::line(&format!("key unwrap failed — {why}; kept as {}, a new key made", Path::new(&to).display())); Some(()) }
        Err(e) => { crate::log::line(&format!("key unwrap failed — {why}; couldn't set it aside ({e}), no history this run")); None }
    }
}

static GLOBAL: OnceLock<Option<Arc<Crypto>>> = OnceLock::new();

/// Hover's key, loaded or made on first use (the C# static field); None when this run
/// has none (see load_or_create), and then the history is off.
pub fn global() -> Option<Arc<Crypto>> {
    GLOBAL.get_or_init(|| Crypto::load_or_create(&crate::paths::key(), &crate::platform::SystemKeyGuard::default()).map(Arc::new)).clone()
}

/// Crypto.InitializeKey: the key a host supplies (the Mac app's, from its Keychain),
/// used in place of note.key for the whole run. Once, 32 bytes, and before anything
/// asked for the key: a repeated or late call, or a key of another length, is refused and
/// changes nothing.
pub fn use_host_key(key: &[u8]) -> Result<(), String> {
    let key: [u8; 32] = key.try_into().map_err(|_| "Invalid or repeated history key initialization.".to_owned())?;
    GLOBAL.set(Some(Arc::new(Crypto::with_key(key)))).map_err(|_| "Invalid or repeated history key initialization.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host's key is taken once, 32 bytes, and then it is Hover's key. This test owns
    /// the process's key (the other tests here never call `global`).
    #[test]
    fn the_hosts_key_is_taken_once() {
        assert!(use_host_key(&[1; 31]).is_err());
        assert!(use_host_key(&[]).is_err());
        assert!(use_host_key(&[9; 32]).is_ok());
        let sealed = global().expect("the host's key").seal("x");
        assert_eq!(Crypto::with_key([9; 32]).open(&sealed), "x");
        assert!(use_host_key(&[9; 32]).is_err(), "not twice");
    }

    /// NIST SP 800-38D / the GCM spec's test case 14 (256-bit zero key, zero IV, one
    /// zero block): the framing puts its IV, ciphertext and tag end to end.
    #[test]
    fn frames_a_known_gcm_vector() {
        let c = Crypto::with_key([0; 32]);
        let sealed = c.seal_with("\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", [0; 12]);
        let hex: String = sealed.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "000000000000000000000000cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919");
        assert_eq!(c.open(&sealed), "\0".repeat(16));
    }

    /// CryptoTests, ported.
    #[test]
    fn round_trips_unicode_with_a_fresh_nonce_and_refuses_tampering() {
        let c = Crypto::with_key([7; 32]);
        let text = "Привет 👋\nsecret note";
        let (a, b) = (c.seal(text), c.seal(text));
        assert_eq!(c.open(&a), text);
        assert_eq!(c.open(&b), text);
        assert_ne!(a, b);
        assert_eq!(a.len(), 12 + text.len() + 16);
        let mut t = c.seal("do not alter");
        *t.last_mut().unwrap() ^= 1;
        assert_eq!(c.open(&t), "");
        assert_eq!(c.open(&[0; 8]), "");
        assert_eq!(c.open(&[]), "");
        assert_eq!(Crypto::with_key([8; 32]).open(&a), "");
        assert_eq!(c.open(&c.seal("")), "");
    }

    /// XOR stands in for the platform's wrapping; "LOCKED" reads as a keyring that
    /// isn't there now, "GONE" as one whose item is gone for good.
    struct Plain;
    impl KeyGuard for Plain {
        fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> { Ok(key.iter().map(|b| b ^ 0x5a).collect()) }
        fn unwrap(&self, s: &[u8]) -> Result<Vec<u8>, KeyError> {
            match s {
                b"LOCKED" => Err(KeyError::not_now("the keyring isn't running")),
                b"GONE" => Err(KeyError::never("the keyring has no such item")),
                _ => Ok(s.iter().map(|b| b ^ 0x5a).collect()),
            }
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hover-key-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn aside(d: &Path) -> Vec<Vec<u8>> {
        std::fs::read_dir(d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("note.key.unreadable-"))
            .map(|e| std::fs::read(e.path()).unwrap()).collect()
    }

    #[test]
    fn the_key_is_kept_and_an_unreadable_one_set_aside_never_lost() {
        let d = dir("kept");
        let f = d.join("note.key");
        let sealed = Crypto::load_or_create(&f, &Plain).unwrap().seal("x");
        assert_eq!(Crypto::load_or_create(&f, &Plain).unwrap().open(&sealed), "x");
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600); }
        for bad in [&b"short"[..], b"GONE"] {
            std::fs::write(&f, bad).unwrap();
            let c = Crypto::load_or_create(&f, &Plain).unwrap();
            assert_eq!(c.open(&sealed), "");
            assert_eq!(std::fs::read(&f).unwrap().len(), 32, "a new key");
            assert!(aside(&d).iter().any(|a| a == bad), "the old one kept beside it");
            for e in std::fs::read_dir(&d).unwrap().flatten() { if e.file_name() != "note.key" { std::fs::remove_file(e.path()).unwrap(); } }
        }
    }

    #[test]
    fn a_history_without_its_note_key_keeps_the_key_an_earlier_build_left() {
        /// The earlier build's key, or a store that can't be asked.
        struct Earlier(Result<Option<Vec<u8>>, KeyError>);
        impl KeyGuard for Earlier {
            fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> { Plain.wrap(key) }
            fn unwrap(&self, s: &[u8]) -> Result<Vec<u8>, KeyError> { Plain.unwrap(s) }
            fn inherited(&self) -> Result<Option<Vec<u8>>, KeyError> { self.0.clone() }
        }
        let old = Crypto::with_key([4; 32]).seal("history");
        // No history yet: the earlier key is not asked for, a new one is made.
        let d = dir("inherit-none");
        let fresh = Crypto::load_or_create(&d.join("note.key"), &Earlier(Ok(Some(vec![4; 32])))).unwrap();
        assert_eq!(fresh.open(&old), "");
        // A history: its key is carried on with, and written back the usual way.
        let d = dir("inherit");
        std::fs::create_dir_all(d.join("agents")).unwrap();
        std::fs::write(d.join("agents/index.dat"), &old).unwrap();
        let c = Crypto::load_or_create(&d.join("note.key"), &Earlier(Ok(Some(vec![4; 32])))).unwrap();
        assert_eq!(c.open(&old), "history");
        assert_eq!(Crypto::load_or_create(&d.join("note.key"), &Plain).unwrap().open(&old), "history");
        // No earlier key, or one that isn't 32 bytes: a new key, as before.
        for none in [Ok(None), Ok(Some(vec![1; 5]))] {
            let d = dir("inherit-new");
            std::fs::create_dir_all(d.join("agents")).unwrap();
            std::fs::write(d.join("agents/index.dat"), &old).unwrap();
            assert!(Crypto::load_or_create(&d.join("note.key"), &Earlier(none)).is_some());
        }
        // The store can't be asked now: no key this run, and no note.key written.
        let d = dir("inherit-later");
        std::fs::create_dir_all(d.join("agents")).unwrap();
        std::fs::write(d.join("agents/index.dat"), &old).unwrap();
        assert!(Crypto::load_or_create(&d.join("note.key"), &Earlier(Err(KeyError::not_now("locked")))).is_none());
        assert!(!d.join("note.key").exists());
    }

    #[test]
    fn a_key_that_cant_be_read_now_is_left_alone_and_nothing_is_sealed() {
        let d = dir("locked");
        let f = d.join("note.key");
        std::fs::write(&f, b"LOCKED").unwrap();
        assert!(Crypto::load_or_create(&f, &Plain).is_none());
        assert_eq!(std::fs::read(&f).unwrap(), b"LOCKED", "untouched, for the next start");
        assert!(aside(&d).is_empty());
    }
}
