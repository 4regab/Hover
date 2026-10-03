//! Services/Spaces.cs (Arz's 8d55562): each project's own desktop, a Cua Space
//! (spaces.cua.ai), a VM or container that the agents working in that folder share (each
//! with its own cursor), and that the user watches and steps into, instead of the user's own
//! screen. Hover goes through Cua's own `cua` CLI (MIT): `cua spaces create|start|stop|delete`
//! for the Space, `cua mcp --sandbox <space>` as the session's computer-use MCP server (run
//! by Hover outside the agents' sandbox and joined to the agent over the same socket as
//! Hover's browser, `browser::bridge`), `cua sb view` for the live viewer the Screen panel
//! shows, and `cua sb cp` / `sb exec` for an app or files dragged onto the notch. Only the CLI
//! and Lume are used, never Cua's own app: the desktops are Hover's. Spaces are local and
//! free; nothing goes through Cua's relay. A project's Space is made when its first agent's
//! run starts (a clone, about 25 s, once the image is on the Mac), stopped when no agent of
//! that project is left in the office, and deleted with the project's last session.
//!
//! Cua is kept to the Mac here, as Cua Driver is (`computer_use`): where `supported()` is
//! false the switch is off with `UNSUPPORTED` beside it. Upstream also ran on Linux; this
//! port doesn't. Everything that can be a plain function (names, the lists Cua and Lume
//! print, the install script, the progress lines, the viewer's address) is one, compiled
//! and tested on every OS.
//!
//! Unlike the C#, nothing here is async: every call blocks, so the backend runs them on
//! threads of its own.

use crate::browser;
use crate::cancel::Cancel;
use crate::computer_use::McpServer;
use crate::proc::{hidden, home, on_path, strip_ansi, Group};
use crate::session::{RunArgs, RunTask};
use crate::setup::StepError;
use crate::stream::KiroPhase;
use fancy_regex::Regex;
use hover_core::json::{self, Json};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The MCP server a session's tool is given for its project's desktop.
pub const SERVER_NAME: &str = "cua-space";

/// What Settings shows beside the switch where Spaces can't run.
pub const UNSUPPORTED: &str = "Agent desktops need macOS 26 or later on Apple silicon.";

/// The largest app that goes into a desktop (its files added up).
pub const APP_LIMIT: u64 = 4 << 30;

/// Computer use inside the Space, and its files; never its shell (the agent has its own) or
/// Spaces' admin tools.
pub const PERMISSIONS: &str = "computer:screenshot,computer:click,computer:type,computer:key,computer:scroll,computer:drag,computer:hotkey,computer:window,computer:accessibility,computer:clipboard";

// MARK: Whether and which

/// What the settings say, read whenever it matters: the switch, and the image a new Space
/// starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Switches { pub on: bool, pub linux: bool }

type Source = Arc<dyn Fn() -> Switches + Send + Sync>;
static SOURCE: Mutex<Option<Source>> = Mutex::new(None);

/// Where the switches come from: the backend hands in a reader of its settings
/// (`Settings::agent_spaces` and `space_image`). Off, on the macOS image, until it does.
pub fn set_source(f: impl Fn() -> Switches + Send + Sync + 'static) { *SOURCE.lock().unwrap() = Some(Arc::new(f)); }

fn switches() -> Switches {
    let f = SOURCE.lock().unwrap().clone();
    f.map_or_else(Switches::default, |f| f())
}

/// Cua Spaces runs its macOS VMs with Apple's virtualization, on macOS 26 or later and Apple
/// silicon only. The decision as a function of what the machine says, so it is tested
/// everywhere.
pub fn supported_on(os: &str, major: u32, arch: &str) -> bool { os == "macos" && major >= 26 && matches!(arch, "aarch64" | "arm64") }

/// The major number of a `sw_vers -productVersion` ("26.0.1"), or 0.
pub fn product_major(text: &str) -> u32 { text.trim().split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0) }

pub fn supported() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| {
        if !cfg!(target_os = "macos") { return false; }
        let (code, text) = run(Path::new("/usr/bin/sw_vers"), Duration::from_secs(10), &["-productVersion"]);
        supported_on(std::env::consts::OS, if code == 0 { product_major(&text) } else { 0 }, std::env::consts::ARCH)
    })
}

/// Why the switch is disabled here, or none.
pub fn note() -> Option<&'static str> { (!supported()).then_some(UNSUPPORTED) }

/// Off until switched on in Settings → Computer Use; then it replaces Cua Driver on the
/// user's own desktop.
pub fn wanted() -> bool { switches().on && supported() }

/// The image a new Space starts from: the macOS VM (two at most on a Mac) or Linux.
pub fn image() -> &'static str { if switches().linux || !cfg!(target_os = "macos") { "linux" } else { "macos:26" } }

