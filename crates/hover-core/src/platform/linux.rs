//! Linux: $XDG_DATA_HOME (~/.local/share), the Secret Service for note.key with a
//! 0600 file when there is none, and an XDG autostart entry for launch at login.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use crate::crypto::KeyError;

/// Environment.SpecialFolder.UserProfile.
pub fn home() -> Option<PathBuf> { std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from) }

/// Windows-only known folders (the editors' install folders); none on Linux.
pub fn local_app_data() -> Option<PathBuf> { None }
pub fn program_files() -> Option<PathBuf> { None }

/// $XDG_CONFIG_HOME (~/.config): where Electron apps, KDE and GTK keep their settings.
pub fn config_dir() -> Option<PathBuf> { xdg("XDG_CONFIG_HOME", ".config") }


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

    fn unwrap(&self, stored: &[u8]) -> Result<Vec<u8>, KeyError> {
        if let Some(id) = std::str::from_utf8(stored).ok().and_then(|s| s.strip_prefix(MARKER)) {
            // No keyring now (not started, no session bus, a locked prompt dismissed)
            // may be one later; an item that isn't there is gone for good.
            let ss = SecretService::connect(self.bus.as_deref()).map_err(KeyError::not_now)?;
            return match ss.find(id.trim()) {
                Ok(Some(k)) => Ok(k),
                Ok(None) => Err(KeyError::never(format!("the keyring has no Hover key {}", id.trim()))),
                Err(e) => Err(KeyError::not_now(e)),
            };
        }
        if stored.len() == 32 { return Ok(stored.to_vec()); }
        Err(KeyError::never("note.key is not a key this build can read (a Windows DPAPI key only opens on Windows)"))
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

    /// The key, or None when the keyring has no such item.
    fn find(&self, id: &str) -> Result<Option<Vec<u8>>, String> {
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            self.service()?.call("SearchItems", &(Self::attributes(id),)).map_err(e)?;
        let items: Vec<OwnedObjectPath> = unlocked.into_iter().chain(locked.iter().cloned()).collect();
        let Some(first) = items.first().cloned() else { return Ok(None) };
        self.unlock(locked)?;
        let secrets: HashMap<OwnedObjectPath, (OwnedObjectPath, Vec<u8>, Vec<u8>, String)> =
            self.service()?.call("GetSecrets", &(vec![first.clone()], &self.session)).map_err(e)?;
        secrets.get(&first).map(|s| Some(s.2.clone())).ok_or_else(|| "the keyring gave no secret".into())
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

// MARK: The desktop's look: dark or light, and whether things may move

/// What Theme.SystemDark and Animator.Still read on Windows, from the desktop: the
/// settings portal first (org.freedesktop.appearance color-scheme; GNOME's
/// enable-animations, which its portal passes through), else the desktop's own
/// files: GNOME through gsettings, KDE's kdeglobals, GTK's settings.ini. With
/// nothing to go by: light, as Windows' missing value means, and animations on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look { pub dark: bool, pub animations: bool }

pub fn look() -> Look { look_on(None) }

pub fn look_on(bus: Option<&str>) -> Look {
    let p = portal(bus);
    let dark = p.as_ref().and_then(|p| p.0).map(|scheme| scheme == 1).or_else(files_dark).unwrap_or(false);
    let animations = p.as_ref().and_then(|p| p.1).or_else(files_animations).unwrap_or(true);
    Look { dark, animations }
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";

fn connect(bus: Option<&str>) -> Option<Connection> {
    match bus {
        Some(a) => zbus::blocking::connection::Builder::address(a).ok()?.build().ok(),
        None => Connection::session().ok(),
    }
}

/// A setting through ReadOne (portal version 2), else Read, whose value comes wrapped
/// in one more variant.
fn portal_read(p: &Proxy<'_>, ns: &str, key: &str) -> Option<OwnedValue> {
    if let Ok(v) = p.call::<_, _, OwnedValue>("ReadOne", &(ns, key)) { return Some(v); }
    let v: OwnedValue = p.call("Read", &(ns, key)).ok()?;
    match &*v { Value::Value(inner) => OwnedValue::try_from(&**inner).ok(), _ => Some(v) }
}

/// (color-scheme, enable-animations) from the portal; None when there is no portal.
fn portal(bus: Option<&str>) -> Option<(Option<u32>, Option<bool>)> {
    let c = connect(bus)?;
    let p = Proxy::new(&c, PORTAL, PORTAL_PATH, SETTINGS).ok()?;
    let scheme = portal_read(&p, "org.freedesktop.appearance", "color-scheme").and_then(|v| u32::try_from(v).ok());
    let anim = portal_read(&p, "org.gnome.desktop.interface", "enable-animations").and_then(|v| bool::try_from(v).ok());
    if scheme.is_none() && anim.is_none() { return None; }
    Some((scheme, anim))
}

/// `gsettings get <schema> <key>`, when GNOME's tools are there.
fn gsettings(schema: &str, key: &str) -> Option<String> {
    let o = std::process::Command::new("gsettings").args(["get", schema, key]).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_owned())
}

fn read_config(rel: &str) -> Option<String> { config_dir().and_then(|c| std::fs::read_to_string(c.join(rel)).ok()) }

fn files_dark() -> Option<bool> {
    if let Some(s) = gsettings("org.gnome.desktop.interface", "color-scheme") {
        match s.as_str() { "prefer-dark" => return Some(true), "prefer-light" => return Some(false), _ => {} }
    }
    if let Some(d) = std::env::var("GTK_THEME").ok().and_then(|t| gtk_theme_dark(&t)) { return Some(d); }
    read_config("kdeglobals").and_then(|t| kde_dark(&t))
        .or_else(|| read_config("gtk-4.0/settings.ini").and_then(|t| gtk_dark(&t)))
        .or_else(|| read_config("gtk-3.0/settings.ini").and_then(|t| gtk_dark(&t)))
}

fn files_animations() -> Option<bool> {
    if let Some(s) = gsettings("org.gnome.desktop.interface", "enable-animations") { return Some(s == "true"); }
    read_config("kdeglobals").and_then(|t| kde_animations(&t))
}

/// One key of one [group] of an INI-style file (kdeglobals, settings.ini).
fn ini<'a>(text: &'a str, group: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') { inside = line == format!("[{group}]"); continue; }
        if !inside { continue; }
        if let Some((k, v)) = line.split_once('=') { if k.trim() == key { return Some(v.trim()); } }
    }
    None
}

