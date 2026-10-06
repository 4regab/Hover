//! Hover on the user's Discord status: "Playing Hover", the agents at work and for how long
//! Hover has been open. It talks to the Discord app on this computer over the local
//! connection Discord opens for this (a Unix socket, a named pipe on Windows). No internet,
//! no sign-in and no token. Off until the user switches it on in Settings → Integrations.
//!
//! The status clears by itself when Hover quits (the connection closes) or the switch goes off.

use hover_core::json::{self, Json};
use hover_core::settings::Settings;
use crate::session::KiroSessions;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The "Hover" app in Discord's Developer Portal. Its name there is what shows after "Playing".
const APP_ID: &str = "1556886510786314252";
/// The picture beside the status (Discord fetches it through its own proxy).
const ICON: &str = "https://raw.githubusercontent.com/4regab/Hover/main/assets/hover.png";
/// Discord allows 5 status changes per 20 s. One per 5 s stays under it.
const MIN_GAP: Duration = Duration::from_secs(5);
/// Without news, the status is sent again this often: it finds Discord after it was started
/// late and puts the status back after Discord was restarted.
const TICK: Duration = Duration::from_secs(30);

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// Something changed (a switch, a task): look again now.
pub fn wake() {
    *WAKE.0.lock().unwrap() = true;
    WAKE.1.notify_all();
}

/// Waits for `wake` or `d`; true when woken.
fn sleep(d: Duration) -> bool {
    let g = WAKE.0.lock().unwrap();
    let (mut g, _) = WAKE.1.wait_timeout_while(g, d, |w| !*w).unwrap();
    std::mem::take(&mut *g)
}

/// Starts the status for this run. The thread idles while the switch is off.
pub fn start(settings: Arc<Settings>, sessions: KiroSessions) {
    let s = sessions.clone();
    sessions.on_changed(wake);
    let want = move || settings.discord_presence().then(|| working(&s));
    if let Err(e) = std::thread::Builder::new().name("discord".into()).spawn(move || run(APP_ID, want, connect)) {
        hover_core::log::line(&format!("discord status not started: {e}"));
    }
}

/// The tools with a task at work, each once, in the order they started.
fn working(sessions: &KiroSessions) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = vec![];
    for s in sessions.all_light().iter().filter(|s| s.busy()) {
        if !names.contains(&s.tool.name()) { names.push(s.tool.name()); }
    }
    names
}

/// The status: how many agents are at work and which, or Idle.
fn activity(tools: &[&str], started: u64) -> Json {
    let details = match tools.len() { 0 => "Idle".to_owned(), 1 => "1 agent working".to_owned(), n => format!("{n} agents working") };
    let mut props = vec![("details", Json::str(details))];
    if !tools.is_empty() { props.push(("state", Json::str(tools.join(", ").chars().take(128).collect::<String>()))); }
    props.push(("timestamps", Json::obj(vec![("start", Json::int(started as i64))])));
    props.push(("assets", Json::obj(vec![("large_image", Json::str(ICON)), ("large_text", Json::str("Hover"))])));
    Json::obj(props)
}

/// The loop: `want` says None while the status is off, else the tools at work.
fn run(app_id: &str, want: impl Fn() -> Option<Vec<&'static str>>, connect: impl Fn() -> io::Result<Box<dyn Pipe>>) {
    let started = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut link: Option<Link> = None;
    // What Discord shows now (None: nothing), and when it was last told.
    let (mut shown, mut told): (Option<String>, Option<Instant>) = (None, None);
    let mut complained = false;
    let mut force = false;
    loop {
        let body = want().map(|t| activity(&t, started));
        let text = body.as_ref().map(Json::compact);
        let mut wait = TICK;
        match body {
            None => {
                if let Some(mut l) = link.take() { let _ = l.set(None); }
                shown = None;
                complained = false;
            }
            Some(b) if force || text != shown => {
                let gap = told.map_or(Duration::ZERO, |t| MIN_GAP.saturating_sub(t.elapsed()));
                if !gap.is_zero() {
                    wait = gap;
                } else {
                    told = Some(Instant::now());
                    let sent = match link.take() {
                        Some(l) => Ok(l),
                        None => connect().and_then(|p| Link::open(p, app_id)),
                    }.and_then(|mut l| l.set(Some(&b)).map(|_| l));
                    match sent {
                        Ok(l) => { link = Some(l); shown = text; complained = false; }
                        // Not running is the usual reason, and is tried again on the next tick.
                        Err(e) => { if !complained { hover_core::log::line(&format!("discord status not sent: {e}")); complained = true; } }
                    }
                }
            }
            Some(_) => {}
        }
        force = !sleep(wait);
    }
}

