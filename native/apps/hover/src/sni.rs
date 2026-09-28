//! Services/TrayIcon.cs on Linux: a StatusNotifierItem (the tray protocol KDE, the
//! GNOME AppIndicator extension, Xfce, Cinnamon and wlroots bars host) with its menu
//! over com.canonical.dbusmenu, and the balloon as an org.freedesktop.Notifications
//! notification. A left click opens the app window; the menu is Actions.BuildMainMenu.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure, StructureBuilder, Value};

pub use crate::rest::Menu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event { Activate, Item(usize) }

type On = Arc<dyn Fn(Event) + Send + Sync>;
/// A pixmap: width, height, ARGB32 bytes.
type Pixmap = (i32, i32, Vec<u8>);

struct Item { on: On, icon: Vec<Pixmap> }

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &str { "ApplicationStatus" }
    #[zbus(property)]
    fn id(&self) -> &str { "hover" }
    #[zbus(property)]
    fn title(&self) -> &str { "Hover" }
    #[zbus(property)]
    fn status(&self) -> &str { "Active" }
    #[zbus(property)]
    fn window_id(&self) -> i32 { 0 }
    #[zbus(property)]
    fn icon_name(&self) -> &str { "" }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> { self.icon.clone() }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) { (String::new(), vec![], "Hover".into(), String::new()) }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool { false }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath { OwnedObjectPath::try_from("/MenuBar").unwrap() }

    fn activate(&self, _x: i32, _y: i32) { (self.on)(Event::Activate); }
    fn secondary_activate(&self, _x: i32, _y: i32) { (self.on)(Event::Activate); }
    // The host shows the menu itself from /MenuBar.
    fn context_menu(&self, _x: i32, _y: i32) {}
    fn scroll(&self, _delta: i32, _orientation: &str) {}
}

struct DbusMenu { on: On, menu: Arc<Mutex<Menu>>, revision: Arc<Mutex<u32>> }

fn v<'a>(x: impl Into<Value<'a>>) -> OwnedValue { x.into().try_to_owned().unwrap() }

fn props(item: &Option<(String, Option<bool>)>) -> HashMap<String, OwnedValue> {
    let mut p = HashMap::new();
    match item {
        None => { p.insert("type".into(), v("separator")); }
        Some((label, check)) => {
            p.insert("label".into(), v(label.as_str()));
            p.insert("enabled".into(), v(true));
            if let Some(on) = check {
                p.insert("toggle-type".into(), v("checkmark"));
                p.insert("toggle-state".into(), v(if *on { 1i32 } else { 0i32 }));
            }
        }
    }
    p.insert("visible".into(), v(true));
    p
}

/// A menu node, (ia{sv}av): its id, its properties, its children as variants.
#[derive(serde::Serialize, zbus::zvariant::Type)]
struct Layout { id: i32, props: HashMap<String, OwnedValue>, children: Vec<OwnedValue> }

fn node(id: i32, p: HashMap<String, OwnedValue>, children: Vec<OwnedValue>) -> Layout { Layout { id, props: p, children } }

fn child(l: Layout) -> OwnedValue {
    let dict: HashMap<String, Value<'static>> = l.props.into_iter().map(|(k, x)| (k, Value::from(x))).collect();
    let s: Structure<'static> = StructureBuilder::new().add_field(l.id).add_field(dict).add_field(Vec::<Value<'static>>::new()).build().unwrap();
    Value::from(s).try_to_owned().unwrap()
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    #[zbus(property)]
    fn version(&self) -> u32 { 3 }
    #[zbus(property)]
    fn text_direction(&self) -> &str { "ltr" }
    #[zbus(property)]
    fn status(&self) -> &str { "normal" }
    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> { vec![] }

    /// Item n of the menu has id n + 1; the root is 0.
    fn get_layout(&self, parent: i32, _depth: i32, _names: Vec<String>) -> (u32, Layout) {
        let menu = self.menu.lock().unwrap().clone();
        let rev = *self.revision.lock().unwrap();
        let item = |i: usize| node(i as i32 + 1, props(&menu[i]), vec![]);
        if parent > 0 && (parent as usize) <= menu.len() { return (rev, item(parent as usize - 1)); }
        let mut root = HashMap::new();
        root.insert("children-display".to_string(), v("submenu"));
        (rev, node(0, root, (0..menu.len()).map(|i| child(item(i))).collect()))
    }

    fn get_group_properties(&self, ids: Vec<i32>, _names: Vec<String>) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let menu = self.menu.lock().unwrap();
        ids.into_iter().filter(|i| *i >= 1 && (*i as usize) <= menu.len()).map(|i| (i, props(&menu[i as usize - 1]))).collect()
    }

    fn get_property(&self, id: i32, name: &str) -> OwnedValue {
        let menu = self.menu.lock().unwrap();
        if id >= 1 && (id as usize) <= menu.len() { if let Some(x) = props(&menu[id as usize - 1]).remove(name) { return x; } }
        v("")
    }

    fn event(&self, id: i32, event: &str, _data: OwnedValue, _time: u32) {
        if event == "clicked" && id >= 1 { (self.on)(Event::Item(id as usize - 1)); }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        for (id, e, _, _) in events { if e == "clicked" && id >= 1 { (self.on)(Event::Item(id as usize - 1)); } }
        vec![]
    }

    fn about_to_show(&self, _id: i32) -> bool { false }
    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) { (vec![], vec![]) }

    #[zbus(signal)]
    async fn layout_updated(e: &zbus::object_server::SignalEmitter<'_>, revision: u32, parent: i32) -> zbus::Result<()>;
}