fn first_file(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> { paths.into_iter().find(|p| p.is_file()) }

/// The cua CLI, from PATH or where its installer puts it.
pub fn exe() -> Option<PathBuf> {
    on_path("cua").or_else(|| first_file([home().join(".local").join("bin").join("cua"), "/usr/local/bin/cua".into(), "/opt/homebrew/bin/cua".into()]))
}

/// Lume, which runs the macOS VMs: the source of truth for whether one is on, and how it is
/// sized. Cua's own list has no power state for local VMs.
pub fn lume_exe() -> Option<PathBuf> {
    on_path("lume").or_else(|| first_file([
        home().join(".local").join("bin").join("lume"),
        home().join(".local").join("share").join("lume").join("lume.app").join("Contents").join("MacOS").join("lume"),
    ]))
}

// MARK: Names

/// Path.GetFullPath, without the disk: made absolute, `.` and `..` resolved, no trailing
/// separator (except a root's).
fn full_path(folder: &str) -> String {
    use std::path::Component;
    let p = Path::new(folder);
    let abs = if p.has_root() || p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => { if !matches!(out.components().next_back(), None | Some(Component::RootDir | Component::Prefix(_))) { out.pop(); } }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Path.GetFileName: after the last separator.
fn file_name(p: &str) -> &str {
    let cut = if cfg!(windows) { p.rfind(['\\', '/', ':']) } else { p.rfind('/') };
    cut.map_or(p, |i| &p[i + 1..])
}

/// SHA-256 (FIPS 180-4), for the short hash in a Space's name: the C# hashed with
/// System.Security.Cryptography, and the names of Spaces made by it must come out the same.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() { w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh.wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25)).wrapping_add((e & f) ^ (!e & g)).wrapping_add(K[i]).wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) { *x = x.wrapping_add(y); }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() { out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes()); }
    out
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// The Space a project's agents share: "hover-", the folder's name and a short hash of its
/// full path, so two folders called "app" get two desktops. The same folder, however it is
/// written, gets the same one; paths compare without case except on Linux.
pub fn name_for(folder: &str) -> String {
    let mut full = full_path(folder);
    if !cfg!(target_os = "linux") { full = full.to_lowercase(); }
    let hash = hex(&sha256(full.as_bytes()))[..6].to_owned();
    let mut slug = String::new();
    for c in file_name(&full).to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() { slug.push(c); } else if !slug.ends_with('-') { slug.push('-'); }
    }
    let mut slug = slug.trim_matches('-').to_owned();
    if slug.len() > 20 { slug.truncate(20); slug = slug.trim_end_matches('-').to_owned(); }
    format!("hover-{}{hash}", if slug.is_empty() { String::new() } else { format!("{slug}-") })
}

pub fn id_for(folder: &str) -> String { format!("local:{}", name_for(folder)) }

/// The project's name as the desktop shows it.
pub fn title(folder: &str) -> String {
    let trimmed = folder.trim_end_matches(if cfg!(windows) { &['\\', '/'][..] } else { &['/'][..] });
    match file_name(trimmed) { "" => folder.to_owned(), n => n.to_owned() }
}

/// Whether two folders are one project.
pub fn same_project(a: &str, b: &str) -> bool { name_for(a) == name_for(b) }

// MARK: What Cua and Lume print

/// What a desktop is given, from this Mac's size: enough to be smooth, leaving the user most
/// of their machine. Cua's default (2 cores, 4 GB, 1024×768) is sluggish for macOS 26 and
/// blurry in the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Size { pub cpus: i32, pub memory_gb: i32, pub display: &'static str }

/// The size for a Mac of this much memory (whole GB) and this many cores.
pub fn target_for(total_gb: u64, cores: usize) -> Size {
    Size { cpus: (cores / 3).clamp(2, 6) as i32, memory_gb: if total_gb >= 24 { 8 } else if total_gb >= 16 { 6 } else { 4 }, display: "1024x768" }
}

/// Physical memory in whole GB, or 0 when it can't be read.
fn total_gb() -> u64 {
    static G: OnceLock<u64> = OnceLock::new();
    *G.get_or_init(|| {
        let bytes = if cfg!(target_os = "macos") {
            let (code, text) = run(Path::new("/usr/sbin/sysctl"), Duration::from_secs(10), &["-n", "hw.memsize"]);
            if code == 0 { text.trim().parse::<u64>().unwrap_or(0) } else { 0 }
        } else if cfg!(target_os = "linux") {
            std::fs::read_to_string("/proc/meminfo").ok()
                .and_then(|t| t.lines().find_map(|l| l.strip_prefix("MemTotal:")?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())).map_or(0, |kb| kb << 10)
        } else { 0 };
        bytes >> 30
    })
}

pub fn target() -> Size { target_for(total_gb(), std::thread::available_parallelism().map_or(2, |n| n.get())) }

/// A local VM as Lume sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct VmInfo { pub exists: bool, pub running: bool, pub cpus: i32, pub memory_gb: i32, pub display: Option<String> }

fn string_of<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }

/// `lume get --format json`, read loosely: text before the JSON is skipped, and it may be
/// one object or a list of them.
pub fn parse_vm(text: &str) -> Option<VmInfo> {
    let v = json::parse(&text[text.find(['{', '['])?..]).ok()?;
    let e = match &v { Json::Arr(a) => a.first()?, other => other };
    if !matches!(e, Json::Obj(_)) { return None; }
    let status = string_of(e, "status").unwrap_or("").to_lowercase();
    let cpus = match e.get("cpuCount") { Some(Json::Num(n)) => n.parse::<i32>().unwrap_or(0), _ => 0 };
    let memory_gb = match e.get("memorySize") { Some(Json::Num(n)) => n.parse::<i64>().map_or(0, |m| (m / (1 << 30)) as i32), _ => 0 };
    Some(VmInfo { exists: true, running: matches!(status.as_str(), "running" | "booting" | "starting"), cpus, memory_gb, display: string_of(e, "display").map(str::to_owned) })
}

/// A local VM as Lume sees it (status running/stopped, its size); none when Lume isn't
/// there or doesn't know it.
fn vm(name: &str) -> Option<VmInfo> {
    let lume = lume_exe()?;
    let (code, text) = run(&lume, Duration::from_secs(15), &["get", name, "--format", "json"]);
    if code != 0 { return text.to_lowercase().contains("not found").then_some(VmInfo { exists: false, running: false, cpus: 0, memory_gb: 0, display: None }); }
    parse_vm(&text)
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpaceInfo { pub id: String, pub name: String, pub running: bool, pub os: Option<String> }

/// `cua spaces ls --json`, read loosely: a list, or an object with the list in `spaces`,
/// after whatever notice Cua printed first. A Space without a power state is on.
pub fn parse_list(text: &str) -> Vec<SpaceInfo> {
    let Some(start) = text.find(['[', '{']) else { return vec![] };
    let Ok(v) = json::parse(&text[start..]) else { return vec![] };
    let arr = match &v { Json::Arr(_) => &v, other => match other.get("spaces") { Some(a) => a, None => return vec![] } };
    let Json::Arr(items) = arr else { return vec![] };
    items.iter().map(|e| {
        let id = string_of(e, "id").unwrap_or("").to_owned();
        let name = string_of(e, "name").map_or_else(|| id.rsplit(':').next().unwrap_or("").to_owned(), str::to_owned);
        let power = string_of(e, "power_state").or_else(|| string_of(e, "state")).unwrap_or("running").to_lowercase();
        SpaceInfo { id, name, running: matches!(power.as_str(), "running" | "ready" | "on"), os: string_of(e, "os").map(str::to_owned) }
    }).collect()
}

/// The guest's side of an app sent in: unpack into /Applications (or ~/Applications),
/// replacing an older copy, and open it. Every name is single-quoted, so none is read as
/// shell.
pub fn install_script(guest_zip: &str, app_name: &str) -> String {
    fn q(v: &str) -> String { format!("'{}'", v.replace('\'', "'\\''")) }
    format!("set -e; z={}; a={}; d=/Applications; [ -w \"$d\" ] || {{ d=\"$HOME/Applications\"; mkdir -p \"$d\"; }}; ", q(guest_zip), q(app_name))
        + "rm -rf \"$d/$a\"; /usr/bin/ditto -x -k \"$z\" \"$d\"; rm -f \"$z\"; /usr/bin/open \"$d/$a\""
}

/// One line of progress from `cua` or an installer: what to show, and how far along, when
/// it says.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame { pub line: String, pub fraction: Option<f64> }

static PERCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,3}(?:\.\d+)?)\s?%").unwrap());
static OF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d+)\s+(\d+)\s*$").unwrap());

