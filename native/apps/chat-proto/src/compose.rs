//! The composer's images: what main.js does with a pasted, dropped or picked image
//! (`shrink`, `addPics`: at most 4, the long side at most 2000 px, re-encoded as JPEG 90
//! unless the original is small and a web format), and what KiroPage.SaveImages does
//! with them on send (data:image/*;base64 only, 8 MiB each, into `kiro-images` as
//! `yyyyMMdd-HHmmss-<guid>` cut to 24 characters, plus the extension).

use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const MAX_PICS: usize = 4;

/// One pending image: its data URL (what the page sends) and its pixels, for the strip.
#[derive(Clone)]
pub struct Pic {
    pub url: String,
    pub rgba: image::RgbaImage,
}

fn mime_of(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some("image/png"),
        image::ImageFormat::Jpeg => Some("image/jpeg"),
        image::ImageFormat::Gif => Some("image/gif"),
        image::ImageFormat::WebP => Some("image/webp"),
        image::ImageFormat::Bmp => Some("image/bmp"),
        _ => None,
    }
}

/// main.js shrink(file): None when it doesn't decode.
pub fn shrink(bytes: &[u8]) -> Option<Pic> {
    let mime = mime_of(bytes)?;
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    let k = (2000.0 / w.max(h) as f64).min(1.0);
    let web = matches!(mime, "image/png" | "image/jpeg" | "image/webp" | "image/gif");
    let url = if k == 1.0 && bytes.len() < 3_000_000 && web {
        format!("data:{mime};base64,{}", b64(bytes))
    } else {
        let (nw, nh) = (((w as f64) * k).round().max(1.0) as u32, ((h as f64) * k).round().max(1.0) as u32);
        // A canvas has no alpha in JPEG: transparent pixels come out black, as toDataURL's.
        let rgb = image::DynamicImage::ImageRgba8(img.resize_exact(nw, nh, image::imageops::FilterType::Triangle).to_rgba8()).to_rgb8();
        let mut out = Cursor::new(vec![]);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&rgb).ok()?;
        format!("data:image/jpeg;base64,{}", b64(out.get_ref()))
    };
    Some(Pic { url, rgba: thumb(&img) })
}

/// `.shot img { object-fit: cover }` in a 52 px square with radius 9, at 2x: cropped
/// to the middle square and rounded in its pixels (the software renderer doesn't clip
/// to rounded corners).
fn thumb(img: &image::DynamicImage) -> image::RgbaImage {
    const S: u32 = 104;
    const R: f32 = 18.0;
    let side = img.width().min(img.height());
    let sq = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let mut t = sq.resize_exact(S, S, image::imageops::FilterType::Triangle).to_rgba8();
    for (x, y, p) in t.enumerate_pixels_mut() {
        // Distance past the corner's arc, for a one-pixel soft edge.
        let (cx, cy) = ((x as f32 + 0.5).clamp(R, S as f32 - R), (y as f32 + 0.5).clamp(R, S as f32 - R));
        let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
        let a = (R + 0.5 - d).clamp(0.0, 1.0);
        p[3] = (p[3] as f32 * a).round() as u8;
    }
    t
}

/// A clipboard bitmap, which Chromium hands the page as a PNG file.
pub fn from_rgba(w: u32, h: u32, rgba: Vec<u8>) -> Option<Pic> {
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut png = Cursor::new(vec![]);
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    shrink(png.get_ref())
}

/// KiroPage.SaveImages (hover-core): the paths written, in order.
pub fn save(urls: &[String], folder: &Path) -> Vec<PathBuf> {
    let items: Vec<hover_core::json::Json> = urls.iter().map(hover_core::json::Json::str).collect();
    let _ = std::fs::create_dir_all(folder);
    hover_core::images::save(&items, folder)
}

/// The URL the page shows a saved image at (KiroPage.State).
pub fn url_for(p: &Path) -> String {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let enc: String = name.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect();
    format!("https://hover.images/{enc}")
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(b: &[u8]) -> String {
    let mut o = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            o.push(if i <= c.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    o
}

/// The system's file picker, for "Attach an image": png, jpeg, gif and webp, several.
#[cfg(windows)]
pub fn pick() -> Vec<PathBuf> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, SIGDN_FILESYSPATH};
    let mut out = vec![];
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(d) = CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) else { return out };
        let spec = [COMDLG_FILTERSPEC { pszName: w!("Images"), pszSpec: w!("*.png;*.jpg;*.jpeg;*.gif;*.webp") }];
        let _ = d.SetFileTypes(&spec);
        if let Ok(o) = d.GetOptions() { let _ = d.SetOptions(o | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST); }
        if d.Show(None).is_err() { return out; }
        let Ok(items) = d.GetResults() else { return out };
        for i in 0..items.GetCount().unwrap_or(0) {
            if let Ok(p) = items.GetItemAt(i).and_then(|it| it.GetDisplayName(SIGDN_FILESYSPATH)) {
                out.push(PathBuf::from(PCWSTR(p.0).to_string().unwrap_or_default()));
                CoTaskMemFree(Some(p.0 as *const _));
            }
        }
    }
    out
}

/// On the dev VM: zenity, if it is there.
#[cfg(not(windows))]
pub fn pick() -> Vec<PathBuf> {
    let r = std::process::Command::new("zenity").args(["--file-selection", "--multiple", "--separator=\n", "--file-filter=*.png *.jpg *.jpeg *.gif *.webp"]).output();
    r.map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = Cursor::new(vec![]);
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255])).write_to(&mut b, image::ImageFormat::Png).unwrap();
        b.into_inner()
    }

    #[test]
    fn small_web_images_go_as_they_are_and_big_ones_as_jpeg() {
        let small = png(300, 200);
        let p = shrink(&small).unwrap();
        assert_eq!(p.url, format!("data:image/png;base64,{}", b64(&small)));
        assert_eq!(hover_core::images::from_base64(&p.url[22..]).unwrap(), small);
        let big = shrink(&png(4000, 1000)).unwrap();
        assert!(big.url.starts_with("data:image/jpeg;base64,"));
        let j = hover_core::images::from_base64(&big.url[23..]).unwrap();
        let d = image::load_from_memory(&j).unwrap();
        assert_eq!((d.width(), d.height()), (2000, 500));
        assert!(shrink(b"not an image").is_none());
    }

    #[test]
    fn save_images_keeps_to_the_hosts_rules() {
        let dir = std::env::temp_dir().join(format!("hover-pics-{}", std::process::id()));
        let ok = shrink(&png(8, 8)).unwrap().url;
        let urls = vec!["data:text/plain;base64,AAAA".into(), "data:image/bmp;base64,AAAA".into(), ok.clone(), "data:image/png;base64,@@".into(),
            ok.clone(), ok.clone(), ok.clone(), ok.clone()];
        let saved = save(&urls, &dir);
        assert_eq!(saved.len(), 4, "text, bmp and bad base64 skipped; at most 4");
        for p in &saved {
            let n = p.file_name().unwrap().to_str().unwrap();
            assert_eq!(n.len(), 24 + 4, "{n}");
            assert!(n.ends_with(".png") && n.as_bytes()[8] == b'-' && n.as_bytes()[15] == b'-');
            assert_eq!(std::fs::read(p).unwrap(), png(8, 8));
            assert!(url_for(p).starts_with("https://hover.images/"));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
