//! Linux: $XDG_DATA_HOME (~/.local/share), the Secret Service for note.key with a
//! 0600 file when there is none, and an XDG autostart entry for launch at login.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

fn home() -> Option<PathBuf> { std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from) }

/// An XDG base directory: the variable when it holds an absolute path (the spec
/// ignores a relative one), else the fallback under $HOME.
fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(fallback)))
}

pub fn app_data() -> Option<PathBuf> { xdg("XDG_DATA_HOME", ".local/share") }

pub fn full_path(p: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    crate::paths::lexical_full_path(p, &cwd)
}

// MARK: note.key

/// What note.key holds when the key itself is in the Secret Service: this marker and
/// the item's id. A key made for another item never replaces this one's.
const MARKER: &str = "hover-key:secret-service:";

/// The Secret Service (GNOME Keyring, KWallet, KeePassXC) keeps the key, encrypted
/// with the login and unlocked with it, as DPAPI keeps it on Windows. Without one
/// (no session bus, no keyring) note.key holds the key itself, readable by this user
/// only (0600). Either way, other programs of the same user can read it, as with DPAPI.
#[derive(Default)]
pub struct SystemKeyGuard {
    /// A D-Bus address instead of the session bus (the tests' own bus).
    pub bus: Option<String>,
}

impl crate::crypto::KeyGuard for SystemKeyGuard {
    fn wrap(&self, key: &[u8]) -> Result<Vec<u8>, String> {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(|e| e.to_string())?;
        let id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        match SecretService::connect(self.bus.as_deref()).and_then(|s| s.store(&id, key)) {
            Ok(()) => Ok(format!("{MARKER}{id}\n").into_bytes()),
            Err(e) => {
                crate::log::line(&format!("no Secret Service ({e}); note.key keeps the key, for this user only"));
                Ok(key.to_vec())
            }
        }
    }

    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, String> {
        if let Some(id) = std::str::from_utf8(stored).ok().and_then(|s| s.strip_prefix(MARKER)) {
            return SecretService::connect(self.bus.as_deref()).and_then(|s| s.find(id.trim()));
        }
        if stored.len() == 32 { return Ok(stored.to_vec()); }
        Err("note.key is not a key this build can read (a Windows DPAPI key only opens on Windows)".into())
    }
}

const BUS: &str = "org.freedesktop.secrets";
const PROMPT_WAIT: Duration = Duration::from_secs(120);

struct SecretService { conn: Connection, session: OwnedObjectPath }

fn e(x: impl std::fmt::Display) -> String { x.to_string() }

impl SecretService {
    fn connect(bus: Option<&str>) -> Result<SecretService, String> {
        let conn = match bus {
            Some(a) => zbus::blocking::connection::Builder::address(a).map_err(e)?.build().map_err(e)?,
            None => Connection::session().map_err(e)?,
        };
        let svc = Proxy::new(&conn, BUS, "/org/freedesktop/secrets", "org.freedesktop.Secret.Service").map_err(e)?;
        let (_, session): (OwnedValue, OwnedObjectPath) = svc.call("OpenSession", &("plain", Value::from(""))).map_err(e)?;
        Ok(SecretService { conn, session })
    }