fn clip(l: &str, n: usize) -> String { if l.chars().count() > n { format!("{}…", l.chars().take(n - 1).collect::<String>()) } else { l.to_owned() } }

/// A line as the tool printed it: the line itself without colour codes (what an error says),
/// and the frame to show. A JSON progress line ({"phase": .., "fraction": ..}), a
/// percentage, or "<sent> <total>" give the fraction. None for a blank one.
pub fn frame_of(raw: &str) -> Option<(String, Frame)> {
    let l = strip_ansi(raw).trim().to_owned();
    if l.is_empty() { return None; }
    let tail = l.clone();
    let (mut line, mut fraction) = (l, None);
    if line.starts_with('{') {
        if let Ok(d) = json::parse(&line) {
            if let Some(Json::Num(n)) = d.get("fraction") { fraction = n.parse::<f64>().ok(); }
            let phase = string_of(&d, "phase");
            line = match phase {
                Some("pulling") => "Downloading the desktop image…".into(), Some("creating") => "Making the desktop…".into(), Some("booting") => "Starting it up…".into(),
                Some("waiting_for_services" | "connecting") => "Almost ready…".into(), Some("ready") => "Ready.".into(), Some(p) => p.into(), None => line,
            };
        }
    } else if let Ok(Some(m)) = PERCENT.captures(&line) {
        fraction = m.get(1).and_then(|p| p.as_str().parse::<f64>().ok()).map(|p| p / 100.0);
    } else if let Ok(Some(o)) = OF.captures(&line) {
        let n = |i| o.get(i).and_then(|g| g.as_str().parse::<f64>().ok());
        if let (Some(sent), Some(total)) = (n(1), n(2)) { if total > 0.0 { fraction = Some(sent / total); } }
    }
    Some((tail, Frame { line: clip(&line, 140), fraction }))
}

/// The last non-empty line of some output, cut at 200.
fn last_line(text: &str) -> Option<String> {
    text.replace('\r', "").split('\n').map(str::trim).rfind(|l| !l.is_empty()).map(|l| clip(l, 200))
}

/// The address of a Space's viewer in what `cua sb view` printed: http(s), anything up to
/// `/viewer/#` and something after it, none of it blank or quoted.
pub fn viewer_url(text: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find("http") {
        let start = from + at;
        from = start + 4;
        let rest = &text[start..];
        let scheme = if rest.starts_with("https://") { 8 } else if rest.starts_with("http://") { 7 } else { continue };
        let run: &str = &rest[scheme..];
        let end = run.find(|c: char| c.is_whitespace() || c == '"' || c == '\'').unwrap_or(run.len());
        let run = &run[..end];
        if run.match_indices("/viewer/#").any(|(i, m)| i >= 1 && run.len() > i + m.len()) { return Some(rest[..scheme + end].to_owned()); }
    }
    None
}

/// Where a viewer's address points: its scheme, host and port, and the rest of it after the
/// authority. None for one with a login in it or a port that isn't one.
pub fn split_url(url: &str) -> Option<(&str, String, u16, &str)> {
    let (scheme, after) = url.split_once("://")?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(end);
    if authority.is_empty() || authority.contains('@') { return None; }
    let default = if scheme.eq_ignore_ascii_case("https") { 443 } else { 80 };
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, p) = v6.split_once(']')?;
        (format!("[{h}]"), if p.is_empty() { default } else { p.strip_prefix(':')?.parse().ok()? })
    } else {
        match authority.rsplit_once(':') { Some((h, p)) => (h.to_owned(), p.parse().ok()?), None => (authority.to_owned(), default) }
    };
    Some((scheme, host.to_lowercase(), port, rest))
}

/// Whether a viewer at this host is on this Mac or its own network: loopback, or a private
/// address of the kind the VM has.
pub fn local_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]") || host.parse::<std::net::Ipv4Addr>().is_ok_and(|a| a.octets()[0] == 10 || a.octets()[..2] == [192, 168])
}

// MARK: Status and setup

/// Installed, ready (the image is on the Mac and Spaces answer), and what to do.
#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub installed: bool, pub ready: bool, pub version: Option<String>, pub hint: String, pub running: i32 }

/// What a setup is doing ("installing" or "preparing"), its newest line and how far along,
/// and why it stopped if it failed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Progress { pub step: Option<String>, pub line: String, pub fraction: Option<f64>, pub error: Option<String> }

struct Shared { progress: Progress, known: Option<(Instant, Status)>, setup: Option<Cancel> }

static SHARED: Mutex<Shared> = Mutex::new(Shared { progress: Progress { step: None, line: String::new(), fraction: None, error: None }, known: None, setup: None });

