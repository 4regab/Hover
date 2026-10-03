//! Whether a newer release of each tool is out, and its one-click update with the tool's
//! own updater. Read where each maker publishes it, read-only and without the user's
//! sign-in: Kiro's release manifest, the npm registry (Codex's ACP adapter, OpenCode,
//! Claude Code), the build Cursor's installer downloads, and `cua-driver check-update`.
//! Asked at most every six hours, all at once on one short-lived thread per tool; a check
//! that fails shows nothing (no badge, no error). What is kept is a few short strings per
//! tool. Nothing here blocks the caller but `check` and `update`, which the backend runs
//! on threads of its own.

use hover_agents::{agents, computer_use, spaces};
use hover_core::json::{self, Json};
use hover_core::model::AgentTool;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Cua Driver's id here, beside the agents' own.
pub const CUA_DRIVER: &str = "cua-driver";

/// Every id that can have an update: the agents', then Cua Driver.
pub fn ids() -> impl Iterator<Item = &'static str> { AgentTool::ALL.iter().map(|t| t.id()).chain([CUA_DRIVER]) }

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Info { pub installed: Option<String>, pub latest: Option<String>, pub available: bool }

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Progress { pub busy: bool, pub line: String, pub error: Option<String> }

#[derive(Default)]
struct Shared { known: HashMap<&'static str, (Instant, Info)>, doing: HashMap<&'static str, Progress>, checking: bool }

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Default::default);
// The vendors' updaters and npm write shared folders (~/.local/bin, an npm prefix): one at a time.
static GATE: Mutex<()> = Mutex::new(());

type Listener = Arc<dyn Fn() + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Called on the thread that made the change; a listener must only post, never block.
pub fn on_change(f: impl Fn() + Send + Sync + 'static) { LISTENERS.lock().unwrap().push(Arc::new(f)); }

fn changed() {
    let all: Vec<Listener> = LISTENERS.lock().unwrap().clone();
    for f in all { f(); }
}

pub fn of(id: &str) -> Option<Info> { SHARED.lock().unwrap().known.get(id).map(|k| k.1.clone()) }
pub fn doing(id: &str) -> Progress { SHARED.lock().unwrap().doing.get(id).cloned().unwrap_or_default() }

/// What the office and Settings are told of a tool, or Null when there is nothing to say
/// (no newer release, nothing going on).
pub fn state(id: &str) -> Json {
    let (u, p) = (of(id), doing(id));
    if !u.as_ref().is_some_and(|u| u.available) && !p.busy && p.error.is_none() { return Json::Null; }
    Json::obj(vec![
        ("installed", Json::opt_str_of(u.as_ref().and_then(|u| u.installed.as_deref()))),
        ("latest", Json::opt_str_of(u.as_ref().and_then(|u| u.latest.as_deref()))),
        ("available", Json::Bool(u.is_some_and(|u| u.available))),
        ("busy", Json::Bool(p.busy)), ("line", Json::str(p.line)), ("error", Json::opt_str_of(p.error.as_deref())),
    ])
}

/// Every installed tool's latest release, kept six hours; one check at a time, each tool
/// on its own thread (most of the wait is the network). Blocks until done.
pub fn check(fresh: bool) {
    let due: Vec<&'static str> = {
        let mut s = SHARED.lock().unwrap();
        if s.checking { return; }
        let due: Vec<_> = ids().filter(|id| fresh || s.known.get(id).is_none_or(|k| k.0.elapsed() > Duration::from_secs(6 * 3600))).collect();
        if due.is_empty() { return; }
        s.checking = true;
        due
    };
    let threads: Vec<_> = due.into_iter().map(|id| std::thread::Builder::new().name("update-check".into()).spawn(move || (id, look(id)))).filter_map(Result::ok).collect();
    let found: Vec<_> = threads.into_iter().filter_map(|t| t.join().ok()).collect();
    {
        let mut s = SHARED.lock().unwrap();
        for (id, info) in found { s.known.insert(id, (Instant::now(), info)); }
        s.checking = false;
    }
    changed();
}

