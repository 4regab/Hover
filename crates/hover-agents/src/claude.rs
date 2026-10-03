//! Claude Code as T3 Code runs it: the `claude` CLI in the Agent SDK's own mode (what
//! @anthropic-ai/claude-agent-sdk's query() starts), JSON lines both ways over stdio
//! with its control protocol. Hover sends `initialize` and `interrupt`, and answers
//! every `can_use_tool` itself, so each permission and each AskUserQuestion comes to
//! Hover. One process per conversation, as the SDK runs one per query: started in the
//! session's folder (Claude Code takes its project from there), kept for the replies
//! that follow, and shut down after the idle time in its settings. A reply after that
//! starts it again with --resume. Prompts go over stdin, never on a command line.
//!
//! What it says is put into ACP's shapes (session/update) and read by KiroStream, so
//! its steps, thoughts, changes and command output show as the other tools' do.
//!
//! Access: Full is bypassPermissions (it never asks; AskUserQuestion still does). Ask
//! first and Ask always are its default mode, where everything past its own read-only
//! defaults asks Hover, and Hover's rules (ask::needs_asking) decide what reaches the
//! user. Read only switches off its edit and command tools (a deny beats any allow rule
//! in the user's settings) and refuses whatever else would change something.

use crate::acp::{default_mcp, Asking, Events, McpFn, Progress};
use crate::agents;
use crate::ask::{self, AgentAsk, AgentQuestion, Answers, AskAnswer};
use crate::cancel::Cancel;
use crate::computer_use;
use crate::opencode::Questioning;
use crate::proc::{strip_ansi, Link};
use crate::stream::{KiroEvent, KiroPhase, KiroResult, KiroStream};
use hover_core::json::{self, Json};
use hover_core::model::{AcpChoice, AcpOption, AgentApproval, AgentOptions, AgentTool, KiroState};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

const NAME: &str = "Claude Code";

/// Starts Claude Code in a folder with these arguments: its pipes, or none when it
/// isn't installed (tests hand in a stand-in).
pub type Connect = Box<dyn Fn(&str, &[String]) -> std::io::Result<Option<Link>> + Send + Sync>;
type Seen = Box<dyn Fn(AgentTool, &[AcpOption]) + Send + Sync>;

/// How long a start (initialize) and a stop wait.
#[derive(Clone, Debug)]
pub struct Timeouts { pub start: Duration, pub stop_grace: Duration }

impl Default for Timeouts {
    fn default() -> Self { Timeouts { start: Duration::from_secs(90), stop_grace: Duration::from_secs(8) } }
}

/// The edit and command tools Read only switches off.
pub const READ_ONLY_DENIED: &str = "Edit,MultiEdit,Write,NotebookEdit,Bash,PowerShell";

/// Which of its tools a conversation's process has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tools { All, ReadOnly, None }

/// How a conversation's process was started. A turn that needs another (a model, an
/// effort or an access picked since) starts a new one on the same conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup { pub folder: String, pub mode: &'static str, pub tools: Tools, pub model: Option<String>, pub effort: Option<String> }

/// The arguments for a process with this setup, carrying on the conversation `resume`
/// names, if any.
pub fn launch_args(s: &Setup, resume: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = agents::arguments(AgentTool::Claude).iter().map(|x| x.to_string()).collect();
    a.extend(["--permission-mode".into(), s.mode.into()]);
    if s.mode == "bypassPermissions" { a.push("--allow-dangerously-skip-permissions".into()); }
    match s.tools {
        Tools::All => {}
        Tools::ReadOnly => a.extend(["--disallowedTools".into(), READ_ONLY_DENIED.into()]),
        // Voice's routing turn: nothing to use at all, reading included.
        Tools::None => a.extend(["--tools".into(), String::new()]),
    }
    if let Some(m) = &s.model { a.extend(["--model".into(), m.clone()]); }
    if let Some(e) = &s.effort { a.extend(["--effort".into(), e.clone()]); }
    // Its own settings, the project's and the local ones, as T3 Code asks for: the
    // user's CLAUDE.md, permissions, hooks and MCP servers apply as in a terminal.
    a.push("--setting-sources=user,project,local".into());
    if let Some(r) = resume { a.push(format!("--resume={r}")); }
    a
}

/// Where the MCP configs Hover hands Claude Code are written: its own folder, which the
/// sandbox lets the tool read but not write. A file, not the command line: the config
/// carries the session's browser token.
pub fn mcp_dir() -> std::path::PathBuf { hover_core::paths::support().join("mcp") }

/// Writes `--mcp-config`'s file for a session (its tag; one file for an untagged start),
/// readable by the user only, replaced whole: its path.
fn mcp_file(tag: Option<&str>, config: &str) -> std::io::Result<String> {
    let dir = mcp_dir();
    std::fs::create_dir_all(&dir)?;
    let name: String = tag.unwrap_or("none").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).take(64).collect();
    let file = dir.join(format!("claude-{name}.json"));
    let tmp = dir.join(format!("claude-{name}.json.tmp"));
    std::fs::write(&tmp, config)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?; }
    std::fs::rename(&tmp, &file)?;
    Ok(file.to_string_lossy().into_owned())
}

fn s<'a>(e: &'a Json, k: &str) -> Option<&'a str> { e.get(k).and_then(Json::as_str) }
fn st(v: &str) -> Json { Json::str(v) }
fn num(e: Option<&Json>, k: &str) -> f64 { match e.and_then(|e| e.get(k)) { Some(v @ Json::Num(_)) => v.f64().unwrap_or(0.0), _ => 0.0 } }
fn arr(e: Option<&Json>) -> &[Json] { match e { Some(Json::Arr(a)) => a, _ => &[] } }
fn log(t: &str) { hover_core::log::line(&format!("claude: {t}")); }

/// Claude Code's tools in ACP's kinds, for the chat's icons and for what asks.
pub fn kind_of(tool: &str) -> &'static str {
    match tool {
        "Read" | "NotebookRead" => "read",
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "edit",
        "Bash" | "PowerShell" | "BashOutput" | "KillShell" | "KillBash" | "Monitor" => "execute",
        "Glob" | "Grep" | "LS" | "ToolSearch" => "search",
        "WebFetch" | "WebSearch" => "fetch",
        "Task" | "Agent" => "agent",
        "EnterPlanMode" | "ExitPlanMode" => "switch_mode",
        t if t == "TodoWrite" || t == "TodoRead" || t.starts_with("Task") => "think",
        _ => "other",
    }
}

