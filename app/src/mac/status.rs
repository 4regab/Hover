//! MenuBar.swift: Hover's place in the menu bar. The usage rings live here on a Mac, not
//! in the notch: each tool switched on shows its logo in a ring of its used share and the
//! percentage beside it (green, amber from 70 %, red from 90 %), as its own status item.
//! With none switched on there is one plain item (SF Symbols' sparkles). Every item opens
//! the same menu (`menu.rs`): the readings' details, "Show in menu bar", the agents at
//! work, Open Office, Office in a Window, Start a Voice Task, Open on Hover, Launch at
//! Login, Settings, Refresh and Quit.
//!
//! The rings are pixels (`bar.rs`) in the ink of the menu bar's appearance, drawn again
//! when it changes (`refresh_appearance`, called from main.rs's two-second look).
//! A click on a menu item asks for an `Act`; `on` is told, and main.rs does it after the
//! menu has closed.

use super::bar;
use super::menu::{Act, Entry, Icon, Item};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags,
    NSFont, NSFontWeightMedium, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{NSArray, NSData, NSObject, NSObjectProtocol, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// One ring to show: the tool, its used share (None: unknown), and the words beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub id: String,
    pub used: Option<f64>,
    /// "38%", "—" or "…".
    pub percent: String,
    /// The tool's name and reading, for the tooltip.
    pub tip: String,
}

struct Ivars {
    on: Box<dyn Fn(Act)>,
    acts: RefCell<Vec<Act>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "HoverStatusTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        /// Every menu item's action: the item's tag is its place in `acts`.
        #[unsafe(method(clicked:))]
        fn clicked(&self, sender: &NSMenuItem) {
            let act = self.ivars().acts.borrow().get(sender.tag() as usize).cloned();
            if let Some(a) = act { (self.ivars().on)(a); }
        }
    }

    unsafe impl NSObjectProtocol for Target {}
);

impl Target {
    fn new(mtm: MainThreadMarker, on: Box<dyn Fn(Act)>) -> Retained<Target> {
        let this = Self::alloc(mtm).set_ivars(Ivars { on, acts: RefCell::new(vec![]) });
        unsafe { msg_send![super(this), init] }
    }
}

pub struct StatusBar {
    mtm: MainThreadMarker,
    target: Retained<Target>,
    plain: Retained<NSStatusItem>,
    rings: RefCell<Vec<(Ring, Retained<NSStatusItem>)>>,
    entries: RefCell<Vec<Entry>>,
    dark: Cell<Option<bool>>,
}

fn ns(s: &str) -> Retained<NSString> { NSString::from_str(s) }

/// A PNG as an image of `pts` points (its pixels are `scale` times that).
fn image_of(png: &[u8], pts: f64) -> Option<Retained<NSImage>> {
    let data = NSData::with_bytes(png);
    let img = NSImage::initWithData(NSImage::alloc(), &data)?;
    img.setSize(NSSize::new(pts, pts));
    Some(img)
}