pub struct Tray { conn: Connection, menu: Arc<Mutex<Menu>>, revision: Arc<Mutex<u32>> }

pub fn connect(bus: Option<&str>) -> Result<Connection, String> {
    match bus {
        Some(a) => zbus::blocking::connection::Builder::address(a).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string()),
        None => Connection::session().map_err(|e| e.to_string()),
    }
}

impl Tray {
    /// The icon (ARGB32 in network order, as the protocol asks), the menu, and what a
    /// click does. Err when there is no session bus or no tray host to register with.
    pub fn start(bus: Option<&str>, icon: Vec<(i32, i32, Vec<u8>)>, menu: Menu, on: impl Fn(Event) + Send + Sync + 'static) -> Result<Tray, String> {
        let conn = connect(bus)?;
        let on: On = Arc::new(on);
        let menu = Arc::new(Mutex::new(menu));
        let revision = Arc::new(Mutex::new(1u32));
        {
            let s = conn.object_server();
            s.at("/StatusNotifierItem", Item { on: on.clone(), icon }).map_err(|e| e.to_string())?;
            s.at("/MenuBar", DbusMenu { on, menu: menu.clone(), revision: revision.clone() }).map_err(|e| e.to_string())?;
        }
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        conn.request_name(name.as_str()).map_err(|e| e.to_string())?;
        let watcher = zbus::blocking::Proxy::new(&conn, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher").map_err(|e| e.to_string())?;
        watcher.call::<_, _, ()>("RegisterStatusNotifierItem", &(name.as_str(),)).map_err(|e| format!("no tray host ({e})"))?;
        Ok(Tray { conn, menu, revision })
    }

    /// The menu changed (the shortcut's label, Launch at Login's tick).
    pub fn set_menu(&self, menu: Menu) {
        *self.menu.lock().unwrap() = menu;
        let rev = { let mut r = self.revision.lock().unwrap(); *r += 1; *r };
        if let Ok(i) = self.conn.object_server().interface::<_, DbusMenu>("/MenuBar") {
            let _ = zbus::block_on(DbusMenu::layout_updated(i.signal_emitter(), rev, 0));
        }
    }
}

/// TrayIcon.Notify: a notification of six seconds, as the balloon was.
pub fn notify(bus: Option<&str>, title: &str, text: &str) -> Result<u32, String> {
    let conn = connect(bus)?;
    let p = zbus::blocking::Proxy::new(&conn, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").map_err(|e| e.to_string())?;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("hover"));
    p.call("Notify", &("Hover", 0u32, "hover", title, text, Vec::<&str>::new(), hints, 6000i32)).map_err(|e| e.to_string())
}

/// hover.ico's 32 px frame as the protocol's ARGB32, big-endian.
pub fn icon_pixmaps(ico: &[u8]) -> Vec<(i32, i32, Vec<u8>)> {
    let Ok(img) = image::load_from_memory_with_format(ico, image::ImageFormat::Ico) else { return vec![] };
    let mut out = vec![];
    for size in [16u32, 22, 24, 32, 48] {
        let r = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
        let data = r.pixels().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
        out.push((size as i32, size as i32, data));
    }
    out
}
