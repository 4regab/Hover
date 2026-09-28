//! Windows: the notch window (Interop/HostWindow.cs, Screens.cs), the shortcut
//! (HotKeys.cs), the tray icon (Services/TrayIcon.cs), the resume and unlock that make
//! the next opening say hello, and the file pickers. notch-proto's code, grown.

use std::sync::atomic::{AtomicIsize, Ordering};

use hover_notch::Rect;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;


/// How the empty part of the window lets clicks through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitMode {
    /// WS_EX_LAYERED | WS_EX_TRANSPARENT while the pointer is off the shape (notch-proto
    /// also tried WS_EX_TRANSPARENT alone; Windows gates both, RUN-ON-WINDOWS).
    Layered,
}

pub fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    window.with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    })?
}

/// WS_EX_TOOLWINDOW always; WS_EX_NOACTIVATE unless the office takes keys; the
/// click-through bits while the pointer is off the shape.
pub fn apply_styles(h: HWND, accepts_keys: bool, click_through: bool, mode: HitMode) {
    unsafe {
        let mut ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let before = ex;
        ex |= WS_EX_TOOLWINDOW.0;
        if accepts_keys { ex &= !WS_EX_NOACTIVATE.0 } else { ex |= WS_EX_NOACTIVATE.0 }
        if mode == HitMode::Layered { ex |= WS_EX_LAYERED.0 }
        if click_through { ex |= WS_EX_TRANSPARENT.0 } else { ex &= !WS_EX_TRANSPARENT.0 }
        if ex != before {
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize);
            if mode == HitMode::Layered && before & WS_EX_LAYERED.0 == 0 {
                // A layered window with no attributes is never drawn; fully opaque keeps the
                // DirectComposition content (and its per-pixel alpha) as it is.
                let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
            }
        }
    }
}

pub fn disable_transitions(h: HWND) {
    let on: BOOL = true.into();
    unsafe {
        let _ = DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED, &on as *const _ as _, std::mem::size_of::<BOOL>() as u32);
    }
}

pub fn place(h: HWND, r: Rect) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), r.left, r.top, r.width(), r.height(), SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn raise(h: HWND) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

#[derive(Clone, Debug)]
pub struct Monitor {
    pub device: String,
    pub primary: bool,
    pub bounds: Rect,
    pub work: Rect,
    pub scale: f64,
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(m, &mut info.monitorInfo as *mut _) }.as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let scale = if unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_ok() { dx as f64 / 96.0 } else { 1.0 };
            let r = |r: RECT| Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
            let n = info.szDevice.iter().position(|c| *c == 0).unwrap_or(32);
            list.push(Monitor {
                device: String::from_utf16_lossy(&info.szDevice[..n]),
                primary: info.monitorInfo.dwFlags & 1 != 0,
                bounds: r(info.monitorInfo.rcMonitor),
                work: r(info.monitorInfo.rcWork),
                scale,
            });
        }
        true.into()
    }
    let mut list: Vec<Monitor> = vec![];
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize)); }
    list
}

pub fn primary() -> Monitor {
    let all = monitors();
    all.iter().find(|m| m.primary).or(all.first()).cloned().unwrap_or(Monitor {
        device: "?".into(), primary: true, bounds: Rect { left: 0, top: 0, right: 1920, bottom: 1080 }, work: Rect { left: 0, top: 0, right: 1920, bottom: 1040 }, scale: 1.0,
    })
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe { let _ = GetCursorPos(&mut p); }
    (p.x, p.y)
}

pub fn buttons_down() -> bool {
    unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0) }
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn set_foreground(h: HWND) -> bool {
    unsafe { SetForegroundWindow(h).as_bool() }
}

pub fn is_window(h: HWND) -> bool {
    unsafe { IsWindow(Some(h)).as_bool() }
}

/// Our window, or one it owns (a menu, a dialog).
pub fn is_ours(h: HWND, ours: HWND) -> bool {
    h == ours || unsafe { GetAncestor(h, GA_ROOTOWNER) } == ours
}


// MARK: Messages winit doesn't pass on

pub const HOTKEY_ID: i32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;

#[derive(Clone, Copy, Debug)]
pub enum Msg {
    Hotkey,
    /// WA_INACTIVE: another window took the foreground.
    Deactivated,
    /// A resume or an unlock: the next opening says hello (NotchManager.OnPower, OnSession).
    Greet,
    TrayLeft,
    TrayMenu(usize),
}

thread_local! {
    static QUEUE: std::cell::RefCell<Vec<Msg>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn take_messages() -> Vec<Msg> { QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut())) }

fn push(m: Msg) { QUEUE.with(|q| q.borrow_mut().push(m)); }

static NOTCH: AtomicIsize = AtomicIsize::new(0);

