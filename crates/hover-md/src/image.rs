//! `imageFor` from main.js: where an image in an answer may load from. Web addresses as
//! they are; anything else only inside the session's own folder, through its files host.

/// A session as the resolver sees it: its files host (`f<key12>.hover`) and folder.
pub struct Session<'a> {
    pub files: Option<&'a str>,
    pub folder: &'a str,
}

fn is_web(s: &str) -> bool {
    let l = s.get(..8).unwrap_or(s).to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

// decodeURIComponent: None when it would throw (bad escape or bad UTF-8).
fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// encodeURIComponent.
fn encode(s: &str) -> String {
    let mut o = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) {
            o.push(c as char);
        } else {
            o.push_str(&format!("%{c:02X}"));
        }
    }
    o
}

// The rest of s after `units` UTF-16 code units.
fn skip_units(s: &str, units: usize) -> &str {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= units {
            return &s[i..];
        }
        n += c.len_utf16();
    }
    ""
}

pub fn image_for(s: &Session, src: &str) -> Option<String> {
    if is_web(src) {
        return Some(src.to_string());
    }
    let files = s.files?;
    let lower = src.to_ascii_lowercase();
    let mut q = if lower.starts_with("file:/") { src[5..].trim_start_matches('/') } else { src }.replace('\\', "/");
    if let Some(d) = decode(&q) {
        q = d;
    }
    let root = format!("{}/", s.folder.replace('\\', "/").trim_end_matches('/'));
    let b = q.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/' {
        if !q.to_lowercase().starts_with(&root.to_lowercase()) {
            return None;
        }
        q = skip_units(&q, root.encode_utf16().count()).to_string();
    }
    let q = q.strip_prefix("./").unwrap_or(&q);
    if q.starts_with('/') || q.split('/').any(|p| p == "..") {
        return None;
    }
    Some(format!("https://{files}/{}", q.split('/').map(encode).collect::<Vec<_>>().join("/")))
}
