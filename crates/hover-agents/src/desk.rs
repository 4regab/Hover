//! Owl/DeskInfo.cs: what the desk card shows of one session, as T3 Code's right panel
//! does: its commands and their output (Terminal), the pages it opened (Browser), its
//! folder's files, the working tree's diff, the branch's pull request and the ones it
//! linked, its subagents, and the apps its computer use opened (Screen). Plain structs,
//! for any UI to draw; no UI in here.
//!
//! What comes from the session's steps (`Snap::of`) is read on the caller's thread,
//! where the session changes. git and gh run off it, in `Desk`'s methods, which block:
//! call them from a thread of your own. They start as hidden children with an argument
//! list (never a shell), a timeout and a cap on what is read; git runs with optional
//! locks off, so a status never takes the index lock from an agent that is working.
//! Nothing here writes to the folder except `Desk::create_pr`, which the user asks for.
//!
//! What a file request may read is only inside the session's folder, links followed.

use crate::github::{run, GitHubCli, Ran};
use crate::proc::on_path;
use crate::session::KiroSession;
use fancy_regex::Regex;
use hover_core::json::{self, Json};
use hover_core::model::KiroStep;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MS: fn(u64) -> Duration = Duration::from_millis;

fn re(p: &str) -> Regex { Regex::new(p).unwrap() }

// MARK: The session, as the panels read it

/// One step of a session: KiroStep, with the call's raw input and a longer end of what
/// it printed, which the panels use. The state message leaves them out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeskStep {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub target: Option<String>,
    pub status: String,
    pub added: i32,
    pub removed: i32,
    pub diff: Option<String>,
    pub output: Option<String>,
    pub exit: Option<i32>,
    pub ms: Option<f64>,
    /// The call's raw input as JSON (`{"command":...}`); None where the session didn't keep it.
    pub input: Option<String>,
    /// A longer end of what it printed or returned; `output` stands in while None.
    pub log: Option<String>,
}

impl DeskStep {
    /// Log, else Output: what the step printed.
    fn text(&self) -> Option<&str> { self.log.as_deref().or(self.output.as_deref()) }
    fn is(&self, kinds: &[&str]) -> bool { kinds.contains(&self.kind.as_str()) }
}

impl From<&KiroStep> for DeskStep {
    fn from(x: &KiroStep) -> DeskStep {
        DeskStep {
            id: x.id.clone(), kind: x.kind.clone(), title: x.title.clone(), target: x.target.clone(), status: x.status.clone(),
            added: x.added, removed: x.removed, diff: x.diff.clone(), output: x.output.clone(), exit: x.exit, ms: x.ms,
            input: x.input.clone(), log: x.log.clone(),
        }
    }
}

/// One step with the turn it was in.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub turn: usize,
    pub step: DeskStep,
}

/// A session as the panels need it, copied on the caller's thread (DeskInfo.Snap).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snap {
    /// The session's lasting key (history, and `note_app`'s registry).
    pub key: String,
    pub folder: String,
    pub busy: bool,
    /// The turn that runs or ran last (not a queued reply); its steps are "now".
    pub current: Option<usize>,
    pub steps: Vec<Item>,
    /// Every turn's prompt and then its answer (empty if none yet), oldest first.
    pub texts: Vec<String>,
    /// A Kiro Web session: the GitHub repos it was given. Its work is in Kiro's cloud, so
    /// this computer's folder says nothing about it; its pull request is the way to its changes.
    pub cloud: Option<Vec<String>>,
}

impl Snap {
    /// DeskInfo.Take. Needs the whole session (`KiroSessions::get`), not `all_light`,
    /// which drops the steps' output.
    pub fn of(s: &KiroSession) -> Snap {
        Snap {
            key: s.key.clone(),
            folder: s.folder.clone(),
            busy: s.busy(),
            current: s.turns.iter().rposition(|t| !t.queued),
            steps: s.turns.iter().enumerate().flat_map(|(i, t)| t.steps.iter().map(move |x| Item { turn: i, step: x.into() })).collect(),
            texts: s.turns.iter().flat_map(|t| [t.prompt.clone(), t.result.as_ref().map(|r| r.text.clone()).unwrap_or_default()]).collect(),
            cloud: s.cloud.clone(),
        }
    }

    fn now(&self) -> impl Iterator<Item = &DeskStep> {
        self.steps.iter().filter(|i| Some(i.turn) == self.current).map(|i| &i.step)
    }

    fn last3(&self) -> Vec<&DeskStep> {
        let all: Vec<_> = self.now().collect();
        all[all.len().saturating_sub(3)..].to_vec()
    }

    /// DeskInfo.Testing: the agent is testing on the screen now (it runs, and computer
    /// use is among its last few steps). The screen panel then shows the screen live.
    pub fn testing(&self) -> bool { self.busy && self.last3().into_iter().any(is_screen) }

    /// DeskInfo.Browsing: the agent is using Hover's browser now.
    pub fn browsing(&self) -> bool { self.busy && self.last3().into_iter().any(|x| browser_op(&x.title).is_some()) }
}

// MARK: Screen (computer use)

static SCREEN_TOOL: LazyLock<Regex> = LazyLock::new(|| re(
    r"(?:^|[\s/.:_-])(screenshot|double_click|right_click|left_click|click|type_text|press_key|hotkey|scroll|drag|move_(?:mouse|cursor)|launch_app|open_app|list_apps|list_windows|get_window_state|get_screen_size)(?:$|[\s(:])"));
static BROWSER_OP: LazyLock<Regex> = LazyLock::new(|| re(
    r"(?:^|[^a-z])browser_(open|snapshot|click|type|press|scroll|screenshot|evaluate|wait|console|back|reload)(?:$|[^a-z_])"));
static PID_FIELD: LazyLock<Regex> = LazyLock::new(|| re(r#""pid"\s*:\s*(\d{1,7})"#));

/// The name of Hover's own browser's MCP server.
pub const BROWSER_SERVER: &str = "hover-browser";

/// BrowserTool.Op: which of Hover's browser tools a step's title names (browser_click → "click").
pub fn browser_op(title: &str) -> Option<String> {
    BROWSER_OP.captures(title).ok().flatten().map(|m| m[1].to_lowercase())
}

/// DeskInfo.IsScreen: a computer-use call: anything through Cua Driver, or a tool named
/// like one of its actions (the tools title MCP calls differently: "cua-driver/click",
/// "mcp__cua-driver__click", "click").
pub fn is_screen(x: &DeskStep) -> bool {
    // Hover's own browser (browser_click, browser_type…) is a page, not the screen.
    if browser_op(&x.title).is_some() || x.title.to_lowercase().contains(BROWSER_SERVER) { return false; }
    let title = x.title.to_lowercase();
    let input = x.input.as_deref().unwrap_or("").to_lowercase();
    if title.contains("cua") || title.contains("computer use") || title.contains("computer_use") || input.contains("cua-driver") { return true; }
    x.is(&["other", "execute"]) && SCREEN_TOOL.is_match(&title).unwrap_or(false)
}

/// The apps a session's computer use opened or acted on: their process ids, bundle ids
/// and names. The screen panel shows only these over the desktop, never the user's own
/// windows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentApps {
    pub pids: Vec<u32>,
    pub bundles: Vec<String>,
    pub names: Vec<String>,
}

const APPS_KEPT: usize = 16;

fn push_new<T: PartialEq>(list: &mut Vec<T>, v: T) { if !list.contains(&v) { list.push(v); } }
fn last_n<T: Clone>(list: &[T], n: usize) -> Vec<T> { list[list.len().saturating_sub(n)..].to_vec() }

impl AgentApps {
    fn add_pid(&mut self, pid: u32) { if pid > 1 { push_new(&mut self.pids, pid); } }
    fn add_bundle(&mut self, b: &str) { if !b.is_empty() && b.len() < 200 { push_new(&mut self.bundles, b.to_owned()); } }
    fn add_name(&mut self, n: &str) {
        if (1..80).contains(&n.chars().count()) && !self.names.iter().any(|x| x.eq_ignore_ascii_case(n)) { self.names.push(n.to_owned()); }
    }
    fn trimmed(mut self) -> Option<AgentApps> {
        self.pids = last_n(&self.pids, APPS_KEPT);
        self.bundles = last_n(&self.bundles, APPS_KEPT);
        self.names = last_n(&self.names, APPS_KEPT);
        (self.pids.len() + self.bundles.len() + self.names.len() > 0).then_some(self)
    }
}

/// DeskInfo.Apps: from a session's steps (the last 200 computer-use calls): process ids
/// from their inputs and what launch_app answered, bundle ids and names.
pub fn apps_of_steps<'a>(all: impl IntoIterator<Item = &'a DeskStep>) -> Option<AgentApps> {
    let screen: Vec<&DeskStep> = all.into_iter().filter(|x| is_screen(x)).collect();
    let screen = &screen[screen.len().saturating_sub(200)..];
    if screen.is_empty() { return None; }
    let mut a = AgentApps::default();
    for x in screen {
        for text in [x.input.as_deref(), x.text()].into_iter().flatten().filter(|t| !t.is_empty()) {
            for m in PID_FIELD.captures_iter(text).flatten() {
                if let Ok(pid) = m[1].parse::<u32>() { a.add_pid(pid); }
            }
        }
        if let Some(b) = field(x.input.as_deref(), &["bundle_id", "bundleId", "bundle_identifier"]) { a.add_bundle(&b); }
        // launch_app names its app; other calls name the one they act on.
        if let Some(n) = field(x.input.as_deref(), &["app_name", "appName", "application", "app", "name"]) { a.add_name(&n); }
    }
    a.trimmed()
}

/// What the integrations tell Hover of the apps a session's computer use opened, by the
/// session's key: the steps don't always say (a launch's process id is in the answer, which
/// the session may not keep whole).
static NOTED: Mutex<Vec<(String, AgentApps)>> = Mutex::new(Vec::new());
const NOTED_SESSIONS: usize = 64;

