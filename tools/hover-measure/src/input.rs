//! Real input, as a user gives it: the system's pointer and keyboard (SendInput on
//! Windows, xdotool on Linux), and on Windows the UI Automation tree Slint publishes,
//! to find a control by its accessible name and click the middle of it.

#[derive(Clone, Debug)]
pub struct Found { pub name: String, pub role: i32, pub rect: (i32, i32, i32, i32) }

pub use imp::*;

/// "ctrl+shift+a" into its keys.
pub fn chord(s: &str) -> Vec<String> { s.split('+').map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect() }

#[cfg(windows)]
mod imp {
    use super::Found;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, TreeScope_Children, TreeScope_Descendants};
    use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXSCREEN};

    /// Physical pixels everywhere, as Hover's own numbers are.
    pub fn init() { unsafe { let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); } }

    /// The primary display's width (the notch sits at its top centre).
    pub fn primary_width() -> i32 { unsafe { GetSystemMetrics(SM_CXSCREEN) } }

    fn send(inputs: &[INPUT]) { unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32); } }

    fn mouse(flags: MOUSE_EVENT_FLAGS, x: i32, y: i32, data: i32) -> INPUT {
        INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: x, dy: y, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    fn abs(x: i32, y: i32) -> (i32, i32) {
        unsafe {
            let (vx, vy, vw, vh) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            (((x - vx) * 65535) / (vw - 1).max(1), ((y - vy) * 65535) / (vh - 1).max(1))
        }
    }

    pub fn move_to(x: i32, y: i32) {
        let (ax, ay) = abs(x, y);
        send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, ax, ay, 0)]);
    }

    pub fn click(x: i32, y: i32) {
        move_to(x, y);
        std::thread::sleep(std::time::Duration::from_millis(60));
        send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0)]);
        std::thread::sleep(std::time::Duration::from_millis(40));
        send(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0)]);
    }

    pub fn wheel(x: i32, y: i32, notches: i32) {
        move_to(x, y);
        std::thread::sleep(std::time::Duration::from_millis(30));
        send(&[mouse(MOUSEEVENTF_WHEEL, 0, 0, notches * 120)]);
    }

    fn vk(k: &str) -> Option<VIRTUAL_KEY> {
        Some(match k {
            "ctrl" | "control" => VK_CONTROL, "alt" => VK_MENU, "shift" => VK_SHIFT, "win" | "super" => VK_LWIN,
            "enter" | "return" => VK_RETURN, "esc" | "escape" => VK_ESCAPE, "tab" => VK_TAB, "space" => VK_SPACE,
            "backspace" => VK_BACK, "delete" => VK_DELETE, "up" => VK_UP, "down" => VK_DOWN, "left" => VK_LEFT, "right" => VK_RIGHT,
            "home" => VK_HOME, "end" => VK_END, "pageup" => VK_PRIOR, "pagedown" => VK_NEXT,
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => VIRTUAL_KEY(k.to_ascii_uppercase().as_bytes()[0] as u16),
            k if k.starts_with('f') && k[1..].parse::<u16>().is_ok() => VIRTUAL_KEY(VK_F1.0 + k[1..].parse::<u16>().unwrap() - 1),
            _ => return None,
        })
    }

    fn key(v: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: v, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } } }
    }

    /// A chord: every key down in order, then up in reverse.
    pub fn keys(keys: &[String]) -> Result<(), String> {
        let vs: Vec<VIRTUAL_KEY> = keys.iter().map(|k| vk(k).ok_or_else(|| format!("unknown key {k}"))).collect::<Result<_, _>>()?;
        for v in &vs { send(&[key(*v, false)]); std::thread::sleep(std::time::Duration::from_millis(15)); }
        for v in vs.iter().rev() { send(&[key(*v, true)]); std::thread::sleep(std::time::Duration::from_millis(15)); }
        Ok(())
    }

    /// Text as typed characters (KEYEVENTF_UNICODE: no layout involved).
    pub fn type_text(t: &str) {
        for u in t.encode_utf16() {
            let k = |up: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } } };
            send(&[k(false), k(true)]);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    /// The screen's pixels in a rectangle (physical pixels), as the user sees them.
    pub fn capture(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>, String> {
        use windows::Win32::Graphics::Gdi::*;
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
            let mut info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut px = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            if !ok || lines == 0 { return Err("couldn't read the screen".into()); }
            // BGRA to RGBA.
            for p in px.chunks_mut(4) { p.swap(0, 2); p[3] = 255; }
            Ok(px)
        }
    }

    /// Every named element in the process's windows, as UI Automation sees them.
    pub fn elements(pid: u32) -> Result<Vec<Found>, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let ua: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
            let all = ua.CreateTrueCondition().map_err(|e| e.to_string())?;
            let root = ua.GetRootElement().map_err(|e| e.to_string())?;
            let tops = root.FindAll(TreeScope_Children, &all).map_err(|e| e.to_string())?;
            let mut out = vec![];
            for i in 0..tops.Length().unwrap_or(0) {
                let Ok(w) = tops.GetElement(i) else { continue };
                if w.CurrentProcessId().unwrap_or(0) as u32 != pid { continue; }
                let Ok(list) = w.FindAll(TreeScope_Descendants, &all) else { continue };
                for j in 0..list.Length().unwrap_or(0) {
                    let Ok(e) = list.GetElement(j) else { continue };
                    let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
                    if name.is_empty() { continue; }
                    let r: RECT = e.CurrentBoundingRectangle().unwrap_or_default();
                    if r.right <= r.left || r.bottom <= r.top { continue; }
                    if e.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false) { continue; }
                    out.push(Found { name, role: e.CurrentControlType().map(|c| c.0).unwrap_or(0), rect: (r.left, r.top, r.right, r.bottom) });
                }
            }
            Ok(out)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Found;
    pub fn init() {}
    pub fn primary_width() -> i32 {
        std::process::Command::new("xdotool").arg("getdisplaygeometry").output().ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().and_then(|w| w.parse().ok())).unwrap_or(1920)
    }
    fn xdo(args: &[&str]) { let _ = std::process::Command::new("xdotool").args(args).status(); }
    pub fn move_to(x: i32, y: i32) { xdo(&["mousemove", &x.to_string(), &y.to_string()]); }
    pub fn click(x: i32, y: i32) { move_to(x, y); xdo(&["click", "1"]); }
    pub fn wheel(x: i32, y: i32, notches: i32) {
        move_to(x, y);
        let b = if notches > 0 { "4" } else { "5" };
        for _ in 0..notches.abs() { xdo(&["click", b]); }
    }
    pub fn keys(keys: &[String]) -> Result<(), String> { xdo(&["key", &keys.join("+")]); Ok(()) }
    pub fn type_text(t: &str) { xdo(&["type", "--delay", "8", t]); }
    pub fn elements(_pid: u32) -> Result<Vec<Found>, String> { Err("the accessibility tree is read on Windows only; use the self-test on Linux".into()) }
    pub fn capture(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>, String> {
        // ImageMagick's import, when it is there (the self-test takes its own on X11).
        let tmp = std::env::temp_dir().join(format!("hover-shot-{}.rgba", std::process::id()));
        let st = std::process::Command::new("import").args(["-window", "root", "-crop", &format!("{w}x{h}+{x}+{y}"), "-depth", "8", &format!("rgba:{}", tmp.display())]).status().map_err(|e| e.to_string())?;
        if !st.success() { return Err("import failed".into()); }
        std::fs::read(&tmp).map_err(|e| e.to_string())
    }
}