type Listener = Arc<dyn Fn() + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Called on the thread that made the change, whenever the status, a setup's progress or a
/// desktop's state changes: a listener must only post, never block.
pub fn on_change(f: impl Fn() + Send + Sync + 'static) { LISTENERS.lock().unwrap().push(Arc::new(f)); }

fn changed() {
    let all: Vec<Listener> = LISTENERS.lock().unwrap().clone();
    for f in all { f(); }
}

// Progress comes many times a second while an image downloads; the office is told at most
// four times a second, which is all a progress bar needs.
static PENDING: AtomicBool = AtomicBool::new(false);

fn notify() {
    if PENDING.swap(true, Ordering::SeqCst) { return; }
    let _ = std::thread::Builder::new().name("spaces-notify".into()).spawn(|| {
        std::thread::sleep(Duration::from_millis(250));
        PENDING.store(false, Ordering::SeqCst);
        changed();
    });
}

pub fn setup() -> Progress { SHARED.lock().unwrap().progress.clone() }
pub fn known() -> Option<Status> { SHARED.lock().unwrap().known.as_ref().map(|k| k.1.clone()) }
pub fn busy() -> bool { SHARED.lock().unwrap().setup.is_some() }
pub fn cancel() { if let Some(c) = SHARED.lock().unwrap().setup.as_ref() { c.cancel(); } }

fn report(p: Progress) { SHARED.lock().unwrap().progress = p; changed(); }

/// Hover remembers the image it prepared; Cua's cache keeps the image itself.
fn marker() -> PathBuf { hover_core::paths::support().join("spaces").join(format!("prepared-{}", image().replace(':', "-"))) }
fn image_pulled() -> bool { marker().is_file() }

/// Installed, ready and what to do, from `cua`'s own commands. Kept for a minute. Blocks:
/// call it off the UI thread.
pub fn check(fresh: bool) -> Status {
    if !fresh {
        if let Some((at, s)) = &SHARED.lock().unwrap().known { if at.elapsed() < Duration::from_secs(60) { return s.clone(); } }
    }
    let s = look();
    SHARED.lock().unwrap().known = Some((Instant::now(), s.clone()));
    changed();
    s
}

fn look() -> Status {
    let no = |hint: &str| Status { installed: false, ready: false, version: None, hint: hint.into(), running: 0 };
    if !supported() { return no(UNSUPPORTED); }
    let Some(cua) = exe() else { return no("Set up Cua’s desktop tools to give each agent a desktop of its own.") };
    let (vc, vt) = run(&cua, Duration::from_secs(20), &["--version"]);
    let version = (vc == 0).then(|| Regex::new(r"\d+\.\d+\.\d+").unwrap().find(&vt).ok().flatten().map(|m| m.as_str().to_owned()).unwrap_or_default());
    let (lc, lt) = run(&cua, Duration::from_secs(30), &["spaces", "ls", "--json"]);
    let list = if lc == 0 { parse_list(&lt) } else { vec![] };
    let ready = lc == 0 && image_pulled();
    let hint = if lc != 0 { format!("Cua Spaces isn’t answering: {}.", last_line(&lt).unwrap_or_else(|| "run cua doctor".into())) }
        else if ready { String::new() } else { "Prepare the desktop image once (a one-time download).".into() };
    // A hover- Space by its id (`local:hover-…`): Cua's name for a VM can be the guest's hostname.
    let mine = list.iter().filter(|x| x.running && (x.name.starts_with("hover-") || x.id.strip_prefix("local:").is_some_and(|n| n.starts_with("hover-")))).count();
    Status { installed: true, ready, version, hint, running: mine as i32 }
}

/// One click: installs Cua's CLI if missing (Cua's own installer, no sign-in), sets up the
/// runtime the image needs (Lume for macOS VMs), and makes and deletes one Space so the image
/// is on the Mac before the first task. Blocks until done; it runs only when the user asks.
pub fn run_setup() {
    let ct = Cancel::new();
    {
        let mut s = SHARED.lock().unwrap();
        if s.setup.is_some() { return; }
        s.setup = Some(ct.clone());
    }
    match setup_steps(&ct) {
        Ok(()) | Err(StepError::Cancelled) => report(Progress::default()),
        Err(StepError::Failed(m)) => report(Progress { error: Some(m), ..Default::default() }),
    }
    SHARED.lock().unwrap().setup = None;
    check(true);
}

fn setup_steps(ct: &Cancel) -> Result<(), StepError> {
    if !supported() { return Err(StepError::Failed(UNSUPPORTED.into())); }
    let installing = |f: Frame| report(Progress { step: Some("installing".into()), line: f.line, fraction: f.fraction, error: None });
    if exe().is_none() {
        // Only Cua's command-line tool: the desktops live in Hover, never in an app of Cua's.
        report(Progress { step: Some("installing".into()), line: "Installing Cua’s desktop tools…".into(), ..Default::default() });
        stream(Path::new("/bin/bash"), &["-c", "set -o pipefail; curl -fsSL https://cua.ai/install.sh | sh -s -- --cli-only --yes --no-onboarding"],
            Duration::from_secs(15 * 60), ct, &installing)?;
        if exe().is_none() { return Err(StepError::Failed("The installer finished, but cua still isn’t found.".into())); }
    }
    let cua = exe().unwrap();
    if image().starts_with("macos") {
        report(Progress { step: Some("installing".into()), line: "Setting up the macOS desktop runtime (Lume)…".into(), ..Default::default() });
        stream(&cua, &["runtime", "setup", "lume"], Duration::from_secs(15 * 60), ct, &installing)?;
    }
    report(Progress { step: Some("preparing".into()), line: "Downloading the desktop image (one time; macOS is about 23 GB)…".into(), fraction: Some(0.0), error: None });
    let probe = "hover-prepare";
    stream(&cua, &["spaces", "create", image(), "--name", probe, "--json"], Duration::from_secs(60 * 60), ct,
        &|f| report(Progress { step: Some("preparing".into()), line: f.line, fraction: f.fraction, error: None }))?;
    run(&cua, Duration::from_secs(2 * 60), &["spaces", "delete", &format!("local:{probe}"), "--force"]);
    let m = marker();
    if let Some(dir) = m.parent() { let _ = std::fs::create_dir_all(dir); }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = std::fs::write(&m, now.to_string());
    Ok(())
}

// MARK: One Space per project

/// What a project's Space is doing, for its desks: "creating", "starting", "ready", "stopped"
/// or "failed" with a reason.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceState { pub phase: String, pub line: String, pub fraction: Option<f64>, pub error: Option<String> }

impl SpaceState {
    fn new(phase: &str, line: &str, fraction: Option<f64>, error: Option<&str>) -> SpaceState {
        SpaceState { phase: phase.into(), line: line.into(), fraction, error: error.map(str::to_owned) }
    }
}

static STATES: LazyLock<Mutex<HashMap<String, SpaceState>>> = LazyLock::new(Default::default);
static GATES: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Default::default);
// Each Space by name, and the folder it is for (the bridge only knows the name).
static FOLDERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
// Each desktop's viewer link, kept while its ticket lasts: opening the panel again (or
// another agent's of the same project) shows the same live view, not a reload.
static VIEWERS: LazyLock<Mutex<HashMap<String, (String, Instant)>>> = LazyLock::new(Default::default);
static HOMES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static FORWARDS: LazyLock<Mutex<HashMap<String, u16>>> = LazyLock::new(Default::default);