fn noted(key: &str, f: impl FnOnce(&mut AgentApps)) {
    if key.is_empty() { return; }
    let mut all = NOTED.lock().unwrap();
    let at = match all.iter().position(|(k, _)| k == key) {
        Some(i) => i,
        None => {
            if all.len() >= NOTED_SESSIONS { all.remove(0); }
            all.push((key.to_owned(), AgentApps::default()));
            all.len() - 1
        }
    };
    let a = &mut all[at].1;
    f(a);
    // However long a session runs, what is kept of it is bounded.
    a.pids = last_n(&a.pids, 64);
    a.bundles = last_n(&a.bundles, 64);
    a.names = last_n(&a.names, 64);
}

/// Computer use opened or acted on an app of this session: its process id and name.
pub fn note_app(session_key: &str, pid: u32, name: &str) {
    noted(session_key, |a| { a.add_pid(pid); a.add_name(name.trim()); });
}

/// … and its bundle id (macOS).
pub fn note_bundle(session_key: &str, bundle_id: &str) {
    noted(session_key, |a| a.add_bundle(bundle_id.trim()));
}

/// What `note_app` and `note_bundle` were told of a session.
pub fn noted_apps(session_key: &str) -> Option<AgentApps> {
    NOTED.lock().unwrap().iter().find(|(k, _)| k == session_key).and_then(|(_, a)| a.clone().trimmed())
}

/// The session is deleted or its apps are closed: nothing more to show.
pub fn forget_apps(session_key: &str) {
    NOTED.lock().unwrap().retain(|(k, _)| k != session_key);
}

/// The apps of a session: those in its steps, and those the integrations noted.
pub fn apps(snap: &Snap) -> Option<AgentApps> {
    let mut a = apps_of_steps(snap.steps.iter().map(|i| &i.step)).unwrap_or_default();
    if let Some(n) = noted_apps(&snap.key) {
        for p in n.pids { a.add_pid(p); }
        for b in &n.bundles { a.add_bundle(b); }
        for x in &n.names { a.add_name(x); }
    }
    a.trimmed()
}

// MARK: Reading a step's input

/// The first of these string fields in a step's input JSON (DeskInfo.Field).
pub fn field(json: Option<&str>, names: &[&str]) -> Option<String> {
    let text = json.filter(|t| t.starts_with('{'))?;
    let v = json::parse(text).ok()?;
    names.iter().find_map(|n| v.get(n).and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_owned))
}

fn chars(s: &str) -> usize { s.chars().count() }
fn head(s: &str, n: usize) -> &str { s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i]) }
fn tail(s: &str, n: usize) -> &str {
    let total = chars(s);
    if total <= n { s } else { s.char_indices().nth(total - n).map_or(s, |(i, _)| &s[i..]) }
}

/// The first line with something on it, cut at 200 (DeskInfo.Line).
fn line(text: &str) -> Option<String> {
    let l = text.replace('\r', "").split('\n').map(str::trim).find(|l| !l.is_empty())?.to_owned();
    Some(if chars(&l) > 200 { format!("{}…", head(&l, 199)) } else { l })
}

/// A thousands-separated number (toLocaleString).
pub fn num(n: i64) -> String {
    let d = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

// MARK: Terminal

/// What one command of the Terminal panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct TermCommand {
    pub id: String,
    pub turn: usize,
    pub cmd: String,
    /// "in_progress", "completed" or "failed".
    pub status: String,
    pub exit: Option<i32>,
    pub ms: Option<f64>,
    pub out: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terminal {
    /// Oldest first; the last 80.
    pub commands: Vec<TermCommand>,
}

const TERMINAL_BUDGET: usize = 400 * 1024;

/// DeskInfo.Terminal: the commands the session ran (not computer use) and their output,
/// up to a budget of text: older output is cut first when it is all long.
pub fn terminal(s: &Snap) -> Terminal {
    let runs: Vec<&Item> = s.steps.iter().filter(|i| i.step.kind == "execute" && !is_screen(&i.step)).collect();
    let runs = &runs[runs.len().saturating_sub(80)..];
    let mut budget = TERMINAL_BUDGET;
    let mut rows = vec![];
    for it in runs.iter().rev() {
        let x = &it.step;
        let mut text = x.text().unwrap_or("");
        let len = chars(text);
        if len > budget {
            text = if budget > 2000 { tail(text, budget) } else if len > 2000 { tail(text, 2000) } else { text };
        }
        budget = budget.saturating_sub(chars(text));
        rows.push(TermCommand { id: x.id.clone(), turn: it.turn, cmd: command_of(x), status: x.status.clone(), exit: x.exit, ms: x.ms, out: text.to_owned() });
    }
    rows.reverse();
    Terminal { commands: rows }
}

/// DeskInfo.CommandOf: the command line a step ran: the input's command (a list in Codex:
/// ["bash", "-lc", "…"]), or its target, or its title.
pub fn command_of(x: &DeskStep) -> String {
    if let Some(v) = x.input.as_deref().and_then(|t| json::parse(t).ok()) {
        match v.get("command") {
            Some(Json::Str(c)) => return c.clone(),
            Some(Json::Arr(list)) => {
                let parts: Vec<&str> = list.iter().filter_map(Json::as_str).collect();
                // "bash -lc <script>" is the script.
                if parts.len() == 3 && matches!(parts[1], "-lc" | "-c") { return parts[2].to_owned(); }
                if !parts.is_empty() { return parts.join(" "); }
            }
            _ => {}
        }
    }
    x.target.clone().unwrap_or_else(|| x.title.clone())
}

// MARK: Subagents

pub(crate) const AGENT_KEYS: [&str; 5] = ["subagent_type", "subagent", "agent_type", "agent_name", "agentName"];
static AGENT_TITLE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(sub-?agents?|use_subagent|spawn_agent|delegat(e|ing))\b"));

/// DeskInfo.IsSubagent: a call that hands work to a subagent: Claude's and OpenCode's
/// task tool (kind "agent", or a subagent_type in the input), Codex's spawn_agent, Kiro's
/// subagent tool.
pub fn is_subagent(x: &DeskStep) -> bool {
    if x.kind == "agent" || field(x.input.as_deref(), &AGENT_KEYS).is_some() { return true; }
    x.is(&["other", "think"]) && AGENT_TITLE.is_match(&x.title).unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Subagent {
    pub id: String,
    pub turn: usize,
    pub name: String,
    pub task: String,
    pub prompt: Option<String>,
    pub status: String,
    pub ms: Option<f64>,
    pub out: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Subagents {
    /// Oldest first; the last 40.
    pub agents: Vec<Subagent>,
    pub running: usize,
}

pub fn subagents(s: &Snap) -> Subagents {
    let all: Vec<&Item> = s.steps.iter().filter(|i| is_subagent(&i.step)).collect();
    let agents: Vec<Subagent> = all[all.len().saturating_sub(40)..].iter().map(|i| {
        let x = &i.step;
        Subagent {
            id: x.id.clone(), turn: i.turn,
            name: field(x.input.as_deref(), &AGENT_KEYS).unwrap_or_else(|| "Subagent".into()),
            task: field(x.input.as_deref(), &["description"]).unwrap_or_else(|| x.title.clone()),
            prompt: field(x.input.as_deref(), &["prompt", "message", "task", "query", "instructions"]),
            status: x.status.clone(), ms: x.ms, out: x.text().map(str::to_owned),
        }
    }).collect();
    let running = agents.iter().filter(|a| a.status == "in_progress").count();
    Subagents { agents, running }
}

// MARK: Browser

static URL: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)https?://[^\s"'<>()\[\]{}`\\]+"#));
static LOCAL_URL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^https?://(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(:\d+)?(/|$)"));

/// An http(s) address as its host (with any port) and what follows it, or None when it
/// isn't one.
fn split_url(url: &str) -> Option<(&str, &str)> {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") { &url[8..] } else if lower.starts_with("http://") { &url[7..] } else { return None };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or("");
    (!host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']' | '%'))).then_some((host, &rest[end..]))
}

