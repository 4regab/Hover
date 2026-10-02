//! macOS: ~/Library/Application Support (where the C# build kept its data there too),
//! the login Keychain for note.key with a 0600 file when there is none, a LaunchAgent
//! for launch at login, and `defaults` for the look.
//!
//! This file is compiled on every OS, so the parts that don't need macOS (the plist
//! text, `defaults`' output, the key marker, the guard over a stand-in Keychain) are
//! tested on Windows and Linux too. Only macOS re-exports it as `platform::*`; only
//! the calls into Security.framework are behind `target_os = "macos"`.

use std::path::{Path, PathBuf};
use crate::crypto::{KeyError, KeyGuard};

/// Environment.SpecialFolder.UserProfile.
pub fn home() -> Option<PathBuf> { std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from) }

/// Windows-only known folders (the editors' install folders); none on macOS, whose
/// editors are .app bundles in /Applications (see palette::roots).
pub fn local_app_data() -> Option<PathBuf> { None }
pub fn program_files() -> Option<PathBuf> { None }

/// ~/Library/Application Support: the data folder's base, and where Electron apps
/// (Code, Cursor) keep their settings.
pub fn app_data() -> Option<PathBuf> { home().map(|h| h.join("Library").join("Application Support")) }
pub fn config_dir() -> Option<PathBuf> { app_data() }

pub fn full_path(p: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    crate::paths::lexical_full_path(p, &cwd)
}

// MARK: note.key

/// What the Keychain is asked, so the guard's logic runs (and is tested) anywhere.
pub trait Keychain {
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String>;
    /// None when there is no such item; Err when the Keychain can't be asked now
    /// (locked, a prompt dismissed or denied, no login session).
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String>;
}

/// The generic password's service; its account is `note.key:<id>`.
pub const SERVICE: &str = "Hover";

/// What note.key holds when the key itself is in the Keychain: this marker and the
/// item's id. A key made for another item never replaces this one's.
const MARKER: &str = "hover-key:keychain:";

/// Where the Swift host of the first macOS build kept the history key (32 bytes).
const SWIFT_SERVICE: &str = "dev.hover.history";
const SWIFT_ACCOUNT: &str = "history-v1";

pub fn account(id: &str) -> String { format!("note.key:{id}") }

pub fn marker_file(id: &str) -> Vec<u8> { format!("{MARKER}{id}\n").into_bytes() }

/// The item id a note.key names, or None when it is not a Keychain marker. An id is
/// what wrap makes (hex); anything else is not trusted as an account name.
pub fn marker_id(stored: &[u8]) -> Option<&str> {
    let id = std::str::from_utf8(stored).ok()?.strip_prefix(MARKER)?.trim();
    (!id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then_some(id)
}

/// The Keychain keeps the key, encrypted with the login and unlocked with it, as DPAPI
/// keeps it on Windows. Without one (no login keychain, a refused write) note.key holds
/// the key itself, readable by this user only (0600). Either way, other programs of the
/// same user can read it, as with DPAPI.
pub struct KeychainGuard<K: Keychain>(pub K);

impl<K: Keychain> KeyGuard for KeychainGuard<K> {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(|e| e.to_string())?;
        let id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        match self.0.set(SERVICE, &account(&id), key) {
            Ok(()) => Ok(marker_file(&id)),
            Err(e) => {
                crate::log::line(&format!("no Keychain ({e}); note.key keeps the key, for this user only"));
                Ok(key.to_vec())
            }
        }
    }

    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, KeyError> {
        if let Some(id) = marker_id(stored) {
            // A locked Keychain or a dismissed prompt may be fine next time; an item
            // that isn't there is gone for good.
            return match self.0.get(SERVICE, &account(id)) {
                Ok(Some(k)) => Ok(k),
                Ok(None) => Err(KeyError::never(format!("the Keychain has no Hover key {id}"))),
                Err(e) => Err(KeyError::not_now(e)),
            };
        }
        if stored.len() == 32 { return Ok(stored.to_vec()); }
        Err(KeyError::never("note.key is not a key this build can read (a Windows DPAPI key only opens on Windows)"))
    }

    fn inherited(&self) -> Result<Option<Vec<u8>>, KeyError> {
        match self.0.get(SWIFT_SERVICE, SWIFT_ACCOUNT) {
            Ok(Some(k)) if k.len() == 32 => Ok(Some(k)),
            Ok(_) => Ok(None),
            Err(e) => Err(KeyError::not_now(e)),
        }
    }
}

/// errSecItemNotFound.
pub const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Security.framework's generic passwords in the login keychain, through the pure-Rust
/// binding (a secret on a `security` command line shows in ps).
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct Login;