fn look(id: &str) -> Info {
    if id == CUA_DRIVER {
        let Some(driver) = computer_use::exe() else { return Info::default() };
        let (code, text) = spaces::run(&driver, Duration::from_secs(30), &["check-update", "--json"]);
        return if code == 0 { parse_driver_check(&text) } else { Info::default() };
    }
    let Some(t) = AgentTool::parse(Some(id)) else { return Info::default() };
    let Some(exe) = agents::exe(t) else { return Info::default() };
    let Some(installed) = installed_version(t, &exe) else { return Info::default() };
    let latest = latest_version(t);
    let available = latest.as_deref().is_some_and(|l| newer(t, l, &installed));
    Info { installed: Some(installed), latest, available }
}

fn installed_version(t: AgentTool, exe: &Path) -> Option<String> {
    if t == AgentTool::Codex {
        // The adapter's package.json, beside the script its bin links to.
        let pkg = codex_package(exe)?;
        let v = json::parse(&std::fs::read_to_string(pkg.join("package.json")).ok()?).ok()?;
        return v.get("version").and_then(Json::as_str).map(str::to_owned);
    }
    let (code, text) = agents::ask(exe, &["--version"]);
    if code == 0 { parse_version(t, &text) } else { None }
}

/// The installed version from a tool's `--version`: "kiro-cli 2.27.0", Cursor's
/// "2026.10.01-14929f9", OpenCode's "1.18.31", Claude Code's "2.1.3 (Claude Code)".
pub fn parse_version(t: AgentTool, text: &str) -> Option<String> {
    for word in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
        let w = word.trim_start_matches('v');
        let ok = if t == AgentTool::Cursor { cursor_build(w) } else { semver_like(w) };
        if ok { return Some(w.to_owned()); }
    }
    None
}

/// "1.2.3", with a pre-release or build after - or +.
fn semver_like(w: &str) -> bool {
    let core = w.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Cursor's builds: a date and a short hash, "2026.10.01-14929f9".
fn cursor_build(w: &str) -> bool {
    let Some((date, hash)) = w.split_once('-') else { return false };
    let d: Vec<&str> = date.split('.').collect();
    d.len() == 3 && d[0].len() == 4 && d.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && hash.len() >= 5 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Where npm put the adapter: the first folder up from its script with a package.json
/// (<prefix>/lib/node_modules/@agentclientprotocol/codex-acp).
pub fn codex_package(exe: &Path) -> Option<PathBuf> {
    let target = std::fs::canonicalize(exe).ok()?;
    target.ancestors().skip(1).find(|d| d.join("package.json").is_file()).map(Path::to_path_buf)
}

/// The HTTPS client for the makers' release notes: 15 s, statuses read by the caller.
fn http() -> &'static ureq::Agent {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        let cfg = ureq::Agent::config_builder();
        #[cfg(windows)]
        let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
        cfg.timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into()
    });
    &AGENT
}