pub fn state_of(folder: &str) -> Option<SpaceState> { STATES.lock().unwrap().get(&name_for(folder)).cloned() }

fn set(folder: &str, s: SpaceState) {
    let name = name_for(folder);
    let phase_changed = STATES.lock().unwrap().insert(name, s.clone()).is_none_or(|was| was.phase != s.phase);
    // A new phase at once; progress within one, a few times a second.
    if phase_changed { changed(); } else { notify(); }
}

fn explain(error: &str) -> String {
    let e = error.to_lowercase();
    // A Mac runs two macOS VMs at most: say so plainly.
    if e.contains("limit") { "This Mac already runs two macOS desktops (Apple’s limit). Remove the agents of another project to free one.".into() }
    else if e.contains("insufficient") || e.contains("memory") { "There isn’t enough free memory or disk for the desktop right now.".into() }
    else { error.to_owned() }
}

/// The project's Space, made or started before an agent's run, with progress. Two agents
/// starting together wait for one create. None when it is ready; else why not (the run then
/// goes on without a desktop).
pub fn ensure(folder: &str, ct: &Cancel) -> Option<String> {
    let cua = match exe() { Some(c) if wanted() => c, _ => return Some("Agent desktops are off.".into()) };
    let name = name_for(folder);
    FOLDERS.lock().unwrap().insert(name.clone(), folder.to_owned());
    let gate = GATES.lock().unwrap().entry(name.clone()).or_default().clone();
    // Waited for as WaitAsync(ct) did: a Stop while another agent of the project makes the
    // Space (up to an hour on the first image) is heard. The state is that agent's, so a
    // stop here leaves it alone; only this call's own create clears it.
    let _held = loop {
        match gate.try_lock() {
            Ok(g) => break g,
            Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if ct.is_cancelled() => return Some("Stopped.".to_owned()),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    if ct.is_cancelled() { return Some("Stopped.".to_owned()); }
    let stopped = || { STATES.lock().unwrap().remove(&name_for(folder)); changed(); Some("Stopped.".to_owned()) };
    let size = target();
    let ready = || set(folder, SpaceState::new("ready", "The project’s desktop is ready.", Some(1.0), None));
    // Lume knows whether the VM is on; Cua's list only knows it was made.
    let machine = vm(&name);
    let id = format!("local:{name}");
    let (lc, lt) = run(&cua, Duration::from_secs(30), &["spaces", "ls", "--json"]);
    let known = lc == 0 && parse_list(&lt).iter().any(|x| x.id == id || x.name == name);
    if machine.as_ref().is_some_and(|v| v.exists && v.running) || machine.is_none() && known { ready(); return None; }
    if machine.as_ref().is_some_and(|v| v.exists) || known {
        // Off: sized up first if Cua made it small (only while it is off).
        if let (Some(_), Some(lume)) = (machine.as_ref().filter(|v| v.exists && (v.cpus < size.cpus || v.memory_gb < size.memory_gb)), lume_exe()) {
            set(folder, SpaceState::new("starting", "Giving the desktop more room…", None, None));
            let (zc, zt) = run(&lume, Duration::from_secs(2 * 60), &["set", &name, "--cpu", &size.cpus.to_string(), "--memory", &format!("{}GB", size.memory_gb)]);
            if zc != 0 { hover_core::log::line(&format!("spaces: couldn't size {name}: {}", last_line(&zt).unwrap_or_default())); }
        }
        set(folder, SpaceState::new("starting", "Starting the project’s desktop…", None, None));
        let (sc, st) = run(&cua, Duration::from_secs(4 * 60), &["spaces", "start", &id, "--json"]);
        if sc != 0 {
            let why = explain(&last_line(&st).unwrap_or_else(|| "The desktop didn’t start.".into()));
            set(folder, SpaceState::new("failed", "", None, Some(&why)));
            return Some(why);
        }
        ready();
        return None;
    }
    set(folder, SpaceState::new("creating", "Making the project’s desktop…", Some(0.0), None));
    let (cpus, mb) = (size.cpus.to_string(), (size.memory_gb * 1024).to_string());
    let timeout = Duration::from_secs(if image_pulled() { 10 * 60 } else { 60 * 60 });
    match stream(&cua, &["spaces", "create", image(), "--name", &name, "--cpus", &cpus, "--memory-mb", &mb, "--json"], timeout, ct,
        &|f| set(folder, SpaceState::new("creating", &f.line, f.fraction, None))) {
        Ok(()) => { ready(); None }
        Err(StepError::Cancelled) => stopped(),
        Err(StepError::Failed(m)) => {
            let why = explain(&m);
            set(folder, SpaceState::new("failed", "", None, Some(&why)));
            Some(why)
        }
    }
}

/// Off when no agent of the project is left in the office; deleted with the project's last
/// session (the backend decides when). Blocks.
pub fn stop(folder: &str) {
    let Some(cua) = exe().filter(|_| wanted()) else { return };
    VIEWERS.lock().unwrap().remove(&name_for(folder));
    run(&cua, Duration::from_secs(2 * 60), &["spaces", "stop", &id_for(folder)]);
    set(folder, SpaceState::new("stopped", "The desktop is off. The project’s next task starts it again.", None, None));
}

/// The project's Space and everything in it, gone. Blocks.
pub fn delete(folder: &str) {
    let Some(cua) = exe().filter(|_| supported()) else { return };
    run(&cua, Duration::from_secs(3 * 60), &["spaces", "delete", &id_for(folder), "--force"]);
    VIEWERS.lock().unwrap().remove(&name_for(folder));
    STATES.lock().unwrap().remove(&name_for(folder));
    changed();
}

/// A session's run, with its project's desktop made or started first: Starting is reported
/// while that goes on, and a desktop that can't be had lets the run go on without one.
pub fn around(inner: RunTask) -> RunTask {
    Arc::new(move |a: RunArgs| {
        if wanted() {
            (a.progress)(KiroPhase::Starting);
            if let Some(why) = ensure(&a.folder, &a.ct) { hover_core::log::line(&format!("spaces: {}: {why}", a.folder)); }
        }
        inner(a)
    })
}

// MARK: The viewer

fn err(message: &str) -> Json { Json::obj(vec![("error", Json::str(message))]) }

/// The live viewer of the project's Space: Cua's own HTML5 viewer, interactive (the user can
/// step in), with dropped files going to the Space's Downloads. Blocks; the answer is the
/// Screen panel's: `{phase, url}` when it is ready, `{phase, line, fraction, error}` while it
/// isn't, `{error}` when there is none.
pub fn viewer(folder: &str) -> Json {
    let Some(cua) = exe() else { return err("Cua’s desktop tools aren’t installed.") };
    let name = name_for(folder);
    let cached = VIEWERS.lock().unwrap().get(&name).filter(|(_, until)| *until > Instant::now()).map(|(url, _)| url.clone());
    if let Some(url) = cached.filter(|_| state_of(folder).is_some_and(|s| s.phase == "ready")) {
        return Json::obj(vec![("phase", Json::str("ready")), ("url", Json::str(url))]);
    }
    // On first: the viewer of a desktop that is off never loads.
    if !state_of(folder).is_some_and(|s| s.phase == "creating" || s.phase == "starting") {
        if let Some(why) = ensure(folder, &Cancel::new()) { return Json::obj(vec![("phase", Json::str("failed")), ("error", Json::str(why))]); }
    }
    if let Some(st) = state_of(folder).filter(|s| s.phase != "ready") {
        return Json::obj(vec![("phase", Json::str(st.phase)), ("line", Json::str(st.line)),
            ("fraction", st.fraction.map_or(Json::Null, Json::double)), ("error", Json::opt_str_of(st.error.as_deref()))]);
    }
    let (code, text) = run(&cua, Duration::from_secs(45), &["sb", "view", &id_for(folder), "--no-open", "--ttl", "12h"]);
    let Some(url) = viewer_url(&text).filter(|_| code == 0) else { return err(&last_line(&text).unwrap_or_else(|| "The desktop’s viewer didn’t open.".into())) };
    let Some((scheme, host, port, rest)) = split_url(&url).filter(|(_, h, _, _)| local_host(h)) else { return err("The viewer isn’t on this Mac.") };
    // On the VM's own address the page isn't a secure context, and the viewer falls back to
    // PNG frames (no video, no audio, no clipboard); through localhost it streams.
    let url = match host.as_str() {
        "127.0.0.1" | "localhost" | "[::1]" => url,
        _ => match local_forward(&host, port) { Some(p) => format!("{scheme}://127.0.0.1:{p}{rest}"), None => url },
    };
    VIEWERS.lock().unwrap().insert(name, (url.clone(), Instant::now() + Duration::from_secs(11 * 3600)));
    Json::obj(vec![("phase", Json::str("ready")), ("url", Json::str(url))])
}

/// One loopback port per desktop's viewer, joined byte for byte to the VM's (HTTP and its
/// WebSocket alike); only this Mac can reach it, and the viewer still needs its ticket.
fn local_forward(host: &str, port: u16) -> Option<u16> {
    let key = format!("{host}:{port}");
    let mut forwards = FORWARDS.lock().unwrap();
    if let Some(p) = forwards.get(&key) { return Some(*p); }
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(e) => { hover_core::log::line(&format!("viewer forward: {e}")); return None; }
    };
    let local = listener.local_addr().ok()?.port();
    let target = (host.to_owned(), port);
    let _ = std::thread::Builder::new().name("viewer-forward".into()).spawn(move || {
        for client in listener.incoming().flatten() {
            let target = target.clone();
            let _ = std::thread::Builder::new().name("viewer-join".into()).spawn(move || {
                let Some(addr) = target.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return };
                let Ok(vm) = TcpStream::connect_timeout(&addr, Duration::from_secs(10)) else { return };
                let _ = (client.set_nodelay(true), vm.set_nodelay(true));
                join(client, vm);
            });
        }
    });
    forwards.insert(key, local);
    Some(local)
}

