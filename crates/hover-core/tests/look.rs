//! The desktop's look on Linux through the settings portal: a stand-in portal (GLib's
//! D-Bus, from Python) on a private bus, so the desktop's own is never asked. It
//! answers ReadOne for the two keys Hover reads and sends SettingChanged when told.
//! Skipped (and says so) where dbus-daemon or Python's GLib isn't installed.
#![cfg(target_os = "linux")]

use hover_core::platform::{look_on, watch_look_on, Look};
use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const PORTAL: &str = r#"
import sys
from gi.repository import Gio, GLib
XML = '''<node><interface name="org.freedesktop.portal.Settings">
<method name="ReadOne"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
<signal name="SettingChanged"><arg type="s"/><arg type="s"/><arg type="v"/></signal>
<property name="version" type="u" access="read"/></interface></node>'''
values = {("org.freedesktop.appearance", "color-scheme"): GLib.Variant("u", 1),
          ("org.gnome.desktop.interface", "enable-animations"): GLib.Variant("b", False)}
conn = Gio.DBusConnection.new_for_address_sync(sys.argv[1], Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
def call(c, sender, path, iface, method, params, inv):
    key = tuple(params.unpack())
    if key in values: inv.return_value(GLib.Variant("(v)", (values[key],)))
    else: inv.return_dbus_error("org.freedesktop.portal.Error.NotFound", "no such key")
node = Gio.DBusNodeInfo.new_for_xml(XML)
conn.register_object("/org/freedesktop/portal/desktop", node.interfaces[0], call, None, None)
def own(*a):
    print("ready", flush=True)
Gio.bus_own_name_on_connection(conn, "org.freedesktop.portal.Desktop", 0, own, None)
def line(ch, cond):
    l = sys.stdin.readline()
    if not l: loop.quit(); return False
    values[("org.freedesktop.appearance", "color-scheme")] = GLib.Variant("u", 2)
    conn.emit_signal(None, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings", "SettingChanged",
                     GLib.Variant("(ssv)", ("org.freedesktop.appearance", "color-scheme", GLib.Variant("u", 2))))
    return True
GLib.io_add_watch(GLib.IOChannel.unix_new(0), GLib.IO_IN, line)
loop = GLib.MainLoop(); loop.run()
"#;

struct Bus { daemon: Child, portal: Option<Child>, address: String }

impl Drop for Bus {
    fn drop(&mut self) {
        if let Some(p) = &mut self.portal { let _ = p.kill(); let _ = p.wait(); }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn have(cmd: &str) -> bool { Command::new("sh").args(["-c", cmd]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) }

fn bus() -> Option<Bus> {
    if !have("command -v dbus-daemon") || !have("/usr/bin/python3 -c 'from gi.repository import Gio'") {
        eprintln!("skipped: dbus-daemon or Python's GLib is not installed");
        return None;
    }
    let root = std::env::temp_dir().join(format!("hover-look-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let conf = root.join("bus.conf");
    // No service directories: nothing of the desktop's is activated on this bus.
    std::fs::write(&conf, format!(concat!(
        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">",
        "<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth>",
        "<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>"),
        root.display())).unwrap();
    let mut daemon = Command::new("dbus-daemon").arg(format!("--config-file={}", conf.display())).args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped()).spawn().unwrap();
    let mut address = String::new();
    std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let address = address.trim().to_owned();
    let script = root.join("portal.py");
    std::fs::write(&script, PORTAL).unwrap();
    let mut portal = Command::new("/usr/bin/python3").arg(&script).arg(&address).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(portal.stdout.take().unwrap()).read_line(&mut ready).unwrap();
    assert_eq!(ready.trim(), "ready");
    Some(Bus { daemon, portal: Some(portal), address })
}

#[test]
fn the_portal_gives_dark_and_no_motion_and_says_when_they_change() {
    let Some(mut b) = bus() else { return };
    assert_eq!(look_on(Some(&b.address)), Look { dark: true, animations: false });
    let (tx, rx) = std::sync::mpsc::channel();
    watch_look_on(Some(b.address.clone()), move || { let _ = tx.send(()); });
    // The watcher subscribes on its own thread; give it a moment before the change.
    std::thread::sleep(Duration::from_millis(300));
    writeln!(b.portal.as_mut().unwrap().stdin.as_mut().unwrap(), "light").unwrap();
    rx.recv_timeout(Duration::from_secs(5)).expect("SettingChanged reached the watcher");
    // color-scheme 2 is "prefer light".
    assert!(!look_on(Some(&b.address)).dark);
}
