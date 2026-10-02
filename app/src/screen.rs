//! The desk's Screen panel (Owl/ScreenFeed.cs on Windows, Screen.swift on a Mac): the
//! main display as the agent has it, never the user's own work. It is the desktop (the
//! wallpaper, and on a Mac the desktop's icons) with only the windows of the apps the
//! agent's computer use opened or acted on over it (their process ids, from its steps),
//! so the user's open apps never show. The desk asks for a frame while the panel shows,
//! four a second while the agent tests, and a still at rest.
//!
//! - macOS: the windows the window server lists (CGWindowListCopyWindowInfo), the
//!   desktop's own and the agent's apps' on the main display, drawn together by
//!   CGWindowListCreateImageFromArray; without Screen Recording the picture is the
//!   desktop's wallpaper (`access`, `request_access`). Screen.swift used ScreenCaptureKit,
//!   which is block-and-async objects on top of this; the window list does the same filter.
//! - Windows: each window of those processes through PrintWindow, over the wallpaper.
//! - Linux (X11): the `_NET_WM_PID` windows of those processes, over a plain desktop.
//!   Not on Wayland.
//!
//! The choosing (`shown`), the fitting (`fit`) and the encoding are plain and tested on
//! every OS; the capture is each OS's.

use image::{imageops, Rgb, RgbImage, RgbaImage};

/// The width the panel is sent at (Screen.swift's `width`).
pub const WIDTH: u32 = 1280;

/// The apps whose windows may show: by process id, by bundle id (macOS), by name. What a
/// session's computer-use steps add up to (`hover_agents::desk::AgentApps`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Apps {
    pub pids: Vec<u32>,
    pub bundles: Vec<String>,
    pub names: Vec<String>,
}

impl Apps {
    pub fn from_pids(pids: &[u32]) -> Apps { Apps { pids: pids.to_vec(), ..Default::default() } }
    pub fn none(&self) -> bool { self.pids.is_empty() && self.bundles.is_empty() && self.names.is_empty() }
}

/// One window on screen: its owner, its layer (desktop pictures and icons are below 0 on a
/// Mac) and its rectangle, y down from the top of the main display.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub id: u64,
    pub pid: u32,
    pub layer: i64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Win {
    fn touches(&self, d: (f64, f64, f64, f64)) -> bool { self.x < d.0 + d.2 && self.x + self.w > d.0 && self.y < d.1 + d.3 && self.y + self.h > d.1 }
}

/// Screen.shown: the windows the panel may show. The desktop's (layer at or below
/// `desktop`) and the ordinary windows of the agent's apps, on the display `(x, y, w, h)`;
/// never Hover's own (`me`) nor anyone else's.
pub fn shown(all: &[Win], pids: &[u32], display: (f64, f64, f64, f64), desktop: i64, me: u32) -> Vec<Win> {
    all.iter().filter(|w| w.touches(display) && (w.layer <= desktop || (w.pid != me && pids.contains(&w.pid)))).cloned().collect()
}

/// `size` scaled down to fit `max`, keeping its shape; never scaled up, never empty.
pub fn fit(size: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let (w, h) = (size.0.max(1), size.1.max(1));
    let k = (max.0.max(1) as f64 / w as f64).min(max.1.max(1) as f64 / h as f64).min(1.0);
    (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1))
}

/// `img` fitted into `max`.
pub fn scaled(img: RgbaImage, max: (u32, u32)) -> RgbaImage {
    let (w, h) = fit(img.dimensions(), max);
    if (w, h) == img.dimensions() { img } else { imageops::resize(&img, w, h, imageops::FilterType::Triangle) }
}

/// The frame as JPEG bytes (the panel takes a data URL of it): opaque, at `quality` 1 to 100.
pub fn jpeg(img: &RgbaImage, quality: u8) -> Vec<u8> {
    let rgb: RgbImage = RgbImage::from_fn(img.width(), img.height(), |x, y| { let p = img.get_pixel(x, y); Rgb([p[0], p[1], p[2]]) });
    let mut out = vec![];
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    let _ = image::ImageEncoder::write_image(enc, rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8);
    out
}

/// The same as the data URL the panel's image takes.
pub fn data_url(img: &RgbaImage, quality: u8) -> String { format!("data:image/jpeg;base64,{}", hover_agents::http::base64(&jpeg(img, quality))) }