/// A tool call's title: what it does, a subagent by what it was asked, an MCP tool as
/// "@server/tool" (as Kiro names them), anything else by its own name.
pub fn title_of(tool: &str, input: &Json) -> String {
    match tool {
        "Bash" | "PowerShell" => s(input, "description").filter(|d| !d.is_empty()).unwrap_or("Run a command").to_owned(),
        "Task" | "Agent" => s(input, "description").filter(|d| !d.is_empty()).unwrap_or("Subagent").to_owned(),
        "Write" => "Write".into(),
        "Edit" | "MultiEdit" | "NotebookEdit" => "Edit".into(),
        "WebSearch" => "Search the web".into(),
        "WebFetch" => "Fetch".into(),
        "Glob" | "Grep" => "Search".into(),
        "ExitPlanMode" => "Leave plan mode and start on the plan".into(),
        "EnterPlanMode" => "Plan first".into(),
        t => match t.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((server, name)) => format!("@{server}/{name}"),
            None => t.to_owned(),
        },
    }
}

/// What a permission asks for, in the words the notch uses for every tool's.
fn ask_title(tool: &str, input: &Json) -> String {
    match kind_of(tool) {
        "execute" => "Run a command".into(),
        "edit" if tool == "Write" => "Write a file".into(),
        "edit" => "Edit a file".into(),
        "fetch" => "Use the network".into(),
        "agent" => "Start a subagent".into(),
        _ => title_of(tool, input),
    }
}

/// The change an edit makes, as ACP diff content: Write's whole file, Edit's (and each
/// of MultiEdit's) old and new strings.
fn diff_content(tool: &str, input: &Json) -> Vec<Json> {
    let d = |old: Option<&str>, new: &str| Json::obj(vec![("type", st("diff")), ("oldText", Json::opt_str_of(old)), ("newText", st(new))]);
    match tool {
        "Write" => s(input, "content").map(|c| vec![d(None, c)]).unwrap_or_default(),
        "Edit" => s(input, "new_string").map(|n| vec![d(s(input, "old_string"), n)]).unwrap_or_default(),
        "MultiEdit" => arr(input.get("edits")).iter().filter_map(|e| s(e, "new_string").map(|n| d(s(e, "old_string"), n))).collect(),
        _ => vec![],
    }
}

/// A tool call as an ACP tool_call, for KiroStream and for ask::describe.
fn acp_call(id: &str, tool: &str, input: &Json) -> Json {
    let path = ["file_path", "notebook_path", "path"].iter().find_map(|k| s(input, k));
    let mut props = vec![("toolCallId", st(id)), ("kind", st(kind_of(tool))), ("title", st(&title_of(tool, input))), ("rawInput", input.clone())];
    if let Some(p) = path { props.push(("locations", Json::Arr(vec![Json::obj(vec![("path", st(p))])]))); }
    let diff = diff_content(tool, input);
    if !diff.is_empty() { props.push(("content", Json::Arr(diff))); }
    Json::obj(props)
}

fn update(u: Json) -> String { Json::obj(vec![("method", st("session/update")), ("params", Json::obj(vec![("update", u)]))]).compact() }

fn with(mut u: Json, k: &str, v: Json) -> Json {
    if let Json::Obj(p) = &mut u { p.retain(|(n, _)| n != k); p.push((k.into(), v)); }
    u
}