pub fn set_notch(h: HWND) { NOTCH.store(h.0 as isize, Ordering::SeqCst); }
fn notch() -> Option<HWND> { let v = NOTCH.load(Ordering::SeqCst); (v != 0).then_some(HWND(v as *mut _)) }

unsafe extern "system" fn subclass(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_HOTKEY if wp.0 as i32 == HOTKEY_ID => { push(Msg::Hotkey); return LRESULT(0); }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => push(Msg::Deactivated),
        WM_POWERBROADCAST if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC => push(Msg::Greet),
        WM_WTSSESSION_CHANGE if wp.0 as u32 == WTS_SESSION_UNLOCK => push(Msg::Greet),
        WM_TRAY => match (lp.0 as u32) & 0xFFFF {
            WM_LBUTTONUP => push(Msg::TrayLeft),
            WM_RBUTTONUP | WM_CONTEXTMENU => { if let Some(i) = unsafe { tray_menu(h) } { push(Msg::TrayMenu(i)); } }
            _ => {}
        },
        _ => {}
    }
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}

/// The notch window's messages: the shortcut, activation, resume and unlock, the tray.
pub fn hook(h: HWND, _wake: impl Fn() + 'static) {
    use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
    unsafe {
        let _ = SetWindowSubclass(h, Some(subclass), 1, 0);
        let _ = WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION);
    }
}

// MARK: The shortcut

/// HotKeys.Register: MOD_NOREPEAT and the chord's modifiers; false when Windows
/// refuses (reserved, or another app has it), logged with the error as the C# does.
pub fn register_hotkey(sc: &hover_core::shortcut::Shortcut) -> bool {
    use hover_core::shortcut::Modifiers;
    let Some(h) = notch() else { return false };
    if !sc.is_set() { return true; }
    let Some(vk) = hover_app::keys::vk(sc.key) else {
        hover_core::log::line(&format!("hotkey {} has no Windows virtual-key mapping", sc.label()));
        return false;
    };
    let mut mods = MOD_NOREPEAT;
    if sc.modifiers.has(Modifiers::CONTROL) { mods |= MOD_CONTROL; }
    if sc.modifiers.has(Modifiers::ALT) { mods |= MOD_ALT; }
    if sc.modifiers.has(Modifiers::SHIFT) { mods |= MOD_SHIFT; }
    if sc.modifiers.has(Modifiers::WINDOWS) { mods |= MOD_WIN; }
    match unsafe { RegisterHotKey(Some(h), HOTKEY_ID, mods, vk as u32) } {
        Ok(()) => true,
        Err(e) => { hover_core::log::line(&format!("hotkey {} could not be registered (Win32 error {})", sc.label(), e.code().0 & 0xFFFF)); false }
    }
}

pub fn clear_hotkeys() {
    if let Some(h) = notch() { unsafe { let _ = UnregisterHotKey(Some(h), HOTKEY_ID); } }
}

// MARK: The tray icon

fn wide(s: &str, n: usize) -> Vec<u16> { let mut v: Vec<u16> = s.encode_utf16().take(n - 1).collect(); v.resize(n, 0); v }

fn tray_data(h: HWND) -> windows::Win32::UI::Shell::NOTIFYICONDATAW {
    let mut d = windows::Win32::UI::Shell::NOTIFYICONDATAW { cbSize: std::mem::size_of::<windows::Win32::UI::Shell::NOTIFYICONDATAW>() as u32, hWnd: h, uID: TRAY_ID, ..Default::default() };
    d.uCallbackMessage = WM_TRAY;
    d
}

/// The app's icon at the size the tray draws it (hover.ico has a frame for each).
fn app_icon() -> HICON {
    unsafe {
        let bytes: &[u8] = include_bytes!("../assets/hover.ico");
        let size = GetSystemMetrics(SM_CXSMICON);
        let off = LookupIconIdFromDirectoryEx(bytes.as_ptr(), true, size, size, LR_DEFAULTCOLOR);
        if off <= 0 { return LoadIconW(None, IDI_APPLICATION).unwrap_or_default(); }
        // The directory entry's offset and length, then the image itself.
        let dir = &bytes[6..];
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        for i in 0..count {
            let e = &dir[i * 16..i * 16 + 16];
            let (len, at) = (u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize, u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize);
            if at as i32 == off {
                return CreateIconFromResourceEx(&bytes[at..at + len], true, 0x0003_0000, size, size, LR_DEFAULTCOLOR).unwrap_or_default();
            }
        }
        LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
    }
}

/// Shell_NotifyIcon: the icon, "Hover" as its tip, messages to the notch window.
pub fn tray_start() {
    use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD};
    let Some(h) = notch() else { return };
    let mut d = tray_data(h);
    d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    d.hIcon = app_icon();
    d.szTip.copy_from_slice(&wide("Hover", 128));
    unsafe { let _ = Shell_NotifyIconW(NIM_ADD, &d); }
}