/// A plain desktop of one colour, for where there is no wallpaper to read.
pub fn plain(size: (u32, u32), rgb: [u8; 3]) -> RgbaImage { RgbaImage::from_pixel(size.0.max(1), size.1.max(1), image::Rgba([rgb[0], rgb[1], rgb[2], 255])) }

/// A picture drawn to fill the screen, as the desktop does: covering it, centred.
pub fn cover(src: &RgbaImage, size: (u32, u32)) -> RgbaImage {
    let (sw, sh) = (src.width().max(1) as f64, src.height().max(1) as f64);
    let k = (size.0 as f64 / sw).max(size.1 as f64 / sh);
    let (w, h) = (((sw * k).ceil() as u32).max(size.0), ((sh * k).ceil() as u32).max(size.1));
    let big = imageops::resize(src, w, h, imageops::FilterType::Triangle);
    imageops::crop_imm(&big, (w - size.0) / 2, (h - size.1) / 2, size.0, size.1).to_image()
}

/// Puts `win` at (x, y) on `canvas`, opaque, clipped by the canvas.
pub fn paste(canvas: &mut RgbaImage, win: &RgbaImage, x: i64, y: i64) {
    let mut opaque = win.clone();
    for p in opaque.pixels_mut() { p[3] = 255; }
    imageops::overlay(canvas, &opaque, x, y);
}

/// Whether this OS can show the panel at all.
pub fn supported() -> bool { imp::supported() }

/// Why it can't, for the panel to say; none where it can.
pub fn note() -> Option<&'static str> { (!supported()).then_some("The screen panel needs Windows, macOS or a Linux desktop on X11.") }

/// Whether Hover may read other apps' windows (macOS: System Settings → Privacy & Security
/// → Screen Recording); true where there is no such grant.
pub fn access() -> bool { imp::access() }

/// Asks for it once (macOS); after that the system only says no, so its page in System
/// Settings opens instead. A grant takes effect after Hover is quit and opened again.
pub fn request_access() { imp::request_access() }

/// A frame of the main display with only the windows of `pids`' processes over the desktop,
/// scaled to fit `max`. Never any other window. Blocks for a moment: call it off the UI
/// thread. Err says why there is no frame (no access, no windows, no display); the caller
/// keeps the last one.
pub fn capture(pids: &[u32], max: (u32, u32)) -> Result<RgbaImage, String> { capture_apps(&Apps::from_pids(pids), max) }

/// `capture`, for apps named by process id, bundle id or name (a Mac resolves the last two).
pub fn capture_apps(apps: &Apps, max: (u32, u32)) -> Result<RgbaImage, String> { imp::capture(apps, max).map(|i| scaled(i, max)) }

/// The desktop alone (the panel at rest): the wallpaper, scaled to fit `max`.
pub fn desktop(max: (u32, u32)) -> Result<RgbaImage, String> { imp::desktop().map(|i| scaled(i, max)) }