#[cfg(target_os = "macos")]
fn describe(e: security_framework::base::Error) -> String {
    format!("Keychain status {}{}", e.code(), e.message().map(|m| format!(" ({m})")).unwrap_or_default())
}

#[cfg(target_os = "macos")]
impl Keychain for Login {
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        security_framework::passwords::set_generic_password(service, account, secret).map_err(describe)
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        use security_framework::passwords::{generic_password, PasswordOptions};
        match generic_password(PasswordOptions::new_generic_password(service, account)) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(describe(e)),
        }
    }
}

/// A generic password by service alone, whatever its account: another program's item
/// (Claude Code's sign-in), which the user is asked to allow. Read-only. None when
/// there is no such item.
#[cfg(target_os = "macos")]
pub fn keychain_find(service: &str) -> Result<Option<Vec<u8>>, String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    match ItemSearchOptions::new().class(ItemClass::generic_password()).service(service).load_data(true).limit(1).search() {
        Ok(found) => Ok(found.into_iter().find_map(|r| match r { SearchResult::Data(d) => Some(d), _ => None })),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(describe(e)),
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct SystemKeyGuard;

#[cfg(target_os = "macos")]
impl KeyGuard for SystemKeyGuard {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> { KeychainGuard(Login).wrap(key) }
    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, KeyError> { KeychainGuard(Login).unwrap(stored) }
    fn inherited(&self) -> Result<Option<Vec<u8>>, KeyError> { KeychainGuard(Login).inherited() }
}

// MARK: Launch at login

/// The LaunchAgent's label and file name: the bundle identifier the first macOS build
/// used.
pub const LAUNCH_AGENT_ID: &str = "dev.hover.desktop";

/// Text in a plist <string>.
pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

/// The LaunchAgent that starts Hover at login: the program, no arguments. Aqua only,
/// so an ssh session's login doesn't start a window-less copy.
pub fn launch_agent_plist(id: &str, exe: &Path) -> String {
    format!(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
        "<plist version=\"1.0\">\n<dict>\n",
        "\t<key>Label</key>\n\t<string>{}</string>\n",
        "\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t</array>\n",
        "\t<key>RunAtLoad</key>\n\t<true/>\n",
        "\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n",
        "\t<key>ProcessType</key>\n\t<string>Interactive</string>\n",
        "</dict>\n</plist>\n"), xml_escape(id), xml_escape(&exe.to_string_lossy()))
}

/// `<key>name</key>` followed by `<true/>`.
fn key_is_true(plist: &str, key: &str) -> bool {
    let k = format!("<key>{key}</key>");
    plist.find(&k).is_some_and(|i| plist[i + k.len()..].trim_start().starts_with("<true/>"))
}

/// Whether a LaunchAgent plist starts a program at login: it has one, RunAtLoad is
/// true, and it isn't Disabled.
pub fn plist_starts_at_login(plist: &str) -> bool {
    plist.contains("<key>ProgramArguments</key>") && key_is_true(plist, "RunAtLoad") && !key_is_true(plist, "Disabled")
}

fn agent_file(dir: &Path, id: &str) -> PathBuf { dir.join(format!("{id}.plist")) }

pub fn agent_enabled(dir: &Path, id: &str) -> bool {
    std::fs::read_to_string(agent_file(dir, id)).is_ok_and(|t| plist_starts_at_login(&t))
}

/// Writes or removes the agent. No `launchctl`: launchd reads ~/Library/LaunchAgents at
/// the next login, and starting it now would only launch a second copy.
pub fn set_agent(dir: &Path, id: &str, exe: Option<&Path>) -> Result<(), String> {
    let f = agent_file(dir, id);
    let Some(exe) = exe else {
        return match std::fs::remove_file(&f) { Err(x) if x.kind() != std::io::ErrorKind::NotFound => Err(x.to_string()), _ => Ok(()) };
    };
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&f, launch_agent_plist(id, exe)).map_err(|e| e.to_string())
}

fn launch_agents() -> Option<PathBuf> { home().map(|h| h.join("Library").join("LaunchAgents")) }

/// ~/Library/LaunchAgents/dev.hover.desktop.plist.
#[derive(Default)]
pub struct SystemAutostart;

impl super::Autostart for SystemAutostart {
    fn enabled(&self) -> bool { launch_agents().is_some_and(|d| agent_enabled(&d, LAUNCH_AGENT_ID)) }

    fn set(&self, on: bool) -> Result<(), String> {
        let dir = launch_agents().ok_or("no $HOME")?;
        if !on { return set_agent(&dir, LAUNCH_AGENT_ID, None); }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        set_agent(&dir, LAUNCH_AGENT_ID, Some(&exe))
    }
}