/// Bytes both ways until either side is done, then both are closed.
fn join(a: TcpStream, b: TcpStream) {
    let (Ok(mut a2), Ok(mut b2)) = (a.try_clone(), b.try_clone()) else { return };
    let back = std::thread::spawn(move || {
        let _ = std::io::copy(&mut b2, &mut a2);
        let _ = (a2.shutdown(Shutdown::Both), b2.shutdown(Shutdown::Both));
    });
    let (mut a, mut b) = (a, b);
    let _ = std::io::copy(&mut a, &mut b);
    let _ = (a.shutdown(Shutdown::Both), b.shutdown(Shutdown::Both));
    let _ = back.join();
}

// MARK: Apps and files sent in

/// Every file under a folder, not following links (and, if asked, not the hidden ones).
fn walk(root: &Path, skip_hidden: bool) -> Vec<PathBuf> {
    let (mut files, mut todo) = (vec![], vec![root.to_path_buf()]);
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if skip_hidden && e.file_name().to_string_lossy().starts_with('.') { continue; }
            match e.file_type() {
                Ok(t) if t.is_symlink() => {}
                Ok(t) if t.is_dir() => todo.push(path),
                Ok(_) => files.push(path),
                Err(_) => {}
            }
        }
    }
    files
}

/// The guest's home (`$HOME` there), asked once per Space.
fn home_of(cua: &Path, id: &str) -> Option<String> {
    if let Some(h) = HOMES.lock().unwrap().get(id) { return Some(h.clone()); }
    let (code, text) = run(cua, Duration::from_secs(30), &["sb", "exec", id, "echo $HOME"]);
    let home = if code == 0 { text.replace('\r', "").split('\n').map(str::trim).rfind(|l| l.starts_with('/')).map(str::to_owned) } else { None };
    if let Some(h) = &home { HOMES.lock().unwrap().insert(id.to_owned(), h.clone()); }
    home
}