/// "Adwaita:dark", "Arc-Dark": a GTK theme named for its dark variant.
fn gtk_theme_dark(name: &str) -> Option<bool> {
    let n = name.to_ascii_lowercase();
    if n.is_empty() { return None; }
    Some(n.ends_with(":dark") || n.ends_with("-dark"))
}

/// KDE: the colour scheme's window background, else its name.
pub fn kde_dark(text: &str) -> Option<bool> {
    if let Some(bg) = ini(text, "Colors:Window", "BackgroundNormal") {
        let c: Vec<f64> = bg.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if c.len() >= 3 { return Some((0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0 < 0.5); }
    }
    ini(text, "General", "ColorScheme").map(|s| s.to_ascii_lowercase().contains("dark"))
}

/// KDE: "Animation speed" all the way to instant writes a factor of 0.
pub fn kde_animations(text: &str) -> Option<bool> {
    ini(text, "KDE", "AnimationDurationFactor").and_then(|f| f.parse::<f64>().ok()).map(|f| f > 0.0)
}

/// GTK: gtk-application-prefer-dark-theme, else the theme's name.
pub fn gtk_dark(text: &str) -> Option<bool> {
    if let Some(v) = ini(text, "Settings", "gtk-application-prefer-dark-theme") {
        return Some(matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"));
    }
    ini(text, "Settings", "gtk-theme-name").and_then(gtk_theme_dark)
}

/// Calls `changed` whenever the look may have changed, on a thread of its own:
/// the portal's SettingChanged signal where there is a portal (UserPreferenceChanged's
/// counterpart), else a look at the files every five seconds.
pub fn watch_look(changed: impl Fn() + Send + 'static) { watch_look_on(None, changed) }

pub fn watch_look_on(bus: Option<String>, changed: impl Fn() + Send + 'static) {
    std::thread::Builder::new().name("look".into()).spawn(move || {
        if portal(bus.as_deref()).is_some() {
            if let Some(c) = connect(bus.as_deref()) {
                if let Ok(p) = Proxy::new(&c, PORTAL, PORTAL_PATH, SETTINGS) {
                    if let Ok(signals) = p.receive_signal("SettingChanged") {
                        for m in signals {
                            let Ok((ns, _key, _v)) = m.body().deserialize::<(String, String, OwnedValue)>() else { continue };
                            if ns == "org.freedesktop.appearance" || ns == "org.gnome.desktop.interface" { changed(); }
                        }
                        return;
                    }
                }
            }
        }
        let mut last = look_on(bus.as_deref());
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let now = look_on(bus.as_deref());
            if now != last { last = now; changed(); }
        }
    }).expect("a thread to watch the desktop's look");
}

#[cfg(test)]
mod look_tests {
    use super::*;

    #[test]
    fn reads_the_desktops_own_files() {
        assert_eq!(kde_dark("[General]\nColorScheme=BreezeDark\n"), Some(true));
        assert_eq!(kde_dark("[General]\nColorScheme=BreezeDark\n[Colors:Window]\nBackgroundNormal=239,240,241\n"), Some(false));
        assert_eq!(kde_dark("[Colors:Window]\nBackgroundNormal=32,35,38\n"), Some(true));
        assert_eq!(kde_dark("[KDE]\nSingleClick=false\n"), None);
        assert_eq!(kde_animations("[KDE]\nAnimationDurationFactor=0\n"), Some(false));
        assert_eq!(kde_animations("[KDE]\nAnimationDurationFactor=0.5\n"), Some(true));
        assert_eq!(gtk_dark("[Settings]\ngtk-application-prefer-dark-theme=1\ngtk-theme-name=Adwaita\n"), Some(true));
        assert_eq!(gtk_dark("[Settings]\ngtk-theme-name=Arc-Dark\n"), Some(true));
        assert_eq!(gtk_dark("[Settings]\ngtk-theme-name=Adwaita\n"), Some(false));
        assert_eq!(gtk_theme_dark("Adwaita:dark"), Some(true));
    }
}
