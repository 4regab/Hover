//! The notch on X11 (HostWindow.cs and Screens.cs's counterparts): an override-redirect
//! window with an ARGB visual (winit makes it; the window manager leaves it alone, so
//! it never shows in a task bar or takes focus by itself), whose input shape is the
//! notch's shape, so everything else of it passes the pointer to what is underneath.
//! Plus the global shortcut, grabbed on the root window (HotKeys.cs's RegisterHotKey).

use crate::notch::Plat;
use hover_core::shortcut::{Modifiers, Shortcut};
use hover_notch::Rect;
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

pub struct X {
    pub conn: Arc<RustConnection>,
    pub root: u32,
    pub win: std::rc::Rc<Cell<u32>>,
    screen: (u16, u16),
    /// The scale winit settled on for the window (Xft.dpi), read each layout.
    pub scale: Box<dyn Fn() -> f64>,
    previous: Cell<u32>,
    input: Cell<(i32, i32, i32, i32)>,
}

pub fn connect() -> Option<(Arc<RustConnection>, u32, (u16, u16))> {
    let (c, n) = x11rb::connect(None).ok()?;
    let s = &c.setup().roots[n];
    let (root, size) = (s.root, (s.width_in_pixels, s.height_in_pixels));
    Some((Arc::new(c), root, size))
}

/// The X window behind a Slint window (winit's).
pub fn window_of(w: &slint::Window) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|ww| match ww.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }).flatten()
}

impl X {
    pub fn new(conn: Arc<RustConnection>, root: u32, screen: (u16, u16), scale: Box<dyn Fn() -> f64>) -> X {
        X { conn, root, win: std::rc::Rc::new(Cell::new(0)), screen, scale, previous: Cell::new(0), input: Cell::new((-1, -1, -1, -1)) }
    }

    /// The primary output's rectangle (RandR), else the whole screen.
    pub fn primary_bounds(&self) -> Rect {
        let c = &*self.conn;
        let whole = Rect { left: 0, top: 0, right: self.screen.0 as i32, bottom: self.screen.1 as i32 };
        let Ok(p) = c.randr_get_output_primary(self.root).and_then(|r| Ok(r.reply())) else { return whole };
        let Ok(p) = p else { return whole };
        let out = if p.output != 0 { Some(p.output) } else {
            // No primary set: the first connected output with a CRTC.
            c.randr_get_screen_resources_current(self.root).ok().and_then(|r| r.reply().ok()).and_then(|r| {
                r.outputs.iter().copied().find(|o| c.randr_get_output_info(*o, 0).ok().and_then(|x| x.reply().ok()).is_some_and(|i| i.crtc != 0))
            })
        };
        let Some(out) = out else { return whole };
        let Some(info) = c.randr_get_output_info(out, 0).ok().and_then(|r| r.reply().ok()) else { return whole };
        if info.crtc == 0 { return whole; }
        match c.randr_get_crtc_info(info.crtc, 0).ok().and_then(|r| r.reply().ok()) {
            Some(k) if k.width > 0 => Rect { left: k.x as i32, top: k.y as i32, right: k.x as i32 + k.width as i32, bottom: k.y as i32 + k.height as i32 },
            _ => whole,
        }
    }

    /// _NET_WORKAREA (the window manager's panels taken off), within the display.
    fn work_area(&self, bounds: Rect) -> Rect {
        let c = &*self.conn;
        let atom = |n: &str| c.intern_atom(false, n.as_bytes()).ok().and_then(|r| r.reply().ok()).map(|r| r.atom);
        let (Some(wa), Some(card)) = (atom("_NET_WORKAREA"), atom("CARDINAL")) else { return bounds };
        let Some(r) = c.get_property(false, self.root, wa, card, 0, 4).ok().and_then(|r| r.reply().ok()) else { return bounds };
        let v: Vec<u32> = r.value32().map(|i| i.collect()).unwrap_or_default();
        if v.len() < 4 { return bounds; }
        let w = Rect { left: v[0] as i32, top: v[1] as i32, right: (v[0] + v[2]) as i32, bottom: (v[1] + v[3]) as i32 };
        let i = Rect { left: w.left.max(bounds.left), top: w.top.max(bounds.top), right: w.right.min(bounds.right), bottom: w.bottom.min(bounds.bottom) };
        if i.width() > 0 && i.height() > 0 { i } else { bounds }
    }
}

impl Plat for X {
    fn primary(&self) -> (Rect, f64) { let b = self.primary_bounds(); (self.work_area(b), (self.scale)()) }