/// DeskInfo.Urls: the http(s) addresses in a text.
pub fn urls(text: Option<&str>) -> Vec<String> {
    let Some(t) = text.filter(|t| !t.is_empty()) else { return vec![] };
    URL.find_iter(t).flatten()
        .map(|m| m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']).to_owned())
        .filter(|u| split_url(u).is_some())
        .collect()
}

pub fn is_local(url: &str) -> bool { LOCAL_URL.is_match(url).unwrap_or(false) }

/// host + path, as the Browser tile and the page chips label an address.
pub fn label(url: &str) -> String {
    let Some((host, rest)) = split_url(url) else { return url.to_owned() };
    let path = rest.split(['?', '#']).next().unwrap_or("");
    format!("{host}{}", if path == "/" { "" } else { path })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind { Server, Fetch, Opened, Screen }

impl PageKind {
    /// desk.js's words.
    pub fn name(self) -> &'static str {
        match self { PageKind::Server => "server", PageKind::Fetch => "fetch", PageKind::Opened => "opened", PageKind::Screen => "screen" }
    }
    /// The chip's tooltip prefix.
    pub fn label(self) -> &'static str {
        match self { PageKind::Server => "Local server", PageKind::Fetch => "Fetched", PageKind::Opened => "Opened", PageKind::Screen => "On screen" }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub url: String,
    pub kind: PageKind,
    pub local: bool,
    pub title: Option<String>,
    pub status: String,
    pub turn: usize,
}

/// A dev server listens on 0.0.0.0 or [::1]; the page it serves is at localhost.
fn localise(u: &str) -> String {
    match LOCAL_URL.find(u).ok().flatten() {
        Some(m) => format!("{}{}", m.as_str().replace("0.0.0.0", "localhost").replace("[::1]", "localhost"), &u[m.end()..]),
        None => u.to_owned(),
    }
}

/// DeskInfo.Pages: the pages the agent opened (its fetches, and URLs it handed a
/// browser or a computer-use action) and the local servers its commands started,
/// newest first.
pub fn pages(s: &Snap) -> Vec<Page> {
    // In the order last seen.
    let mut seen: Vec<Page> = vec![];
    for it in &s.steps {
        let x = &it.step;
        let mut found: Vec<(String, PageKind)> = vec![];
        if x.kind == "fetch" {
            for u in urls(x.target.as_deref()).into_iter().chain(urls(field(x.input.as_deref(), &["url"]).as_deref())) { found.push((u, PageKind::Fetch)); }
        } else if let Some(opened) = field(x.input.as_deref(), &["url", "href"]) {
            for u in urls(Some(&opened)) { found.push((u, if is_screen(x) { PageKind::Screen } else { PageKind::Opened })); }
        }
        // A dev server says where it listens; only local addresses count from output.
        if x.kind == "execute" {
            for u in urls(x.text()) { if is_local(&u) { found.push((u, PageKind::Server)); } }
        }
        for (u, kind) in found {
            let url = localise(&u);
            seen.retain(|p| p.url != url);
            seen.push(Page { local: is_local(&url), title: (kind == PageKind::Fetch).then(|| x.title.clone()), status: x.status.clone(), turn: it.turn, url, kind });
        }
    }
    seen.reverse();
    seen.truncate(40);
    seen
}

// MARK: Status and diffs from git's text

/// One line of `git status`: a path relative to the folder, and what happened to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    /// M, A, D, R, or ? for untracked.
    pub status: char,
    pub old: Option<String>,
}

fn strip<'a>(path: &'a str, prefix: &str) -> &'a str {
    if !prefix.is_empty() { path.strip_prefix(prefix).unwrap_or(path) } else { path }
}

/// DeskInfo.ParseStatus: `git status --porcelain=v1 -z`, with the folder's place in the
/// repository ("src/app/") taken off each path.
pub fn parse_status(z: &str, prefix: &str) -> Vec<Change> {
    let mut list = vec![];
    let parts: Vec<&str> = z.split('\0').collect();
    let mut i = 0;
    while i < parts.len() {
        let p = parts[i];
        i += 1;
        let b = p.as_bytes();
        if b.len() < 4 { continue; }
        let (x, y) = (b[0] as char, b[1] as char);
        let Some(path) = p.get(3..) else { continue };
        let mut old = None;
        // A rename or copy is followed by the path it came from.
        if matches!(x, 'R' | 'C') && i < parts.len() { old = Some(parts[i]); i += 1; }
        let status = if x == '?' { '?' } else if matches!(x, 'R' | 'C') { 'R' } else if x == 'A' || y == 'A' { 'A' } else if x == 'D' || y == 'D' { 'D' } else { 'M' };
        list.push(Change { path: strip(path, prefix).to_owned(), status, old: old.map(|o| strip(o, prefix).to_owned()) });
    }
    list
}

/// One file of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old: Option<String>,
    pub status: char,
    pub add: i32,
    pub del: i32,
    pub binary: bool,
    /// Its hunks, from the first @@ on.
    pub patch: String,
}

static DIFF_HEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^diff --git a/(.*) b/(.*)$"));

/// DeskInfo.ParseDiff: a unified diff (git diff) as one entry per file: its path, what
/// happened to it, lines added and removed, and its hunks.
pub fn parse_diff(patch: &str) -> Vec<FileDiff> {
    struct Cur { path: String, old: Option<String>, status: Option<char>, binary: bool, add: i32, del: i32, body: String }
    let mut files = vec![];
    let mut cur: Option<Cur> = None;
    let flush = |cur: &mut Option<Cur>, files: &mut Vec<FileDiff>| {
        if let Some(c) = cur.take() {
            let old = c.old.filter(|o| *o != c.path);
            files.push(FileDiff { path: c.path, old, status: c.status.unwrap_or('M'), add: c.add, del: c.del, binary: c.binary, patch: c.body.trim_end_matches('\n').to_owned() });
        }
    };
    let mut in_hunk = false;
    for raw in patch.replace("\r\n", "\n").split('\n') {
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            flush(&mut cur, &mut files);
            in_hunk = false;
            // "diff --git a/x b/x": the b side, until ---/+++ or a rename says better.
            let (old, path) = match DIFF_HEAD.captures(raw).ok().flatten() {
                Some(m) => (Some(m[1].to_owned()), m[2].to_owned()),
                None => (None, rest.to_owned()),
            };
            cur = Some(Cur { path, old, status: None, binary: false, add: 0, del: 0, body: String::new() });
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        if !in_hunk {
            if raw.starts_with("new file") { c.status = Some('A'); }
            else if raw.starts_with("deleted file") { c.status = Some('D'); }
            else if let Some(o) = raw.strip_prefix("rename from ") { c.old = Some(o.to_owned()); c.status = Some('R'); }
            else if let Some(n) = raw.strip_prefix("rename to ") { c.path = n.to_owned(); }
            else if raw.starts_with("Binary files") || raw.starts_with("GIT binary patch") { c.binary = true; }
            else if raw.starts_with("+++ ") && raw != "+++ /dev/null" { c.path = raw.strip_prefix("+++ b/").unwrap_or(&raw[4..]).to_owned(); }
            else if raw.starts_with("@@") { in_hunk = true; c.body.push_str(raw); c.body.push('\n'); }
            continue;
        }
        if raw.starts_with('+') { c.add += 1; } else if raw.starts_with('-') { c.del += 1; }
        c.body.push_str(raw);
        c.body.push('\n');
    }
    flush(&mut cur, &mut files);
    files
}

/// DeskInfo.Slug: a branch name's part from a title.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !out.is_empty() { out.push('-'); }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
    }
    let cut = head(&out, 40).trim_end_matches('-').to_owned();
    if cut.is_empty() {
        // "changes-" and the month, day, hour and minute.
        let t = hover_core::time::local_compact();
        return format!("changes-{}-{}", t.get(4..8).unwrap_or("0000"), t.get(9..13).unwrap_or("0000"));
    }
    cut
}

/// DeskInfo.ValidRef: a branch name git takes, and nothing that could be read as an option.
pub fn valid_ref(name: &str) -> bool {
    (1..200).contains(&name.len()) && !name.starts_with('-') && !name.contains("..") && !name.ends_with('/') && !name.ends_with(".lock")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

/// DeskInfo.GhReason: why gh has no pull request to show, in a sentence.
pub fn gh_reason(err: &str) -> String {
    let lower = err.to_lowercase();
    if lower.contains("no pull requests found") { return NO_PR.into(); }
    if lower.contains("gh auth login") || lower.contains("not logged") || lower.contains("authentication") { return "Sign in to GitHub to see pull requests.".into(); }
    if lower.contains("not a git repository") { return "Not a Git repository.".into(); }
    if lower.contains("no git remotes") || lower.contains("none of the git remotes") { return "This repository has no GitHub remote.".into(); }
    line(err).unwrap_or_else(|| "gh couldn’t read the pull request.".into())
}

/// gh's answer when the branch has no pull request (the Create pull request form's cue).
pub const NO_PR: &str = "This branch has no pull request yet.";

/// A Kiro Web session that hasn't said where its pull request is.
pub const CLOUD_NO_PR: &str = "This Kiro Web session hasn’t opened a pull request yet.";

// MARK: Paths

/// A path that names a place from the root, however the system writes it.
fn rooted(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || p.starts_with('\\') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// DeskInfo.Relative: a step's target relative to the folder, with forward slashes.
/// None when it is outside it.
pub fn relative(target: Option<&str>, folder: &str) -> Option<String> {
    let target = target.map(str::trim).filter(|t| !t.is_empty())?;
    let mut t = target.replace('\\', "/");
    let f = folder.trim_end_matches(['/', '\\']).replace('\\', "/");
    let cut = f.len() + 1;
    if t.len() >= cut && t.is_char_boundary(cut) && t[..f.len()].eq_ignore_ascii_case(&f) && t.as_bytes()[f.len()] == b'/' {
        t = t[cut..].to_owned();
    } else if rooted(target) {
        return None;
    }
    if let Some(r) = t.strip_prefix("./") { t = r.to_owned(); }
    (!t.is_empty() && !t.split('/').any(|p| p == "..")).then_some(t)
}

/// The path without Windows' `\\?\` prefix, which read_link may add.
fn plain_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{r}")); }
    if let Some(r) = s.strip_prefix(r"\\?\") { return PathBuf::from(r); }
    p
}

/// A path with its `.` and `..` resolved by the text alone.
fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => { if !out.pop() { out.push(".."); } }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// DeskInfo.Real: the path with every link on it followed (realpath), as far as it exists.
pub fn real(path: &Path) -> PathBuf {
    let full = clean(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
    let mut cur = PathBuf::new();
    for c in full.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => cur.push(c.as_os_str()),
            Component::Normal(part) => {
                let mut next = cur.join(part);
                for _ in 0..32 {
                    let is_link = std::fs::symlink_metadata(&next).is_ok_and(|m| m.file_type().is_symlink());
                    if !is_link { break; }
                    let Ok(target) = std::fs::read_link(&next) else { break };
                    let target = plain_path(target);
                    next = clean(&if target.is_absolute() { target } else { next.parent().unwrap_or(&cur).join(target) });
                }
                cur = next;
            }
            _ => {}
        }
    }
    cur
}

fn same_part(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if cfg!(target_os = "linux") { a == b } else { a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() }
}

/// Whether `p` is strictly below `root`.
fn below(root: &Path, p: &Path) -> bool {
    let (r, q): (Vec<_>, Vec<_>) = (root.components().collect(), p.components().collect());
    q.len() > r.len() && r.iter().zip(&q).all(|(a, b)| same_part(a.as_os_str(), b.as_os_str()))
}

/// DeskInfo.Inside: the full path of `rel` inside `folder`, or None when it would be
/// outside it: no "..", no rooted path, and no link that leads out of it.
pub fn inside(folder: &str, rel: Option<&str>) -> Option<PathBuf> {
    let rel = rel.filter(|r| !r.trim().is_empty())?;
    if !crate::usable_folder(Some(folder)) { return None; }
    let r = rel.replace('\\', "/");
    if r.contains('\0') || r.starts_with('/') || rooted(rel) || r.split('/').any(|p| p == ".." || p.contains(':')) { return None; }
    let root = real(Path::new(folder));
    let real_path = real(&root.join(r.split('/').filter(|p| !p.is_empty() && *p != ".").collect::<PathBuf>()));
    below(&root, &real_path).then_some(real_path)
}

// MARK: One file

pub const FILE_LIMIT: usize = 512 * 1024;

/// A file of the Files panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileView {
    Text { path: String, text: String, truncated: bool, size: u64 },
    Binary { path: String, size: u64 },
    Error { path: String, error: String },
}