/// The text of a tool_result's content (a string, or text blocks).
fn result_text(c: Option<&Json>) -> String {
    match c {
        Some(Json::Str(t)) => t.clone(),
        Some(Json::Arr(parts)) => parts.iter().filter_map(|p| s(p, "text")).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// How a turn ended: its result message, or its process gone (with why).
#[derive(Clone)]
enum Ended { Result(Json), Gone(String), Stopped }

struct Turn {
    stream: Mutex<KiroStream>,
    progress: Option<Progress>,
    events: Option<Events>,
    options: AgentOptions,
    folder: String,
    token: Cancel,
    deny_all: bool,
    refused: AtomicBool,
    ended: Mutex<Option<Ended>>,
    cv: Condvar,
    /// The tokens of the last answer, for the context gauge.
    tokens: Mutex<f64>,
    /// Messages whose text and thinking came as they were said (stream events).
    streamed: Mutex<HashSet<String>>,
    message: Mutex<String>,
    /// Each tool call's tool and input, by its id.
    calls: Mutex<HashMap<String, (String, Json)>>,
}

impl Turn {
    fn end(&self, e: Ended) {
        let mut g = self.ended.lock().unwrap();
        if g.is_none() { *g = Some(e); self.cv.notify_all(); }
    }
    fn is_over(&self) -> bool { self.ended.lock().unwrap().is_some() }
    fn wait(&self) -> Ended { self.cv.wait_while(self.ended.lock().unwrap(), |e| e.is_none()).unwrap().clone().unwrap() }
    fn wait_for(&self, d: Duration) -> bool { self.cv.wait_timeout_while(self.ended.lock().unwrap(), d, |e| e.is_none()).unwrap().0.is_some() }

    /// One ACP update through the stream; its phase and events passed on.
    fn feed(&self, u: Json) {
        let (phase, events) = { let mut k = self.stream.lock().unwrap(); (k.feed(&update(u)), k.drain()) };
        if let (Some(p), Some(f)) = (phase, &self.progress) { f(p); }
        if let Some(f) = &self.events { for e in events { f(e); } }
    }

    fn said(&self) -> String { self.stream.lock().unwrap().said().trim().to_owned() }

    fn stopped(&self) -> KiroResult {
        let said = self.said();
        KiroResult::new(KiroState::Cancelled, if said.is_empty() { format!("Stopped before {NAME} finished.") } else { said })
    }
}

/// A conversation's process.
struct Proc {
    gen: u64,
    setup: Setup,
    /// The MCP servers it was started with (computer_use::signature).
    mcp: String,
    writer: Mutex<Box<dyn Write + Send>>,
    kill: Box<dyn Fn() + Send + Sync>,
    errors: Box<dyn Fn() -> String + Send + Sync>,
    sid: Mutex<Option<String>>,
    turn: Mutex<Option<Arc<Turn>>>,
    pending: Mutex<HashMap<String, mpsc::Sender<Result<Json, String>>>>,
    /// Permission requests still open, by request id: control_cancel_request withdraws one.
    open: Mutex<HashMap<String, Cancel>>,
    ids: AtomicU64,
    /// Goes up with each turn, so an idle timer set before one doesn't end it.
    uses: AtomicU64,
    used: Mutex<Instant>,
    dead: AtomicBool,
}

impl Proc {
    fn write(&self, m: &Json) -> bool {
        let mut b = m.compact().into_bytes();
        b.push(b'\n');
        let mut w = self.writer.lock().unwrap();
        w.write_all(&b).and_then(|_| w.flush()).is_ok()
    }

    fn end(&self, why: &str) {
        if self.dead.swap(true, Ordering::SeqCst) { return; }
        log(&format!("{} - {why}", self.sid.lock().unwrap().as_deref().unwrap_or("new conversation")));
        (self.kill)();
    }

    /// A control request, and its answer: Err when it failed, the process went, the
    /// time ran out or the token was cancelled.
    fn request(&self, subtype: &str, mut extra: Vec<(&str, Json)>, timeout: Duration, ct: Option<&Cancel>) -> Result<Json, String> {
        let id = format!("hover-{}", self.ids.fetch_add(1, Ordering::SeqCst) + 1);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx.clone());
        extra.insert(0, ("subtype", st(subtype)));
        let m = Json::obj(vec![("type", st("control_request")), ("request_id", st(&id)), ("request", Json::obj(extra))]);
        let _reg = ct.map(|c| c.on_cancel(move || { let _ = tx.send(Err("cancelled".into())); }));
        if self.dead.load(Ordering::SeqCst) || !self.write(&m) { self.pending.lock().unwrap().remove(&id); return Err(format!("{NAME} stopped.")); }
        let got = rx.recv_timeout(timeout);
        self.pending.lock().unwrap().remove(&id);
        match got {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Err(format!("{NAME} didn’t answer ({subtype}).")),
            Err(RecvTimeoutError::Disconnected) => Err(format!("{NAME} stopped.")),
        }
    }

    /// The last lines it wrote on stderr, as why it stopped.
    fn why(&self) -> String {
        let text = strip_ansi(&(self.errors)());
        let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        lines[lines.len().saturating_sub(2)..].join("\n")
    }
}

struct Host {
    options: Box<dyn Fn() -> AgentOptions + Send + Sync>,
    connect: Connect,
    t: Timeouts,
    procs: Mutex<Vec<Arc<Proc>>>,
    gens: AtomicU64,
    /// What the user trusted for the rest of a conversation, by its session id.
    trusted: Mutex<HashMap<String, HashSet<String>>>,
    /// The models it offered at its last start, with their efforts.
    models: Mutex<Vec<AcpChoice>>,
    seen: Mutex<Vec<Seen>>,
    asking: Mutex<Option<Asking>>,
    questioning: Mutex<Option<Questioning>>,
    /// The MCP servers each conversation's process is handed (--mcp-config): computer use's
    /// and Hover's browser.
    mcp: Mutex<McpFn>,
}

/// Claude Code's runtime: shared by every Claude Code session, one process per conversation.
#[derive(Clone)]
pub struct ClaudeHost(Arc<Host>);

/// At most this many conversations keep a process: an idle one goes when another needs
/// one (each is a few hundred MB), and a reply to it starts it again.
pub const MAX_LIVE: usize = 3;

impl ClaudeHost {
    /// Claude Code as Agents finds and starts it.
    pub fn new(options: impl Fn() -> AgentOptions + Send + Sync + 'static) -> ClaudeHost {
        ClaudeHost::with_connect(options, |folder: &str, args: &[String]| match agents::exe(AgentTool::Claude) {
            None => Ok(None),
            Some(exe) => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                // In the sandbox when it is wanted (sandbox.rs): started in the session's
                // folder, which the sandbox opens for it.
                crate::sandbox::launch(AgentTool::Claude, &exe, &args, &[], Some(std::path::Path::new(folder)), None).map(Some)
            }
        }, Timeouts::default())
    }

    /// With the process given (tests hand in a stand-in), and the waits.
    pub fn with_connect(options: impl Fn() -> AgentOptions + Send + Sync + 'static,
        connect: impl Fn(&str, &[String]) -> std::io::Result<Option<Link>> + Send + Sync + 'static, t: Timeouts) -> ClaudeHost {
        ClaudeHost(Arc::new(Host {
            options: Box::new(options), connect: Box::new(connect), t, procs: Mutex::new(vec![]), gens: AtomicU64::new(0), trusted: Default::default(),
            models: Mutex::new(vec![]), seen: Mutex::new(vec![]), asking: Mutex::new(None), questioning: Mutex::new(None), mcp: Mutex::new(default_mcp(AgentTool::Claude)),
        }))
    }

    /// The MCP servers each conversation's process is handed, read as it starts (the
    /// default: Cua Driver's when computer use is on, and Hover's browser).
    pub fn set_mcp(&self, f: impl Fn(Option<&str>) -> Vec<computer_use::McpServer> + Send + Sync + 'static) { *self.0.mcp.lock().unwrap() = Arc::new(f); }

    pub fn tool(&self) -> AgentTool { AgentTool::Claude }
    /// Some conversation's process is up.
    pub fn alive(&self) -> bool { self.0.procs.lock().unwrap().iter().any(|p| !p.dead.load(Ordering::SeqCst)) }
    /// How many conversations have a process now.
    pub fn live(&self) -> usize { self.0.procs.lock().unwrap().iter().filter(|p| !p.dead.load(Ordering::SeqCst)).count() }
    pub fn on_options_seen(&self, f: impl Fn(AgentTool, &[AcpOption]) + Send + Sync + 'static) { self.0.seen.lock().unwrap().push(Box::new(f)); }
    pub fn set_asking(&self, f: Asking) { *self.0.asking.lock().unwrap() = Some(f); }
    pub fn set_questioning(&self, f: Questioning) { *self.0.questioning.lock().unwrap() = Some(f); }

    /// Ends every conversation's process now. Runs still going fail; the next one starts again.
    pub fn shutdown(&self, why: &str) {
        let all: Vec<Arc<Proc>> = std::mem::take(&mut *self.0.procs.lock().unwrap());
        for p in all { p.end(why); }
    }

    /// Runs one turn: a new conversation, or the one resume names. Never fails outright.
    /// Blocks: run it off the UI thread.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access, None)
    }

    /// run, naming the Hover session (its key) the run is for: the tag Hover's browser
    /// server is made for.
    #[allow(clippy::too_many_arguments)]
    pub fn run_tagged(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>, tag: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access, tag)
    }

    pub fn runner(&self) -> crate::session::RunTask {
        let h = self.clone();
        Arc::new(move |a: crate::session::RunArgs| { let tag = crate::runtime::tag_of(&a); h.run_tagged(&a.folder, &a.prompt, Some(a.progress), &a.ct, a.resume.as_deref(), Some(a.events), a.access.as_deref(), tag.as_deref()) })
    }
}

/// Said under the answer when the conversation it carried on was gone.
const LOST: &str = "*Claude Code no longer had the earlier conversation, so this reply started a new one.*";