    fn signature(&self) -> String { let (r, s) = self.primary(); format!("{r:?}@{s}") }

    fn cursor(&self) -> (i32, i32) {
        self.conn.query_pointer(self.root).ok().and_then(|r| r.reply().ok()).map_or((-1, -1), |p| (p.root_x as i32, p.root_y as i32))
    }

    fn buttons(&self) -> bool {
        use xproto::KeyButMask as M;
        self.conn.query_pointer(self.root).ok().and_then(|r| r.reply().ok())
            .is_some_and(|p| p.mask.intersects(M::BUTTON1 | M::BUTTON2 | M::BUTTON3))
    }

    fn place(&self, r: Rect) {
        let w = self.win.get();
        if w == 0 { return; }
        let _ = self.conn.configure_window(w, &xproto::ConfigureWindowAux::new().x(r.left).y(r.top).width(r.width() as u32).height(r.height() as u32));
        let _ = self.conn.flush();
    }

    fn raise(&self) {
        let w = self.win.get();
        if w == 0 { return; }
        let _ = self.conn.configure_window(w, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE));
        let _ = self.conn.flush();
    }

    // The window manager never gives an override-redirect window the focus: only focus()
    // does, so there is no bit to take off.
    fn set_accepts_keys(&self, _on: bool) {}

    fn remember_foreground(&self) {
        if let Some(f) = self.conn.get_input_focus().ok().and_then(|r| r.reply().ok()) {
            if f.focus != self.win.get() { self.previous.set(f.focus); }
        }
    }

    fn restore_foreground(&self) {
        let (p, w) = (self.previous.get(), self.win.get());
        if p > 1 && self.foreground_is_ours() && w != 0 {
            let _ = self.conn.set_input_focus(xproto::InputFocus::PARENT, p, x11rb::CURRENT_TIME);
            let _ = self.conn.flush();
        }
    }

    fn focus(&self) {
        let w = self.win.get();
        if w == 0 { return; }
        let _ = self.conn.set_input_focus(xproto::InputFocus::PARENT, w, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
    }

    /// The input shape: only the notch's shape (and its shadow's reach) takes the
    /// pointer. Set only when it changes.
    fn set_hit(&self, _over: bool, s: (f64, f64, f64, f64), scale: f64) {
        let w = self.win.get();
        if w == 0 { return; }
        let r = ((s.0 * scale).floor() as i32, (s.1 * scale).floor() as i32, (s.2 * scale).ceil() as i32, (s.3 * scale).ceil() as i32);
        if r == self.input.get() { return; }
        self.input.set(r);
        let rects: Vec<xproto::Rectangle> = if r.2 > 0 && r.3 > 0 {
            vec![xproto::Rectangle { x: r.0 as i16, y: r.1 as i16, width: r.2 as u16, height: r.3 as u16 }]
        } else { vec![] };
        let _ = self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, w, 0, 0, &rects);
        let _ = self.conn.flush();
    }

    fn foreground_is_ours(&self) -> bool {
        let w = self.win.get();
        self.conn.get_input_focus().ok().and_then(|r| r.reply().ok()).is_some_and(|f| f.focus == w && w != 0)
    }
}

impl X {
    /// The window's input shape as the server holds it (for the self-test).
    pub fn input_rects(&self) -> Vec<(i16, i16, u16, u16)> {
        let w = self.win.get();
        self.conn.shape_get_rectangles(w, shape::SK::INPUT).ok().and_then(|r| r.reply().ok())
            .map(|r| r.rectangles.iter().map(|x| (x.x, x.y, x.width, x.height)).collect()).unwrap_or_default()
    }

    /// The screen's pixels in a rectangle, as RGB (the self-test's screenshots).
    pub fn capture(&self, r: Rect) -> Option<Vec<u8>> {
        let img = self.conn.get_image(xproto::ImageFormat::Z_PIXMAP, self.root, r.left as i16, r.top as i16, r.width() as u16, r.height() as u16, !0).ok()?.reply().ok()?;
        let mut out = Vec::with_capacity((r.width() * r.height() * 3) as usize);
        for px in img.data.chunks(4) { out.extend([px[2], px[1], px[0]]); }
        Some(out)
    }
}

// MARK: The global shortcut

/// XGrabKey on the root window, for the chord with and without Caps Lock and Num Lock
/// (which X counts as modifiers). Another client holding the chord makes the grab fail
/// with BadAccess: the counterpart of RegisterHotKey refusing.
pub struct Grab { conn: Arc<RustConnection>, root: u32, held: Mutex<Vec<(u8, u16)>> }