// MARK: macOS

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNumber, NSString};
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect { x: f64, y: f64, w: f64, h: f64 }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut c_void;
        fn CGWindowLevelForKey(key: i32) -> i32;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageRelease(image: *const c_void);
        fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
        fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row: usize, space: *mut c_void, info: u32) -> *mut c_void;
        fn CGContextDrawImage(ctx: *mut c_void, rect: CGRect, image: *const c_void);
        fn CGContextRelease(ctx: *mut c_void);
    }

    /// kCGWindowListOptionOnScreenOnly.
    const ON_SCREEN: u32 = 1;
    /// kCGDesktopIconWindowLevelKey: windows at or below it are the desktop's.
    const DESKTOP_ICON_KEY: i32 = 18;
    /// kCGWindowImageNominalResolution: one pixel to the point.
    const NOMINAL: u32 = 1 << 4;
    /// kCGImageAlphaPremultipliedLast: RGBA bytes.
    const RGBA: u32 = 1;

    pub fn supported() -> bool { true }
    pub fn access() -> bool { unsafe { CGPreflightScreenCaptureAccess() } }

    pub fn request_access() {
        if unsafe { CGRequestScreenCaptureAccess() } { return; }
        let _ = std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture").spawn();
    }

    fn number(d: &NSDictionary<NSString, AnyObject>, key: &NSString) -> Option<f64> {
        d.objectForKey(key).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    }

    /// The on-screen windows, front to back, as CGWindowListCopyWindowInfo lists them.
    fn windows() -> Vec<Win> {
        let raw = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN, 0) };
        // A CFArray of CFDictionary is an NSArray of NSDictionary (toll-free); the call is a "Copy": ours.
        let Some(list) = (unsafe { Retained::from_raw(raw as *mut NSArray<NSDictionary<NSString, AnyObject>>) }) else { return vec![] };
        list.iter().filter_map(|d| {
            let bounds = d.objectForKey(ns_string!("kCGWindowBounds")).and_then(|o| o.downcast::<NSDictionary>().ok())?;
            // SAFETY: the window server's bounds are a dictionary of string keys to numbers.
            let bounds: Retained<NSDictionary<NSString, AnyObject>> = unsafe { Retained::cast_unchecked(bounds) };
            Some(Win {
                id: number(&d, ns_string!("kCGWindowNumber"))? as u64,
                pid: number(&d, ns_string!("kCGWindowOwnerPID"))? as u32,
                layer: number(&d, ns_string!("kCGWindowLayer")).unwrap_or(0.0) as i64,
                x: number(&bounds, ns_string!("X"))?, y: number(&bounds, ns_string!("Y"))?,
                w: number(&bounds, ns_string!("Width"))?, h: number(&bounds, ns_string!("Height"))?,
            })
        }).collect()
    }

    /// The pids of the apps named by pid, bundle id or name (compared without case).
    fn resolve(apps: &Apps) -> Vec<u32> {
        let mut pids = apps.pids.clone();
        if !apps.bundles.is_empty() || !apps.names.is_empty() {
            let lower = |v: &Vec<String>| v.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
            let (bundles, names) = (lower(&apps.bundles), lower(&apps.names));
            for a in NSWorkspace::sharedWorkspace().runningApplications().iter() {
                let b = a.bundleIdentifier().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
                let n = a.localizedName().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
                if (!b.is_empty() && bundles.contains(&b)) || (!n.is_empty() && names.contains(&n)) { pids.push(a.processIdentifier() as u32); }
            }
        }
        pids.retain(|p| *p > 1);
        pids.sort_unstable();
        pids.dedup();
        pids
    }

    /// The window server's picture of exactly those windows, as RGBA.
    fn draw(display: CGRect, ids: &[u64]) -> Result<RgbaImage, String> {
        // CGWindowListCreateImageFromArray is deprecated in favour of ScreenCaptureKit and
        // looked up by name: a system that no longer has it says so instead of failing to start.
        type Create = unsafe extern "C" fn(CGRect, *const c_void, u32) -> *const c_void;
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGWindowListCreateImageFromArray".as_ptr()) };
        if sym.is_null() { return Err("This macOS no longer has the window capture Hover uses.".into()); }
        let create: Create = unsafe { std::mem::transmute(sym) };
        let numbers: Vec<Retained<NSNumber>> = ids.iter().map(|i| NSNumber::new_i32(*i as i32)).collect();
        let array = NSArray::from_retained_slice(&numbers);
        let img = unsafe { create(display, Retained::as_ptr(&array).cast(), NOMINAL) };
        if img.is_null() { return Err("The screen couldn’t be read.".into()); }
        let (w, h) = unsafe { (CGImageGetWidth(img), CGImageGetHeight(img)) };
        let mut buf = vec![0u8; w * h * 4];
        unsafe {
            let space = CGColorSpaceCreateDeviceRGB();
            let ctx = CGBitmapContextCreate(buf.as_mut_ptr().cast(), w, h, 8, w * 4, space, RGBA);
            if !ctx.is_null() { CGContextDrawImage(ctx, CGRect { x: 0.0, y: 0.0, w: w as f64, h: h as f64 }, img); CGContextRelease(ctx); }
            CGColorSpaceRelease(space);
            CGImageRelease(img);
        }
        // Premultiplied over an opaque desktop is as good as straight; alpha is made opaque.
        for p in buf.chunks_exact_mut(4) { p[3] = 255; }
        RgbaImage::from_raw(w as u32, h as u32, buf).ok_or_else(|| "The screen couldn’t be read.".into())
    }

    pub fn capture(apps: &Apps, _max: (u32, u32)) -> Result<RgbaImage, String> {
        if !access() { return desktop(); }
        let d = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        let display = (d.x, d.y, d.w, d.h);
        let desktop_level = unsafe { CGWindowLevelForKey(DESKTOP_ICON_KEY) } as i64;
        let me = std::process::id();
        let list = windows();
        let pids = resolve(apps);
        let picked = shown(&list, &pids, display, desktop_level, me);
        if picked.is_empty() { return desktop(); }
        let ids: Vec<u64> = picked.iter().map(|w| w.id).collect();
        draw(d, &ids)
    }

    /// The desktop picture's file, drawn to fill the main display as macOS does.
    pub fn desktop() -> Result<RgbaImage, String> {
        use objc2_app_kit::NSScreen;
        let mtm = objc2::MainThreadMarker::new();
        let d = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        let size = (d.w.max(1.0) as u32, d.h.max(1.0) as u32);
        // Off the main thread the screen can't be asked for; the plain desktop then.
        let path = mtm.and_then(|m| NSScreen::mainScreen(m)).and_then(|s| NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&s)).and_then(|u| u.path()).map(|p| p.to_string());
        if let Some(p) = path {
            if let Ok(img) = image::open(&p) { return Ok(cover(&img.to_rgba8(), size)); }
        }
        Ok(plain(size, [0x1c, 0x1c, 0x1e]))
    }
}