impl Host {
    #[allow(clippy::too_many_arguments)]
    fn run(self: &Arc<Self>, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>, tag: Option<&str>) -> KiroResult {
        if !crate::usable_folder(Some(folder)) { return KiroResult::new(KiroState::Failed, "That folder isn’t there any more. Choose another one."); }
        if prompt.trim().is_empty() { return KiroResult::new(KiroState::Failed, format!("Tell {NAME} what to do first.")); }
        // The sandbox of the process this run starts opens this folder.
        crate::sandbox::remember(folder);
        if crate::sandbox::wanted() && crate::agents::toggles().computer_use && !crate::spaces::wanted() { computer_use::ensure_daemon(); }
        let o = (self.options)().with_access(access);
        let turn = Arc::new(Turn {
            stream: Mutex::new(KiroStream::new(NAME)), progress, events, options: o.clone(), folder: folder.into(), token: ct.clone(), deny_all: access == Some("none"),
            refused: AtomicBool::new(false), ended: Mutex::new(None), cv: Condvar::new(), tokens: Mutex::new(0.0), streamed: Default::default(),
            message: Mutex::new(String::new()), calls: Default::default(),
        });
        if let Some(p) = &turn.progress { p(KiroPhase::Starting); }
        let r = self.turn(&turn, prompt, resume, &o, tag);
        // A thought still open when the turn ends (however it ends) ends with it.
        let last = { let mut k = turn.stream.lock().unwrap(); k.end(); k.drain() };
        if let Some(f) = &turn.events { for e in last { f(e); } }
        r
    }

    fn setup(&self, folder: &str, o: &AgentOptions, deny_all: bool) -> Setup {
        let tools = if deny_all { Tools::None } else if o.read_only { Tools::ReadOnly } else { Tools::All };
        let mode = if tools == Tools::All && o.approval == AgentApproval::Autopilot { "bypassPermissions" } else { "default" };
        // Only an effort the picked model takes (none for one without), once its list is known.
        let models = self.models.lock().unwrap();
        let m = models.iter().find(|c| Some(&c.value) == o.model.as_ref()).or_else(|| models.iter().find(|c| o.model.is_none() && c.value == "default"));
        let effort = o.effort.clone().filter(|e| models.is_empty() || m.is_some_and(|m| m.levels.as_ref().is_some_and(|l| l.contains(e))));
        Setup { folder: folder.into(), mode, tools, model: o.model.clone().filter(|m| !m.is_empty() && m != "default"), effort }
    }

    fn turn(self: &Arc<Self>, turn: &Arc<Turn>, prompt: &str, resume: Option<&str>, o: &AgentOptions, tag: Option<&str>) -> KiroResult {
        let setup = self.setup(&turn.folder, o, turn.deny_all);
        let ct = turn.token.clone();
        let mut servers = (self.mcp.lock().unwrap().clone())(tag);
        // The project's desktop for a session's run (not a routing turn, which has no tag):
        // every agent in that folder shares it, each with a cursor of its own.
        if let Some(t) = tag { servers.extend(crate::spaces::servers(Some(&turn.folder), t)); }
        let (proc, lost) = match self.take(&setup, resume, &ct, &servers, tag) {
            Ok(x) => x,
            Err(_) if ct.is_cancelled() => return turn.stopped(),
            Err(m) => return KiroResult::new(KiroState::Failed, explain(&m)),
        };
        *proc.turn.lock().unwrap() = Some(turn.clone());
        proc.uses.fetch_add(1, Ordering::SeqCst);
        if let Some(sid) = proc.sid.lock().unwrap().clone() {
            if let Some(e) = &turn.events { e(KiroEvent { session_id: Some(sid), ..Default::default() }); }
        }
        let message = Json::obj(vec![("type", st("user")), ("message", Json::obj(vec![("role", st("user")),
            ("content", Json::Arr(vec![Json::obj(vec![("type", st("text")), ("text", st(prompt.trim()))])]))])), ("parent_tool_use_id", Json::Null), ("session_id", st(""))]);
        let ended = if !proc.write(&message) {
            Ended::Gone(format!("{NAME} stopped."))
        } else {
            let (me, p2, t2) = (Arc::downgrade(self), proc.clone(), turn.clone());
            let _reg = ct.on_cancel(move || { std::thread::spawn(move || if let Some(h) = me.upgrade() { h.stop(&p2, &t2) }); });
            turn.wait()
        };
        *proc.turn.lock().unwrap() = None;
        *proc.used.lock().unwrap() = Instant::now();
        if !proc.dead.load(Ordering::SeqCst) { self.schedule_idle(&proc); }
        let mut r = match ended {
            Ended::Result(m) => self.finish(turn, &m),
            Ended::Stopped => turn.stopped(),
            Ended::Gone(_) if ct.is_cancelled() => turn.stopped(),
            Ended::Gone(why) => KiroResult::new(KiroState::Failed, explain(&why)),
        };
        if lost { r.text = format!("{}\n\n{LOST}", r.text.trim_end()); }
        r
    }

    /// The conversation's process: the one it already has when it was started the same
    /// way, else a new one (with --resume when there is a conversation to carry on).
    /// True with it when that conversation was gone, so a new one began.
    fn take(self: &Arc<Self>, setup: &Setup, resume: Option<&str>, ct: &Cancel, servers: &[computer_use::McpServer], tag: Option<&str>) -> Result<(Arc<Proc>, bool), String> {
        let resume = resume.filter(|r| !r.is_empty());
        let mcp = computer_use::signature(servers);
        {
            let mut procs = self.procs.lock().unwrap();
            procs.retain(|p| !p.dead.load(Ordering::SeqCst));
            if let Some(r) = resume {
                if let Some(i) = procs.iter().position(|p| p.sid.lock().unwrap().as_deref() == Some(r)) {
                    let p = procs[i].clone();
                    if p.setup == *setup && p.mcp == mcp && p.turn.lock().unwrap().is_none() { return Ok((p, false)); }
                    // Started another way (a new model, effort or access, or MCP servers
                    // switched since): this one goes, and the conversation carries on in a
                    // process started as asked.
                    procs.remove(i);
                    p.end("restarted with new settings");
                }
            }
            // Room for one more: the least recently used idle one goes.
            while procs.len() >= MAX_LIVE {
                let Some(i) = procs.iter().enumerate().filter(|(_, p)| p.turn.lock().unwrap().is_none()).min_by_key(|(_, p)| *p.used.lock().unwrap()).map(|(i, _)| i) else { break };
                procs.remove(i).end("making room for another conversation");
            }
        }
        match self.start(setup, resume, ct, servers, tag) {
            Err(m) if resume.is_some() && m.contains("No conversation found") => {
                log(&format!("{} is gone; a new conversation", resume.unwrap()));
                self.start(setup, None, ct, servers, tag).map(|p| (p, true))
            }
            r => r.map(|p| (p, false)),
        }
    }

