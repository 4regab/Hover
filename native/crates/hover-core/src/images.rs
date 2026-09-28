//! KiroPage.ImagesFolder and SaveImages: pasted pictures kept as files in
//! kiro-images, for the agent to read and the office to show; files older than two
//! weeks go the first time the folder is asked for in a run.

use crate::json::Json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const KEEP: Duration = Duration::from_secs(14 * 86_400);

static SWEPT: AtomicBool = AtomicBool::new(false);

/// The folder under the data folder, made if needed, swept once per run.
pub fn folder(support: &Path) -> PathBuf {
    let dir = support.join("kiro-images");
    let _ = std::fs::create_dir_all(&dir);
    if !SWEPT.swap(true, Ordering::SeqCst) { sweep(&dir, SystemTime::now()); }
    dir
}

/// Deletes the files (not folders) last written more than 14 days before now.
pub fn sweep(dir: &Path, now: SystemTime) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let Some(cut) = now.checked_sub(KEEP) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if md.is_file() && md.modified().is_ok_and(|m| m < cut) { let _ = std::fs::remove_file(e.path()); }
    }
}

/// SaveImages: a message's images, as data: URLs, saved into the folder. Only PNG,
/// JPEG, GIF and WebP, up to four of 8 MiB; anything else is skipped, and an item that
/// isn't a string ends the list, as the C# loop breaks there.
pub fn save(items: &[Json], dir: &Path) -> Vec<PathBuf> {
    let mut saved = vec![];
    for item in items {
        let Some(url) = item.as_str() else { break };
        if saved.len() >= MAX_IMAGES { break; }
        let Some(comma) = url.find(',') else { continue };
        if !url.starts_with("data:image/") || !url[..comma].ends_with(";base64") { continue; }
        // url[11..url.IndexOf(';')]: up to the first ';', wherever it is.
        let semi = url.find(';').unwrap();
        let ext = match url.get(11..semi) { Some("png") => ".png", Some("jpeg") => ".jpg", Some("gif") => ".gif", Some("webp") => ".webp", _ => continue };
        // (url.Length - comma) * 3 / 4, in UTF-16 units as C# counts them.
        if (url[comma..].encode_utf16().count()) * 3 / 4 > MAX_IMAGE_BYTES { continue; }
        let Some(bytes) = from_base64(&url[comma + 1..]) else { continue };
        let name: String = format!("{}-{}", crate::time::local_compact(), crate::guid_n()).chars().take(24).collect();
        let file = dir.join(format!("{name}{ext}"));
        match std::fs::write(&file, bytes) {
            Ok(()) => saved.push(file),
            Err(e) => crate::log::line(&format!("kiro office: couldn't save a pasted image - {e}")),
        }
    }
    saved
}

/// Convert.FromBase64String: spaces, tabs, CR and LF anywhere are skipped; the rest
/// must be whole quads with at most two '=' at the end. None when it isn't.
pub fn from_base64(s: &str) -> Option<Vec<u8>> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s: Vec<u8> = s.bytes().filter(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n')).collect();
    if !s.len().is_multiple_of(4) { return None; }
    let mut o = Vec::with_capacity(s.len() / 4 * 3);
    let quads = s.len() / 4;
    for (q, c) in s.chunks(4).enumerate() {
        let mut n = 0u32;
        let mut pad = 0;
        for (i, &ch) in c.iter().enumerate() {
            let v = match ch {
                b'=' if i >= 2 && q + 1 == quads => { pad += 1; 0 }
                _ if pad > 0 => return None,
                _ => A.iter().position(|&x| x == ch)? as u32,
            };
            n = n << 6 | v;
        }
        o.push((n >> 16) as u8);
        if pad < 2 { o.push((n >> 8) as u8); }
        if pad < 1 { o.push(n as u8); }
    }
    Some(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-images-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn saves_as_save_images_does() {
        let d = tmp("save");
        let s = |x: &str| Json::str(x);
        let items = [s("data:image/png;base64,iVBO"), s("data:image/bmp;base64,AAAA"), s("data:text/plain;base64,AAAA"),
            s("data:image/png,AAAA"), s("data:image/jpeg;x=1;base64,/9j/"), s("data:image/gif;base64,R0l"), Json::int(1), s("data:image/png;base64,AAAA")];
        let saved = save(&items, &d);
        let names: Vec<String> = saved.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names[0].ends_with(".png") && names[1].ends_with(".jpg"));
        assert_eq!(names[0].len(), 24 + 4);
        assert_eq!(&names[0][8..9], "-");
        assert_eq!(std::fs::read(&saved[0]).unwrap(), [0x89, 0x50, 0x4E]);
        let five: Vec<Json> = (0..5).map(|_| s("data:image/webp;base64,UklG")).collect();
        assert_eq!(save(&five, &d).len(), 4);
    }

    #[test]
    fn the_size_check_counts_the_comma_as_csharp_does() {
        let d = tmp("size");
        // (len - comma) * 3 / 4 must not pass 8 MiB, with the comma counted.
        let ok = format!("data:image/png;base64,{}", "A".repeat(11_184_808));
        assert_eq!(save(&[Json::str(ok)], &d).len(), 1);
        let big = format!("data:image/png;base64,{}", "A".repeat(11_184_812));
        assert_eq!(save(&[Json::str(big)], &d).len(), 0);
    }

    #[test]
    fn old_files_are_swept() {
        let d = tmp("sweep");
        std::fs::write(d.join("new.png"), "x").unwrap();
        std::fs::create_dir(d.join("sub")).unwrap();
        sweep(&d, SystemTime::now() + Duration::from_secs(13 * 86_400));
        assert!(d.join("new.png").exists());
        sweep(&d, SystemTime::now() + Duration::from_secs(15 * 86_400));
        assert!(!d.join("new.png").exists() && d.join("sub").exists());
    }

    #[test]
    fn base64_as_convert_reads_it() {
        assert_eq!(from_base64("SGVs bG8=\r\n").unwrap(), b"Hello");
        assert_eq!(from_base64("").unwrap(), b"");
        for bad in ["SGVsbG8", "SG=sbG8=", "SGVs=G8=", "S===", "SGVsbG8\u{c}="] { assert!(from_base64(bad).is_none(), "{bad}"); }
    }
}