fn has_nul(bytes: &[u8]) -> bool { bytes[..bytes.len().min(8000)].contains(&0) }

/// DeskInfo.FileText: a file in the session's folder, for the files panel: its text (up
/// to FILE_LIMIT), or that it is binary. Only inside the folder.
pub fn file_text(folder: &str, rel: Option<&str>) -> FileView {
    use std::io::Read;
    let path = rel.unwrap_or("").to_owned();
    let err = |e: &str| FileView::Error { path: path.clone(), error: e.to_owned() };
    let Some(full) = inside(folder, rel) else { return err("That file isn’t in the session’s folder.") };
    let Ok(meta) = std::fs::metadata(&full) else { return err("That file isn’t there any more.") };
    if !meta.is_file() { return err("That file isn’t there any more."); }
    let mut buf = vec![];
    let read = std::fs::File::open(&full).and_then(|f| f.take(FILE_LIMIT as u64).read_to_end(&mut buf));
    if let Err(e) = read { return err(&e.to_string()); }
    if has_nul(&buf) { return FileView::Binary { path, size: meta.len() }; }
    FileView::Text { path, text: String::from_utf8_lossy(&buf).into_owned(), truncated: meta.len() > FILE_LIMIT as u64, size: meta.len() }
}

// MARK: What the panels hold

/// Whether the folder is in a Git work tree, its branch, and where the folder is in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repo {
    pub git: bool,
    pub branch: Option<String>,
    /// Where the folder is in the repository ("src/app/"), or "" at its top.
    pub prefix: String,
    /// The repository has a commit.
    pub head: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: char,
    pub old: Option<String>,
    pub add: i32,
    pub del: i32,
}

/// What the agent did to a file, from its steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    pub path: String,
    pub read: u32,
    pub edit: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Files {
    pub git: bool,
    pub branch: Option<String>,
    pub changed: Vec<ChangedFile>,
    /// In the order first touched.
    pub touched: Vec<Touched>,
    /// Paths relative to the folder, sorted; at most 5,000.
    pub tree: Vec<String>,
    /// The tree was cut.
    pub more: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    pub git: bool,
    /// Not a Git repository: the files are the parts of each edit the session kept.
    pub partial: bool,
    pub branch: Option<String>,
    /// The diff was longer than is read.
    pub truncated: bool,
    pub error: Option<String>,
    pub files: Vec<FileDiff>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrBrief {
    pub number: i32,
    pub title: String,
    /// "open", "merged" or "closed".
    pub state: String,
    pub is_draft: bool,
}

/// The desk's probe: what the tiles say and which are grey.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    /// The folder is there.
    pub folder: bool,
    pub git_installed: bool,
    pub git: bool,
    pub branch: Option<String>,
    pub changed: usize,
    pub add: i64,
    pub del: i64,
    pub gh: bool,
    pub gh_auth: bool,
    pub gh_user: Option<String>,
    pub pr: Option<PrBrief>,
    pub pr_reason: Option<String>,
    pub commands: usize,
    pub agents: usize,
    pub running: usize,
    pub pages: usize,
    pub linked: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: String,
    /// "pass", "fail", "pending" or "skip".
    pub state: String,
    pub url: Option<String>,
}

/// A pull request as the panel shows it (gh's JSON cut to what is drawn).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrDetail {
    pub number: i32,
    pub title: String,
    /// "open", "merged" or "closed".
    pub state: String,
    pub is_draft: bool,
    pub url: String,
    pub head: String,
    pub base: String,
    pub additions: i32,
    pub deletions: i32,
    pub changed_files: i32,
    pub body: String,
    pub author: Option<String>,
    /// APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED, or none.
    pub review: Option<String>,
    pub updated_at: Option<String>,
    pub comments: usize,
    /// At most 30.
    pub checks: Vec<Check>,
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
    pub skip: usize,
}

/// What gh needs before the panel can show anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setup { Install, SignIn }

/// What Create pull request's form starts from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreateInfo {
    pub branch: Option<String>,
    /// The repository's default branch: where the pull request goes.
    pub base: String,
    /// On the default branch (or none): a new branch is needed.
    pub on_default: bool,
    /// A name for it: "hover/" and the title's words.
    pub suggest: Option<String>,
    /// Commits ahead of the default branch.
    pub ahead: u32,
    /// Files not committed yet.
    pub changed: usize,
    pub title: String,
    pub body: String,
    /// The agent works in the folder: no pull request now.
    pub busy: bool,
}