// MARK: Windows

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub fn supported() -> bool { true }
    pub fn access() -> bool { true }
    pub fn request_access() {}

    fn screen() -> (u32, u32) { unsafe { (GetSystemMetrics(SM_CXSCREEN).max(1) as u32, GetSystemMetrics(SM_CYSCREEN).max(1) as u32) } }

    /// The desktop picture (or Windows' own copy of it), drawn to fill the screen; the
    /// desktop's colour when there is none to read.
    pub fn desktop() -> Result<RgbaImage, String> {
        let size = screen();
        let mut buf = [0u16; 520];
        let got = unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr().cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok();
        let path = if got { String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())]) } else { String::new() };
        let transcoded = std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("Microsoft").join("Windows").join("Themes").join("TranscodedWallpaper"));
        for file in [Some(std::path::PathBuf::from(path)), transcoded].into_iter().flatten() {
            if file.as_os_str().is_empty() || !file.is_file() { continue; }
            let read = image::ImageReader::open(&file).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string()).and_then(|r| r.decode().map_err(|e| e.to_string()));
            match read {
                Ok(img) => return Ok(cover(&img.to_rgba8(), size)),
                Err(e) => hover_core::log::line(&format!("screen: couldn’t read the desktop picture — {e}")),
            }
        }
        let c = unsafe { GetSysColor(COLOR_DESKTOP) };
        Ok(plain(size, [(c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8]))
    }

    struct Found { pids: Vec<u32>, hits: Vec<(HWND, RECT)> }

    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let f = unsafe { &mut *(l.0 as *mut Found) };
        if !unsafe { IsWindowVisible(h) }.as_bool() || unsafe { IsIconic(h) }.as_bool() { return true.into(); }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let mut r = RECT::default();
        if f.pids.contains(&pid) && unsafe { GetWindowRect(h, &mut r) }.is_ok() && r.right > r.left && r.bottom > r.top { f.hits.push((h, r)); }
        true.into()
    }

    /// One window's own picture, whatever is over it (PW_RENDERFULLCONTENT).
    fn grab(h: HWND, w: i32, hh: i32) -> Option<RgbaImage> {
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -hh, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0);
            let out = match bmp {
                Ok(bmp) if !bits.is_null() => {
                    let old = SelectObject(mem, bmp.into());
                    let ok = PrintWindow(h, mem, PRINT_WINDOW_FLAGS(2)).as_bool();
                    let bytes = std::slice::from_raw_parts(bits as *const u8, (w * hh * 4) as usize);
                    let mut rgba = Vec::with_capacity(bytes.len());
                    for p in bytes.chunks_exact(4) { rgba.extend_from_slice(&[p[2], p[1], p[0], 255]); }
                    SelectObject(mem, old);
                    let _ = DeleteObject(bmp.into());
                    if ok { RgbaImage::from_raw(w as u32, hh as u32, rgba) } else { None }
                }
                _ => None,
            };
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            out
        }
    }

    pub fn capture(apps: &Apps, _max: (u32, u32)) -> Result<RgbaImage, String> {
        let mut canvas = desktop()?;
        if apps.pids.is_empty() { return Ok(canvas); }
        let mut f = Found { pids: apps.pids.clone(), hits: vec![] };
        unsafe { let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Found as isize)); }
        // EnumWindows runs front to back; the back ones are drawn first.
        for (h, r) in f.hits.iter().rev() {
            if let Some(img) = grab(*h, r.right - r.left, r.bottom - r.top) { paste(&mut canvas, &img, r.left as i64, r.top as i64); }
        }
        Ok(canvas)
    }
}

