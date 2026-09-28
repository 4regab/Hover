//! The Linux tray and notifications against a stand-in tray host: a
//! StatusNotifierWatcher and a notification server (GLib's D-Bus, from Python) on a
//! private bus. The host does what a panel does: reads the item's properties and its
//! menu, clicks a menu item and the icon. Skipped (and says so) without dbus-daemon or
//! Python's GLib.
#![cfg(target_os = "linux")]

use hover_app::sni::{self, Event};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const HOST: &str = r#"
import sys, json
from gi.repository import Gio, GLib
XML = '''<node>
<interface name="org.kde.StatusNotifierWatcher"><method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method></interface>
<interface name="org.freedesktop.Notifications"><method name="Notify">
<arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/>
<arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method></interface></node>'''
conn = Gio.DBusConnection.new_for_address_sync(sys.argv[1], Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
def out(**kw): print(json.dumps(kw), flush=True)
def get(dest, path, iface, prop):
    return conn.call_sync(dest, path, "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", (iface, prop)), None, 0, 2000, None).unpack()[0]
def visit(dest):
    sni = "org.kde.StatusNotifierItem"
    title, menu, pix = get(dest, "/StatusNotifierItem", sni, "Title"), get(dest, "/StatusNotifierItem", sni, "Menu"), get(dest, "/StatusNotifierItem", sni, "IconPixmap")
    rev, layout = conn.call_sync(dest, menu, "com.canonical.dbusmenu", "GetLayout", GLib.Variant("(iias)", (0, -1, [])), None, 0, 2000, None).unpack()
    items = [(c[0], c[1].get("label", ""), c[1].get("type", ""), c[1].get("toggle-state", -1)) for c in layout[2]]
    out(kind="item", title=title, menu=menu, sizes=[p[0] for p in pix], items=items)
    conn.call_sync(dest, menu, "com.canonical.dbusmenu", "Event", GLib.Variant("(isvu)", (1, "clicked", GLib.Variant("i", 0), 0)), None, 0, 2000, None)
    conn.call_sync(dest, "/StatusNotifierItem", sni, "Activate", GLib.Variant("(ii)", (0, 0)), None, 0, 2000, None)
    out(kind="clicked")
    return False
def call(c, sender, path, iface, method, params, inv):
    if method == "RegisterStatusNotifierItem":
        inv.return_value(None)
        GLib.idle_add(visit, sender)
    else:
        a = params.unpack()
        out(kind="notify", app=a[0], summary=a[3], body=a[4], timeout=a[7])
        inv.return_value(GLib.Variant("(u)", (7,)))
node = Gio.DBusNodeInfo.new_for_xml(XML)
for i, path in ((0, "/StatusNotifierWatcher"), (1, "/org/freedesktop/Notifications")):
    conn.register_object(path, node.interfaces[i], call, None, None)
owned = [0]
def own(*a):
    owned[0] += 1
    if owned[0] == 2: print(json.dumps({"kind": "ready"}), flush=True)
Gio.bus_own_name_on_connection(conn, "org.kde.StatusNotifierWatcher", 0, own, None)
Gio.bus_own_name_on_connection(conn, "org.freedesktop.Notifications", 0, own, None)
GLib.MainLoop().run()
"#;

struct Bus { daemon: Child, host: Child, address: String }

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.host.kill();
        let _ = self.host.wait();
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn have(cmd: &str) -> bool { Command::new("sh").args(["-c", cmd]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) }

#[test]
fn a_tray_host_sees_the_icon_and_the_menu_and_clicks_them() {
    if !have("command -v dbus-daemon") || !have("/usr/bin/python3 -c 'from gi.repository import Gio'") {
        eprintln!("skipped: dbus-daemon or Python's GLib is not installed");
        return;
    }
    let root = std::env::temp_dir().join(format!("hover-tray-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let conf = root.join("bus.conf");
    std::fs::write(&conf, format!(concat!(
        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">",
        "<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth>",
        "<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>"),
        root.display())).unwrap();
    let mut daemon = Command::new("dbus-daemon").arg(format!("--config-file={}", conf.display())).args(["--nofork", "--print-address=1"]).stdout(Stdio::piped()).spawn().unwrap();
    let mut address = String::new();
    BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let address = address.trim().to_owned();
    std::fs::write(root.join("host.py"), HOST).unwrap();
    let mut host = Command::new("/usr/bin/python3").arg(root.join("host.py")).arg(&address).stdout(Stdio::piped()).spawn().unwrap();
    let mut lines = BufReader::new(host.stdout.take().unwrap()).lines();
    let bus = Bus { daemon, host, address };
    assert!(lines.next().unwrap().unwrap().contains("ready"));

    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let icon = sni::icon_pixmaps(include_bytes!("../assets/hover.ico"));
    let menu = hover_app::rest::tray_menu("Alt+N", true);
    let _tray = sni::Tray::start(Some(&bus.address), icon, menu, move |e| { let _ = tx.lock().unwrap().send(e); }).unwrap();

    let item = lines.next().unwrap().unwrap();
    assert!(item.contains(r#""title": "Hover""#) && item.contains(r#""menu": "/MenuBar""#), "{item}");
    assert!(item.contains(r#""sizes": [16, 22, 24, 32, 48]"#), "{item}");
    // Actions.BuildMainMenu's items, the separators, and Launch at Login ticked.
    for want in [r#"[1, "Open Agent Office  Alt+N", "", -1]"#, r#"[2, "Open App Window", "", -1]"#, r#"[3, "", "separator", -1]"#,
        r#"[4, "Launch at Login", "", 1]"#, r#"[6, "Settings\u2026", "", -1]"#, r#"[7, "Quit Hover", "", -1]"#] {
        assert!(item.contains(want), "{want} in {item}");
    }
    assert!(lines.next().unwrap().unwrap().contains("clicked"));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Item(0));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Activate);

    assert_eq!(sni::notify(Some(&bus.address), "Kiro is done: Tidy up", "Fixed it").unwrap(), 7);
    let n = lines.next().unwrap().unwrap();
    assert!(n.contains(r#""app": "Hover""#) && n.contains(r#""summary": "Kiro is done: Tidy up""#) && n.contains(r#""body": "Fixed it""#) && n.contains(r#""timeout": 6000"#), "{n}");
}