/// A GET's body when it answered 2xx, at most 2 MB (Cursor's install script is 10 KB).
fn get(url: &str) -> Option<String> {
    let mut res = http().get(url).header("User-Agent", "Hover").call().ok()?;
    if !res.status().is_success() { return None; }
    let mut body = Vec::new();
    res.body_mut().as_reader().take(2 << 20).read_to_end(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

fn npm_latest(package: &str) -> Option<String> {
    let v = json::parse(&get(&format!("https://registry.npmjs.org/{package}/latest"))?).ok()?;
    v.get("version").and_then(Json::as_str).map(str::to_owned)
}

fn latest_version(t: AgentTool) -> Option<String> {
    match t {
        AgentTool::Kiro => json::parse(&get("https://prod.download.cli.kiro.dev/stable/latest/manifest.json")?).ok()?.get("version").and_then(Json::as_str).map(str::to_owned),
        AgentTool::Codex => npm_latest("@agentclientprotocol/codex-acp"),
        AgentTool::OpenCode => npm_latest("opencode-ai"),
        AgentTool::Claude => npm_latest("@anthropic-ai/claude-code"),
        AgentTool::Cursor => parse_cursor_installer(&get("https://cursor.com/install")?),
    }
}

/// The build Cursor's install script downloads (downloads.cursor.com/lab/<build>/…).
pub fn parse_cursor_installer(script: &str) -> Option<String> {
    let at = script.find("downloads.cursor.com/lab/")? + "downloads.cursor.com/lab/".len();
    let build = script[at..].split('/').next()?;
    cursor_build(build).then(|| build.to_owned())
}

pub fn parse_driver_check(text: &str) -> Info {
    let Some(v) = text.find('{').and_then(|i| json::parse(&text[i..]).ok()) else { return Info::default() };
    let s = |n: &str| v.get(n).and_then(Json::as_str).map(str::to_owned);
    Info { installed: s("current_version"), latest: s("latest_version"), available: matches!(v.get("update_available"), Some(Json::Bool(true))) }
}

/// Whether `latest` is a newer release than `installed`. Cursor's builds are a date and a
/// hash: a later day, or the same day's other build (the installer's is the newest).
pub fn newer(t: AgentTool, latest: &str, installed: &str) -> bool {
    if latest == installed { return false; }
    if t == AgentTool::Cursor { return latest.split('-').next() >= installed.split('-').next() && cursor_build(latest); }
    let v = |s: &str| -> Option<Vec<u64>> { s.split(['-', '+']).next()?.split('.').map(|p| p.parse().ok()).collect() };
    match (v(latest), v(installed)) { (Some(l), Some(i)) => l > i, _ => false }
}

/// The tool's own updater, as a bash command (every path quoted).
pub fn command(id: &str, exe: Option<&Path>) -> Option<String> {
    fn q(p: &Path) -> String { format!("'{}'", p.to_string_lossy().replace('\'', "'\\''")) }
    if id == CUA_DRIVER { return Some(format!("{} update --apply", q(&exe.map(Path::to_path_buf).or_else(computer_use::exe)?))); }
    let t = AgentTool::parse(Some(id))?;
    let exe = exe.map(Path::to_path_buf).or_else(|| agents::exe(t))?;
    Some(match t {
        AgentTool::Kiro => format!("{} update --non-interactive", q(&exe)),
        AgentTool::Cursor => format!("{} update", q(&exe)),
        AgentTool::OpenCode => format!("{} upgrade", q(&exe)),
        AgentTool::Claude => format!("{} update", q(&exe)),
        // Into the prefix it is installed in (<prefix>/lib/node_modules/@scope/name), so the
        // copy on PATH is the one updated.
        AgentTool::Codex => {
            let pkg = codex_package(&exe)?;
            let prefix = pkg.ancestors().nth(4)?;
            format!("npm install --global --no-fund --no-audit --prefix {} @agentclientprotocol/codex-acp@latest", q(prefix))
        }
    })
}

fn set(id: &'static str, p: Progress) { SHARED.lock().unwrap().doing.insert(id, p); changed(); }

/// Updates one tool; `busy` says whether a task of it is at work (then it waits for
/// another time). Then the tool is checked again. Blocks for as long as the updater runs
/// (ten minutes at most).
pub fn update(id: &str, busy: impl Fn() -> bool) {
    let Some(id) = ids().find(|i| *i == id) else { return };
    {
        let mut s = SHARED.lock().unwrap();
        if s.doing.get(id).is_some_and(|p| p.busy) { return; }
        s.doing.insert(id, Progress { busy: true, line: "Waiting for another update to finish…".into(), error: None });
    }
    changed();
    let fail = |m: String| set(id, Progress { busy: false, line: String::new(), error: Some(m) });
    if busy() { return fail("A task of this tool is at work. Update it when the task is done.".into()); }
    let Some(command) = command(id, None) else { return fail("Hover doesn’t know how this copy was installed. Update it the way you installed it.".into()) };
    let (code, text) = {
        let _one = GATE.lock().unwrap_or_else(|p| p.into_inner());
        set(id, Progress { busy: true, line: "Updating…".into(), error: None });
        spaces::run(Path::new("/bin/bash"), Duration::from_secs(10 * 60), &["-c", &format!("set -o pipefail; {command}")])
    };
    if code != 0 {
        let last = text.lines().map(str::trim).rfind(|l| !l.is_empty()).map(|l| l.chars().take(160).collect::<String>());
        return fail(format!("The update stopped: {}", last.unwrap_or_else(|| format!("exit code {code}"))));
    }
    let info = look(id);
    SHARED.lock().unwrap().known.insert(id, (Instant::now(), info));
    if let Some(t) = AgentTool::parse(Some(id)) { agents::check(t, true); }
    if id == CUA_DRIVER { computer_use::check(true); }
    set(id, Progress::default());
}
