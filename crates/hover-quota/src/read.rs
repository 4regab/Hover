//! The readers: Quota.Kiro, Codex, Cursor and Claude, which touch kiro-cli, the disk
//! and the network. Each blocks; OwlApp ran them off the UI thread, and so does
//! `schedule`. The Windows locations are the C#'s; the Linux ones are where the same
//! tools keep the same files there (see the report); on a Mac Cursor's and Codex's
//! files are where Electron and Codex put them, and Claude Code's sign-in is in the
//! login Keychain.

use crate::sqlite::{self, Scalar};
use crate::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn home() -> PathBuf { hover_agents::proc::home() }

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from)
}

// MARK: Kiro CLI

/// kiro-cli's own report: `kiro-cli chat --no-interactive /usage`. One deadline of 25 s
/// for the exit and both pipes: a grandchild that inherits stdout can hold it open
/// after kiro-cli itself has gone. On a timeout the whole tree is killed.
pub fn kiro() -> Reading {
    let Some(exe) = hover_agents::agents::find("kiro-cli") else { return Reading::fail("kiro-cli isn’t installed or isn’t on PATH.") };
    kiro_with(&exe, &["chat", "--no-interactive", "/usage"], Duration::from_secs(25))
}

pub fn kiro_with(exe: &Path, args: &[&str], limit: Duration) -> Reading {
    use hover_agents::proc::{hidden, Group};
    let g = match Group::spawn(hidden(exe, args)) {
        Ok(g) => g,
        Err(e) => return Reading::fail(format!("kiro-cli failed: {e}")),
    };
    let start = std::time::Instant::now();
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    let (tx, rx) = std::sync::mpsc::channel::<(usize, String)>();
    for (i, p) in [stdout.map(|p| Box::new(p) as Box<dyn Read + Send>), stderr.map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().enumerate() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut s = vec![];
            if let Some(mut p) = p { let _ = p.read_to_end(&mut s); }
            let _ = tx.send((i, String::from_utf8_lossy(&s).into_owned()));
        });
    }
    drop(tx);
    let timed_out = || { g.kill(); Reading::fail("kiro-cli didn’t answer in time.") };
    if g.wait_timeout(limit).is_none() { return timed_out(); }
    let mut out = [String::new(), String::new()];
    for _ in 0..2 {
        let left = limit.saturating_sub(start.elapsed());
        match rx.recv_timeout(left) {
            Ok((i, s)) => out[i] = s,
            Err(_) => return timed_out(),
        }
    }
    parse_kiro(&format!("{}\n{}", out[0], out[1]))
}

// MARK: Codex

/// CODEX_HOME, else ~/.codex (the same on Windows, Linux and macOS).
pub fn codex_home() -> PathBuf { env_dir("CODEX_HOME").unwrap_or_else(|| home().join(".codex")) }

pub const CODEX_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Codex's limits as its own /status shows them, asked of chatgpt.com with the sign-in
/// Codex keeps in auth.json (read-only, never refreshed). The logs only hold what Codex
/// last saw: a limit reset early, or used from another machine or the web, stayed wrong
/// there until Codex ran again. They are the fallback when the live answer can't be had
/// (an API key, an expired sign-in, offline).
pub fn codex(now: DateTime<Utc>) -> Reading {
    let home = codex_home();
    match codex_live(&home.join("auth.json"), CODEX_URL, now) {
        Some(r) if r.ok() => r,
        live => {
            let logged = codex_in(&home, now);
            if logged.ok() { logged } else { live.unwrap_or(logged) }
        }
    }
}

/// The live read; None when there is nothing to ask with or no answer worth showing (the
/// logs are read then), a failure only for a sign-in chatgpt.com refused.
pub fn codex_live(auth: &Path, url: &str, now: DateTime<Utc>) -> Option<Reading> {
    let text = std::fs::read(auth).ok()?;
    let (token, account) = codex_sign_in(&hover_core::json::text_of(&text), now)?;
    let bearer = format!("Bearer {token}");
    let mut headers = vec![("Authorization", bearer.as_str()), ("Accept", "application/json"), ("User-Agent", "Hover")];
    if let Some(a) = account.as_deref() { headers.push(("ChatGPT-Account-Id", a)); }
    match get(url, &headers) {
        Ok((401 | 403, _)) => Some(Reading::fail("Codex’s sign-in has expired — run codex to renew it.")),
        Ok((s, body)) if (200..300).contains(&s) => Some(parse_codex_usage(&body, now)).filter(Reading::ok),
        _ => None,
    }
}

