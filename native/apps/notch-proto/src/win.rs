//! The Windows side of the notch (Interop/HostWindow.cs, Screens.cs, HotKeys.cs), plus
//! what the self-test needs to watch it from outside: screen capture, synthetic input,
//! and a helper window in another process standing in for "the app underneath".

use std::sync::atomic::{AtomicIsize, Ordering};

use hover_notch::Rect;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

pub use windows::Win32::Foundation::HWND as Hwnd;

/// How the empty part of the window lets clicks through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitMode {
    /// WS_EX_LAYERED | WS_EX_TRANSPARENT while the pointer is off the shape (the default).
    Layered,
    /// WS_EX_TRANSPARENT alone (for comparison in the self-test).
    Transparent,
}

pub fn hwnd_of(window: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    window.with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    })?
}

pub fn ex_style(h: HWND) -> isize {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) }
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

pub fn window_rect(h: HWND) -> Rect {
    let mut r = RECT::default();
    unsafe { let _ = GetWindowRect(h, &mut r); }
    Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
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

pub fn window_from_point(x: i32, y: i32) -> HWND {
    unsafe { WindowFromPoint(POINT { x, y }) }
}

// ---- messages the notch needs that winit doesn't pass on -----------------------------

pub const HOTKEY_ID: i32 = 1;

#[derive(Clone, Copy, Debug)]
pub enum Msg {
    Hotkey,
    /// WA_INACTIVE: another window took the foreground.
    Deactivated,
}

thread_local! {
    static QUEUE: std::cell::RefCell<Vec<Msg>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn take_messages() -> Vec<Msg> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

unsafe extern "system" fn subclass(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_HOTKEY if wp.0 as i32 == HOTKEY_ID => {
            QUEUE.with(|q| q.borrow_mut().push(Msg::Hotkey));
            WAKE.with(|w| if let Some(f) = &*w.borrow() { f() });
            return LRESULT(0);
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            QUEUE.with(|q| q.borrow_mut().push(Msg::Deactivated));
            WAKE.with(|w| if let Some(f) = &*w.borrow() { f() });
        }
        _ => {}
    }
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}

thread_local! {
    static WAKE: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

/// Alt+N (the default shortcut), and window activation, delivered to `wake`.
pub fn hook(h: HWND, wake: impl Fn() + 'static) -> bool {
    WAKE.with(|w| *w.borrow_mut() = Some(Box::new(wake)));
    unsafe {
        let _ = SetWindowSubclass(h, Some(subclass), 1, 0);
        RegisterHotKey(Some(h), HOTKEY_ID, MOD_ALT | MOD_NOREPEAT, 'N' as u32).is_ok()
    }
}

pub fn cpu_ms() -> f64 {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u); }
    let t = |f: windows::Win32::Foundation::FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 10_000.0;
    t(k) + t(u)
}

pub fn private_bytes() -> u64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
    unsafe {
        let _ = GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut _, std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32);
    }
    pmc.PrivateUsage as u64
}

// ---- what the self-test uses to look from outside ------------------------------------------

/// The composed desktop in a rectangle (device px), as RGB rows, through the screen DC
/// (DWM gives it with every layered and DirectComposition window drawn in).
pub fn capture(r: Rect) -> (u32, u32, Vec<u8>) {
    let (w, h) = (r.width().max(1), r.height().max(1));
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let bmp = CreateDIBSection(Some(mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY | CAPTUREBLT);
        let raw = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
        let rgb: Vec<u8> = raw.chunks(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (w as u32, h as u32, rgb)
    }
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32); }
}

pub fn move_to(x: i32, y: i32) {
    unsafe { let _ = SetCursorPos(x, y); }
}

pub fn click_at(x: i32, y: i32) {
    move_to(x, y);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let m = |f: MOUSE_EVENT_FLAGS| INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: f, ..Default::default() } } };
    send(&[m(MOUSEEVENTF_LEFTDOWN), m(MOUSEEVENTF_LEFTUP)]);
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } } }
}

pub fn alt_n() {
    send(&[key(VK_MENU, false), key(VIRTUAL_KEY('N' as u16), false), key(VIRTUAL_KEY('N' as u16), true), key(VK_MENU, true)]);
}

pub fn escape() {
    send(&[key(VK_ESCAPE, false), key(VK_ESCAPE, true)]);
}

pub fn type_text(s: &str) {
    let mut v = vec![];
    for u in s.encode_utf16() {
        for up in [false, true] {
            v.push(INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wScan: u, dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } } });
        }
    }
    send(&v);
}

// ---- the helper: "the app underneath", in its own process ------------------------------------

static CLICKS: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn helper_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            let n = CLICKS.fetch_add(1, Ordering::SeqCst) + 1;
            let (x, y) = ((lp.0 & 0xFFFF) as i16, ((lp.0 >> 16) & 0xFFFF) as i16);
            println!("click {n} {x} {y}");
            LRESULT(0)
        }
        WM_ACTIVATE => {
            println!("active {}", (wp.0 & 0xFFFF) != 0);
            unsafe { DefWindowProcW(h, msg, wp, lp) }
        }
        WM_ERASEBKGND => {
            let mut r = RECT::default();
            unsafe {
                let _ = GetClientRect(h, &mut r);
                let b = CreateSolidBrush(COLORREF(0x00FF00FF));
                FillRect(HDC(wp.0 as *mut _), &r, b);
                let _ = DeleteObject(b.into());
            }
            LRESULT(1)
        }
        WM_DESTROY => { unsafe { PostQuitMessage(0) }; LRESULT(0) }
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

/// A plain magenta window at r, activated, printing each click to stdout.
pub fn run_helper(r: Rect) {
    use std::io::Write;
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(helper_proc), hInstance: inst.into(), lpszClassName: w!("HoverNotchProtoHelper"), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc);
        let h = CreateWindowExW(WINDOW_EX_STYLE(0), w!("HoverNotchProtoHelper"), w!("Notch test: the app underneath"), WS_POPUP | WS_VISIBLE,
            r.left, r.top, r.width(), r.height(), None, None, Some(inst.into()), None).unwrap();
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        println!("hwnd {}", h.0 as isize);
        let _ = std::io::stdout().flush();
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
            let _ = std::io::stdout().flush();
        }
    }
    let _ = PCWSTR::null();
}