// MARK: Linux

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, ImageFormat, MapState};

    pub fn supported() -> bool { std::env::var_os("DISPLAY").is_some() }
    pub fn access() -> bool { true }
    pub fn request_access() {}

    /// A plain desktop: X draws no wallpaper of its own that a client can read back.
    pub fn desktop() -> Result<RgbaImage, String> {
        let (c, n) = x11rb::connect(None).map_err(|e| format!("No X display: {e}"))?;
        let s = &c.setup().roots[n];
        Ok(plain((s.width_in_pixels as u32, s.height_in_pixels as u32), [0x24, 0x27, 0x2e]))
    }

    pub fn capture(apps: &Apps, _max: (u32, u32)) -> Result<RgbaImage, String> {
        let (c, n) = x11rb::connect(None).map_err(|e| format!("No X display: {e}"))?;
        let s = &c.setup().roots[n];
        let (root, size) = (s.root, (s.width_in_pixels as u32, s.height_in_pixels as u32));
        let mut canvas = plain(size, [0x24, 0x27, 0x2e]);
        let atom = |name: &str| c.intern_atom(false, name.as_bytes()).ok().and_then(|r| r.reply().ok()).map(|r| r.atom);
        let (Some(list), Some(pid)) = (atom("_NET_CLIENT_LIST_STACKING"), atom("_NET_WM_PID")) else { return Ok(canvas) };
        let clients: Vec<u32> = c.get_property(false, root, list, AtomEnum::WINDOW, 0, 4096).ok().and_then(|r| r.reply().ok())
            .and_then(|r| r.value32().map(|i| i.collect())).unwrap_or_default();
        // The stacking list runs bottom to top, which is the order to draw in.
        for w in clients {
            let owner = c.get_property(false, w, pid, AtomEnum::CARDINAL, 0, 1).ok().and_then(|r| r.reply().ok()).and_then(|r| r.value32().and_then(|mut i| i.next()));
            if !owner.is_some_and(|p| apps.pids.contains(&p)) { continue; }
            let viewable = c.get_window_attributes(w).ok().and_then(|r| r.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE);
            let (Some(g), Some(t)) = (c.get_geometry(w).ok().and_then(|r| r.reply().ok()), c.translate_coordinates(w, root, 0, 0).ok().and_then(|r| r.reply().ok())) else { continue };
            if !viewable || g.width == 0 || g.height == 0 { continue; }
            let Some(img) = c.get_image(ImageFormat::Z_PIXMAP, w, 0, 0, g.width, g.height, !0).ok().and_then(|r| r.reply().ok()) else { continue };
            // 24- and 32-bit ZPixmap on a little-endian server: B, G, R, pad.
            if img.data.len() != g.width as usize * g.height as usize * 4 { continue; }
            let mut rgba = Vec::with_capacity(img.data.len());
            for p in img.data.chunks_exact(4) { rgba.extend_from_slice(&[p[2], p[1], p[0], 255]); }
            if let Some(win) = RgbaImage::from_raw(g.width as u32, g.height as u32, rgba) { paste(&mut canvas, &win, t.dst_x as i64, t.dst_y as i64); }
        }
        Ok(canvas)
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod imp {
    use super::*;
    pub fn supported() -> bool { false }
    pub fn access() -> bool { false }
    pub fn request_access() {}
    pub fn desktop() -> Result<RgbaImage, String> { Err("The screen panel isn’t available here.".into()) }
    pub fn capture(_: &Apps, _: (u32, u32)) -> Result<RgbaImage, String> { desktop() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u64, pid: u32, layer: i64, x: f64, y: f64, w: f64, h: f64) -> Win { Win { id, pid, layer, x, y, w, h } }

    /// Screen.shown: the desktop's windows and the agent's apps', never the user's or Hover's.
    #[test]
    fn only_the_desktop_and_the_agents_apps_show() {
        let display = (0.0, 0.0, 1512.0, 982.0);
        let desktop = -2_147_483_608;
        let all = vec![
            win(1, 500, 0, 100.0, 100.0, 400.0, 300.0),     // the agent's app
            win(2, 777, 0, 0.0, 0.0, 1512.0, 982.0),        // the user's browser, full screen
            win(3, 42, 25, 600.0, 0.0, 300.0, 32.0),        // Hover's own notch
            win(4, 123, desktop, 0.0, 0.0, 1512.0, 982.0),  // the wallpaper
            win(5, 123, desktop + 5, 50.0, 50.0, 64.0, 64.0), // a desktop icon
            win(6, 500, 0, 3000.0, 0.0, 400.0, 300.0),      // the agent's app, on another display
            win(7, 42, 0, 10.0, 10.0, 100.0, 100.0),        // Hover's dashboard
        ];
        let ids: Vec<u64> = shown(&all, &[500, 42], display, desktop + 20, 42).iter().map(|w| w.id).collect();
        assert_eq!(ids, [1, 4, 5], "the agent's window, the wallpaper and its icon");
        // No apps: the desktop alone.
        let ids: Vec<u64> = shown(&all, &[], display, desktop + 20, 42).iter().map(|w| w.id).collect();
        assert_eq!(ids, [4, 5]);
    }

    #[test]
    fn frames_fit_without_growing_or_distorting() {
        assert_eq!(fit((2560, 1440), (1280, 800)), (1280, 720));
        assert_eq!(fit((1000, 600), (1280, 800)), (1000, 600));
        assert_eq!(fit((1440, 3000), (1280, 800)), (384, 800));
        assert_eq!(fit((0, 0), (0, 0)), (1, 1));
        let img = plain((2000, 1000), [1, 2, 3]);
        assert_eq!(scaled(img, (1000, 1000)).dimensions(), (1000, 500));
    }

    #[test]
    fn a_window_is_pasted_opaque_and_clipped() {
        let mut canvas = plain((100, 100), [10, 10, 10]);
        let win = RgbaImage::from_pixel(50, 50, image::Rgba([200, 0, 0, 0]));
        paste(&mut canvas, &win, 80, 80);
        assert_eq!(canvas.get_pixel(90, 90).0, [200, 0, 0, 255]);
        assert_eq!(canvas.get_pixel(79, 79).0, [10, 10, 10, 255]);
        paste(&mut canvas, &win, -40, -40);
        assert_eq!(canvas.get_pixel(5, 5).0, [200, 0, 0, 255]);
        assert_eq!(canvas.get_pixel(20, 20).0, [10, 10, 10, 255]);
    }

    #[test]
    fn a_wallpaper_covers_the_screen_centred() {
        // Wider than the screen: the sides are cut, the middle is kept.
        let mut src = RgbaImage::from_pixel(400, 100, image::Rgba([0, 0, 255, 255]));
        for y in 0..100 { for x in 180..220 { src.put_pixel(x, y, image::Rgba([255, 0, 0, 255])); } }
        let out = cover(&src, (100, 100));
        assert_eq!(out.dimensions(), (100, 100));
        assert_eq!(out.get_pixel(50, 50).0[0], 255);
        assert_eq!(out.get_pixel(2, 50).0[2], 255);
    }

    #[test]
    fn the_jpeg_decodes_and_the_url_is_a_data_url() {
        let img = plain((64, 48), [30, 90, 200]);
        let bytes = jpeg(&img, 80);
        let back = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48));
        assert!(data_url(&img, 60).starts_with("data:image/jpeg;base64,/9j/"));
        let apps = Apps::from_pids(&[3, 4]);
        assert!(!apps.none() && Apps::default().none());
    }

    #[test]
    fn where_the_panel_cannot_run_it_says_so() {
        // Every OS this is built for can; the note is for the others.
        assert_eq!(note().is_none(), supported());
    }

    /// On a desktop the real thing works: the wallpaper (or a plain one) comes back at a
    /// sane size. Skipped where there is no screen to read (a build server, a headless run).
    #[cfg(windows)]
    #[test]
    fn the_windows_desktop_is_readable() {
        let img = desktop((640, 400)).expect("a desktop");
        assert!(img.width() <= 640 && img.height() <= 400 && img.width() > 0);
        // No process asked for: no window of anyone's is on it beyond the desktop.
        let a = capture(&[], (320, 200)).expect("a frame");
        assert!(a.width() <= 320);
    }
}