/// Quota.LastEvent: when a Codex session log last had an event, the "timestamp" that
/// starts its last line, read from the file's last 8 KB. None when it can't be read.
fn last_event(path: &Path) -> Option<DateTime<Utc>> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let n = len.min(8192);
    f.seek(SeekFrom::End(-(n as i64))).ok()?;
    let mut buf = vec![0; n as usize];
    f.read_exact(&mut buf).ok()?;
    let tail = String::from_utf8_lossy(&buf);
    const KEY: &str = "{\"timestamp\":\"";
    let i = tail.rfind(KEY)? + KEY.len();
    let end = tail[i..].find('"')?;
    DateTime::parse_from_rfc3339(&tail[i..i + end]).ok().map(|t| t.with_timezone(&Utc))
}

fn rollouts(dir: &Path, out: &mut Vec<(std::time::SystemTime, PathBuf)>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let t = e.file_type()?;
        let p = e.path();
        if t.is_dir() {
            rollouts(&p, out)?;
        } else {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                out.push((e.metadata()?.modified()?, p));
            }
        }
    }
    Ok(())
}

pub fn codex_in(home: &Path, now: DateTime<Utc>) -> Reading {
    let sessions = home.join("sessions");
    if !sessions.is_dir() { return Reading::fail("No Codex sessions on this PC yet."); }
    let mut found = vec![];
    if let Err(e) = rollouts(&sessions, &mut found) { return Reading::fail(format!("Couldn’t read Codex’s logs: {e}")); }
    // Ranked by the time of each file's last event, not its modified time: Codex keeps
    // a session's file open and Windows leaves the modified time at about when it was
    // made, so a long session's new limits were skipped. Opens every rollout file for
    // its last few KB; fine for thousands of sessions.
    let mut files: Vec<(DateTime<Utc>, PathBuf)> = found.into_iter().map(|(m, f)| (last_event(&f).unwrap_or_else(|| DateTime::<Utc>::from(m)), f)).collect();
    // OrderByDescending is stable: equal times keep the walk's order.
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, f) in files.into_iter().take(8) {
        match last_limits(&f, now) {
            Ok(Some(q)) => return q,
            Ok(None) => {}
            Err(e) => return Reading::fail(format!("Couldn’t read Codex’s logs: {e}")),
        }
    }
    Reading::fail("Codex hasn’t recorded any limits yet — use it once.")
}

/// The newest limits in a session log, read from its end a block at a time: the line
/// wanted is nearly always among the last few, and a long session's log runs to hundreds
/// of MB (reading all of it took seconds and as much memory). Codex may be writing to it
/// right now; a plain read shares it on every system.
fn last_limits(path: &Path, now: DateTime<Utc>) -> std::io::Result<Option<Reading>> {
    use std::io::{Seek, SeekFrom};
    const BLOCK: u64 = 256 << 10;
    let mut f = std::fs::File::open(path)?;
    let mut end = f.metadata()?.len();
    // The start of a line cut by the block below it, carried up to the next read.
    let mut carry: Vec<u8> = Vec::new();
    let mut block = Vec::with_capacity(BLOCK as usize);
    while end > 0 {
        let start = end.saturating_sub(BLOCK);
        block.clear();
        block.resize((end - start) as usize, 0);
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut block)?;
        block.extend_from_slice(&carry);
        // Every whole line in it, newest first (\n, \r and \r\n all end one); the first
        // piece is whole only at the file's start.
        let mut pieces: Vec<&[u8]> = block.split(|b| *b == b'\n' || *b == b'\r').collect();
        let first = if start > 0 { pieces.remove(0).to_vec() } else { Vec::new() };
        for line in pieces.iter().rev() {
            if line.windows(13).any(|w| w == b"\"rate_limits\"") {
                if let Some(q) = parse_codex_line(&String::from_utf8_lossy(line), now) { return Ok(Some(q)); }
            }
        }
        carry = first;
        end = start;
    }
    Ok(None)
}