/// A connection to Discord that has shaken hands.
struct Link { pipe: Box<dyn Pipe>, nonce: u64 }

/// One message: opcode and length (both u32, little endian), then JSON.
fn frame(op: u32, body: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + body.len());
    v.extend_from_slice(&op.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body.as_bytes());
    v
}

fn read_frame(p: &mut dyn Read) -> io::Result<(u32, Json)> {
    let mut head = [0u8; 8];
    p.read_exact(&mut head)?;
    let (op, len) = (u32::from_le_bytes(head[..4].try_into().unwrap()), u32::from_le_bytes(head[4..].try_into().unwrap()) as usize);
    if len > 64 * 1024 { return Err(io::Error::new(io::ErrorKind::InvalidData, "Discord sent an oversized message")); }
    let mut body = vec![0u8; len];
    p.read_exact(&mut body)?;
    let json = json::parse(&String::from_utf8_lossy(&body)).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok((op, json))
}

impl Link {
    fn open(mut pipe: Box<dyn Pipe>, app_id: &str) -> io::Result<Link> {
        let hello = Json::obj(vec![("v", Json::int(1)), ("client_id", Json::str(app_id))]).compact();
        pipe.write_all(&frame(0, &hello))?;
        let (_, reply) = read_frame(&mut pipe)?;
        match reply.get("evt").and_then(Json::as_str) {
            Some("READY") => Ok(Link { pipe, nonce: 0 }),
            _ => Err(io::Error::other(format!("Discord refused: {}", reply.compact()))),
        }
    }

    /// Shows the status, or clears it (None). Waits for Discord's answer so an error is seen.
    fn set(&mut self, activity: Option<&Json>) -> io::Result<()> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let mut args = vec![("pid", Json::int(std::process::id() as i64))];
        if let Some(a) = activity { args.push(("activity", a.clone())); }
        let msg = Json::obj(vec![("cmd", Json::str("SET_ACTIVITY")), ("args", Json::obj(args)), ("nonce", Json::str(&nonce))]);
        self.pipe.write_all(&frame(1, &msg.compact()))?;
        // ponytail: on Windows a pipe read has no timeout, so a Discord that never answers
        // stalls this thread (only the status). Upgrade: overlapped I/O.
        for _ in 0..8 {
            let (op, reply) = read_frame(&mut self.pipe)?;
            match op {
                // Ping: answered with the same body.
                3 => self.pipe.write_all(&frame(4, &reply.compact()))?,
                // Closed by Discord.
                2 => return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Discord closed the connection")),
                _ if reply.get("nonce").and_then(Json::as_str) == Some(&nonce) => {
                    return match reply.get("evt").and_then(Json::as_str) {
                        Some("ERROR") => Err(io::Error::other(format!("Discord said: {}", reply.get("data").map_or(String::new(), Json::compact)))),
                        _ => Ok(()),
                    };
                }
                _ => {}
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "Discord did not answer"))
    }
}

/// Discord's connection: the first of discord-ipc-0 to -9 that answers, in the places the
/// desktop app, Flatpak and Snap put it.
#[cfg(unix)]
fn connect() -> io::Result<Box<dyn Pipe>> {
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    const INSIDE: [&str; 7] = ["", "app/com.discordapp.Discord/", "app/dev.vencord.Vesktop/", ".flatpak/com.discordapp.Discord/xdg-run/",
        ".flatpak/dev.vencord.Vesktop/xdg-run/", "snap.discord/", "snap.discord-canary/"];
    let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    bases.push("/tmp".into());
    bases.dedup();
    for base in bases {
        for inside in INSIDE {
            for i in 0..10 {
                let path = base.join(inside).join(format!("discord-ipc-{i}"));
                if !path.exists() { continue; }
                if let Ok(s) = UnixStream::connect(&path) {
                    s.set_read_timeout(Some(Duration::from_secs(5)))?;
                    s.set_write_timeout(Some(Duration::from_secs(5)))?;
                    return Ok(Box::new(s));
                }
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "Discord isn't running"))
}

#[cfg(windows)]
fn connect() -> io::Result<Box<dyn Pipe>> {
    for i in 0..10 {
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")) { return Ok(Box::new(f)); }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "Discord isn't running"))
}