    fn service(&self) -> Result<Proxy<'_>, String> {
        Proxy::new(&self.conn, BUS, "/org/freedesktop/secrets", "org.freedesktop.Secret.Service").map_err(e)
    }

    fn attributes(id: &str) -> HashMap<&str, &str> {
        HashMap::from([("xdg:schema", "dev.hover.Key"), ("application", "Hover"), ("hover-key", id)])
    }

    /// Unlocks what is locked, through the keyring's own prompt when it needs one.
    fn unlock(&self, paths: Vec<OwnedObjectPath>) -> Result<(), String> {
        if paths.is_empty() { return Ok(()); }
        let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = self.service()?.call("Unlock", &(paths,)).map_err(e)?;
        self.prompt(prompt).map(|_| ())
    }

    fn prompt(&self, prompt: OwnedObjectPath) -> Result<Option<OwnedValue>, String> {
        if prompt.as_str() == "/" { return Ok(None); }
        let p = Proxy::new(&self.conn, BUS, prompt.as_str().to_owned(), "org.freedesktop.Secret.Prompt").map_err(e)?;
        let mut done = p.receive_signal("Completed").map_err(e)?;
        p.call_method("Prompt", &("",)).map_err(e)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || { let _ = tx.send(done.next()); });
        let msg = rx.recv_timeout(PROMPT_WAIT).map_err(|_| "the keyring's prompt got no answer".to_string())?.ok_or("the keyring went away")?;
        let (dismissed, result): (bool, OwnedValue) = msg.body().deserialize().map_err(e)?;
        if dismissed { return Err("the keyring's prompt was dismissed".into()); }
        Ok(Some(result))
    }

    fn find(&self, id: &str) -> Result<Vec<u8>, String> {
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            self.service()?.call("SearchItems", &(Self::attributes(id),)).map_err(e)?;
        let items: Vec<OwnedObjectPath> = unlocked.into_iter().chain(locked.iter().cloned()).collect();
        let Some(first) = items.first().cloned() else { return Err(format!("the keyring has no Hover key {id}")) };
        self.unlock(locked)?;
        let secrets: HashMap<OwnedObjectPath, (OwnedObjectPath, Vec<u8>, Vec<u8>, String)> =
            self.service()?.call("GetSecrets", &(vec![first.clone()], &self.session)).map_err(e)?;
        secrets.get(&first).map(|s| s.2.clone()).ok_or_else(|| "the keyring gave no secret".into())
    }

    fn store(&self, id: &str, key: &[u8]) -> Result<(), String> {
        let coll: OwnedObjectPath = self.service()?.call("ReadAlias", &("default",)).map_err(e)?;
        if coll.as_str() == "/" { return Err("the keyring has no default collection".into()); }
        let c = Proxy::new(&self.conn, BUS, coll.as_str().to_owned(), "org.freedesktop.Secret.Collection").map_err(e)?;
        if c.get_property::<bool>("Locked").map_err(e)? { self.unlock(vec![coll.clone()])?; }
        let attrs: HashMap<String, String> = Self::attributes(id).into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        let props: HashMap<&str, Value> = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from("Hover (note.key)")),
            ("org.freedesktop.Secret.Item.Attributes", Value::from(attrs)),
        ]);
        let secret = (&self.session, Vec::<u8>::new(), key.to_vec(), "application/octet-stream");
        let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = c.call("CreateItem", &(props, secret, true)).map_err(e)?;
        self.prompt(prompt).map(|_| ())
    }
}

// MARK: Launch at login

/// $XDG_CONFIG_HOME/autostart/hover.desktop, which every XDG desktop starts at login.
#[derive(Default)]
pub struct SystemAutostart;

fn autostart_file() -> Option<PathBuf> { xdg("XDG_CONFIG_HOME", ".config").map(|c| c.join("autostart").join("hover.desktop")) }

/// The program to start: the AppImage itself when run from one (its mount point
/// changes each run), else this executable.
fn exe() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").filter(|a| !a.is_empty()).map(PathBuf::from).or_else(|| std::env::current_exe().ok())
}

/// A desktop entry's Exec argument, quoted as the spec asks.
fn quote_exec(p: &Path) -> String {
    let mut s = String::from("\"");
    for c in p.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') { s.push('\\'); }
        s.push(c);
    }
    s.push('"');
    s
}

impl super::Autostart for SystemAutostart {
    fn enabled(&self) -> bool {
        let Some(text) = autostart_file().and_then(|f| std::fs::read_to_string(f).ok()) else { return false };
        text.lines().any(|l| l.strip_prefix("Exec=").is_some_and(|v| !v.trim().is_empty())) && !text.lines().any(|l| l.trim() == "Hidden=true")
    }

    fn set(&self, on: bool) -> Result<(), String> {
        let f = autostart_file().ok_or("no $HOME")?;
        if !on {
            return match std::fs::remove_file(&f) { Err(x) if x.kind() != std::io::ErrorKind::NotFound => Err(x.to_string()), _ => Ok(()) };
        }
        let exe = exe().ok_or("no executable path")?;
        std::fs::create_dir_all(f.parent().unwrap()).map_err(e)?;
        let entry = format!("[Desktop Entry]\nType=Application\nName=Hover\nComment=The agent office in the notch\nExec={}\nIcon=hover\nTerminal=false\nX-GNOME-Autostart-enabled=true\n", quote_exec(&exe));
        std::fs::write(&f, entry).map_err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_is_quoted_as_the_desktop_entry_spec_asks() {
        assert_eq!(quote_exec(Path::new("/opt/My $App/hover\"x")), "\"/opt/My \\$App/hover\\\"x\"");
    }
}
