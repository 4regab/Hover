//! note.key on Linux against a real Secret Service: GNOME Keyring on a private
//! session bus, started here, so the user's own keyring is never touched. Skipped
//! (and says so) where dbus-daemon or gnome-keyring-daemon isn't installed.
#![cfg(target_os = "linux")]

use hover_core::crypto::{Crypto, KeyGuard};
use hover_core::platform::SystemKeyGuard;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Bus { daemon: Child, keyring: Option<Child>, address: String, root: PathBuf }

impl Drop for Bus {
    fn drop(&mut self) {
        if let Some(k) = &mut self.keyring { let _ = k.kill(); let _ = k.wait(); }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn have(bin: &str) -> bool { Command::new("sh").args(["-c", &format!("command -v {bin}")]).output().is_ok_and(|o| o.status.success()) }

fn bus(name: &str) -> Option<Bus> {
    if !have("dbus-daemon") || !have("gnome-keyring-daemon") {
        eprintln!("skipped: dbus-daemon or gnome-keyring-daemon is not installed");
        return None;
    }
    let root = std::env::temp_dir().join(format!("hover-ss-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("run")).unwrap();
    // A bus of its own with no service directories: the session config would start the
    // desktop's keyring by activation, in the real home, before ours.
    let conf = root.join("bus.conf");
    std::fs::write(&conf, format!(concat!(
        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">",
        "<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth>",
        "<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>"),
        root.display())).unwrap();
    let mut daemon = Command::new("dbus-daemon").arg(format!("--config-file={}", conf.display())).args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(daemon.stdout.take().unwrap()), &mut line).unwrap();
    let address = line.trim().to_owned();
    let mut b = Bus { daemon, keyring: None, address, root };
    b.start_keyring();
    Some(b)
}

impl Bus {
    fn start_keyring(&mut self) {
        let mut k = Command::new("gnome-keyring-daemon").args(["--unlock", "--components=secrets", "--foreground"])
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address).env("HOME", &self.root).env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_RUNTIME_DIR", self.root.join("run")).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        k.stdin.take().unwrap().write_all(b"test-login").unwrap();
        self.keyring = Some(k);
        // Up once the default collection answers.
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            let ok = Command::new("dbus-send").args(["--print-reply", "--dest=org.freedesktop.secrets", "/org/freedesktop/secrets",
                "org.freedesktop.Secret.Service.ReadAlias", "string:default"]).env("DBUS_SESSION_BUS_ADDRESS", &self.address)
                .output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("/collection/login"));
            if ok { return; }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("gnome-keyring didn't come up");
    }

    fn stop_keyring(&mut self) {
        if let Some(mut k) = self.keyring.take() { let _ = k.kill(); let _ = k.wait(); }
    }
}

fn setup() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // The log goes to the data folder; keep it out of the real one.
    ONCE.call_once(|| unsafe { std::env::set_var("HOVER_DATA_DIR", std::env::temp_dir().join(format!("hover-ss-data-{}", std::process::id()))) });
}

#[test]
fn the_key_lives_in_the_keyring_and_note_key_names_it() {
    setup();
    let Some(mut b) = bus("keyring") else { return };
    let guard = SystemKeyGuard { bus: Some(b.address.clone()) };
    let file = b.root.join("note.key");
    let first = Crypto::load_or_create(&file, &guard).unwrap();
    let stored = std::fs::read(&file).unwrap();
    let text = String::from_utf8(stored.clone()).unwrap();
    assert!(text.starts_with("hover-key:secret-service:") && text.ends_with('\n'), "{text:?}");
    let sealed = first.seal("sealed with the keyring's key");
    // A second run finds the same key through the marker.
    assert_eq!(Crypto::load_or_create(&file, &guard).unwrap().open(&sealed), "sealed with the keyring's key");
    let key = guard.unwrap(&stored).unwrap();
    assert_eq!(key.len(), 32);

    // With the keyring gone, the marker can't be read now: nothing is made or moved,
    // and this run has no key (decided: a key is never lost).
    b.stop_keyring();
    assert!(guard.unwrap(&stored).is_err_and(|e| e.transient));
    assert!(Crypto::load_or_create(&file, &guard).is_none());
    assert_eq!(std::fs::read(&file).unwrap(), stored, "note.key untouched");
    // And a new key made meanwhile goes into the file itself, for this user only.
    let raw = guard.wrap(&[9; 32]).unwrap();
    assert_eq!(raw, [9; 32]);
    assert_eq!(guard.unwrap(&raw).unwrap(), [9; 32]);

    // The keyring back: the old item is still there, and the next start reads it.
    b.start_keyring();
    assert_eq!(guard.unwrap(&stored).unwrap(), key);
    assert_eq!(Crypto::load_or_create(&file, &guard).unwrap().open(&sealed), "sealed with the keyring's key");
    // A marker for an item the keyring doesn't have is gone for good: set aside.
    assert!(guard.unwrap(b"hover-key:secret-service:0000000000000000\n").is_err_and(|e| !e.transient));
}

#[test]
fn without_a_bus_note_key_holds_the_key_for_this_user_only() {
    setup();
    let dir = std::env::temp_dir().join(format!("hover-ss-nobus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.key");
    let _ = std::fs::remove_file(&file);
    let guard = SystemKeyGuard { bus: Some("unix:path=/nonexistent/hover-bus".into()) };
    let c = Crypto::load_or_create(&file, &guard).unwrap();
    let stored = std::fs::read(&file).unwrap();
    assert_eq!(stored.len(), 32);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(Crypto::load_or_create(&file, &guard).unwrap().open(&c.seal("x")), "x");
    // A Windows note.key (a DPAPI blob) is not a key here.
    assert!(guard.unwrap(&[1u8; 230]).is_err());
}