// MARK: The look: dark or light, and whether things may move

/// What Theme.SystemDark and Animator.Still read on Windows: the appearance (System
/// Settings → Appearance) and "Reduce motion" (Accessibility → Display).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look { pub dark: bool, pub animations: bool }

/// `defaults read -g AppleInterfaceStyle` prints "Dark" in dark mode; in light mode the
/// key does not exist and `defaults` fails (None here).
pub fn parse_dark(out: Option<&str>) -> bool { out.is_some_and(|s| s.trim().eq_ignore_ascii_case("dark")) }

/// `defaults read com.apple.universalaccess reduceMotion`: 1 or 0 (a missing key: None).
pub fn parse_reduce_motion(out: Option<&str>) -> Option<bool> {
    match out?.trim().to_ascii_lowercase().as_str() { "1" | "true" | "yes" => Some(true), "0" | "false" | "no" => Some(false), _ => None }
}

/// `defaults read <domain> <key>`'s output, or None when it fails (no such key).
fn defaults(domain: &str, key: &str) -> Option<String> {
    let o = std::process::Command::new("/usr/bin/defaults").args(["read", domain, key])
        .stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// With nothing to go by: light, and animations on, as Windows' missing values mean.
pub fn look() -> Look {
    let dark = parse_dark(defaults("-g", "AppleInterfaceStyle").as_deref());
    let reduce = parse_reduce_motion(defaults("com.apple.universalaccess", "reduceMotion").as_deref()).unwrap_or(false);
    Look { dark, animations: !reduce }
}

/// The other platforms' look_on reads a given bus; macOS has none.
pub fn look_on(_bus: Option<&str>) -> Look { look() }

/// Calls `changed` whenever the look may have changed, on a thread of its own, by
/// reading it again every three seconds. The system's own signal (the distributed
/// notification AppleInterfaceThemeChangedNotification) needs an Objective-C binding
/// and a run loop on some thread, neither of which this crate has; two `defaults`
/// reads cost a few milliseconds, and a theme switch a few seconds late is not seen.
pub fn watch_look(changed: impl Fn() + Send + 'static) {
    std::thread::Builder::new().name("look".into()).spawn(move || {
        let mut last = look();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let now = look();
            if now != last { last = now; changed(); }
        }
    }).expect("a thread to watch the look");
}