    fn start(self: &Arc<Self>, setup: &Setup, resume: Option<&str>, ct: &Cancel, servers: &[computer_use::McpServer], tag: Option<&str>) -> Result<Arc<Proc>, String> {
        if ct.is_cancelled() { return Err("cancelled".into()); }
        let mut args = launch_args(setup, resume);
        if let Some(config) = computer_use::claude_config(servers) {
            match mcp_file(tag, &config) {
                Ok(f) => args.extend(["--mcp-config".into(), f]),
                Err(e) => log(&format!("couldn’t write its MCP servers ({e}); it starts without them")),
            }
        }
        let link = match (self.connect)(&setup.folder, &args) {
            Err(e) => return Err(format!("{NAME} couldn’t start: {e}")),
            Ok(None) => return Err(format!("{NAME} isn’t installed. {}", agents::install_hint(AgentTool::Claude))),
            Ok(Some(l)) => l,
        };
        let Link { to_agent, from_agent, kill, errors } = link;
        let proc = Arc::new(Proc {
            gen: self.gens.fetch_add(1, Ordering::SeqCst) + 1, setup: setup.clone(), mcp: computer_use::signature(servers), writer: Mutex::new(to_agent), kill, errors,
            sid: Mutex::new(resume.map(str::to_owned)), turn: Mutex::new(None), pending: Default::default(), open: Default::default(),
            ids: AtomicU64::new(0), uses: AtomicU64::new(0), used: Mutex::new(Instant::now()), dead: AtomicBool::new(false),
        });
        let (me, p2) = (Arc::downgrade(self), proc.clone());
        std::thread::Builder::new().name("claude-read".into()).spawn(move || read(me, p2, from_agent)).map_err(|e| e.to_string())?;
        match proc.request("initialize", vec![("hooks", Json::Null)], self.t.start, Some(ct)) {
            Ok(r) => self.offered(&r),
            Err(e) => {
                // Gone before it answered: what it said on its way out is why (a
                // conversation --resume couldn't find, a setting it refused).
                std::thread::sleep(Duration::from_millis(50));
                let why = proc.why();
                proc.end("didn't start");
                return Err(if why.is_empty() || e == "cancelled" { e } else { why });
            }
        }
        log(&format!("started #{} in {} ({})", proc.gen, setup.folder, resume.map_or("new conversation".into(), |r| format!("resuming {r}"))));
        self.procs.lock().unwrap().push(proc.clone());
        Ok(proc)
    }