// MARK: HTTP

fn agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder();
    #[cfg(windows)]
    let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
    // HttpClient's 15 s timeout; statuses are the caller's to read.
    cfg.timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into()
}

/// A GET with the given headers: its status and body, or Err when the host couldn't
/// be reached (HttpRequestException or the timeout).
fn get(url: &str, headers: &[(&str, &str)]) -> Result<(u16, String), ()> {
    let mut req = agent().get(url);
    for (k, v) in headers { req = req.header(*k, *v); }
    let mut res = req.call().map_err(|_| ())?;
    let status = res.status().as_u16();
    let mut body = vec![];
    res.body_mut().as_reader().take(16 << 20).read_to_end(&mut body).map_err(|_| ())?;
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

// MARK: Cursor

/// Where Cursor (an Electron app) keeps its state: %APPDATA%\Cursor on Windows,
/// ~/Library/Application Support/Cursor on a Mac, $XDG_CONFIG_HOME/Cursor
/// (~/.config/Cursor) on Linux.
pub fn cursor_db() -> Option<PathBuf> {
    #[cfg(any(windows, target_os = "macos"))]
    let base = hover_core::platform::app_data();
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = env_dir("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| Some(home().join(".config")));
    base.map(|b| cursor_db_under(&b))
}

/// Cursor's state database under the folder its Electron shell keeps settings in.
pub fn cursor_db_under(base: &Path) -> PathBuf { base.join("Cursor").join("User").join("globalStorage").join("state.vscdb") }

pub const CURSOR_URL: &str = "https://cursor.com/api/usage-summary";

pub fn cursor(now: DateTime<Utc>) -> Reading {
    match cursor_db() { Some(db) => cursor_at(&db, CURSOR_URL, now), None => Reading::fail("Cursor isn’t installed, or hasn’t been signed in to.") }
}

/// The token Cursor keeps in its database, as CursorToken reads it: text, or a blob
/// in UTF-16 when its second byte is zero, else UTF-8.
pub fn cursor_token(db: &Path) -> Result<Option<String>, String> {
    Ok(match sqlite::scalar(db, "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'")? {
        Scalar::Text(s) => clean_token(&s),
        Scalar::Blob(b) => {
            let s = if b.len() > 1 && b[1] == 0 {
                let units: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
                // An odd last byte is a broken unit, as Encoding.Unicode reads it.
                let mut s = String::from_utf16_lossy(&units[..b.len() / 2]);
                if b.len() % 2 == 1 { s.push('\u{FFFD}'); }
                s
            } else {
                String::from_utf8_lossy(&b).into_owned()
            };
            clean_token(&s)
        }
        Scalar::Other => None,
    })
}

pub fn cursor_at(db: &Path, url: &str, now: DateTime<Utc>) -> Reading {
    if !db.is_file() { return Reading::fail("Cursor isn’t installed, or hasn’t been signed in to."); }
    let token = match cursor_token(db) {
        Ok(t) => t,
        Err(e) => return Reading::fail(format!("Couldn’t read Cursor’s sign-in: {e}")),
    };
    let Some(token) = token else { return Reading::fail("Sign in to Cursor first.") };
    let Some(cookie) = cursor_cookie(&token, now) else { return Reading::fail("Cursor’s sign-in has expired — open Cursor to renew it.") };
    match get(url, &[("Cookie", &cookie), ("Accept", "application/json"), ("User-Agent", "Hover")]) {
        Err(()) => Reading::fail("Couldn’t reach cursor.com."),
        Ok((401 | 403, _)) => Reading::fail("cursor.com refused Cursor’s sign-in — open Cursor to renew it."),
        Ok((s, _)) if !(200..300).contains(&s) => Reading::fail(format!("cursor.com answered {s}.")),
        Ok((_, body)) => parse_cursor_summary(&body),
    }
}

// MARK: Claude Code

/// CLAUDE_CONFIG_DIR, else ~/.claude (the same on Windows, Linux and macOS).
pub fn claude_home() -> PathBuf { env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home().join(".claude")) }

pub const CLAUDE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

const CLAUDE_SIGN_IN: &str = "Sign in to Claude Code with a Claude plan (Pro or Max) first.";

/// What the quota says on a Mac when neither the Keychain nor the file gives a sign-in.
pub const CLAUDE_KEYCHAIN_MISSING: &str = "Claude Code credentials are unavailable. Sign in and allow Hover to read the Claude Code Keychain entry.";

/// The Keychain item Claude Code keeps its sign-in in on a Mac: a generic password of
/// this service, whose value is the text .credentials.json holds elsewhere.
pub const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Claude Code's sign-in from the login Keychain (macOS; the user is asked to allow it
/// once). Ok(None) where there is no Keychain or no such item. Read-only.
fn claude_keychain() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    { hover_core::platform::keychain_find(CLAUDE_KEYCHAIN_SERVICE).map(|found| found.map(|b| hover_core::json::text_of(&b))) }
    #[cfg(not(target_os = "macos"))]
    { Ok(None) }
}