/// The Pull request tab.
#[derive(Clone, Debug, PartialEq)]
pub enum PrPanel {
    /// Why there is nothing to show.
    Error(String),
    /// gh isn't installed, or isn't signed in: the setup card, with this sentence.
    Setup { need: Setup, message: String },
    /// The branch has no pull request: the form to make one.
    NoPr { message: String, create: CreateInfo },
    Open(Box<PrDetail>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkedPr {
    pub url: String,
    pub repo: String,
    pub number: u32,
    pub title: Option<String>,
    /// "open", "merged" or "closed"; None where gh wasn't asked or couldn't say.
    pub state: Option<String>,
    pub is_draft: bool,
    pub additions: i32,
    pub deletions: i32,
    pub head: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linked {
    /// gh is installed (the state of each is known).
    pub gh: bool,
    pub prs: Vec<LinkedPr>,
}

/// What the Create pull request form sends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreatePrArgs {
    pub title: String,
    pub body: String,
    /// Into this branch; the default branch if none.
    pub base: Option<String>,
    /// A new branch to make first (when on the default one).
    pub branch: Option<String>,
    /// Commit what isn't committed first.
    pub commit: bool,
    pub draft: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreatePrResult {
    pub ok: bool,
    pub url: Option<String>,
    pub error: Option<String>,
    /// What was done, in order (also when a later step failed).
    pub steps: Vec<String>,
}

// MARK: The tiles

/// The desk card's tiles, in T3 Code's order with its letters.
pub const SURFACES: [(&str, &str, char); 8] = [
    ("browser", "Browser", 'B'), ("terminal", "Terminal", 'T'), ("files", "Files", 'F'), ("diff", "Diff", 'D'),
    ("pr", "Pull request", 'P'), ("linked", "Linked pull requests", 'L'), ("agents", "Agents", 'A'), ("screen", "Screen", 'S'),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub id: &'static str,
    pub title: &'static str,
    pub letter: char,
    pub enabled: bool,
    /// Why it is grey; empty when it isn't.
    pub reason: String,
    /// The line under it: what there is to see, at a glance.
    pub detail: String,
}

/// What the tiles need beyond the probe.
#[derive(Clone, Copy, Debug, Default)]
pub struct TileContext<'a> {
    /// The address open in the browser panel.
    pub browser_url: Option<&'a str>,
    /// `pages(snap)`.
    pub pages: &'a [Page],
    /// Tiles this system can't run, with the note to show: (id, note). They are grey
    /// with that reason, whatever the probe says.
    pub off: &'a [(&'a str, &'a str)],
}

/// DeskInfo's tile rows (desk.js's availability and tileDetail). `probe` is None until
/// the probe is back: the tiles then say what the session's own steps do.
pub fn tiles(probe: Option<&Probe>, snap: &Snap, ctx: &TileContext) -> Vec<Tile> {
    let p = probe;
    let runs = snap.steps.iter().filter(|i| i.step.kind == "execute" && !is_screen(&i.step)).count();
    let edits = snap.steps.iter().filter(|i| i.step.kind == "edit").count();
    let n = |k: usize, one: &str, many: &str| format!("{} {}", num(k as i64), if k == 1 { one } else { many });
    let wait = "Checking…";
    let subs = snap.steps.iter().filter(|i| is_subagent(&i.step)).count();
    let linked = p.map_or_else(|| linked_urls(snap).len(), |p| p.linked);
    let agents = p.map_or(subs, |p| p.agents);
    let apps = apps(snap).is_some();
    SURFACES.iter().map(|&(id, title, letter)| {
        let (mut enabled, mut reason) = match id {
            "terminal" => (p.map_or(runs, |p| p.commands) > 0, "No commands run yet.".to_owned()),
            "files" => (p.is_none_or(|p| p.folder), "The folder isn’t there any more.".into()),
            // A Kiro Web session's changes are its pull request's, or the edits it reported.
            "diff" if snap.cloud.is_some() => (true, String::new()),
            "diff" => (p.is_none_or(|p| p.folder && (p.git || edits > 0)),
                match p { Some(p) if !p.folder => "The folder isn’t there any more.", Some(p) if !p.git_installed => "Git isn’t installed.", _ => "No changes yet." }.into()),
            // Open even without one: the panel sets up gh, or opens a pull request.
            "pr" => (snap.cloud.is_some() || p.is_none_or(|p| p.git), match p { None => wait, Some(p) if !p.git_installed => "Git isn’t installed.", Some(_) => "Not a Git repository." }.into()),
            "linked" => (linked > 0, if p.is_some() { "No pull requests mentioned in this session." } else { wait }.into()),
            "agents" => (agents > 0, if p.is_some() { "No subagents in this session." } else { wait }.into()),
            _ => (true, String::new()),
        };
        if let Some((_, note)) = ctx.off.iter().find(|(i, _)| *i == id) { enabled = false; reason = (*note).to_owned(); }
        let detail = match id {
            "browser" => if snap.browsing() { "In use now".to_owned() } else if let Some(u) = ctx.browser_url.filter(|u| !u.is_empty()) { label(u) } else if let Some(pg) = ctx.pages.first() { label(&pg.url) } else { "Open a page".into() },
            "terminal" => match p.map_or(runs, |p| p.commands) { 0 => "Nothing run".into(), k => n(k, "command", "commands") },
            "files" => match p { Some(p) if p.changed > 0 => format!("{} changed", num(p.changed as i64)), Some(p) if p.git => "No changes".into(), _ => "Browse".into() },
            "diff" => match p {
                Some(p) if p.add > 0 || p.del > 0 => format!("+{} −{}", num(p.add), num(p.del)),
                Some(p) if p.changed > 0 => format!("{} file{}", num(p.changed as i64), if p.changed == 1 { "" } else { "s" }),
                _ => "Clean".into(),
            },
            "pr" => match p {
                None => "…".into(),
                Some(p) => match &p.pr {
                    Some(pr) => format!("#{} {}", pr.number, if pr.is_draft { "draft" } else { pr.state.as_str() }),
                    None if !p.git && snap.cloud.is_none() => "No repository".into(),
                    None if !p.gh => "Set up GitHub".into(),
                    None if !p.gh_auth => "Sign in".into(),
                    None if snap.cloud.is_some() => "None yet".into(),
                    None => "Open one".into(),
                },
            },
            "linked" => if linked > 0 { format!("{} mentioned", num(linked as i64)) } else { "None".into() },
            "agents" => match p { Some(p) if p.running > 0 => format!("{} working", num(p.running as i64)), _ if agents > 0 => n(agents, "subagent", "subagents"), _ => "None yet".into() },
            "screen" => if snap.testing() { "Live".into() } else if apps { "Desktop + its apps".into() } else { "Desktop".into() },
            _ => String::new(),
        };
        if enabled { reason.clear(); }
        Tile { id, title, letter, enabled, reason, detail }
    }).collect()
}

// MARK: Linked pull requests

static PR_URL: LazyLock<Regex> = LazyLock::new(|| re(r"https://github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/(\d+)"));

/// DeskInfo.LinkedUrls: the pull requests the session mentions: in a prompt, an answer,
/// or what a command printed (gh pr create says where it made one). Newest first; 12.
pub fn linked_urls(s: &Snap) -> Vec<(String, String, u32)> {
    let steps = s.steps.iter().flat_map(|i| [i.step.target.as_deref(), i.step.input.as_deref(), i.step.text()]);
    let mut found: Vec<(String, String, u32)> = vec![];
    for t in s.texts.iter().map(|t| Some(t.as_str())).chain(steps).flatten().filter(|t| !t.is_empty()) {
        for m in PR_URL.captures_iter(t).flatten() {
            let Ok(n) = m[2].parse::<u32>() else { continue };
            let key = (m[0].to_owned(), m[1].to_owned(), n);
            found.retain(|f| *f != key);
            found.push(key);
        }
    }
    found.reverse();
    found.truncate(12);
    found
}

/// The pull request this session made: the newest address a step that opens one printed
/// (`gh pr create`, or a GitHub tool's create pull request).
pub fn created_pr(s: &Snap) -> Option<String> {
    s.steps.iter().rev().find_map(|i| {
        let x = &i.step;
        let said = [x.target.as_deref(), x.input.as_deref(), Some(x.title.as_str())].into_iter().flatten().collect::<Vec<_>>().join(" ").to_lowercase();
        if !["pr create", "create_pull_request", "create pull request", "create a pull request"].iter().any(|k| said.contains(k)) { return None; }
        let out = x.text()?;
        let m = PR_URL.captures_iter(out).flatten().last()?;
        Some(m[0].to_owned())
    })
}

/// The newest pull request the session mentions. A Kiro Web session's own repos first:
/// a link to some other repository is not its pull request.
pub fn mentioned_pr(s: &Snap) -> Option<String> {
    let all = linked_urls(s);
    match s.cloud.as_deref().filter(|r| !r.is_empty()) {
        Some(repos) => all.into_iter().find(|u| repos.iter().any(|r| r.eq_ignore_ascii_case(&u.1))).map(|u| u.0),
        None => all.into_iter().next().map(|u| u.0),
    }
}

/// "owner/name" from a GitHub remote: https://github.com/o/n(.git), git@github.com:o/n.git
/// or ssh://git@github.com/o/n.git.
pub fn github_name(url: &str) -> Option<String> {
    let rest = ["https://github.com/", "http://github.com/", "git@github.com:", "ssh://git@github.com/", "git://github.com/"].iter().find_map(|p| url.strip_prefix(p))?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(o), Some(n), None) if !o.is_empty() && !n.is_empty() => Some(format!("{o}/{n}")),
        _ => None,
    }
}

// MARK: Desk

type Cache = HashMap<String, (Instant, Arc<dyn Any + Send + Sync>)>;

const TREE_LIMIT: usize = 5000;
const PATCH_LIMIT: usize = 1536 * 1024;
const NEW_FILE_LINES: usize = 400;
const SKIP_DIRS: [&str; 17] = [".git", "node_modules", "bin", "obj", "dist", "build", "out", ".next", ".nuxt", "target", "__pycache__", ".venv", "venv", ".gradle", ".idea", ".vs", "DerivedData"];
const PR_FIELDS: &str = "number,title,state,isDraft,url,headRefName,baseRefName,additions,deletions,changedFiles,body,author,reviewDecision,statusCheckRollup,updatedAt,comments";

/// git, from PATH or where Git's installers put it that a desktop app's PATH may not list.
pub fn find_git() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        for v in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(p) = std::env::var_os(v) { places.push(PathBuf::from(p).join("Git").join("cmd").join("git.exe")); }
        }
        if let Some(p) = std::env::var_os("LOCALAPPDATA") { places.push(PathBuf::from(p).join("Programs").join("Git").join("cmd").join("git.exe")); }
    } else {
        places.extend(["/opt/homebrew/bin/git", "/usr/local/bin/git", "/usr/bin/git"].map(PathBuf::from));
    }
    on_path("git").into_iter().chain(places).find(|p| p.is_file() && !is_git_stub(p))
}

/// macOS's /usr/bin/git is a stub that, without the Command Line Tools, opens the dialog
/// offering to install them instead of running: never started from here.
fn is_git_stub(p: &Path) -> bool {
    cfg!(target_os = "macos") && p == Path::new("/usr/bin/git")
        && !["/Library/Developer/CommandLineTools/usr/bin/git", "/Applications/Xcode.app/Contents/Developer/usr/bin/git"].iter().any(|t| Path::new(t).is_file())
}

/// The desk's reader of git and gh, with what it has read lately (a status for 2 s, a
/// pull request for 30 s).
pub struct Desk {
    gh: Arc<GitHubCli>,
    git: Option<PathBuf>,
    cache: Mutex<Cache>,
}

impl Desk {
    /// The desk of the app: the shared gh, and the git on this computer.
    pub fn shared() -> Arc<Desk> {
        static ONE: OnceLock<Arc<Desk>> = OnceLock::new();
        ONE.get_or_init(|| Arc::new(Desk::new(crate::github::shared(), find_git()))).clone()
    }

    pub fn new(gh: Arc<GitHubCli>, git: Option<PathBuf>) -> Desk { Desk { gh, git, cache: Mutex::new(HashMap::new()) } }

    pub fn gh(&self) -> &Arc<GitHubCli> { &self.gh }