    /// The models initialize lists, each with the efforts it takes, for Settings and
    /// the new-task box.
    fn offered(&self, r: &Json) {
        let models: Vec<AcpChoice> = arr(r.get("models")).iter().filter_map(|m| {
            let v = s(m, "value").filter(|v| !v.is_empty())?;
            let levels: Vec<String> = arr(m.get("supportedEffortLevels")).iter().filter_map(Json::as_str).map(str::to_owned).collect();
            Some(AcpChoice { value: v.into(), name: s(m, "displayName").unwrap_or(v).into(), levels: Some(levels) })
        }).collect();
        if models.is_empty() { return; }
        *self.models.lock().unwrap() = models.clone();
        let offers = [AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: models }];
        for f in self.seen.lock().unwrap().iter() { f(AgentTool::Claude, &offers); }
    }

    /// Stop: Claude Code is asked to interrupt the turn, and the questions it left are
    /// withdrawn (their tokens are the turn's). One that hasn't ended the turn within
    /// the grace period is ended: the process is this conversation's alone.
    fn stop(&self, proc: &Arc<Proc>, turn: &Arc<Turn>) {
        if turn.is_over() { return; }
        if let Err(e) = proc.request("interrupt", vec![], Duration::from_secs(5), None) { log(&format!("interrupt - {e}")); }
        if !turn.wait_for(self.t.stop_grace) {
            log(&format!("didn't stop within {:.0}s", self.t.stop_grace.as_secs_f64()));
            proc.end("didn't stop when asked");
            turn.end(Ended::Stopped);
        }
    }

    fn schedule_idle(self: &Arc<Self>, proc: &Arc<Proc>) {
        let uses = proc.uses.load(Ordering::SeqCst);
        let after = Duration::from_secs(60 * (self.options)().idle_minutes.max(1) as u64);
        let (me, p) = (Arc::downgrade(self), Arc::downgrade(proc));
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let (Some(h), Some(p)) = (me.upgrade(), p.upgrade()) else { return };
            if p.uses.load(Ordering::SeqCst) == uses && p.turn.lock().unwrap().is_none() {
                h.procs.lock().unwrap().retain(|x| !Arc::ptr_eq(x, &p));
                p.end("idle");
            }
        });
    }

    fn finish(&self, turn: &Turn, m: &Json) -> KiroResult {
        // The context the last answer filled, of the model's window.
        let window = match m.get("modelUsage") { Some(Json::Obj(u)) => u.iter().map(|(_, x)| num(Some(x), "contextWindow")).fold(0.0, f64::max), _ => 0.0 };
        let used = *turn.tokens.lock().unwrap();
        if window > 0.0 && used > 0.0 {
            turn.feed(Json::obj(vec![("sessionUpdate", st("usage_update")), ("used", Json::double(used)), ("size", Json::double(window))]));
        }
        if turn.token.is_cancelled() { return turn.stopped(); }
        let text = s(m, "result").unwrap_or("").trim().to_owned();
        if m.get("is_error") == Some(&Json::Bool(true)) {
            let errors: Vec<&str> = arr(m.get("errors")).iter().filter_map(Json::as_str).filter(|e| !e.starts_with("[ede_diagnostic]")).collect();
            let why = match s(m, "subtype") {
                Some("error_max_turns") => format!("{NAME} stopped after the most turns it may take."),
                Some("error_max_budget_usd") => format!("{NAME} stopped at its spending limit."),
                _ if !text.is_empty() => text,
                _ if !errors.is_empty() => errors.join("\n"),
                _ => format!("{NAME} couldn’t finish."),
            };
            return KiroResult::new(KiroState::Failed, explain(&why));
        }
        if s(m, "stop_reason") == Some("refusal") { return KiroResult::new(KiroState::Failed, format!("{NAME} declined this request.")); }
        let mut said = crate::stream::clip(if text.is_empty() { turn.said() } else { text }.trim(), 20000);
        let refused = turn.refused.load(Ordering::SeqCst);
        if refused && said.is_empty() {
            return KiroResult::new(KiroState::Failed, format!("{NAME} wanted to change files or run a command, and it is set to read only (Settings → {NAME})."));
        }
        // The model's own words can claim it did what read only refused.
        if refused { said.push_str(&format!("\n\n*Hover has {NAME} set to read only, so the changes or commands it tried were refused.*")); }
        KiroResult::new(KiroState::Completed, if said.is_empty() { format!("Done. {NAME} didn’t leave a summary.") } else { said })
    }

    // MARK: What it says

    fn handle(self: &Arc<Self>, proc: &Arc<Proc>, line: &str) {
        let Ok(m @ Json::Obj(_)) = json::parse(line) else { return };
        let turn = proc.turn.lock().unwrap().clone();
        match s(&m, "type") {
            Some("control_response") => {
                let Some(r) = m.get("response") else { return };
                let Some(tx) = s(r, "request_id").and_then(|id| proc.pending.lock().unwrap().remove(id)) else { return };
                let _ = tx.send(if s(r, "subtype") == Some("success") { Ok(r.get("response").cloned().unwrap_or(Json::Null)) }
                    else { Err(s(r, "error").unwrap_or("It refused.").to_owned()) });
            }
            Some("control_request") => {
                let Some(id) = s(&m, "request_id").map(str::to_owned) else { return };
                let req = m.get("request").cloned().unwrap_or(Json::Null);
                if s(&req, "subtype") == Some("can_use_tool") {
                    // Answered on its own thread: the user may take minutes, and the
                    // rest of what it says comes down this same pipe meanwhile.
                    let (me, p) = (self.clone(), proc.clone());
                    std::thread::Builder::new().name("claude-permission".into()).spawn(move || {
                        let answer = me.permission(&p, &id, turn.as_deref(), &req);
                        p.open.lock().unwrap().remove(&id);
                        p.write(&Json::obj(vec![("type", st("control_response")), ("response", Json::obj(vec![("subtype", st("success")), ("request_id", st(&id)), ("response", answer)]))]));
                    }).expect("a thread for the question");
                } else {
                    // Hooks and SDK MCP servers are the SDK's; Hover registers none.
                    proc.write(&Json::obj(vec![("type", st("control_response")), ("response", Json::obj(vec![("subtype", st("error")), ("request_id", st(&id)), ("error", st("Not supported by Hover."))]))]));
                }
            }
            Some("control_cancel_request") => {
                let c = s(&m, "request_id").and_then(|id| proc.open.lock().unwrap().get(id).cloned());
                if let Some(c) = c { c.cancel(); }
            }
            Some("system") if s(&m, "subtype") == Some("init") => {
                let Some(sid) = s(&m, "session_id").filter(|x| !x.is_empty()).map(str::to_owned) else { return };
                let changed = proc.sid.lock().unwrap().replace(sid.clone()).as_deref() != Some(&sid);
                if let (true, Some(t)) = (changed, &turn) { if let Some(e) = &t.events { e(KiroEvent { session_id: Some(sid), ..Default::default() }); } }
            }
            Some("result") => { if let Some(t) = turn { t.end(Ended::Result(m)); } }
            Some(kind @ ("stream_event" | "assistant" | "user")) => {
                // A subagent's own messages stay inside its step.
                if !matches!(m.get("parent_tool_use_id"), None | Some(Json::Null)) { return; }
                let Some(t) = turn else { return };
                match kind { "stream_event" => said_now(&t, m.get("event")), "assistant" => answer(&t, m.get("message")), _ => results(&t, &m) }
            }
            _ => {}
        }
    }

    /// Read only allows reading; otherwise what the access setting leaves alone is
    /// allowed and the rest goes to the user, unless they trusted it earlier in the
    /// conversation. AskUserQuestion goes to the user as it is. A stopped run, or the
    /// request withdrawn, answers no.
    fn permission(&self, proc: &Proc, id: &str, turn: Option<&Turn>, r: &Json) -> Json {
        let input = r.get("input").cloned().unwrap_or_else(|| Json::obj(vec![]));
        let allow = |input: Json| Json::obj(vec![("behavior", st("allow")), ("updatedInput", input)]);
        let deny = |why: &str| Json::obj(vec![("behavior", st("deny")), ("message", st(why))]);
        let Some(turn) = turn else { return deny("Hover has no task running for this.") };
        let tool = s(r, "tool_name").unwrap_or("").to_owned();
        let token = Cancel::new();
        proc.open.lock().unwrap().insert(id.into(), token.clone());
        let t2 = token.clone();
        let _stop = turn.token.on_cancel(move || t2.cancel());
        let sid = proc.sid.lock().unwrap().clone().unwrap_or_default();

        if tool == "AskUserQuestion" {
            let q = self.questioning.lock().unwrap().clone();
            let questions = questions_of(&input);
            let (Some(q), false, Some(first)) = (q, turn.deny_all, questions.first().cloned()) else { return deny("Hover can’t ask the user this now.") };
            let ask = AgentAsk { id: s(r, "tool_use_id").unwrap_or(id).into(), kind: "question".into(), title: first.header.clone(), command: None, path: None,
                preview: None, added: 0, removed: 0, reason: first.question.clone(), danger: false, questions: Some(questions.clone()) };
            let (tx, rx) = mpsc::channel::<Option<Answers>>();
            let tx2 = tx.clone();
            let _w = token.on_cancel(move || { let _ = tx2.send(None); });
            q(&sid, ask, &token, Box::new(move |a| { let _ = tx.send(Some(a)); }));
            return match rx.recv() {
                Ok(Some(Some(picked))) if !token.is_cancelled() && !picked.is_empty() => {
                    // Answers by each question's own text, several picks as one, as
                    // Claude Code reads them (T3 Code's handleAskUserQuestion).
                    let answers: Vec<(String, Json)> = questions.iter().zip(picked.iter()).map(|(q, a)| (q.question.clone(), st(&a.join(", ")))).collect();
                    allow(with(input, "answers", Json::Obj(answers)))
                }
                Ok(Some(_)) if !token.is_cancelled() => deny("The user skipped the question."),
                _ => deny("The question was withdrawn."),
            };
        }

        let kind = kind_of(&tool);
        if turn.deny_all { turn.refused.store(true, Ordering::SeqCst); return deny("Hover lets this task use no tools."); }
        if turn.options.read_only {
            if matches!(kind, "read" | "search" | "fetch" | "think" | "switch_mode") { return allow(input); }
            turn.refused.store(true, Ordering::SeqCst);
            return deny(&format!("Hover has {NAME} set to read only: it can read and search, not change files or run commands."));
        }
        let mut call = acp_call(s(r, "tool_use_id").unwrap_or(id), &tool, &input);
        // A command names the path it touches (blocked_path): inside the folder or not.
        if let Some(b) = s(r, "blocked_path").filter(|b| !b.is_empty() && call.get("locations").is_none()) {
            call = with(call, "locations", Json::Arr(vec![Json::obj(vec![("path", st(b))])]));
        }
        let (mut question, outside) = ask::describe(&call, kind, &turn.folder);
        question.title = ask_title(&tool, &input);
        if !ask::needs_asking(turn.options.approval, kind, outside) { return allow(input); }
        let key = ask::key(&question);
        if self.trusted.lock().unwrap().get(&sid).is_some_and(|k| k.contains("*") || k.contains(&key)) { return allow(input); }
        let asking = self.asking.lock().unwrap().clone();
        let Some(asking) = asking.filter(|_| !sid.is_empty()) else { return deny("Hover had nobody to ask.") };
        let (tx, rx) = mpsc::channel::<Option<AskAnswer>>();
        let tx2 = tx.clone();
        let _w = token.on_cancel(move || { let _ = tx2.send(None); });
        asking(&sid, question, &token, Box::new(move |a| { let _ = tx.send(Some(a)); }));
        // Trust is Hover's, for the conversation: Claude Code's own suggestions would
        // write a rule into the user's settings, which a click in the notch must never do.
        match rx.recv() {
            Ok(Some(a)) if !token.is_cancelled() => match a {
                AskAnswer::Allow => allow(input),
                AskAnswer::Trust | AskAnswer::TrustAll => {
                    self.trusted.lock().unwrap().entry(sid).or_default().insert(if a == AskAnswer::Trust { key } else { "*".into() });
                    allow(input)
                }
                AskAnswer::Deny => deny("The user declined this."),
            },
            _ => deny("The request was withdrawn."),
        }
    }
}