/// An app dragged onto the notch, onto a project's desktop: Hover copies the app itself into
/// the desktop (a zip of the bundle, through `cua sb cp`) and opens it there. Only the app
/// goes, never its data or the user's sign-ins. (Cua's teleport, which moves sessions, ships
/// only in Cua's own app, which Hover doesn't use.) `progress` is told each step. The answer
/// is `{ok, app}` or `{error}`.
pub fn send_app(folder: &str, app_path: &str, progress: &dyn Fn(&str)) -> Json {
    let Some(cua) = exe() else { return err("Cua’s desktop tools aren’t installed.") };
    let app = app_path.trim_end_matches(['/', '\\']);
    // From the page: only a whole path, so nothing can reach ditto or cua as an option.
    if !Path::new(app).is_absolute() || !app.to_lowercase().ends_with(".app") || !Path::new(app).is_dir() || app.contains('\0') { return err("That isn’t an app Hover can send."); }
    let name = file_name(app).to_owned();
    let mut size = 0u64;
    for f in walk(Path::new(app), false) {
        size += std::fs::metadata(&f).map_or(0, |m| m.len());
        if size > APP_LIMIT { return err(&format!("{name} is over 4 GB, too big to copy into the desktop.")); }
    }
    progress("Starting the desktop…");
    if let Some(why) = ensure(folder, &Cancel::new()) { return err(&why); }
    let id = id_for(folder);
    let Some(home) = home_of(&cua, &id) else { return err("The desktop didn’t answer.") };
    let mut rnd = [0u8; 4];
    // Two apps sent at once must not share a zip.
    if getrandom::fill(&mut rnd).is_err() { return err("The app didn’t go."); }
    let zip = std::env::temp_dir().join(format!("hover-app-{}.zip", hex(&rnd)));
    let out = send_app_zip(&cua, &id, &home, app, &name, &zip, size, progress);
    let _ = std::fs::remove_file(&zip);
    out
}

#[allow(clippy::too_many_arguments)]
fn send_app_zip(cua: &Path, id: &str, home: &str, app: &str, name: &str, zip: &Path, size: u64, progress: &dyn Fn(&str)) -> Json {
    progress(&format!("Packing {name}…"));
    // No extended attributes: a quarantine flag would stop it opening there.
    let (zc, zt) = run(Path::new("/usr/bin/ditto"), Duration::from_secs(10 * 60), &["-c", "-k", "--keepParent", "--norsrc", "--noextattr", app, &zip.to_string_lossy()]);
    if zc != 0 { return err(&last_line(&zt).unwrap_or_else(|| format!("{name} couldn’t be packed."))); }
    progress(&format!("Copying {name} ({} MB)…", size / (1 << 20)));
    let guest_zip = format!("{home}/Downloads/.hover-{}", zip.file_name().unwrap_or_default().to_string_lossy());
    let (cc, ct) = run(cua, Duration::from_secs(20 * 60), &["sb", "cp", &zip.to_string_lossy(), &format!("{id}:{guest_zip}")]);
    if cc != 0 { return err(&last_line(&ct).unwrap_or_else(|| format!("{name} couldn’t be copied."))); }
    progress(&format!("Opening {name}…"));
    let (ec, et) = run(cua, Duration::from_secs(5 * 60), &["sb", "exec", id, &install_script(&guest_zip, name)]);
    if ec != 0 {
        return err(&if et.contains("-10825") { format!("{name} needs a newer macOS than the desktop runs.") }
            else { last_line(&et).unwrap_or_else(|| format!("{name} didn’t open in the desktop.")) });
    }
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Json::obj(vec![("ok", Json::Bool(true)), ("app", Json::str(stem))])
}