impl StatusBar {
    pub fn new(mtm: MainThreadMarker, on: impl Fn(Act) + 'static) -> Rc<StatusBar> {
        let target = Target::new(mtm, Box::new(on));
        let bar = NSStatusBar::systemStatusBar();
        let plain = bar.statusItemWithLength(-1.0);
        if let Some(b) = plain.button(mtm) {
            let sparkles = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns("sparkles"), Some(&ns("Hover")));
            if let Some(i) = &sparkles { i.setTemplate(true); }
            b.setImage(sparkles.as_deref());
            b.setToolTip(Some(&ns("Hover")));
        }
        Rc::new(StatusBar { mtm, target, plain, rings: RefCell::new(vec![]), entries: RefCell::new(vec![]), dark: Cell::new(None) })
    }

    /// Whether the menu bar is drawn dark now (it follows the wallpaper, not the app).
    fn is_dark(&self) -> bool {
        let Some(b) = self.plain.button(self.mtm) else { return true };
        let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe { NSAppearanceNameDarkAqua }]);
        b.effectiveAppearance().bestMatchFromAppearancesWithNames(&names).is_some_and(|n| *n == *unsafe { NSAppearanceNameDarkAqua })
    }

    /// The menu and the rings; nothing is touched that didn't change.
    pub fn set(&self, entries: Vec<Entry>, rings: Vec<Ring>) {
        let dark = self.is_dark();
        let dark_changed = self.dark.get() != Some(dark);
        let menu_changed = *self.entries.borrow() != entries;
        let ids_same = {
            let cur = self.rings.borrow();
            cur.len() == rings.len() && cur.iter().zip(&rings).all(|(a, b)| a.0.id == b.id)
        };
        if menu_changed { *self.entries.borrow_mut() = entries; }
        if !ids_same {
            // The set of readers changed: the items are made again, in order.
            let bar = NSStatusBar::systemStatusBar();
            for (_, item) in self.rings.borrow_mut().drain(..) { bar.removeStatusItem(&item); }
            let mut made = vec![];
            // Status items go in from the right: the first made is the rightmost.
            for r in rings.iter().rev() { made.push((r.clone(), bar.statusItemWithLength(-1.0))); }
            made.reverse();
            *self.rings.borrow_mut() = made;
        }
        if !ids_same || menu_changed || dark_changed {
            self.dark.set(Some(dark));
            self.plain.setVisible(rings.is_empty());
            // Each item has a menu of its own; an item's tag is its place in one table of
            // actions, which the menus built together share.
            self.target.ivars().acts.borrow_mut().clear();
            let entries = self.entries.borrow();
            self.plain.setMenu(Some(&self.menu(&entries, dark)));
            for (_, item) in self.rings.borrow().iter() { item.setMenu(Some(&self.menu(&entries, dark))); }
        }
        let mut cur = self.rings.borrow_mut();
        for ((old, item), new) in cur.iter_mut().zip(&rings) {
            if !ids_same || old != new || dark_changed || menu_changed { self.paint(item, new, dark); }
            *old = new.clone();
        }
    }

    /// Draw again if the menu bar changed its ink. Cheap when it didn't.
    pub fn refresh_appearance(&self) {
        let dark = self.is_dark();
        if self.dark.get() == Some(dark) { return; }
        let rings: Vec<Ring> = self.rings.borrow().iter().map(|r| r.0.clone()).collect();
        let entries = self.entries.borrow().clone();
        self.set(entries, rings);
    }

    fn paint(&self, item: &NSStatusItem, r: &Ring, dark: bool) {
        let Some(b) = item.button(self.mtm) else { return };
        if let Some(img) = bar::ring_png(&r.id, r.used, dark, 2.0).and_then(|p| image_of(&p, bar::SIZE as f64)) {
            img.setTemplate(false);
            b.setImage(Some(&img));
        }
        b.setImagePosition(NSCellImagePosition::ImageLeft);
        b.setTitle(&ns(&r.percent));
        b.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })));
        b.setToolTip(Some(&ns(&r.tip)));
    }

    fn icon(&self, icon: &Icon, dark: bool) -> Option<Retained<NSImage>> {
        match icon {
            Icon::Ring(id, used) => bar::ring_png(id, *used, dark, 2.0).and_then(|p| image_of(&p, bar::SIZE as f64)),
            Icon::Tile(id) => bar::tile_png(id, 16.0, 2.0).and_then(|p| image_of(&p, 16.0)),
            Icon::Symbol(name) => NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None),
        }
    }

    /// NSMenu for the entries.
    fn menu(&self, entries: &[Entry], dark: bool) -> Retained<NSMenu> {
        let mtm = self.mtm;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for e in entries {
            match e {
                Entry::Sep => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Entry::Header(h) => menu.addItem(&self.header(h)),
                Entry::Item(i) => {
                    menu.addItem(&self.item(i, dark));
                    // A reading's details are a quiet line under it.
                    if let Some(d) = &i.detail {
                        let line = self.plain_item(&format!("    {d}"));
                        menu.addItem(&line);
                    }
                }
            }
        }
        menu
    }

    fn header(&self, title: &str) -> Retained<NSMenuItem> {
        // Section headers are macOS 14's; before that a greyed line.
        if NSMenuItem::class().responds_to(sel!(sectionHeaderWithTitle:)) { return NSMenuItem::sectionHeaderWithTitle(&ns(title), self.mtm); }
        self.plain_item(title)
    }

    /// A line that only reads.
    fn plain_item(&self, title: &str) -> Retained<NSMenuItem> {
        let i = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(self.mtm), &ns(title), None, &ns("")) };
        i.setEnabled(false);
        i
    }

    fn item(&self, it: &Item, dark: bool) -> Retained<NSMenuItem> {
        let mtm = self.mtm;
        let action = it.act.as_ref().map(|_| sel!(clicked:));
        let key = it.key.map_or("", |k| k.0);
        let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(&it.title), action, &ns(key)) };
        if let Some((_, mods)) = it.key { item.setKeyEquivalentModifierMask(NSEventModifierFlags(mods)); }
        if let Some(act) = &it.act {
            let mut acts = self.target.ivars().acts.borrow_mut();
            item.setTag(acts.len() as isize);
            acts.push(act.clone());
            unsafe { item.setTarget(Some(&self.target as &AnyObject)) };
        }
        if let Some(on) = it.check { item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff }); }
        item.setEnabled(it.enabled);
        if let Some(t) = &it.tip { item.setToolTip(Some(&ns(t))); }
        if let Some(ic) = &it.icon { if let Some(img) = self.icon(ic, dark) { item.setImage(Some(&img)); } }
        if !it.sub.is_empty() {
            let sub = self.menu(&it.sub, dark);
            item.setSubmenu(Some(&sub));
        }
        item
    }
}