const LOCKS: [u16; 4] = [0, 0x2 /* Lock */, 0x10 /* Mod2: Num Lock */, 0x12];

impl Grab {
    pub fn new(conn: Arc<RustConnection>, root: u32) -> Arc<Grab> { Arc::new(Grab { conn, root, held: Mutex::new(vec![]) }) }

    fn keycode(&self, keysym: u32) -> Option<u8> {
        let s = self.conn.setup();
        let (min, max) = (s.min_keycode, s.max_keycode);
        let m = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per = m.keysyms_per_keycode as usize;
        m.keysyms.chunks(per).position(|ks| ks.contains(&keysym)).map(|i| min + i as u8)
    }

    pub fn clear(&self) {
        for (code, mods) in self.held.lock().unwrap().drain(..) {
            for l in LOCKS { let _ = self.conn.ungrab_key(code, self.root, xproto::ModMask::from(mods | l)); }
        }
        let _ = self.conn.flush();
    }

    /// False when the chord can't be had: no such key here, or someone else has it.
    pub fn register(&self, sc: &Shortcut) -> bool {
        if !sc.is_set() { return true; }
        let Some(code) = hover_app::keys::keysym(sc.key).and_then(|k| self.keycode(k)) else {
            hover_core::log::line(&format!("hotkey {} has no key on this keyboard", sc.label()));
            return false;
        };
        let mut mods = 0u16;
        if sc.modifiers.has(Modifiers::SHIFT) { mods |= 0x1; }
        if sc.modifiers.has(Modifiers::CONTROL) { mods |= 0x4; }
        if sc.modifiers.has(Modifiers::ALT) { mods |= 0x8; }
        if sc.modifiers.has(Modifiers::WINDOWS) { mods |= 0x40; }
        let mut ok = true;
        for l in LOCKS {
            let r = self.conn.grab_key(true, self.root, xproto::ModMask::from(mods | l), code, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC)
                .map_err(|e| e.to_string()).and_then(|c| c.check().map_err(|e| format!("{e:?}")));
            if let Err(e) = r { hover_core::log::line(&format!("hotkey {} could not be registered ({e})", sc.label())); ok = false; }
        }
        self.held.lock().unwrap().push((code, mods));
        if !ok { self.clear(); }
        ok
    }

    /// Calls `pressed` on the event thread for each press of a grabbed chord.
    pub fn listen(self: &Arc<Grab>, pressed: impl Fn() + Send + 'static) {
        let me = self.clone();
        std::thread::Builder::new().name("hotkey".into()).spawn(move || loop {
            match me.conn.wait_for_event() {
                Ok(x11rb::protocol::Event::KeyPress(e)) => {
                    let held = me.held.lock().unwrap().clone();
                    let state = u16::from(e.state) & !0x12;
                    if held.iter().any(|(c, m)| *c == e.detail && *m == state) { pressed(); }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }).expect("a thread for the shortcut");
    }
}

// MARK: Pickers

/// The folder or theme-file picker: the desktop portal's FileChooser would need a
/// window handle exported to it; zenity (GNOME) and kdialog (KDE) are what desktops
/// ship, so those, in that order. None when neither is there or nothing was picked.
pub fn pick(folder: bool) -> Option<String> {
    let home = hover_core::platform::home().unwrap_or_default().to_string_lossy().into_owned();
    let tries: Vec<(&str, Vec<String>)> = if folder {
        vec![("zenity", vec!["--file-selection".into(), "--directory".into(), "--title=Choose the agents' folder".into()]),
             ("kdialog", vec!["--getexistingdirectory".into(), home])]
    } else {
        vec![("zenity", vec!["--file-selection".into(), "--title=Import a VS Code theme file".into(), "--file-filter=VS Code colour theme (*.json) | *.json".into(), "--file-filter=All files | *".into()]),
             ("kdialog", vec!["--getopenfilename".into(), home, "VS Code colour theme (*.json)".into()])]
    };
    for (exe, args) in tries {
        match std::process::Command::new(exe).args(&args).output() {
            Ok(o) if o.status.success() => { let p = String::from_utf8_lossy(&o.stdout).trim().to_owned(); return (!p.is_empty()).then_some(p); }
            // Cancelled: don't ask the next one.
            Ok(_) => return None,
            Err(_) => continue,
        }
    }
    hover_core::log::line("no file picker (zenity or kdialog) on this desktop");
    None
}