pub fn watch_look_on(_bus: Option<String>, changed: impl Fn() + Send + 'static) { watch_look(changed) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A Keychain in memory; `down` is one that can't be asked (locked, denied).
    #[derive(Default)]
    struct Fake { items: Mutex<HashMap<(String, String), Vec<u8>>>, down: bool, read_only: bool }

    impl Keychain for Fake {
        fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
            if self.down || self.read_only { return Err("Keychain status -25308".into()); }
            self.items.lock().unwrap().insert((service.into(), account.into()), secret.to_vec());
            Ok(())
        }
        fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
            if self.down { return Err("Keychain status -25308".into()); }
            Ok(self.items.lock().unwrap().get(&(service.to_owned(), account.to_owned())).cloned())
        }
    }

    #[test]
    fn the_key_goes_to_the_keychain_and_note_key_holds_only_its_id() {
        let g = KeychainGuard(Fake::default());
        let key = [9u8; 32];
        let stored = g.wrap(&key).unwrap();
        let id = marker_id(&stored).expect("a marker").to_owned();
        assert_eq!(id.len(), 16);
        assert!(std::str::from_utf8(&stored).unwrap().starts_with("hover-key:keychain:"));
        assert_eq!(g.0.items.lock().unwrap().get(&("Hover".to_owned(), format!("note.key:{id}"))), Some(&key.to_vec()));
        assert_eq!(g.unwrap(&stored).unwrap(), key);
        // Two keys, two items.
        assert_ne!(g.wrap(&key).unwrap(), stored);
    }

    #[test]
    fn without_a_keychain_note_key_holds_the_key_and_reads_back() {
        let g = KeychainGuard(Fake { read_only: true, ..Fake::default() });
        let key = [3u8; 32];
        let stored = g.wrap(&key).unwrap();
        assert_eq!(stored, key);
        assert_eq!(g.unwrap(&stored).unwrap(), key);
    }

    #[test]
    fn a_missing_item_is_for_good_and_a_keychain_that_is_down_is_not() {
        let g = KeychainGuard(Fake::default());
        let gone = g.unwrap(&marker_file("00ff00ff00ff00ff")).unwrap_err();
        assert!(!gone.transient, "{gone:?}");
        let g = KeychainGuard(Fake { down: true, ..Fake::default() });
        let locked = g.unwrap(&marker_file("00ff00ff00ff00ff")).unwrap_err();
        assert!(locked.transient, "{locked:?}");
        // Not a key at all: 5 bytes, a Linux marker, a DPAPI blob's start.
        for foreign in [&b"short"[..], b"hover-key:secret-service:0011", &[1, 0, 0, 0, 0xd0, 0x8c, 0x9d, 0xdf][..]] {
            assert!(!g.unwrap(foreign).unwrap_err().transient);
        }
    }

    #[test]
    fn the_marker_is_read_strictly() {
        assert_eq!(marker_id(b"hover-key:keychain:0123abcd\n"), Some("0123abcd"));
        assert_eq!(marker_id(b"hover-key:keychain:  \n"), None);
        assert_eq!(marker_id(b"hover-key:keychain:a/../b"), None);
        assert_eq!(marker_id(b"hover-key:secret-service:0123"), None);
        assert_eq!(marker_id(&[0xff, 0xfe]), None);
        assert_eq!(account("0123abcd"), "note.key:0123abcd");
    }

    #[test]
    fn the_first_macos_builds_history_key_is_inherited() {
        let g = KeychainGuard(Fake::default());
        assert_eq!(g.inherited().unwrap(), None);
        g.0.set("dev.hover.history", "history-v1", &[5u8; 32]).unwrap();
        assert_eq!(g.inherited().unwrap(), Some(vec![5u8; 32]));
        // A value that isn't a key is not one.
        g.0.set("dev.hover.history", "history-v1", b"nope").unwrap();
        assert_eq!(g.inherited().unwrap(), None);
        let down = KeychainGuard(Fake { down: true, ..Fake::default() });
        assert!(down.inherited().unwrap_err().transient);
    }

    #[test]
    fn the_launch_agent_is_valid_xml_text_with_the_path_escaped() {
        let p = launch_agent_plist("dev.hover.desktop", Path::new("/Applications/R&D <Hover>.app/Contents/MacOS/Hover"));
        assert!(p.contains("<string>dev.hover.desktop</string>"));
        assert!(p.contains("<string>/Applications/R&amp;D &lt;Hover&gt;.app/Contents/MacOS/Hover</string>"), "{p}");
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(plist_starts_at_login(&p));
        assert_eq!(xml_escape("a\"b'c"), "a&quot;b&apos;c");
    }

    #[test]
    fn a_plist_starts_at_login_only_when_it_says_so() {
        assert!(!plist_starts_at_login("<dict><key>ProgramArguments</key><array><string>/x</string></array><key>RunAtLoad</key><false/></dict>"));
        assert!(!plist_starts_at_login("<dict><key>RunAtLoad</key><true/></dict>"));
        assert!(!plist_starts_at_login("<dict><key>ProgramArguments</key><array/><key>RunAtLoad</key><true/><key>Disabled</key> <true/></dict>"));
        assert!(plist_starts_at_login("<dict><key>ProgramArguments</key><array/><key>RunAtLoad</key>\n  <true/><key>Disabled</key><false/></dict>"));
    }

    #[test]
    fn the_agent_file_is_written_found_and_removed() {
        let dir = std::env::temp_dir().join(format!("hover-launch-agent-{}", std::process::id())).join("LaunchAgents");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        assert!(!agent_enabled(&dir, "dev.hover.test"));
        // Removing what is not there is fine.
        set_agent(&dir, "dev.hover.test", None).unwrap();
        set_agent(&dir, "dev.hover.test", Some(Path::new("/Applications/Hover.app/Contents/MacOS/Hover"))).unwrap();
        assert!(agent_enabled(&dir, "dev.hover.test"));
        assert!(dir.join("dev.hover.test.plist").is_file());
        set_agent(&dir, "dev.hover.test", None).unwrap();
        assert!(!agent_enabled(&dir, "dev.hover.test"));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn defaults_output_is_read_as_the_system_prints_it() {
        assert!(parse_dark(Some("Dark\n")));
        assert!(!parse_dark(Some("Light")));
        assert!(!parse_dark(None), "no AppleInterfaceStyle key is light mode");
        assert_eq!(parse_reduce_motion(Some("1\n")), Some(true));
        assert_eq!(parse_reduce_motion(Some("0\n")), Some(false));
        assert_eq!(parse_reduce_motion(Some("")), None);
        assert_eq!(parse_reduce_motion(None), None);
    }

    #[test]
    fn app_data_is_library_application_support() {
        if let (Some(h), Some(a)) = (home(), app_data()) { assert_eq!(a, h.join("Library").join("Application Support")); }
    }
}