/// Files dropped on a project's desktop in the notch: to its Downloads, through `cua sb cp`
/// (folders file by file, at most 20 dropped items and 500 files). The answer is
/// `{ok, sent}` or `{error}` (with how many had gone, when some did).
pub fn send_files(folder: &str, paths: &[String]) -> Json {
    let Some(cua) = exe() else { return err("Cua’s desktop tools aren’t installed.") };
    if let Some(why) = ensure(folder, &Cancel::new()) { return err(&why); }
    let id = id_for(folder);
    let Some(home) = home_of(&cua, &id) else { return err("The desktop didn’t answer.") };
    let mut files: Vec<(PathBuf, String)> = vec![];
    for p in paths.iter().take(20) {
        let path = Path::new(p);
        // From the page: only whole paths, so none reaches cua as an option.
        if !path.is_absolute() { continue; }
        if path.is_file() {
            files.push((path.to_path_buf(), path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        } else if path.is_dir() {
            let trimmed = Path::new(p.trim_end_matches(['/', '\\']));
            let root = trimmed.parent().unwrap_or(path);
            for f in walk(path, true).into_iter().take(500usize.saturating_sub(files.len())) {
                let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
                files.push((f, rel));
            }
        }
    }
    let mut sent = 0;
    for (from, to) in &files {
        let (code, text) = run(&cua, Duration::from_secs(5 * 60), &["sb", "cp", &from.to_string_lossy(), &format!("{id}:{home}/Downloads/{to}")]);
        if code != 0 { return Json::obj(vec![("error", Json::str(last_line(&text).unwrap_or_else(|| "The files didn’t go.".into()))), ("sent", Json::int(sent))]); }
        sent += 1;
    }
    Json::obj(vec![("ok", Json::Bool(true)), ("sent", Json::int(sent))])
}

// MARK: The session's computer-use server

/// The MCP server a session's tool gets: Hover's relay (as for its browser) to a `cua mcp`
/// Hover runs for the project's Space, outside the agents' sandbox. Every agent in that
/// folder gets the same Space, each with its own cursor in it. None when desktops are off
/// or the tool can't reach Hover's socket (not a Unix).
pub fn servers(folder: &str) -> Vec<McpServer> {
    if !wanted() || folder.is_empty() || exe().is_none() { return vec![]; }
    let name = name_for(folder);
    FOLDERS.lock().unwrap().insert(name.clone(), folder.to_owned());
    browser::bridge(&format!("space:{name}"), SERVER_NAME, Arc::new(serve))
}

/// `cua mcp` for one agent's connection: the agent's lines go to it, what it prints goes
/// back as it comes, and it ends with the connection.
fn serve(tag: &str, mut from_agent: Box<dyn BufRead + Send>, mut to_agent: Box<dyn Write + Send>) {
    const LINE_LIMIT: u64 = 4 * 1024 * 1024;
    let Some(name) = tag.strip_prefix("space:") else { return };
    let Some(folder) = FOLDERS.lock().unwrap().get(name).cloned() else { return };
    if let Some(why) = ensure(&folder, &Cancel::new()) { hover_core::log::line(&format!("spaces: {folder}: {why}")); }
    let Some(cua) = exe() else { return };
    let mut cmd = hidden(&cua, &["mcp", "--sandbox", &id_for(&folder), "--permissions", PERMISSIONS]);
    cmd.env("CUA_TELEMETRY", "0");
    let group = match Group::spawn(cmd) {
        Ok(g) => g,
        Err(e) => { hover_core::log::line(&format!("spaces: couldn’t start cua mcp - {e}")); return; }
    };
    let (stdin, stdout, stderr) = group.take_pipes();
    if let Some(err) = stderr {
        let _ = std::thread::Builder::new().name("cua-mcp-stderr".into()).spawn(move || {
            for l in std::io::BufReader::new(err).lines().map_while(Result::ok) { hover_core::log::line(&format!("cua mcp: {l}")); }
        });
    }
    // The agent's side: a line at a time, as MCP's stdio frames it. It ends with the
    // agent's connection (a reader waiting on it is not waited for here).
    let _ = std::thread::Builder::new().name("cua-mcp-in".into()).spawn(move || {
        let Some(mut stdin) = stdin else { return };
        let mut line = Vec::new();
        loop {
            line.clear();
            match (&mut from_agent).take(LINE_LIMIT + 1).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                // As the browser's bridge: no line of MCP is this long; the agent is cut off.
                Ok(_) if line.len() as u64 > LINE_LIMIT => break,
                Ok(_) => {
                    if line.last() != Some(&b'\n') { line.push(b'\n'); }
                    if stdin.write_all(&line).and_then(|_| stdin.flush()).is_err() { break; }
                }
            }
        }
    });
    if let Some(mut out) = stdout {
        let mut buf = [0u8; 16384];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 || to_agent.write_all(&buf[..n]).and_then(|_| to_agent.flush()).is_err() { break; }
        }
    }
    group.kill();
}

// MARK: Running cua

/// `exe args`, with no stdin, and what it printed (both pipes, without colour codes) when it
/// ends; (-1, why) when it couldn't start or took longer than `timeout`.
pub fn run(exe: &Path, timeout: Duration, args: &[&str]) -> (i32, String) {
    let mut cmd = hidden(exe, args);
    cmd.env("CUA_TELEMETRY", "0");
    let g = match Group::spawn(cmd) { Ok(g) => g, Err(e) => return (-1, e.to_string()) };
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    let read = |p: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(mut p) = p { let _ = p.read_to_end(&mut s); }
        String::from_utf8_lossy(&s).into_owned()
    });
    let o = read(stdout.map(|p| Box::new(p) as Box<dyn Read + Send>));
    let e = read(stderr.map(|p| Box::new(p) as Box<dyn Read + Send>));
    match g.wait_timeout(timeout) {
        None => { g.kill(); (-1, "Timed out.".into()) }
        Some(code) => {
            // Ended on its own: what it started (Lume's VM or daemon) is left running.
            g.release();
            (code, strip_ansi(&format!("{}\n{}", o.join().unwrap_or_default(), e.join().unwrap_or_default())))
        }
    }
}

/// `exe args` with a line at a time of what it prints handed on as a frame; ends it (and
/// whatever it started) when `ct` is cancelled or the time is up. Fails with its last line.
fn stream(exe: &Path, args: &[&str], timeout: Duration, ct: &Cancel, frame: &dyn Fn(Frame)) -> Result<(), StepError> {
    let mut cmd = hidden(exe, args);
    cmd.env("CUA_TELEMETRY", "0").env("CUA_INSTALL_NONINTERACTIVE", "1").env("NONINTERACTIVE", "1");
    let g = Group::spawn(cmd).map_err(|e| StepError::Failed(e.to_string()))?;
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    let (tx, rx) = mpsc::channel::<String>();
    fn pump(p: Option<impl Read + Send + 'static>, tx: mpsc::Sender<String>) {
        if let Some(p) = p {
            std::thread::spawn(move || {
                let mut r = std::io::BufReader::new(p);
                let mut buf = Vec::new();
                while let Ok(n) = r.read_until(b'\n', &mut buf) {
                    if n == 0 { break; }
                    // A carriage return redraws a progress line: each piece is a line.
                    for piece in String::from_utf8_lossy(&buf).split(['\n', '\r']) { let _ = tx.send(piece.to_owned()); }
                    buf.clear();
                }
            });
        }
    }
    pump(stdout, tx.clone());
    pump(stderr, tx);
    let tail = std::cell::RefCell::new(String::new());
    let take = |raw: String| {
        if let Some((last, f)) = frame_of(&raw) { *tail.borrow_mut() = last; frame(f); }
    };
    let began = Instant::now();
    let code = loop {
        while let Ok(l) = rx.try_recv() { take(l); }
        if ct.is_cancelled() { g.kill(); return Err(StepError::Cancelled); }
        if let Some(c) = g.wait_timeout(Duration::from_millis(50)) { break c; }
        if began.elapsed() >= timeout { g.kill(); return Err(StepError::Failed("It took too long and was stopped.".into())); }
    };
    // It ended on its own: what it started (a VM) is Cua's to keep, as the C# left it.
    g.release();
    // What it printed last, still on its way through the pipes.
    let until = Instant::now() + Duration::from_millis(500);
    while let Ok(l) = rx.recv_timeout(Duration::from_millis(100)) { take(l); if Instant::now() > until { break; } }
    if code != 0 {
        let tail = tail.borrow();
        return Err(StepError::Failed(if tail.is_empty() { format!("exit code {code}") } else { tail.clone() }));
    }
    Ok(())
}