/// What it is saying now (partial messages): the answer's text and its thinking, as
/// they come.
fn said_now(t: &Turn, e: Option<&Json>) {
    let Some(e) = e else { return };
    match s(e, "type") {
        Some("message_start") => { *t.message.lock().unwrap() = e.get("message").and_then(|m| s(m, "id")).unwrap_or("").to_owned(); }
        Some("content_block_delta") => {
            let Some(d) = e.get("delta") else { return };
            let mid = t.message.lock().unwrap().clone();
            let (kind, text) = match s(d, "type") {
                Some("text_delta") => ("agent_message_chunk", s(d, "text")),
                Some("thinking_delta") => ("agent_thought_chunk", s(d, "thinking")),
                _ => return,
            };
            let Some(text) = text else { return };
            t.streamed.lock().unwrap().insert(mid.clone());
            t.feed(Json::obj(vec![("sessionUpdate", st(kind)), ("messageId", st(&mid)), ("content", Json::obj(vec![("type", st("text")), ("text", st(text))]))]));
        }
        _ => {}
    }
}

/// A finished message part: a tool call starts a step; text and thinking only when they
/// didn't come as they were said.
fn answer(t: &Turn, m: Option<&Json>) {
    let Some(m) = m else { return };
    let mid = s(m, "id").unwrap_or("").to_owned();
    if let Some(u) = m.get("usage") {
        let used = num(Some(u), "input_tokens") + num(Some(u), "cache_creation_input_tokens") + num(Some(u), "cache_read_input_tokens") + num(Some(u), "output_tokens");
        if used > 0.0 { *t.tokens.lock().unwrap() = used; }
    }
    let streamed = t.streamed.lock().unwrap().contains(&mid);
    for b in arr(m.get("content")) {
        match s(b, "type") {
            Some("tool_use") => {
                let (Some(id), Some(tool)) = (s(b, "id"), s(b, "name")) else { continue };
                let input = b.get("input").cloned().unwrap_or_else(|| Json::obj(vec![]));
                t.calls.lock().unwrap().insert(id.into(), (tool.into(), input.clone()));
                if tool == "AskUserQuestion" { continue; }
                t.feed(with(with(acp_call(id, tool, &input), "sessionUpdate", st("tool_call")), "status", st("in_progress")));
            }
            Some("text") if !streamed => {
                if let Some(x) = s(b, "text") { t.feed(Json::obj(vec![("sessionUpdate", st("agent_message_chunk")), ("messageId", st(&mid)), ("content", Json::obj(vec![("type", st("text")), ("text", st(x))]))])); }
            }
            Some("thinking") if !streamed => {
                if let Some(x) = s(b, "thinking") { t.feed(Json::obj(vec![("sessionUpdate", st("agent_thought_chunk")), ("content", Json::obj(vec![("type", st("text")), ("text", st(x))]))])); }
            }
            _ => {}
        }
    }
}

/// Tool results: each step ends, with what a command printed and the change an edit
/// made where Claude Code says (its tool_use_result).
fn results(t: &Turn, m: &Json) {
    let full = m.get("tool_use_result");
    for b in arr(m.get("message").and_then(|x| x.get("content"))) {
        if s(b, "type") != Some("tool_result") { continue; }
        let Some(id) = s(b, "tool_use_id") else { continue };
        let Some((tool, input)) = t.calls.lock().unwrap().get(id).cloned() else { continue };
        if tool == "AskUserQuestion" { continue; }
        let failed = b.get("is_error") == Some(&Json::Bool(true));
        let mut u = vec![("sessionUpdate", st("tool_call_update")), ("toolCallId", st(id)), ("status", st(if failed { "failed" } else { "completed" }))];
        let text = result_text(b.get("content"));
        if kind_of(&tool) == "execute" {
            u.push(("rawOutput", match full { Some(o @ Json::Obj(_)) if o.get("stdout").is_some() => o.clone(), _ => st(&text) }));
        }
        if !failed {
            if let Some((line, diff)) = full.and_then(|f| patch(&tool, &input, f)) {
                u.push(("content", Json::Arr(diff)));
                if let Some(l) = line { u.push(("locations", Json::Arr(vec![Json::obj(vec![("path", st(s(&input, "file_path").unwrap_or(""))), ("line", Json::int(l))])]))); }
            }
        }
        t.feed(Json::obj(u));
    }
}

/// The change as made: Write over a file, its old and new text; an edit, its one hunk
/// with the line it starts at (several hunks: their text, no line numbers made up).
fn patch(tool: &str, input: &Json, full: &Json) -> Option<(Option<i64>, Vec<Json>)> {
    let d = |old: Option<&str>, new: &str| Json::obj(vec![("type", st("diff")), ("oldText", Json::opt_str_of(old)), ("newText", st(new))]);
    if tool == "Write" {
        let new = s(full, "content").or_else(|| s(input, "content"))?;
        return Some((None, vec![d(s(full, "originalFile"), new)]));
    }
    let hunks = arr(full.get("structuredPatch"));
    if hunks.is_empty() || !matches!(tool, "Edit" | "MultiEdit") { return None; }
    let mut out = vec![];
    for h in hunks {
        let (mut old, mut new) = (vec![], vec![]);
        for l in arr(h.get("lines")).iter().filter_map(Json::as_str) {
            match l.as_bytes().first() { Some(b'-') => old.push(&l[1..]), Some(b'+') => new.push(&l[1..]), _ => { let c = l.get(1..).unwrap_or(""); old.push(c); new.push(c); } }
        }
        out.push(d(Some(&old.join("\n")), &new.join("\n")));
    }
    let line = (hunks.len() == 1).then(|| hunks[0].get("oldStart").and_then(|v| v.i64().ok())).flatten();
    Some((line, out))
}

