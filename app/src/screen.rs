//! The desk's Screen panel (Owl/ScreenFeed.cs on Windows): the
//! main display as the agent has it, never the user's own work. It is the desktop (the
//! wallpaper) with only the windows of the apps the
//! agent's computer use opened or acted on over it (their process ids, from its steps),
//! so the user's open apps never show. The desk asks for a frame while the panel shows,
//! four a second while the agent tests, and a still at rest.
//!
//! - Windows: each window of those processes through PrintWindow, over the wallpaper.
//! - Linux (X11): the `_NET_WM_PID` windows of those processes, over a plain desktop.
//!   Not on Wayland.
//!
//! The fitting (`fit`) and the encoding are plain and tested on every OS; the capture is
//! each OS's. (The Mac app's panel is macos/Sources/Screen.swift.)

use image::{imageops, Rgb, RgbImage, RgbaImage};

/// The width the panel is sent at.
pub const WIDTH: u32 = 1280;

/// The apps whose windows may show. Only the process ids are used here; the bundle ids and
/// names are what a session's computer-use steps add up to (`hover_agents::desk::AgentApps`),
/// which the Mac app resolves itself.
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
pub fn note() -> Option<&'static str> { (!supported()).then_some("The screen panel needs Windows or a Linux desktop on X11.") }

/// A frame of the main display with only the windows of `pids`' processes over the desktop,
/// scaled to fit `max`. Never any other window. Blocks for a moment: call it off the UI
/// thread. Err says why there is no frame (no display); the caller
/// keeps the last one.
pub fn capture(pids: &[u32], max: (u32, u32)) -> Result<RgbaImage, String> { capture_apps(&Apps::from_pids(pids), max) }

/// `capture`, for the apps as a session's steps name them (their process ids are used).
pub fn capture_apps(apps: &Apps, max: (u32, u32)) -> Result<RgbaImage, String> { imp::capture(apps, max).map(|i| scaled(i, max)) }

/// The desktop alone (the panel at rest): the wallpaper, scaled to fit `max`.
pub fn desktop(max: (u32, u32)) -> Result<RgbaImage, String> { imp::desktop().map(|i| scaled(i, max)) }

/// The main display as it is now, every window on it (voice's "take a screenshot"), scaled to
/// fit `max`. Blocks for a moment: call it off the UI thread.
pub fn whole(max: (u32, u32)) -> Result<RgbaImage, String> { imp::whole().map(|i| scaled(i, max)) }

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

    /// The primary display from the screen's own picture (BitBlt with CAPTUREBLT, so layered
    /// windows are in it too).
    pub fn whole() -> Result<RgbaImage, String> {
        let (w, hh) = screen();
        let (w, hh) = (w as i32, hh as i32);
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -hh, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0);
            let out = match bmp {
                Ok(bmp) if !bits.is_null() => {
                    let old = SelectObject(mem, bmp.into());
                    let ok = BitBlt(mem, 0, 0, w, hh, Some(screen), 0, 0, ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0)).is_ok();
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
            out.ok_or_else(|| "Windows didn’t give a picture of the screen.".into())
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

    /// A plain desktop: X draws no wallpaper of its own that a client can read back.
    pub fn desktop() -> Result<RgbaImage, String> {
        let (c, n) = x11rb::connect(None).map_err(|e| format!("No X display: {e}"))?;
        let s = &c.setup().roots[n];
        Ok(plain((s.width_in_pixels as u32, s.height_in_pixels as u32), [0x24, 0x27, 0x2e]))
    }

    /// The root window's picture: the screen as X has it, every window included.
    pub fn whole() -> Result<RgbaImage, String> {
        let (c, n) = x11rb::connect(None).map_err(|e| format!("No X display: {e}"))?;
        let s = &c.setup().roots[n];
        let (w, h) = (s.width_in_pixels, s.height_in_pixels);
        let img = c.get_image(ImageFormat::Z_PIXMAP, s.root, 0, 0, w, h, !0).map_err(|e| e.to_string())?.reply().map_err(|e| format!("X didn’t give a picture of the screen: {e}"))?;
        // 24- and 32-bit ZPixmap on a little-endian server: B, G, R, pad.
        if img.data.len() != w as usize * h as usize * 4 { return Err("The screen’s picture isn’t in a form Hover reads.".into()); }
        let mut rgba = Vec::with_capacity(img.data.len());
        for p in img.data.chunks_exact(4) { rgba.extend_from_slice(&[p[2], p[1], p[0], 255]); }
        RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| "The screen’s picture couldn’t be read.".into())
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

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::*;
    pub fn supported() -> bool { false }
    pub fn desktop() -> Result<RgbaImage, String> { Err("The screen panel isn’t available here.".into()) }
    pub fn whole() -> Result<RgbaImage, String> { desktop() }
    pub fn capture(_: &Apps, _: (u32, u32)) -> Result<RgbaImage, String> { desktop() }
}

#[cfg(test)]
mod tests {
    use super::*;

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