pub fn tray_stop() {
    use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIM_DELETE};
    if let Some(h) = notch() { unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(h)); } }
}

/// TrayIcon.Notify: a balloon of six seconds.
pub fn tray_notify(title: &str, text: &str) {
    use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_INFO, NIIF_NONE, NIM_MODIFY};
    let Some(h) = notch() else { return };
    let mut d = tray_data(h);
    d.uFlags = NIF_INFO;
    d.szInfoTitle.copy_from_slice(&wide(title, 64));
    d.szInfo.copy_from_slice(&wide(text, 256));
    d.dwInfoFlags = NIIF_NONE;
    d.Anonymous.uTimeout = 6000;
    unsafe { let _ = Shell_NotifyIconW(NIM_MODIFY, &d); }
}

thread_local! {
    static MENU: std::cell::RefCell<hover_app::rest::Menu> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn set_tray_menu(m: hover_app::rest::Menu) { MENU.with(|x| *x.borrow_mut() = m); }

/// The menu at the pointer, as the C# opened its ContextMenu there; the item picked.
unsafe fn tray_menu(h: HWND) -> Option<usize> {
    let menu = unsafe { CreatePopupMenu() }.ok()?;
    MENU.with(|m| {
        for (i, item) in m.borrow().iter().enumerate() {
            match item {
                None => unsafe { let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()); },
                Some((label, check)) => {
                    let w: Vec<u16> = label.replace('\u{2026}', "...").encode_utf16().chain(Some(0)).collect();
                    let flags = MF_STRING | if *check == Some(true) { MF_CHECKED } else { MF_UNCHECKED };
                    unsafe { let _ = AppendMenuW(menu, flags, i + 1, PCWSTR(w.as_ptr())); }
                }
            }
        }
    });
    let mut p = POINT::default();
    let picked = unsafe {
        let _ = GetCursorPos(&mut p);
        // The menu closes when clicked away only if the window is in the foreground.
        let _ = SetForegroundWindow(h);
        let r = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), h, None);
        let _ = DestroyMenu(menu);
        r.0
    };
    (picked > 0).then(|| picked as usize - 1)
}

// MARK: Plat

#[derive(Default)]
pub struct Plat { previous: std::cell::Cell<isize>, through: std::cell::Cell<bool>, keys: std::cell::Cell<bool> }

impl crate::notch::Plat for Plat {
    fn primary(&self) -> (Rect, f64) { let m = primary(); (m.work, m.scale) }
    fn signature(&self) -> String { monitors().iter().map(|m| format!("{}{}{:?}{:?}{}", m.device, m.primary, m.bounds, m.work, m.scale)).collect() }
    fn cursor(&self) -> (i32, i32) { cursor() }
    fn buttons(&self) -> bool { buttons_down() }
    fn place(&self, r: Rect) { if let Some(h) = notch() { place(h, r); } }
    fn raise(&self) { if let Some(h) = notch() { raise(h); } }
    fn set_accepts_keys(&self, on: bool) {
        self.keys.set(on);
        if let Some(h) = notch() { apply_styles(h, on, self.through.get(), HitMode::Layered); }
    }
    fn remember_foreground(&self) { self.previous.set(foreground().0 as isize); }
    fn restore_foreground(&self) {
        let (Some(h), p) = (notch(), HWND(self.previous.get() as *mut _)) else { return };
        if foreground() == h && !p.is_invalid() && is_window(p) { set_foreground(p); }
    }
    fn focus(&self) { if let Some(h) = notch() { set_foreground(h); } }
    /// The window takes the pointer only over the shape: WS_EX_TRANSPARENT otherwise.
    fn set_hit(&self, over: bool, _s: (f64, f64, f64, f64), _scale: f64) {
        let through = !over;
        if through == self.through.get() { return; }
        self.through.set(through);
        if let Some(h) = notch() { apply_styles(h, self.keys.get(), through, HitMode::Layered); }
    }
    fn foreground_is_ours(&self) -> bool { notch().is_some_and(|h| is_ours(foreground(), h)) }
}

// MARK: Pickers

/// The folder picker (KiroPage.ChooseFolder) and the theme file dialog
/// (Microsoft.Win32.OpenFileDialog), both the system's IFileOpenDialog.
pub fn pick(folder: bool) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = d.GetOptions().ok()?;
        if folder {
            let _ = d.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
        } else {
            let specs = [COMDLG_FILTERSPEC { pszName: w!("VS Code colour theme (*.json)"), pszSpec: w!("*.json") }, COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") }];
            let _ = d.SetFileTypes(&specs);
        }
        d.Show(notch()).ok()?;
        let item = d.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}