    fn cached<T: Clone + Send + Sync + 'static>(&self, key: String, life: Duration, make: impl FnOnce() -> T) -> T {
        if let Some((at, v)) = self.cache.lock().unwrap().get(&key) {
            if at.elapsed() < life { if let Some(v) = v.downcast_ref::<T>() { return v.clone(); } }
        }
        let value = make();
        let mut c = self.cache.lock().unwrap();
        c.insert(key, (Instant::now(), Arc::new(value.clone())));
        if c.len() > 200 { c.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300)); }
        value
    }

    /// What is cached of a folder's repository, after Hover changed it.
    fn forget(&self, folder: &str) {
        let (end, mid) = (format!("\0{folder}"), format!("\0{folder}\0"));
        self.cache.lock().unwrap().retain(|k, _| !(k.ends_with(&end) || k.contains(&mid)));
    }

    fn git(&self, folder: &str, ms: u64, max: usize, args: &[&str]) -> Ran { self.git_with(folder, ms, max, args, None) }

    fn git_with(&self, folder: &str, ms: u64, max: usize, args: &[&str], stdin: Option<&[u8]>) -> Ran {
        match &self.git {
            None => Ran::failed("Git isn’t installed."),
            Some(g) => run(g, Some(Path::new(folder)), MS(ms), max, args, stdin, self.gh.env()),
        }
    }

    fn gh_run(&self, gh: &Path, folder: &str, ms: u64, max: usize, args: &[&str], stdin: Option<&[u8]>) -> Ran {
        // A pull request named by its address needs no repository: a session whose folder
        // is gone can still look its links up.
        let dir = crate::usable_folder(Some(folder)).then(|| Path::new(folder));
        run(gh, dir, MS(ms), max, args, stdin, self.gh.env())
    }

    // MARK: Repository

    /// DeskInfo.RepoOf (10 s).
    pub fn repo_of(&self, folder: &str) -> Repo {
        self.cached(format!("repo\0{folder}"), Duration::from_secs(10), || {
            let none = Repo::default();
            if !crate::usable_folder(Some(folder)) { return none; }
            let top = self.git(folder, 5000, 4096, &["rev-parse", "--is-inside-work-tree", "--show-prefix"]);
            if top.code != 0 { return none; }
            let out = top.out.replace('\r', "");
            let mut lines = out.split('\n');
            if lines.next().map(str::trim) != Some("true") { return none; }
            let prefix = lines.next().unwrap_or("").trim().to_owned();
            let head = self.git(folder, 5000, 4096, &["rev-parse", "--verify", "-q", "HEAD"]).code == 0;
            let mut branch = self.git(folder, 5000, 4096, &["rev-parse", "--abbrev-ref", "HEAD"]);
            // A repository with no commit yet has a branch and no HEAD to abbreviate.
            if branch.code != 0 { branch = self.git(folder, 5000, 4096, &["symbolic-ref", "--short", "-q", "HEAD"]); }
            Repo { git: true, branch: (branch.code == 0).then(|| branch.out.trim().to_owned()).filter(|b| !b.is_empty()), prefix, head }
        })
    }

    /// DeskInfo.Status: git status for the folder: paths relative to it, and what
    /// happened to each (2 s).
    pub fn status(&self, folder: &str, repo: &Repo) -> Vec<Change> {
        self.cached(format!("status\0{folder}"), Duration::from_secs(2), || {
            let r = self.git(folder, 15000, 2 * 1024 * 1024, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."]);
            if r.code == 0 { parse_status(&r.out, &repo.prefix) } else { vec![] }
        })
    }

    fn num_stat(&self, folder: &str, repo: &Repo) -> HashMap<String, (i32, i32)> {
        let args: &[&str] = if repo.head { &["diff", "HEAD", "--numstat", "--relative", "-z"] } else { &["diff", "--cached", "--numstat", "--relative", "-z"] };
        let r = self.git(folder, 15000, 2 * 1024 * 1024, args);
        let mut map = HashMap::new();
        if r.code != 0 { return map; }
        let parts: Vec<&str> = r.out.split('\0').collect();
        let mut i = 0;
        while i < parts.len() {
            let f: Vec<&str> = parts[i].split('\t').collect();
            i += 1;
            if f.len() < 3 { continue; }
            let mut path = f[2];
            // A rename: "a\tb\t" then the old and the new path.
            if path.is_empty() && i + 1 < parts.len() { path = parts[i + 1]; i += 2; }
            map.insert(path.to_owned(), (f[0].parse().unwrap_or(0), f[1].parse().unwrap_or(0)));
        }
        map
    }

    // MARK: Probe

    /// DeskInfo.Probe: what the desk's tiles need (which are grey, what they say). Runs
    /// git and gh.
    pub fn probe(&self, s: &Snap) -> Probe {
        let repo = self.repo_of(&s.folder);
        let gh = self.gh.exe().is_some();
        let status = gh.then(|| self.gh.check(false));
        let signed_in = status.as_ref().is_some_and(|x| x.signed_in);
        let cloud = s.cloud.is_some();
        let mut changed = if repo.git && !cloud { self.status(&s.folder, &repo).len() } else { 0 };
        let mut pr = None;
        let link = created_pr(s).or_else(|| mentioned_pr(s));
        // Here a Kiro Web session needs no repository, only its pull request's address.
        let can_look = if cloud { link.is_some() } else { repo.git };
        let mut pr_reason = if cloud && !can_look { Some(CLOUD_NO_PR.to_owned()) } else if !can_look { Some("Not a Git repository.".to_owned()) } else if !gh { Some("Install the GitHub CLI (gh) to see pull requests.".into()) } else if !signed_in { Some("Sign in to GitHub to see pull requests.".into()) } else { None };
        let mut from_pr = (0i64, 0i64);
        if can_look && gh && signed_in {
            let full = self.pr_full(s);
            pr_reason = full.error.clone();
            pr = full.data.as_ref().map(|d| PrBrief { number: d.number, title: d.title.clone(), state: d.state.clone(), is_draft: d.is_draft });
            // A Kiro Web session's changes are its pull request's; this folder's are not.
            if let (true, Some(d)) = (cloud, &full.data) { from_pr = (d.additions as i64, d.deletions as i64); changed = d.changed_files.max(0) as usize; }
        }
        let sub: Vec<&Item> = s.steps.iter().filter(|i| is_subagent(&i.step)).collect();
        let counts = if repo.git && !cloud { self.num_stat(&s.folder, &repo) } else { HashMap::new() };
        Probe {
            folder: crate::usable_folder(Some(&s.folder)),
            git_installed: self.git.is_some(),
            git: repo.git,
            branch: repo.branch,
            changed,
            add: if cloud { from_pr.0 } else { counts.values().map(|c| c.0 as i64).sum() },
            del: if cloud { from_pr.1 } else { counts.values().map(|c| c.1 as i64).sum() },
            gh,
            gh_auth: signed_in,
            gh_user: status.and_then(|x| x.user),
            pr,
            pr_reason,
            commands: s.steps.iter().filter(|i| i.step.kind == "execute" && !is_screen(&i.step)).count(),
            agents: sub.len(),
            running: sub.iter().filter(|i| i.step.status == "in_progress").count(),
            pages: pages(s).len(),
            linked: linked_urls(s).len(),
        }
    }

    // MARK: Files

    /// DeskInfo.Files: the session folder's tree (Git's view where it is a repository),
    /// what changed, and what the agent read and edited.
    pub fn files(&self, s: &Snap) -> Files {
        if !crate::usable_folder(Some(&s.folder)) {
            return Files { error: Some("The session's folder isn’t there any more.".into()), ..Default::default() };
        }
        let repo = self.repo_of(&s.folder);
        let mut more = false;
        let mut tree: Vec<String> = if repo.git {
            let r = self.git(&s.folder, 15000, 4 * 1024 * 1024, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
            let mut seen = std::collections::HashSet::new();
            let list: Vec<String> = r.out.split('\0').filter(|p| !p.is_empty() && seen.insert(*p)).map(str::to_owned).collect();
            more = r.capped || list.len() > TREE_LIMIT;
            list
        } else {
            walk(Path::new(&s.folder), &mut more)
        };
        if tree.len() > TREE_LIMIT { tree.truncate(TREE_LIMIT); more = true; }
        tree.sort_by_cached_key(|p| p.to_uppercase());

        let changed = if repo.git { self.status(&s.folder, &repo) } else { vec![] };
        let counts = if repo.git { self.num_stat(&s.folder, &repo) } else { HashMap::new() };
        let mut touched: Vec<Touched> = vec![];
        for it in &s.steps {
            let x = &it.step;
            if !x.is(&["read", "edit", "delete", "move"]) { continue; }
            let Some(p) = relative(x.target.as_deref(), &s.folder) else { continue };
            let at = touched.iter().position(|t| t.path == p).unwrap_or_else(|| { touched.push(Touched { path: p, read: 0, edit: 0 }); touched.len() - 1 });
            if x.kind == "read" { touched[at].read += 1 } else { touched[at].edit += 1 }
        }
        Files {
            git: repo.git,
            branch: repo.branch,
            changed: changed.into_iter().map(|c| {
                let (add, del) = counts.get(&c.path).copied().unwrap_or((0, 0));
                ChangedFile { path: c.path, status: c.status, old: c.old, add, del }
            }).collect(),
            touched,
            tree,
            more,
            error: None,
        }
    }

    /// A file of the folder, read inside it only.
    pub fn file(&self, s: &Snap, rel: &str) -> FileView { file_text(&s.folder, Some(rel)) }

    // MARK: Diff

    /// DeskInfo.Diff: the working tree against the last commit, capped; untracked files
    /// as whole added files. Outside Git, the edits the session made.
    pub fn diff(&self, s: &Snap) -> Diff {
        if s.cloud.is_some() { return self.cloud_diff(s); }
        let repo = self.repo_of(&s.folder);
        if !repo.git { return Diff { git: false, partial: true, files: from_steps(s), ..Default::default() }; }
        let base = ["-c", "core.quotepath=off", "diff"];
        let rest: &[&str] = if repo.head { &["HEAD", "--no-color", "--no-ext-diff", "-M", "--relative"] } else { &["--cached", "--no-color", "--no-ext-diff", "-M", "--relative"] };
        let args: Vec<&str> = base.iter().chain(rest).copied().collect();
        let r = self.git(&s.folder, 20000, PATCH_LIMIT, &args);
        if r.code != 0 && !r.capped {
            return Diff { git: true, error: Some(line(&r.err).unwrap_or_else(|| "git diff failed.".into())), ..Default::default() };
        }
        let mut files = parse_diff(&r.out);
        // Files git doesn't track yet are new: shown whole, as added lines.
        let mut budget = PATCH_LIMIT as i64 - chars(&r.out) as i64;
        for c in self.status(&s.folder, &repo).into_iter().filter(|c| c.status == '?').take(60) {
            if budget <= 0 { break; }
            let Some(full) = inside(&s.folder, Some(&c.path)) else { continue };
            let Ok(meta) = std::fs::metadata(&full) else { continue };
            let binary = || FileDiff { path: c.path.clone(), old: None, status: 'A', add: 0, del: 0, binary: true, patch: String::new() };
            if !meta.is_file() || meta.len() > 256 * 1024 { files.push(binary()); continue; }
            let Ok(bytes) = std::fs::read(&full) else { continue };
            if has_nul(&bytes) { files.push(binary()); continue; }
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            let lines: Vec<&str> = if bytes.is_empty() { vec![] } else { text.trim_end_matches('\n').split('\n').collect() };
            let shown: Vec<String> = lines.iter().take(NEW_FILE_LINES).map(|l| format!("+{l}")).collect();
            let patch = if lines.is_empty() { String::new() } else {
                format!("@@ -0,0 +1,{} @@\n{}{}", lines.len(), shown.join("\n"), if lines.len() > NEW_FILE_LINES { format!("\n\\ {} more lines", lines.len() - NEW_FILE_LINES) } else { String::new() })
            };
            budget -= chars(&patch) as i64;
            files.push(FileDiff { path: c.path, old: None, status: 'A', add: lines.len() as i32, del: 0, binary: false, patch });
        }
        Diff { git: true, partial: false, branch: repo.branch, truncated: r.capped, error: None, files }
    }

    /// A Kiro Web session's changes: its pull request's patch, through gh; before it has
    /// one (or when gh can't say), the edits it reported. This computer's folder is not its.
    fn cloud_diff(&self, s: &Snap) -> Diff {
        let reported = || Diff { git: false, partial: true, files: from_steps(s), ..Default::default() };
        let Some(url) = created_pr(s).or_else(|| mentioned_pr(s)) else { return reported() };
        if self.gh.exe().is_none() || !self.gh.check(false).signed_in { return reported(); }
        let r = self.cached(format!("prdiff\0{}\0{url}", s.folder), Duration::from_secs(30), || {
            self.gh_run(&self.gh.exe().unwrap_or_default(), &s.folder, 30000, PATCH_LIMIT, &["pr", "diff", &url, "--color", "never"], None)
        });
        if r.code != 0 && !r.capped { return reported(); }
        Diff { git: true, partial: false, branch: None, truncated: r.capped, error: None, files: parse_diff(&r.out) }
    }

    // MARK: Pull requests

    /// gh's answer for one pull request, by its address (30 s). It needs no local repository.
    fn pr_at(&self, folder: &str, url: &str) -> PrResult {
        self.cached(format!("pr\0{folder}\0at:{url}"), Duration::from_secs(30), || {
            let Some(gh) = self.gh.exe() else { return PrResult { error: Some("Install the GitHub CLI (gh) to see pull requests.".into()), data: None } };
            let r = self.gh_run(&gh, folder, 20000, 1024 * 1024, &["pr", "view", url, "--json", PR_FIELDS], None);
            if r.code != 0 { return PrResult { error: Some(gh_reason(&r.err)), data: None }; }
            match json::parse(&r.out) {
                Ok(v) => PrResult { error: None, data: Some(slim(&v)) },
                Err(_) => PrResult { error: Some("gh’s answer couldn’t be read.".into()), data: None },
            }
        })
    }

    /// The session's pull request, through gh: the one it created; else (a chat on this
    /// computer) the folder's branch's; else the newest one it mentions. A Kiro Web
    /// session has no branch here, so it goes from the one it created to the one it mentions.
    fn pr_full(&self, s: &Snap) -> PrResult {
        let folder = s.folder.as_str();
        if let Some(url) = created_pr(s) { return self.pr_at(folder, &url); }
        if s.cloud.is_some() {
            return match mentioned_pr(s) { Some(url) => self.pr_at(folder, &url), None => PrResult { error: Some(CLOUD_NO_PR.into()), data: None } };
        }
        let branch = self.repo_of(folder).branch.unwrap_or_default();
        let own = self.cached(format!("pr\0{folder}\0{branch}"), Duration::from_secs(30), || {
            let Some(gh) = self.gh.exe() else { return PrResult { error: Some("Install the GitHub CLI (gh) to see pull requests.".into()), data: None } };
            let r = self.gh_run(&gh, folder, 20000, 1024 * 1024, &["pr", "view", "--json", PR_FIELDS], None);
            if r.code != 0 { return PrResult { error: Some(gh_reason(&r.err)), data: None }; }
            match json::parse(&r.out) {
                Ok(v) => PrResult { error: None, data: Some(slim(&v)) },
                Err(_) => PrResult { error: Some("gh’s answer couldn’t be read.".into()), data: None },
            }
        });
        if own.data.is_some() { return own; }
        // The branch has none (or there is no repository): the one the chat talks about.
        match mentioned_pr(s).map(|url| self.pr_at(folder, &url)) {
            Some(found) if found.data.is_some() => found,
            _ => own,
        }
    }

    /// DeskInfo.Pr: the Pull request tab. Runs gh.
    pub fn pr(&self, s: &Snap) -> PrPanel {
        let repo = self.repo_of(&s.folder);
        let link = created_pr(s).or_else(|| mentioned_pr(s));
        if s.cloud.is_some() {
            if link.is_none() { return PrPanel::Error(CLOUD_NO_PR.into()); }
        } else if !repo.git {
            return PrPanel::Error(if self.git.is_none() { "Git isn’t installed.".into() } else { "Not a Git repository.".into() });
        }
        if self.gh.exe().is_none() {
            return PrPanel::Setup { need: Setup::Install, message: "Install the GitHub CLI to see and open pull requests.".into() };
        }
        if !self.gh.check(false).signed_in {
            return PrPanel::Setup { need: Setup::SignIn, message: "Sign in to GitHub to see and open pull requests.".into() };
        }
        let full = self.pr_full(s);
        if let Some(e) = full.error {
            // No pull request for this branch yet: what Create pull request needs.
            return if e == NO_PR && repo.git { PrPanel::NoPr { message: e, create: self.create_info_for(s, &repo) } } else { PrPanel::Error(e) };
        }
        PrPanel::Open(Box::new(full.data.unwrap_or_default()))
    }

    /// DeskInfo.CreateInfo: what the form starts from.
    pub fn create_info(&self, s: &Snap) -> CreateInfo { self.create_info_for(s, &self.repo_of(&s.folder)) }

    fn create_info_for(&self, s: &Snap, repo: &Repo) -> CreateInfo {
        let def = self.default_branch(&s.folder);
        let on_default = repo.branch.as_deref().is_none_or(|b| b == "HEAD" || b == def);
        let mut ahead = 0;
        if !on_default {
            let r = self.git(&s.folder, 8000, 4096, &["rev-list", "--count", &format!("{}/{def}..HEAD", self.remote(&s.folder))]);
            if r.code == 0 { ahead = r.out.trim().parse().unwrap_or(0); }
        }
        let mut title = s.texts.iter().find(|t| !t.is_empty()).cloned().unwrap_or_default().replace('\n', " ").trim().to_owned();
        if chars(&title) > 72 { title = format!("{}…", head(&title, 71).trim_end()); }
        let answer = s.texts.iter().enumerate().filter(|(i, t)| i % 2 == 1 && !t.is_empty()).map(|(_, t)| t.as_str()).next_back().unwrap_or("");
        CreateInfo {
            branch: repo.branch.clone(),
            suggest: on_default.then(|| format!("hover/{}", slug(&title))),
            base: def,
            on_default,
            ahead,
            changed: self.status(&s.folder, repo).len(),
            title,
            body: head(answer, 6000).to_owned(),
            busy: s.busy,
        }
    }

    /// DeskInfo.Remote: the remote pushes go to: origin, else the first one.
    pub fn remote(&self, folder: &str) -> String {
        let r = self.git(folder, 5000, 4096, &["remote"]);
        let all: Vec<&str> = if r.code == 0 { r.out.lines().map(str::trim).filter(|l| !l.is_empty()).collect() } else { vec![] };
        if all.contains(&"origin") { "origin".into() } else { all.first().map_or("origin".into(), |s| (*s).to_owned()) }
    }

    /// The GitHub repo ("owner/name") the folder's remote points at, for a Kiro Web
    /// session to clone; None when it has no GitHub remote.
    pub fn github_repo(&self, folder: &str) -> Option<String> {
        let r = self.git(folder, 5000, 4096, &["remote", "get-url", &self.remote(folder)]);
        if r.code == 0 { github_name(r.out.trim()) } else { None }
    }

    /// DeskInfo.DefaultBranch: the remote's HEAD, else main or master (2 min).
    pub fn default_branch(&self, folder: &str) -> String {
        self.cached(format!("default\0{folder}"), Duration::from_secs(120), || {
            let remote = self.remote(folder);
            let r = self.git(folder, 5000, 4096, &["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")]);
            let out = r.out.trim();
            if r.code == 0 && !out.is_empty() {
                return out.strip_prefix(&format!("{remote}/")).unwrap_or(out).to_owned();
            }
            for b in ["main", "master"] {
                if self.git(folder, 5000, 4096, &["rev-parse", "--verify", "-q", &format!("refs/remotes/{remote}/{b}")]).code == 0 { return b.into(); }
            }
            "main".into()
        })
    }

    /// DeskInfo.CreatePr: Create pull request, as the user asked in the PR panel: a new
    /// branch first when on the default one, a commit of what isn't committed when asked,
    /// a push, then `gh pr create`. Never while the agent works in the folder. A step that
    /// fails comes back as its reason, with the steps done before it.
    ///
    /// The commit message and the description go over stdin (`git commit -F -`, `gh pr
    /// create --body-file -`), not on a command line.
    pub fn create_pr(&self, s: &Snap, a: &CreatePrArgs) -> CreatePrResult {
        let mut steps: Vec<String> = vec![];
        let fail = |steps: &[String], why: String| CreatePrResult { ok: false, url: None, error: Some(why), steps: steps.to_vec() };
        if s.busy { return fail(&steps, "Wait for the agent to finish first: it is still working in this folder.".into()); }
        let repo = self.repo_of(&s.folder);
        if !repo.git { return fail(&steps, "Not a Git repository.".into()); }
        let Some(gh) = self.gh.exe() else { return fail(&steps, "Install the GitHub CLI first.".into()) };
        let title = a.title.trim();
        if title.is_empty() { return fail(&steps, "Give the pull request a title.".into()); }
        let title = head(title, 256);
        let body = a.body.trim();
        let base = a.base.as_deref().map(str::trim).filter(|b| !b.is_empty() && valid_ref(b)).map_or_else(|| self.default_branch(&s.folder), str::to_owned);
        let folder = s.folder.as_str();
        let must = |r: Ran, what: &str| -> Result<Ran, String> {
            if r.code == 0 { Ok(r) } else { Err(format!("{what}: {}", line(&r.err).or_else(|| line(&r.out)).unwrap_or_else(|| "it failed".into()))) }
        };
        let r = (|| -> Result<String, String> {
            let mut branch = repo.branch.clone();
            if let Some(nb) = a.branch.as_deref().map(str::trim).filter(|b| !b.is_empty() && Some(*b) != branch.as_deref()) {
                if !valid_ref(nb) { return Err("That branch name isn’t valid.".into()); }
                must(self.git(folder, 15000, 65536, &["switch", "-c", nb]), "Couldn’t make the branch")?;
                steps.push(format!("Made branch {nb}"));
                branch = Some(nb.to_owned());
            }
            let branch = match branch { Some(b) if b != "HEAD" => b, _ => return Err("Name a branch for the pull request.".into()) };
            if branch == base { return Err(format!("The pull request needs a branch other than {base}.")); }
            if a.commit {
                let st = self.git(folder, 15000, 2 * 1024 * 1024, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
                if !parse_status(&st.out, "").is_empty() {
                    must(self.git(folder, 30000, 65536, &["add", "-A"]), "Couldn’t stage the changes")?;
                    let message = format!("{title}\n\nCommitted from Hover.\n");
                    must(self.git_with(folder, 30000, 65536, &["commit", "-F", "-"], Some(message.as_bytes())), "Couldn’t commit")?;
                    steps.push("Committed the changes".into());
                }
            }
            let remote = self.remote(folder);
            must(self.git(folder, 120000, 65536, &["push", "-u", &remote, "HEAD"]), "Couldn’t push the branch")?;
            steps.push(format!("Pushed {branch} to {remote}"));
            let mut create = vec!["pr", "create", "--title", title, "--body-file", "-", "--base", base.as_str(), "--head", branch.as_str()];
            if a.draft { create.push("--draft"); }
            let text = if body.is_empty() { title } else { body };
            let r = must(self.gh_run(&gh, folder, 60000, 65536, &create, Some(text.as_bytes())), "gh couldn’t open the pull request")?;
            Ok(urls(Some(&r.out)).into_iter().rev().find(|u| u.contains("/pull/")).unwrap_or_default())
        })();
        self.forget(folder);
        match r {
            Ok(url) => CreatePrResult { ok: true, url: Some(url).filter(|u| !u.is_empty()), error: None, steps },
            Err(e) => fail(&steps, e),
        }
    }

    // MARK: Linked

    /// DeskInfo.Linked: the pull requests the session mentions, with their state where gh can say.
    pub fn linked(&self, s: &Snap) -> Linked {
        let urls = linked_urls(s);
        let gh = self.gh.exe();
        let rows: Vec<LinkedPr> = std::thread::scope(|sc| {
            let handles: Vec<_> = urls.iter().enumerate().map(|(i, u)| {
                let gh = gh.as_deref();
                sc.spawn(move || {
                    let bare = LinkedPr { url: u.0.clone(), repo: u.1.clone(), number: u.2, ..Default::default() };
                    match gh {
                        Some(gh) if i < 8 => self.cached(format!("linked\0{}", u.0), Duration::from_secs(60), || self.linked_one(gh, &s.folder, bare)),
                        _ => bare,
                    }
                })
            }).collect();
            handles.into_iter().zip(&urls).map(|(h, u)| h.join().unwrap_or_else(|_| LinkedPr { url: u.0.clone(), repo: u.1.clone(), number: u.2, ..Default::default() })).collect()
        });
        Linked { gh: gh.is_some(), prs: rows }
    }

    fn linked_one(&self, gh: &Path, folder: &str, mut row: LinkedPr) -> LinkedPr {
        let v = self.gh_run(gh, folder, 15000, 256 * 1024, &["pr", "view", &row.url, "--json", "number,title,state,isDraft,url,additions,deletions,headRefName"], None);
        if v.code != 0 { row.error = Some(gh_reason(&v.err)); return row; }
        if let Ok(e) = json::parse(&v.out) {
            row.title = jstr(&e, "title");
            row.state = Some(pr_state(&e));
            row.is_draft = jbool(&e, "isDraft");
            row.additions = jint(&e, "additions");
            row.deletions = jint(&e, "deletions");
            row.head = jstr(&e, "headRefName");
        }
        row
    }
}

#[derive(Clone)]
struct PrResult {
    error: Option<String>,
    data: Option<PrDetail>,
}

/// Outside Git, the edits the session made, as the parts of each change it kept.
fn from_steps(s: &Snap) -> Vec<FileDiff> {
    s.steps.iter().filter_map(|it| {
        let x = &it.step;
        let d = x.diff.as_ref().filter(|_| x.is(&["edit", "delete"]))?;
        let lines: Vec<String> = d.split('\n').map(|l| {
            // "+ new" is "+new" in a patch: the marker, then the line without its space.
            match l.chars().next() { Some(m @ ('+' | '-' | ' ')) if chars(l) >= 2 => format!("{m}{}", l.chars().skip(2).collect::<String>()), _ => l.to_owned() }
        }).collect();
        Some(FileDiff {
            path: relative(x.target.as_deref(), &s.folder).or_else(|| x.target.clone()).unwrap_or_else(|| x.title.clone()),
            old: None, status: if x.kind == "delete" { 'D' } else { 'M' }, add: x.added, del: x.removed, binary: false,
            patch: format!("@@ edit @@\n{}", lines.join("\n")),
        })
    }).collect()
}

/// DeskInfo.Walk: the files of a folder that isn't a repository, breadth first, skipping
/// build and tool folders. Links are never followed, so the walk stays in the folder.
fn walk(folder: &Path, more: &mut bool) -> Vec<String> {
    let mut list: Vec<String> = vec![];
    let root = folder.to_path_buf();
    let mut queue = std::collections::VecDeque::from([(root.clone(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if list.len() > TREE_LIMIT { break; }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path();
            // A link to a folder is neither listed nor entered; one to a file is a file.
            if ft.is_symlink() && std::fs::metadata(&path).is_ok_and(|m| m.is_dir()) { continue; }
            if ft.is_dir() {
                let name = e.file_name().to_string_lossy().into_owned();
                if depth < 10 && !SKIP_DIRS.iter().any(|d| d.eq_ignore_ascii_case(&name)) { queue.push_back((path, depth + 1)); }
            } else if let Ok(rel) = path.strip_prefix(&root) {
                list.push(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"));
            }
            if list.len() > TREE_LIMIT { *more = true; break; }
        }
    }
    list
}

// MARK: gh's JSON

fn jstr(e: &Json, name: &str) -> Option<String> { e.get(name).and_then(Json::as_str).map(str::to_owned) }
fn jint(e: &Json, name: &str) -> i32 { e.get(name).and_then(|v| v.i32().ok()).unwrap_or(0) }
fn jbool(e: &Json, name: &str) -> bool { matches!(e.get(name), Some(Json::Bool(true))) }

fn pr_state(e: &Json) -> String {
    match jstr(e, "state").unwrap_or_else(|| "OPEN".into()).to_uppercase().as_str() { "MERGED" => "merged", "CLOSED" => "closed", _ => "open" }.into()
}

/// DeskInfo.Slim: gh's JSON cut to what the panel shows: the checks as a tally and a short list.
fn slim(e: &Json) -> PrDetail {
    let mut d = PrDetail::default();
    if let Some(Json::Arr(roll)) = e.get("statusCheckRollup") {
        for c in roll {
            let conclusion = jstr(c, "conclusion").or_else(|| jstr(c, "state")).unwrap_or_default().to_uppercase();
            let status = jstr(c, "status").unwrap_or_default().to_uppercase();
            let state = match conclusion.as_str() {
                "SUCCESS" => "pass",
                "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => "fail",
                "SKIPPED" | "NEUTRAL" | "STALE" => "skip",
                _ if status == "COMPLETED" => "skip",
                _ => "pending",
            };
            match state { "pass" => d.pass += 1, "fail" => d.fail += 1, "skip" => d.skip += 1, _ => d.pending += 1 }
            if d.checks.len() < 30 {
                d.checks.push(Check { name: jstr(c, "name").or_else(|| jstr(c, "context")).unwrap_or_else(|| "Check".into()), state: state.into(), url: jstr(c, "detailsUrl").or_else(|| jstr(c, "targetUrl")) });
            }
        }
    }
    d.number = jint(e, "number");
    d.title = jstr(e, "title").unwrap_or_default();
    d.state = pr_state(e);
    d.is_draft = jbool(e, "isDraft");
    d.url = jstr(e, "url").unwrap_or_default();
    d.head = jstr(e, "headRefName").unwrap_or_default();
    d.base = jstr(e, "baseRefName").unwrap_or_default();
    d.additions = jint(e, "additions");
    d.deletions = jint(e, "deletions");
    d.changed_files = jint(e, "changedFiles");
    d.body = head(&jstr(e, "body").unwrap_or_default(), 20000).to_owned();
    d.author = e.get("author").and_then(|a| jstr(a, "login"));
    d.review = jstr(e, "reviewDecision").filter(|r| !r.is_empty());
    d.updated_at = jstr(e, "updatedAt");
    d.comments = match e.get("comments") { Some(Json::Arr(c)) => c.len(), _ => 0 };
    d
}