/// On a Mac Claude Code keeps its sign-in in the Keychain, and the file is the
/// fallback; elsewhere it is the file.
pub fn claude(now: DateTime<Utc>) -> Reading {
    let file = claude_home().join(".credentials.json");
    match claude_keychain() {
        Ok(Some(text)) => return claude_with(&text, CLAUDE_URL, now),
        Ok(None) => {}
        Err(e) => hover_core::log::line(&format!("claude: the Keychain wouldn't give Claude Code's sign-in - {e}")),
    }
    if cfg!(target_os = "macos") && !file.is_file() { return Reading::fail(CLAUDE_KEYCHAIN_MISSING); }
    claude_at(&file, CLAUDE_URL, now)
}

/// Claude Code's plan limits, as its /usage shows them, asked of api.anthropic.com with
/// its own sign-in, read-only: the sign-in is never refreshed, which would rotate
/// Claude Code's tokens underneath it.
pub fn claude_at(file: &Path, url: &str, now: DateTime<Utc>) -> Reading {
    if !file.is_file() { return Reading::fail(CLAUDE_SIGN_IN); }
    let text = match std::fs::read(file) {
        Ok(b) => hover_core::json::text_of(&b),
        Err(e) => return Reading::fail(format!("Couldn’t read Claude Code’s sign-in: {e}")),
    };
    claude_with(&text, url, now)
}

/// claude_at, with the sign-in's JSON already in hand (from the file or the Keychain).
pub fn claude_with(credentials: &str, url: &str, now: DateTime<Utc>) -> Reading {
    let sign = claude_sign_in(credentials);
    let Some(token) = sign.token else { return Reading::fail(CLAUDE_SIGN_IN) };
    if sign.expires.is_some_and(|exp| exp <= now + chrono::Duration::seconds(60)) {
        return Reading::fail("Claude Code’s sign-in has expired — run claude to renew it.");
    }
    let bearer = format!("Bearer {token}");
    match get(url, &[("Authorization", &bearer), ("anthropic-beta", "oauth-2025-04-20"), ("Accept", "application/json"), ("User-Agent", "Hover")]) {
        Err(()) => Reading::fail("Couldn’t reach api.anthropic.com."),
        Ok((401, _)) => Reading::fail("Anthropic refused Claude Code’s sign-in — run claude to renew it."),
        Ok((403, _)) => Reading::fail("This sign-in can’t read plan usage — run claude and sign in again."),
        Ok((429, _)) => Reading::fail("Anthropic is limiting usage checks; Hover tries again in five minutes."),
        Ok((s, _)) if !(200..300).contains(&s) => Reading::fail(format!("api.anthropic.com answered {s}.")),
        Ok((_, body)) => parse_claude_usage(&body, sign.plan.as_deref(), now),
    }
}

/// One quota by its notch id.
pub fn by_id(id: &str) -> Reading {
    let now = Utc::now();
    match id {
        item::KIRO => kiro(),
        item::CODEX => codex(now),
        item::CLAUDE => claude(now),
        _ => cursor(now),
    }
}