/// AskUserQuestion's questions, as the notch and the office show them. Claude Code
/// always takes an answer in the user's own words ("Other").
fn questions_of(input: &Json) -> Vec<AgentQuestion> {
    arr(input.get("questions")).iter().filter(|q| matches!(q, Json::Obj(_))).map(|q| AgentQuestion {
        header: s(q, "header").filter(|h| !h.is_empty()).unwrap_or("Question").into(),
        question: s(q, "question").unwrap_or("").into(),
        options: arr(q.get("options")).iter().filter_map(|o| s(o, "label").filter(|l| !l.is_empty()).map(|l| (l.to_owned(), s(o, "description").unwrap_or("").to_owned()))).collect(),
        multiple: q.get("multiSelect") == Some(&Json::Bool(true)),
        custom: true,
    }).collect()
}

fn explain(message: &str) -> String {
    let lower = message.to_lowercase();
    if ["/login", "not logged in", "invalid api key", "authentication_error", "oauth token", "api error: 401"].iter().any(|k| lower.contains(k)) {
        return format!("{NAME} needs you to sign in. {}", agents::sign_in_hint(AgentTool::Claude));
    }
    if crate::stream::units(message) > 600 { format!("{}…", crate::stream::head_units(message, 599)) } else { message.to_owned() }
}

/// Lines as they come (\n or \r\n), bad UTF-8 replaced; at the end the turn it was
/// running fails with what it said on stderr.
fn read(me: Weak<Host>, proc: Arc<Proc>, from: Box<dyn std::io::Read + Send>) {
    let mut r = BufReader::new(from);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) { Ok(0) | Err(_) => break, Ok(_) => {} }
        let Some(h) = me.upgrade() else { return };
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.starts_with('{') { h.handle(&proc, line); }
    }
    let was = proc.dead.swap(true, Ordering::SeqCst);
    // Every request waiting on it fails at once (a cancel hook may hold its channel open).
    for (_, tx) in proc.pending.lock().unwrap().drain() { let _ = tx.send(Err(format!("{NAME} stopped."))); }
    let why = proc.why();
    if !was { log(&format!("exited - {why}")); (proc.kill)(); }
    if let Some(t) = proc.turn.lock().unwrap().clone() {
        t.end(Ended::Gone(if why.is_empty() { format!("{NAME} stopped unexpectedly.") } else { format!("{NAME} stopped unexpectedly. {why}") }));
    }
    if let Some(h) = me.upgrade() { h.procs.lock().unwrap().retain(|p| !Arc::ptr_eq(p, &proc)); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> Json { json::parse(s).unwrap() }

    #[test]
    fn its_tools_read_as_acp_kinds_and_titles() {
        assert_eq!(["Read", "Write", "Edit", "Bash", "PowerShell", "Grep", "WebFetch", "Task", "TodoWrite", "TaskCreate", "mcp__x__y"].map(kind_of),
            ["read", "edit", "edit", "execute", "execute", "search", "fetch", "agent", "think", "think", "other"]);
        assert_eq!(title_of("mcp__playwright__click", &Json::Null), "@playwright/click");
        assert_eq!(title_of("Bash", &j(r#"{"command":"ls","description":"List files"}"#)), "List files");
        assert_eq!(title_of("Task", &j(r#"{"description":"Find the tests"}"#)), "Find the tests");
    }

    #[test]
    fn each_access_starts_it_its_own_way() {
        let base = Setup { folder: "/p".into(), mode: "bypassPermissions", tools: Tools::All, model: None, effort: None };
        let a = launch_args(&base, None);
        assert!(a.windows(2).any(|w| w == ["--permission-mode", "bypassPermissions"]) && a.contains(&"--allow-dangerously-skip-permissions".into()));
        assert!(a.contains(&"--permission-prompt-tool".into()) && a.contains(&"--setting-sources=user,project,local".into()));
        assert!(!a.iter().any(|x| x.starts_with("--resume")));
        let ro = launch_args(&Setup { mode: "default", tools: Tools::ReadOnly, model: Some("opus".into()), effort: Some("high".into()), ..base.clone() }, Some("s-1"));
        assert!(ro.windows(2).any(|w| w == ["--disallowedTools", READ_ONLY_DENIED]) && !ro.contains(&"--allow-dangerously-skip-permissions".into()));
        assert!(ro.windows(2).any(|w| w == ["--model", "opus"]) && ro.windows(2).any(|w| w == ["--effort", "high"]) && ro.last().unwrap() == "--resume=s-1");
        let none = launch_args(&Setup { mode: "default", tools: Tools::None, ..base }, None);
        assert!(none.windows(2).any(|w| w == ["--tools", ""]));
    }

    #[test]
    fn an_edit_shows_the_hunk_it_made() {
        let full = j(r#"{"filePath":"/p/a.rs","structuredPatch":[{"oldStart":3,"oldLines":3,"newStart":3,"newLines":3,"lines":[" a","-b","+B"," c"]}]}"#);
        let (line, d) = patch("Edit", &j(r#"{"file_path":"/p/a.rs"}"#), &full).unwrap();
        assert_eq!(line, Some(3));
        assert_eq!((s(&d[0], "oldText"), s(&d[0], "newText")), (Some("a\nb\nc"), Some("a\nB\nc")));
        let (line, d) = patch("Write", &j(r#"{"file_path":"/p/n.txt","content":"x"}"#), &j(r#"{"type":"create","content":"x","originalFile":null}"#)).unwrap();
        assert_eq!((line, d[0].get("oldText")), (None, Some(&Json::Null)));
    }

    #[test]
    fn questions_keep_their_choices() {
        let q = questions_of(&j(r#"{"questions":[{"question":"Tabs or spaces?","header":"Indent","multiSelect":true,"options":[{"label":"Tabs","description":"t"},{"label":""}]}]}"#));
        assert_eq!(q, vec![AgentQuestion { header: "Indent".into(), question: "Tabs or spaces?".into(), options: vec![("Tabs".into(), "t".into())], multiple: true, custom: true }]);
    }

    #[test]
    fn sign_in_failures_say_how_to_sign_in() {
        assert!(explain("Invalid API key · Please run /login").starts_with("Claude Code needs you to sign in."));
        assert_eq!(explain("API Error: 400 bad"), "API Error: 400 bad");
    }
}
