//! Core/Crypto.cs: AES-256-GCM over what Hover seals, framed nonce ‖ ciphertext ‖ tag
//! (12 + n + 16 bytes, no associated data). The key is 32 random bytes kept in
//! note.key, wrapped by the platform's KeyGuard: DPAPI (current user) on Windows,
//! the Secret Service or a 0600 file on Linux.

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, Tag};
use std::path::Path;
use std::sync::{Arc, OnceLock};

pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

/// How note.key keeps the key from anyone else: what the file holds for a key, and
/// the key back from what the file holds.
pub trait KeyGuard {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String>;
    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, String>;
}

pub struct Crypto { key: [u8; 32] }

impl Crypto {
    pub fn with_key(key: [u8; 32]) -> Crypto { Crypto { key } }

    /// LoadOrCreateKey: the stored key when it unwraps to 32 bytes; otherwise a new
    /// one, written wrapped. As in C#, a key that can't be unwrapped is replaced, so
    /// what it sealed can't be opened again (asked about in the Phase 3 report).
    pub fn load_or_create(file: &Path, guard: &dyn KeyGuard) -> Crypto {
        if file.exists() {
            match std::fs::read(file).map_err(|e| e.to_string()).and_then(|s| guard.unwrap(&s)) {
                Ok(plain) if plain.len() == 32 => return Crypto { key: plain.try_into().unwrap() },
                Ok(_) => {}
                Err(e) => crate::log::line(&format!("key unwrap failed — {e}")),
            }
        }
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).expect("the system has no randomness");
        if let Err(e) = guard.wrap(&key).and_then(|w| write_private(file, &w).map_err(|e| e.to_string())) {
            crate::log::line(&format!("key write failed — {e}"));
        }
        Crypto { key }
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

static GLOBAL: OnceLock<Arc<Crypto>> = OnceLock::new();

/// Hover's key, loaded or made on first use (the C# static field).
pub fn global() -> Arc<Crypto> {
    GLOBAL.get_or_init(|| Arc::new(Crypto::load_or_create(&crate::paths::key(), &crate::platform::SystemKeyGuard::default()))).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    struct Plain;
    impl KeyGuard for Plain {
        fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> { Ok(key.iter().map(|b| b ^ 0x5a).collect()) }
        fn unwrap(&self, s: &[u8]) -> Result<Vec<u8>, String> { Ok(s.iter().map(|b| b ^ 0x5a).collect()) }
    }

    #[test]
    fn the_key_is_kept_and_a_bad_one_replaced() {
        let d = std::env::temp_dir().join(format!("hover-key-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("note.key");
        let _ = std::fs::remove_file(&f);
        let a = Crypto::load_or_create(&f, &Plain);
        let sealed = a.seal("x");
        assert_eq!(Crypto::load_or_create(&f, &Plain).open(&sealed), "x");
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600); }
        std::fs::write(&f, b"short").unwrap();
        let c = Crypto::load_or_create(&f, &Plain);
        assert_eq!(c.open(&sealed), "");
        assert_eq!(std::fs::read(&f).unwrap().len(), 32);
    }
}
