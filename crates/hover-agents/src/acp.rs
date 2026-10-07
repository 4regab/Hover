//! Services/AcpHost.cs: one agent tool running as a long-lived ACP server (newline
//! JSON-RPC over stdio), shared by every session of that tool. It starts on the first
//! run, keeps each conversation as an ACP session, and is shut down after the idle
//! time in its settings. A reply after that starts it again and loads the
//! conversation back (session/load, its replay ignored). The prompt only ever goes
//! over stdin. Messages are written as System.Text.Json writes the C# anonymous
//! objects, so the agent gets the same bytes from either build.

use crate::agents;
use crate::ask::{self, AgentAsk, AskAnswer};
use crate::cancel::Cancel;
use crate::computer_use::{self, McpServer};
use crate::proc::{strip_ansi, Link};
use crate::sandbox::{self, Boxed, Fit};
use crate::stream::{KiroEvent, KiroPhase, KiroResult, KiroStream};
use hover_core::json::{self, Json};
use hover_core::model::{AcpChoice, AcpOption, AgentApproval, AgentOptions, AgentTool, KiroState, KiroStep};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// The prompt that stands for Kiro's compaction: a Kiro turn with exactly this text is
/// sent as `_kiro/session/compact` (session.rs's auto compact; a reply of `/compact` too).
pub const COMPACT_PROMPT: &str = "/compact";

/// Not a prompt: a run of exactly this attaches to a Kiro Web session that is still working
/// in the cloud (its connection was lost, or Hover was closed) and follows it on, in the
/// turn that was cut off. It ends with the answer, or with ATTACH_NOTHING when the session
/// sent nothing new, which leaves the earlier failure as it was.
pub const ATTACH_PROMPT: &str = "/hover-attach-cloud";
pub const ATTACH_NOTHING: &str = "The cloud session sent nothing new.";
/// How a failed attempt to open the session begins (the reply the cloud gave, if any, follows).
pub const ATTACH_FAILED: &str = "Couldn’t open this Kiro Web session again";

/// How a pasted picture's line in a prompt begins (KiroTurn::text), then its file.
pub const ATTACHED: &str = "Attached image (read it from this file): ";
/// The most a picture may be, and how many go with one prompt (Kiro's own limits).
const IMAGE_MAX: u64 = 10 * 1024 * 1024;
const IMAGES_MAX: usize = 10;

/// The prompt's content blocks. Kiro gets each pasted picture as an image block (its contents,
/// so a Kiro Web session's sandbox, which can't read this computer's files, sees it too) and the
/// text without those lines. A picture it can't be sent (gone, too big, past the tenth, or the agent
/// takes none) stays a line naming its file, which an agent on this computer can still read.
fn prompt_blocks(prompt: &str, images: bool) -> Json {
    let text = |t: &str| o_(vec![("type", st("text")), ("text", st(t.trim()))]);
    if !images || !prompt.contains(ATTACHED) { return Json::Arr(vec![text(prompt)]); }
    let (mut kept, mut pics) = (vec![], vec![]);
    for line in prompt.lines() {
        let pic = line.strip_prefix(ATTACHED).filter(|_| pics.len() < IMAGES_MAX).and_then(|p| {
            let mime = match std::path::Path::new(p).extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "webp" => "image/webp", _ => return None,
            };
            if std::fs::metadata(p).ok()?.len() > IMAGE_MAX { return None; }
            Some(o_(vec![("type", st("image")), ("mimeType", st(mime)), ("data", st(&crate::http::base64(&std::fs::read(p).ok()?)))]))
        });
        match pic { Some(b) => pics.push(b), None => kept.push(line) }
    }
    let mut blocks = vec![text(&kept.join("\n"))];
    blocks.extend(pics);
    Json::Arr(blocks)
}

/// Where Kiro Web shows a cloud session: this, then the session's id.
pub const KIRO_WEB_SESSION: &str = "https://app.kiro.dev/session/";

pub type Progress = Box<dyn Fn(KiroPhase) + Send + Sync>;
pub type Events = Box<dyn Fn(KiroEvent) + Send + Sync>;
type Connect = Box<dyn Fn() -> std::io::Result<Option<Link>> + Send + Sync>;
type Seen = Box<dyn Fn(AgentTool, &[AcpOption]) + Send + Sync>;
/// The MCP servers a new or loaded session gets, for the Hover session (its key) it is
/// made for: Cua Driver's when computer use is on, Hover's browser where there is one.
pub type McpFn = Arc<dyn Fn(Option<&str>) -> Vec<McpServer> + Send + Sync>;

/// The servers a session gets unless a host is given others.
pub fn default_mcp(tool: AgentTool) -> McpFn {
    Arc::new(move |tag| { let mut all = computer_use::servers(); all.extend(crate::browser::servers(tool, tag)); all.extend(crate::orch::servers(tag)); all })
}

/// AcpHost.Asking: asks the user about a tool call for the ACP session named first;
/// the token ends when the run is stopped. The answer goes to the reply, from any thread.
pub type Asking = Arc<dyn Fn(&str, AgentAsk, &Cancel, Box<dyn FnOnce(AskAnswer) + Send>) + Send + Sync>;

#[derive(Debug, Clone)]
enum CallErr {
    Acp(String),
    Gone(String),
    Cancelled,
}

impl std::fmt::Display for CallErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { CallErr::Acp(m) | CallErr::Gone(m) => f.write_str(m), CallErr::Cancelled => f.write_str("Cancelled.") }
    }
}

enum Msg { Reply(Result<Json, CallErr>), CancelAsked }

struct Turn {
    stream: Mutex<KiroStream>,
    progress: Option<Progress>,
    events: Option<Events>,
    options: AgentOptions,
    folder: String,
    /// Cancelled when the run is stopped, which also withdraws a question.
    token: Cancel,
    /// While a conversation is loaded back, the agent replays it; that isn't news.
    muted: AtomicBool,
    /// Only when attaching: the replay is read into this (the last turn's part of it), and the
    /// stream carries on from it.
    replay: Mutex<Option<Replay>>,
    /// When the agent last sent anything for this turn.
    last_update: Mutex<std::time::Instant>,
    /// Updates for this turn since it was loaded.
    live: AtomicUsize,
    refused: AtomicBool,
    /// The MCP servers the agent said didn't start this turn, in the order it said so.
    mcp_failed: Mutex<Vec<String>>,
    /// Access "none": every request the agent makes is turned down, reading too (voice's
    /// routing turn, which only reads what it is sent).
    deny_all: bool,
}

/// A loaded cloud conversation as it is replayed, turn by turn: each message of the user's
/// starts a turn (its words, and a stream of what came of it). The last is the turn that was cut off.
struct Replay { name: &'static str, turns: Vec<(String, KiroStream)>, in_user: bool, updates: usize, kinds: Vec<(String, usize)> }

/// A content block's text (one block, or a list of them).
fn text_of(c: &Json) -> String { match c { Json::Arr(p) => p.iter().map(text_of).collect(), _ => s(c, "text").unwrap_or("").to_owned() } }

impl Replay {
    fn new(name: &'static str) -> Replay { Replay { name, turns: vec![], in_user: false, updates: 0, kinds: vec![] } }

    fn feed(&mut self, line: &str, update: Option<&Json>) {
        let kind = update.and_then(|u| s(u, "sessionUpdate")).unwrap_or("-").to_owned();
        self.updates += 1;
        match self.kinds.iter_mut().find(|k| k.0 == kind) { Some(k) => k.1 += 1, None => self.kinds.push((kind.clone(), 1)) }
        if kind == "user_message_chunk" {
            if !self.in_user { self.turns.push((String::new(), KiroStream::new(self.name))); }
            self.in_user = true;
            if let (Some(c), Some(t)) = (update.and_then(|u| u.get("content")), self.turns.last_mut()) { t.0.push_str(&text_of(c)); }
            return;
        }
        self.in_user = false;
        if self.turns.is_empty() { self.turns.push((String::new(), KiroStream::new(self.name))); }
        if let Some(t) = self.turns.last_mut() { t.1.feed(line); }
    }

    /// The last turn's stream, which a cut-off turn carries on in.
    fn last(&mut self) -> KiroStream { self.turns.pop().map_or_else(|| KiroStream::new(self.name), |t| t.1) }
}

/// What listing the user's Kiro Web sessions found, and when it found none, in words why.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CloudList { pub sessions: Vec<CloudSession>, pub note: String }

/// A Kiro Web session in the user's Kiro account, as Kiro lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudSession { pub id: String, pub title: String, pub updated: Option<hover_core::time::Stamp> }

/// One turn of a Kiro Web conversation as its replay gave it: what was asked, the answer, the
/// steps, and whether it was reported finished.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudTurn { pub prompt: String, pub text: String, pub steps: Vec<KiroStep>, pub completed: bool }

struct Live {
    gen: u64,
    writer: Mutex<Box<dyn Write + Send>>,
    kill: Box<dyn Fn() + Send + Sync>,
    errors: Box<dyn Fn() -> String + Send + Sync>,
}

struct Host {
    tool: AgentTool,
    options: Box<dyn Fn() -> AgentOptions + Send + Sync>,
    connect: Connect,
    gate: Mutex<()>,
    link: Mutex<Option<Arc<Live>>>,
    gens: AtomicU64,
    pending: Mutex<HashMap<i64, Sender<Msg>>>,
    turns: Mutex<HashMap<String, Arc<Turn>>>,
    session_options: Mutex<HashMap<String, Vec<AcpOption>>>,
    can_load: AtomicBool,
    ids: AtomicI64,
    busy: AtomicUsize,
    idle: AtomicU64,
    seen: Mutex<Vec<Seen>>,
    asking: Mutex<Option<Asking>>,
    /// What the user trusted for the rest of a session, by ACP session id: the keys of
    /// tool calls (ask::key), or "*" for everything.
    trusted: Mutex<HashMap<String, HashSet<String>>>,
    /// The MCP servers each live session was given (computer_use::signature), by ACP
    /// session id; cleared with the process.
    session_mcp: Mutex<HashMap<String, String>>,
    mcp: Mutex<McpFn>,
    /// How the process was sandboxed (sandbox.rs); untouched for a process Hover didn't start.
    boxed: Arc<Boxed>,
    /// The process can run sessions in Kiro's cloud (initialize's executionTargets).
    can_cloud: AtomicBool,
    /// The agent takes pictures in a prompt (initialize's promptCapabilities.image).
    can_image: AtomicBool,
    /// The agent lists its sessions (initialize's sessionCapabilities.list).
    can_list: AtomicBool,
    /// What the agent advertises for Kiro (agentCapabilities._meta.kiro), as it said it.
    kiro_caps: Mutex<Json>,
    /// Cloud sessions whose sandbox said it is ready (its first context_usage), by ACP
    /// session id; cleared with the process.
    ready: Mutex<HashSet<String>>,
}

#[derive(Clone)]
pub struct AcpHost(Arc<Host>);

fn s<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }
fn o(props: Vec<(&str, Json)>) -> Json { Json::obj(props) }
fn st(v: &str) -> Json { Json::str(v) }

impl AcpHost {
    /// The tool as Agents finds and starts it.
    pub fn new(tool: AgentTool, options: impl Fn() -> AgentOptions + Send + Sync + 'static) -> AcpHost {
        let boxed = Arc::new(Boxed::default());
        let b = boxed.clone();
        // In the sandbox, for the folders its sessions use (sandbox.rs), when it is wanted.
        AcpHost::build(tool, options, Box::new(move || match agents::exe(tool) {
            None => Ok(None),
            Some(exe) => sandbox::launch(tool, &exe, agents::arguments(tool), &agents::environment(tool, &exe), None, Some(&b)).map(Some),
        }), boxed)
    }

    pub fn with_connect(tool: AgentTool, options: impl Fn() -> AgentOptions + Send + Sync + 'static,
        connect: impl Fn() -> std::io::Result<Option<Link>> + Send + Sync + 'static) -> AcpHost {
        AcpHost::build(tool, options, Box::new(connect), Arc::new(Boxed::default()))
    }

    fn build(tool: AgentTool, options: impl Fn() -> AgentOptions + Send + Sync + 'static, connect: Connect, boxed: Arc<Boxed>) -> AcpHost {
        AcpHost(Arc::new(Host {
            tool, options: Box::new(options), connect, gate: Mutex::new(()), link: Mutex::new(None), gens: AtomicU64::new(0),
            pending: Mutex::new(HashMap::new()), turns: Mutex::new(HashMap::new()), session_options: Mutex::new(HashMap::new()),
            can_load: AtomicBool::new(false), ids: AtomicI64::new(0), busy: AtomicUsize::new(0), idle: AtomicU64::new(0), seen: Mutex::new(vec![]),
            asking: Mutex::new(None), trusted: Mutex::new(HashMap::new()), session_mcp: Mutex::new(HashMap::new()), mcp: Mutex::new(default_mcp(tool)), boxed, can_cloud: AtomicBool::new(false), can_image: AtomicBool::new(false), can_list: AtomicBool::new(false), kiro_caps: Mutex::new(Json::Null), ready: Mutex::new(HashSet::new()),
        }))
    }

    /// The MCP servers each new or loaded session gets, read at the start of every run
    /// (the default: Cua Driver's when computer use is on, and Hover's browser).
    pub fn set_mcp(&self, f: impl Fn(Option<&str>) -> Vec<McpServer> + Send + Sync + 'static) { *self.0.mcp.lock().unwrap() = Arc::new(f); }

    pub fn tool(&self) -> AgentTool { self.0.tool }

    /// The tool's process is up.
    pub fn alive(&self) -> bool { self.0.link.lock().unwrap().is_some() }

    /// The settings the agent offered for a session, whenever they are read or change.
    /// Called off the UI thread.
    pub fn on_options_seen(&self, f: impl Fn(AgentTool, &[AcpOption]) + Send + Sync + 'static) { self.0.seen.lock().unwrap().push(Box::new(f)); }

    /// Runs one turn in a folder: a new conversation, or the one resume names. Never
    /// fails outright: every way it goes wrong is a Failed result. Cancelling asks the
    /// agent to stop (session/cancel); one that doesn't within 8 s is left, or shut
    /// down when nothing else of it runs. Blocks: run it off the UI thread.
    pub fn run(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, None, None, None)
    }

    /// run, with the session's own tool access (AgentOptions::with_access).
    #[allow(clippy::too_many_arguments)]
    pub fn run_as(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access, None, None)
    }

    /// run_as, naming the Hover session (its key) the run is for: the tag Hover's browser
    /// server is made for (AcpHost.Run's tag).
    #[allow(clippy::too_many_arguments)]
    pub fn run_tagged(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>, tag: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access, tag, None)
    }

    /// The GitHub repos ("owner/name") the user connected to Kiro, which a cloud session
    /// can be given. Starts the tool if it isn't up. Blocks: run it off the UI thread.
    pub fn repos(&self) -> Result<Vec<String>, String> { self.0.repos() }

    /// Every Kiro Web session in the user's Kiro account, newest first as Kiro gives them. Starts
    /// the tool if it isn't up. Blocks: run it off the UI thread.
    pub fn cloud_sessions(&self) -> Result<CloudList, String> { self.0.cloud_sessions() }

    /// A Kiro Web session's whole conversation, from its replay, opened in `folder` (a folder on
    /// this computer; the session works in its own sandbox). Blocks: run it off the UI thread.
    pub fn cloud_transcript(&self, id: &str, folder: &str) -> Result<Vec<CloudTurn>, String> { self.0.cloud_transcript(id, folder) }

    /// Where a question goes. Without one, whatever the settings say should be asked
    /// about is turned down.
    pub fn set_asking(&self, f: Asking) { *self.0.asking.lock().unwrap() = Some(f); }

    /// Ends the tool's process now. Runs still going fail; the next one starts it again.
    pub fn shutdown(&self, why: &str) { self.0.shutdown(why) }

    /// The session's runner for this tool (OwlApp.Kiro's make: Agents[tool].Run).
    pub fn runner(&self) -> crate::session::RunTask {
        let h = self.clone();
        Arc::new(move |a: crate::session::RunArgs| { let tag = crate::runtime::tag_of(&a); h.0.run(&a.folder, &a.prompt, Some(a.progress), &a.ct, a.resume.as_deref(), Some(a.events), a.access.as_deref(), tag.as_deref(), a.cloud.as_deref()) })
    }
}

impl Host {
    fn name(&self) -> &'static str { self.tool.name() }

    #[allow(clippy::too_many_arguments)]
    fn run(self: &Arc<Self>, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>, tag: Option<&str>,
        cloud: Option<&[String]>) -> KiroResult {
        // Kiro's cloud runs in Autopilot (it has no asking), so its sessions are Full.
        let access = if cloud.is_some() { Some("full") } else { access };
        let name = self.name();
        if !crate::usable_folder(Some(folder)) { return KiroResult::new(KiroState::Failed, "That folder isn’t there any more. Choose another one."); }
        if prompt.trim().is_empty() { return KiroResult::new(KiroState::Failed, format!("Tell {name} what to do first.")); }
        // A sandboxed tool reaches only the folders it started with, and the sandbox
        // switched on or off applies from its next start: one that no longer fits is
        // started again when nothing of it runs. Busy in other folders, it can't take
        // this one yet.
        sandbox::remember(folder);
        if self.link.lock().unwrap().is_some() {
            match self.boxed.fit(folder, self.busy.load(Ordering::SeqCst) > 0, sandbox::active()) {
                Fit::Fits => {}
                Fit::Restart => self.shutdown("its sandbox changed"),
                Fit::Outside => return KiroResult::new(KiroState::Failed, sandbox::outside_message(name)),
            }
        }
        // The agent's cua-driver can't start CuaDriver's daemon from inside the sandbox
        // (no Launch Services there), so Hover does, outside it.
        if sandbox::wanted() && crate::agents::toggles().computer_use { computer_use::ensure_daemon(); }
        let o = (self.options)().with_access(access);
        // A cloud session's sandbox can't reach this computer's servers: it gets none.
        let mut servers = if cloud.is_some() { vec![] } else { (self.mcp.lock().unwrap().clone())(tag) };
        // The project's desktop, for a session's run (not the routing turn that has none):
        // every agent in that folder is given the same one.
        if tag.is_some() && cloud.is_none() { servers.extend(crate::spaces::servers(folder)); }
        let mcp = (computer_use::acp(&servers), computer_use::signature(&servers));

        // A session's MCP servers are fixed when it is made or loaded. A reply to one made
        // with others (computer use switched since) loads it again in a fresh process,
        // when nothing else of this tool runs; otherwise it carries on as is.
        if let Some(r) = resume.filter(|r| !r.is_empty()) {
            let had = self.session_mcp.lock().unwrap().get(r).cloned();
            if self.can_load.load(Ordering::SeqCst) && had.is_some_and(|h| h != mcp.1) && self.busy.load(Ordering::SeqCst) == 0 { self.shutdown("its MCP servers changed"); }
        }
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.idle.fetch_add(1, Ordering::SeqCst);
        let turn = Arc::new(Turn { stream: Mutex::new(KiroStream::new(name)), progress, events, options: o.clone(), folder: folder.into(), token: ct.clone(), muted: AtomicBool::new(false),
            replay: Mutex::new(None), last_update: Mutex::new(std::time::Instant::now()), live: AtomicUsize::new(0),
            refused: AtomicBool::new(false), mcp_failed: Mutex::new(vec![]), deny_all: access == Some("none") });
        let mut sid: Option<String> = None;
        let r = self.turn(folder, prompt, ct, resume, &o, &turn, &mut sid, &mcp, cloud);
        // A thought still open when the turn ends (however it ends) ends with it.
        let last = { let mut st = turn.stream.lock().unwrap(); st.end(); st.drain() };
        if let Some(f) = &turn.events { for e in last { f(e); } }
        let mut result = match r {
            Ok(r) => r,
            Err(CallErr::Cancelled) => self.finish(&turn, Some("cancelled"), true),
            Err(CallErr::Acp(m)) => KiroResult::new(KiroState::Failed, self.explain(&m)),
            Err(CallErr::Gone(m)) => if ct.is_cancelled() { self.finish(&turn, Some("cancelled"), true) } else { KiroResult::new(KiroState::Failed, m) },
        };
        // Said under the answer too, however the turn ended: the step sits in a timeline
        // that is folded by default (or hidden, by a setting), and a missing server's
        // tools can be why the answer is what it is.
        if let Some(note) = mcp_note(&turn.mcp_failed.lock().unwrap()) { result.text = format!("{}\n\n{note}", result.text.trim_end()); }
        if let Some(sid) = &sid {
            let mut t = self.turns.lock().unwrap();
            if t.get(sid).is_some_and(|x| Arc::ptr_eq(x, &turn)) { t.remove(sid); }
        }
        if self.busy.fetch_sub(1, Ordering::SeqCst) == 1 && self.link.lock().unwrap().is_some() {
            self.schedule_idle(Duration::from_secs(60 * (self.options)().idle_minutes.max(1) as u64));
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn turn(self: &Arc<Self>, folder: &str, prompt: &str, ct: &Cancel, resume: Option<&str>, o: &AgentOptions, turn: &Arc<Turn>, sid: &mut Option<String>, mcp: &(Json, String),
        cloud: Option<&[String]>) -> Result<KiroResult, CallErr> {
        let name = self.name();
        if let Some(p) = &turn.progress { p(KiroPhase::Starting); }
        self.start(ct)?;
        if cloud.is_some() && !self.can_cloud.load(Ordering::SeqCst) {
            return Err(CallErr::Acp(format!("{name} on this computer can’t run Kiro Web sessions. Update Kiro CLI, and check that cloud sessions are on for your account.")));
        }
        if prompt == ATTACH_PROMPT { return self.attach(folder, ct, resume, turn, sid, mcp, cloud); }
        let mut offered: Option<Vec<AcpOption>> = None;
        if let Some(r) = resume.filter(|r| !r.is_empty()) {
            let known = self.session_options.lock().unwrap().get(r).cloned();
            if let Some(k) = known {
                *sid = Some(r.into());
                offered = Some(k);
            } else if self.can_load.load(Ordering::SeqCst) {
                turn.muted.store(true, Ordering::SeqCst);
                self.turns.lock().unwrap().insert(r.into(), turn.clone());
                let mut params = vec![("sessionId", st(r)), ("cwd", st(folder)), ("mcpServers", mcp.0.clone())];
                // From Kiro's cloud store: without this it reads the local store, finds
                // nothing, and makes an empty local session of the same id.
                if cloud.is_some() { params.push(("_meta", o_(vec![("kiro", o_(vec![("sessionSource", st("remote"))]))]))); }
                match self.call("session/load", o_(params), Some(ct), Some(Duration::from_secs(120))) {
                    Ok(res) => { *sid = Some(r.into()); offered = options(&res); self.session_mcp.lock().unwrap().insert(r.into(), mcp.1.clone()); }
                    Err(CallErr::Acp(m)) if cloud.is_some() => {
                        self.turns.lock().unwrap().remove(r);
                        return Err(CallErr::Acp(format!("Couldn’t open this Kiro Web session again: {m}")));
                    }
                    Err(CallErr::Acp(m)) => {
                        // Gone from the agent's own history: carry on in a new conversation.
                        hover_core::log::line(&format!("acp {name}: couldn't load {r} - {m}"));
                        self.turns.lock().unwrap().remove(r);
                    }
                    Err(e) => return Err(e),
                }
                turn.muted.store(false, Ordering::SeqCst);
            }
        }
        if sid.is_none() {
            // A reply to a cloud session never starts another one in its place.
            if cloud.is_some() && resume.is_some_and(|r| !r.is_empty()) { return Err(CallErr::Acp("Couldn’t open this Kiro Web session again.".into())); }
            let mut params = vec![("cwd", st(folder)), ("mcpServers", mcp.0.clone())];
            if let Some(repos) = cloud {
                let mut kiro = vec![("executionTarget", o_(vec![("kind", st("cloud-sandbox"))]))];
                if !repos.is_empty() {
                    kiro.push(("repositories", Json::Arr(repos.iter().map(|r| o_(vec![("providerType", st("GITHUB")), ("name", st(r))])).collect())));
                }
                params.push(("_meta", o_(vec![("kiro", o_(kiro))])));
            }
            let res = self.call("session/new", o_(params), Some(ct), Some(Duration::from_secs(120)))?;
            *sid = Some(s(&res, "sessionId").ok_or_else(|| CallErr::Acp(format!("{name} didn’t start a session.")))?.to_owned());
            self.session_mcp.lock().unwrap().insert(sid.clone().unwrap(), mcp.1.clone());
            offered = options(&res);
            if cloud.is_some() {
                // Said now, not after the wait: its id is kept even if Hover quits while the
                // sandbox comes up, and the sandbox's own setup steps show in the chat.
                let id = sid.clone().unwrap();
                self.turns.lock().unwrap().insert(id.clone(), turn.clone());
                if let Some(e) = &turn.events { e(KiroEvent { session_id: Some(id.clone()), ..Default::default() }); }
                self.await_ready(&id, ct)?;
            }
        }
        let id = sid.clone().unwrap();
        self.turns.lock().unwrap().insert(id.clone(), turn.clone());
        if let Some(e) = &turn.events { e(KiroEvent { session_id: Some(id.clone()), ..Default::default() }); }
        let configured = self.configure(&id, offered.unwrap_or_default(), o, ct)?;
        self.session_options.lock().unwrap().insert(id.clone(), configured);

        // Kiro's auto compact (session.rs) sends this in place of a reply. Kiro answers a
        // /compact prompt as a chat message (its model says it can't run the command), so the
        // compaction is its own request, which summarises the conversation and answers success.
        if self.tool == AgentTool::Kiro && prompt.trim() == COMPACT_PROMPT {
            let res = self.call("_kiro/session/compact", o_(vec![("sessionId", st(&id))]), Some(ct), None)?;
            return Ok(if matches!(res.get("success"), Some(Json::Bool(false))) { KiroResult::new(KiroState::Failed, format!("{name} couldn’t compact the conversation.")) }
                else { KiroResult::new(KiroState::Completed, "Compacted the conversation.") });
        }

        let images = self.tool == AgentTool::Kiro && self.can_image.load(Ordering::SeqCst);
        let params = o_(vec![("sessionId", st(&id)), ("prompt", prompt_blocks(prompt, images))]);
        let (call, rx) = self.begin_call("session/prompt", params)?;
        let me = Arc::downgrade(self);
        let cancel_sid = id.clone();
        let tx = self.pending.lock().unwrap().get(&call).cloned();
        let _reg = ct.on_cancel(move || {
            if let Some(h) = me.upgrade() { std::thread::spawn(move || h.notify("session/cancel", o_(vec![("sessionId", st(&cancel_sid))]))); }
            if let Some(tx) = tx { let _ = tx.send(Msg::CancelAsked); }
        });
        let reply = loop {
            match rx.recv() {
                Ok(Msg::Reply(r)) => break r,
                Ok(Msg::CancelAsked) => match rx.recv_timeout(Duration::from_secs(8)) {
                    Ok(Msg::Reply(r)) => break r,
                    Ok(Msg::CancelAsked) => continue,
                    Err(_) => {
                        // It didn't stop when asked. Only this run is using it: end it.
                        if self.busy.load(Ordering::SeqCst) == 1 { self.shutdown("didn't stop when asked"); return Ok(self.finish(turn, Some("cancelled"), true)); }
                        // Others share the process, so it stays up, and this turn may
                        // still be going: said as it is, never as stopped.
                        hover_core::log::line(&format!("acp {name}: {id} didn't confirm the stop within 8 s"));
                        return Ok(KiroResult { unconfirmed: true, ..KiroResult::new(KiroState::Failed,
                            format!("{name} didn’t confirm it stopped. It may still be working on this; nothing queued was sent.")) });
                    }
                },
                Err(_) => break Err(CallErr::Gone(format!("{name} stopped."))),
            }
        }?;
        Ok(self.finish(turn, s(&reply, "stopReason"), ct.is_cancelled()))
    }

    /// Attaches to a cloud session that went on working while Hover was away: loads it again
    /// (the cloud replays it, then sends what happens next), reads the replay for the turn that
    /// was cut off, and follows that turn to its end. Logged closely: what Kiro sends to a client
    /// that attaches mid-turn isn't in its docs.
    #[allow(clippy::too_many_arguments)]
    fn attach(self: &Arc<Self>, folder: &str, ct: &Cancel, resume: Option<&str>, turn: &Arc<Turn>, sid: &mut Option<String>, mcp: &(Json, String),
        cloud: Option<&[String]>) -> Result<KiroResult, CallErr> {
        let name = self.name();
        let again = |m: String| CallErr::Acp(format!("{ATTACH_FAILED}{}", if m.is_empty() { ".".into() } else { format!(": {m}") }));
        let Some(r) = resume.filter(|r| !r.is_empty()).filter(|_| cloud.is_some()) else { return Err(again(String::new())) };
        if !self.can_load.load(Ordering::SeqCst) { return Err(again(String::new())); }
        let began = std::time::Instant::now();
        *turn.replay.lock().unwrap() = Some(Replay::new(name));
        turn.muted.store(true, Ordering::SeqCst);
        self.turns.lock().unwrap().insert(r.into(), turn.clone());
        let params = vec![("sessionId", st(r)), ("cwd", st(folder)), ("mcpServers", mcp.0.clone()),
            ("_meta", o_(vec![("kiro", o_(vec![("sessionSource", st("remote"))]))]))];
        let res = match self.call("session/load", o_(params), Some(ct), Some(Duration::from_secs(120))) {
            Ok(res) => res,
            Err(CallErr::Acp(m)) => {
                self.turns.lock().unwrap().remove(r);
                hover_core::log::line(&format!("acp {name}: attach {r}: couldn't load it - {m}"));
                return Err(again(m));
            }
            Err(e) => return Err(e),
        };
        *sid = Some(r.into());
        self.session_mcp.lock().unwrap().insert(r.into(), mcp.1.clone());
        if let Some(o) = options(&res) { self.session_options.lock().unwrap().insert(r.into(), o); }
        // The replay's last turn becomes the stream, and its steps and context go to the chat.
        let Some(mut rep) = turn.replay.lock().unwrap().take() else { return Err(again(String::new())) };
        let kinds = rep.kinds.iter().map(|k| format!("{}={}", k.0, k.1)).collect::<Vec<_>>().join(" ");
        let updates = rep.updates;
        let last = rep.last();
        let (completed, said_len) = (last.completed, last.said().len());
        hover_core::log::line(&format!("acp {name}: attach {r}: replayed {updates} updates in {} ms [{kinds}]; the last turn {} and said {said_len} bytes",
            began.elapsed().as_millis(), if completed { "was completed" } else { "has no completion report" }));
        let events = { let mut st = turn.stream.lock().unwrap(); *st = last; st.drain() };
        if let Some(f) = &turn.events { for e in events { f(e); } }
        *turn.last_update.lock().unwrap() = std::time::Instant::now();
        turn.live.store(0, Ordering::SeqCst);
        turn.muted.store(false, Ordering::SeqCst);
        if completed { return Ok(self.finish(turn, Some("end_turn"), false)); }
        // Not reported complete: follow what comes. A session that is working keeps sending (tool calls,
        // thinking, its context); one that sends nothing for a while is not working on this.
        const FIRST: Duration = Duration::from_secs(30);
        const QUIET: Duration = Duration::from_secs(120);
        loop {
            if ct.is_cancelled() { return Err(CallErr::Cancelled); }
            if self.link.lock().unwrap().is_none() { return Err(CallErr::Gone(format!("{name} stopped."))); }
            let (done, n) = (turn.stream.lock().unwrap().completed, turn.live.load(Ordering::SeqCst));
            let quiet = turn.last_update.lock().unwrap().elapsed();
            if done {
                hover_core::log::line(&format!("acp {name}: attach {r}: the turn completed after {n} live updates, {} s", began.elapsed().as_secs()));
                return Ok(self.finish(turn, Some("end_turn"), false));
            }
            if n == 0 && quiet > FIRST {
                hover_core::log::line(&format!("acp {name}: attach {r}: nothing live in {} s and no completion report; leaving the turn as it was", FIRST.as_secs()));
                return Ok(KiroResult::new(KiroState::Failed, ATTACH_NOTHING));
            }
            if n > 0 && quiet > QUIET {
                hover_core::log::line(&format!("acp {name}: attach {r}: {n} live updates, then quiet for {} s with no completion report; taking it as finished", QUIET.as_secs()));
                return Ok(self.finish(turn, Some("end_turn"), false));
            }
            // ponytail: polled every 100 ms; a condvar is the upgrade if many cloud sessions are followed at once.
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// A new cloud session takes its prompt only once its sandbox is up: one sent before
    /// is answered "cancelled" here while the cloud still runs it (seen Oct 2026). The
    /// sandbox says it is up with its first context_usage, about 15 s after session/new.
    fn await_ready(&self, sid: &str, ct: &Cancel) -> Result<(), CallErr> {
        // ponytail: polled every 250 ms instead of a condvar; it waits seconds at most once per session.
        let until = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if ct.is_cancelled() { return Err(CallErr::Cancelled); }
            if self.link.lock().unwrap().is_none() { return Err(CallErr::Gone(format!("{} stopped.", self.name()))); }
            if self.ready.lock().unwrap().remove(sid) { return Ok(()); }
            if std::time::Instant::now() >= until {
                hover_core::log::line(&format!("acp {}: {sid} didn't say its sandbox was ready in 120 s; prompting anyway", self.name()));
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn repos(self: &Arc<Self>) -> Result<Vec<String>, String> {
        let ct = Cancel::new();
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.idle.fetch_add(1, Ordering::SeqCst);
        let got = (|| -> Result<Vec<String>, CallErr> {
            self.start(&ct)?;
            let mut all = vec![];
            let mut cursor: Option<String> = None;
            loop {
                let mut p = vec![("providerType", st("GITHUB"))];
                if let Some(c) = &cursor { p.push(("cursor", st(c))); }
                let r = self.call("_kiro/sourceProviders/listResources", o_(p), Some(&ct), Some(Duration::from_secs(60)))?;
                if let Some(Json::Arr(list)) = r.get("resources") { all.extend(list.iter().filter_map(|x| s(x, "name")).map(str::to_owned)); }
                match s(&r, "nextCursor") { Some(c) if !c.is_empty() => cursor = Some(c.to_owned()), _ => break }
            }
            Ok(all)
        })();
        if self.busy.fetch_sub(1, Ordering::SeqCst) == 1 && self.link.lock().unwrap().is_some() {
            self.schedule_idle(Duration::from_secs(60 * (self.options)().idle_minutes.max(1) as u64));
        }
        got.map_err(|e| match e { CallErr::Acp(m) => self.explain(&m), CallErr::Gone(m) => m, CallErr::Cancelled => "Stopped.".into() })
    }

    /// Runs `work` with the tool started, counted as busy so it isn't shut down meanwhile; errors in words.
    fn with_tool<T>(self: &Arc<Self>, work: impl FnOnce(&Cancel) -> Result<T, CallErr>) -> Result<T, String> {
        let ct = Cancel::new();
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.idle.fetch_add(1, Ordering::SeqCst);
        let got = self.start(&ct).and_then(|_| work(&ct));
        if self.busy.fetch_sub(1, Ordering::SeqCst) == 1 && self.link.lock().unwrap().is_some() {
            self.schedule_idle(Duration::from_secs(60 * (self.options)().idle_minutes.max(1) as u64));
        }
        got.map_err(|e| match e { CallErr::Acp(m) => self.explain(&m), CallErr::Gone(m) => m, CallErr::Cancelled => "Stopped.".into() })
    }

    /// Every session Kiro lists when asked for `source` ("remote": Kiro Web's, "local": this
    /// computer's), paging through. Each with whether Kiro marked it cloud, and whether local.
    fn list_source(&self, ct: &Cancel, source: &str) -> Result<Vec<(CloudSession, bool, bool)>, CallErr> {
        let mut all: Vec<(CloudSession, bool, bool)> = vec![];
        let mut cursor: Option<String> = None;
        // ponytail: at most 50 pages; a cursor that never ends stops there.
        for _ in 0..50 {
            // Kiro lists Kiro Web's sessions only with listScope "user": its default, "workspace", is
            // this computer's folders, and a cloud session has none (seen in Kiro's agent server, Oct 2026).
            let mut meta = vec![("sessionSource", st(source))];
            if source == "remote" { meta.push(("listScope", st("user"))); }
            let mut p = vec![("_meta", o_(vec![("kiro", o_(meta))]))];
            if let Some(c) = &cursor { p.push(("cursor", st(c))); }
            let r = self.call("session/list", o_(p), Some(ct), Some(Duration::from_secs(60)))?;
            if let Some(Json::Arr(list)) = r.get("sessions") {
                // What Kiro answers, with no titles or paths: how many, and the first one's field names and marks.
                if cursor.is_none() {
                    let first = list.first().map(|x| match x { Json::Obj(f) => format!("fields=[{}] _meta={}", f.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(","),
                        crate::stream::clip(&x.get("_meta").map_or("none".into(), Json::compact), 300)), _ => "not an object".into() }).unwrap_or_else(|| "none".into());
                    hover_core::log::line(&format!("acp {}: session/list sessionSource={source}: {} on the first page; first: {first}", self.name(), list.len()));
                }
                for x in list {
                    let Some(id) = s(x, "sessionId").filter(|i| !i.is_empty()) else { continue };
                    if all.iter().any(|c| c.0.id == id) { continue; }
                    let marks = x.get("_meta").and_then(|m| m.get("kiro")).map(|k| k.compact().to_lowercase()).unwrap_or_default();
                    let (cloud, local) = (marks.contains("cloud") || marks.contains("remote"), marks.contains("\"local\""));
                    all.push((CloudSession { id: id.into(), title: s(x, "title").unwrap_or("").trim().to_owned(), updated: s(x, "updatedAt").and_then(hover_core::time::Stamp::parse) }, cloud, local));
                }
            }
            match s(&r, "nextCursor") { Some(c) if !c.is_empty() => cursor = Some(c.to_owned()), _ => break }
        }
        Ok(all)
    }

    /// The user's Kiro Web sessions. Kiro's docs don't say how a cloud session is marked in its list,
    /// so Hover asks for Kiro Web's and for this computer's, and takes a session for Kiro Web's when
    /// Kiro marks it cloud, or when it is in the first list and neither marked local nor in the
    /// second. If Kiro gives the same sessions for both, Hover can't tell, and says so.
    fn cloud_sessions(self: &Arc<Self>) -> Result<CloudList, String> {
        let name = self.name();
        self.with_tool(|ct| {
            if !self.can_list.load(Ordering::SeqCst) { return Err(CallErr::Acp(format!("This {name} CLI can’t list sessions. Update Kiro CLI."))); }
            let remote = self.list_source(ct, "remote")?;
            let local = self.list_source(ct, "local").ok();
            let local_ids: HashSet<&str> = local.iter().flatten().map(|c| c.0.id.as_str()).collect();
            let same = local.as_ref().is_some_and(|l| !remote.is_empty() && l.len() == remote.len() && remote.iter().all(|c| local_ids.contains(c.0.id.as_str())));
            let sessions: Vec<CloudSession> = remote.iter()
                .filter(|(c, cloud, local_mark)| *cloud || (!*local_mark && !same && !local_ids.contains(c.id.as_str()))).map(|x| x.0.clone()).collect();
            hover_core::log::line(&format!("acp {name}: Kiro Web sessions: {} for Kiro Web, {} for this computer, {} kept; it advertises {}", remote.len(),
                local.as_ref().map_or("?".into(), |l| l.len().to_string()), sessions.len(), crate::stream::clip(&self.kiro_caps.lock().unwrap().compact(), 600)));
            let mut note = String::new();
            if sessions.is_empty() {
                note = format!("{name} listed {} for Kiro Web{}.", remote.len(), local.as_ref().map_or(String::new(), |l| format!(" and {} for this computer", l.len())));
                if same { note.push_str(" They are the same, so Hover can’t tell which are Kiro Web’s."); }
                // What it says it can do, to see how to ask it.
                if let Json::Obj(caps) = &*self.kiro_caps.lock().unwrap() {
                    let offers: Vec<String> = caps.iter().filter_map(|(k, v)| match v {
                        Json::Arr(items) if ["scope", "source", "target"].iter().any(|w| k.to_lowercase().contains(w)) =>
                            Some(format!("{k}: {}", items.iter().filter_map(Json::as_str).collect::<Vec<_>>().join("/"))),
                        _ => None }).collect();
                    if !offers.is_empty() { note.push_str(&format!(" It offers {}.", offers.join("; "))); }
                }
            }
            Ok(CloudList { sessions, note })
        })
    }

    fn cloud_transcript(self: &Arc<Self>, id: &str, folder: &str) -> Result<Vec<CloudTurn>, String> {
        let name = self.name();
        self.with_tool(|ct| {
            if !self.can_load.load(Ordering::SeqCst) { return Err(CallErr::Acp(format!("This {name} CLI can’t open sessions again. Update Kiro CLI."))); }
            // A turn of its own, muted, so the replay is read into it and nothing else hears it.
            let turn = Arc::new(Turn { stream: Mutex::new(KiroStream::new(name)), progress: None, events: None, options: (self.options)(), folder: folder.into(), token: ct.clone(),
                muted: AtomicBool::new(true), replay: Mutex::new(Some(Replay::new(name))), last_update: Mutex::new(std::time::Instant::now()), live: AtomicUsize::new(0),
                refused: AtomicBool::new(false), mcp_failed: Mutex::new(vec![]), deny_all: true });
            self.turns.lock().unwrap().insert(id.into(), turn.clone());
            // A cloud session gets none of this computer's MCP servers.
            let mcp = (computer_use::acp(&[]), computer_use::signature(&[]));
            let params = vec![("sessionId", st(id)), ("cwd", st(folder)), ("mcpServers", mcp.0.clone()),
                ("_meta", o_(vec![("kiro", o_(vec![("sessionSource", st("remote"))]))]))];
            let res = self.call("session/load", o_(params), Some(ct), Some(Duration::from_secs(120)));
            { let mut t = self.turns.lock().unwrap(); if t.get(id).is_some_and(|x| Arc::ptr_eq(x, &turn)) { t.remove(id); } }
            let res = res?;
            // Loaded now: a reply carries on in it without loading it again.
            self.session_mcp.lock().unwrap().insert(id.into(), mcp.1);
            if let Some(o) = options(&res) { self.session_options.lock().unwrap().insert(id.into(), o); }
            let rep = turn.replay.lock().unwrap().take().unwrap_or_else(|| Replay::new(name));
            Ok(rep.turns.into_iter().map(|(prompt, mut st)| {
                st.end();
                let mut steps: Vec<KiroStep> = vec![];
                for e in st.drain() { if let Some(x) = e.step { match steps.iter().position(|y| y.id == x.id) { Some(i) => steps[i] = x, None => steps.push(x) } } }
                CloudTurn { prompt: prompt.trim().to_owned(), text: st.said().trim().to_owned(), steps, completed: st.completed }
            }).collect())
        })
    }

    fn finish(&self, t: &Turn, stop_reason: Option<&str>, cancelled: bool) -> KiroResult {
        let name = self.name();
        let stream = t.stream.lock().unwrap();
        let said = stream.said().trim().to_owned();
        if stop_reason == Some("cancelled") && !cancelled && t.refused.load(Ordering::SeqCst) {
            return KiroResult::new(KiroState::Failed, format!("{name} wanted to change files or run a command, and it is set to read only (Settings → {name})."));
        }
        if cancelled || stop_reason == Some("cancelled") {
            return KiroResult::new(KiroState::Cancelled, if said.is_empty() { format!("Stopped before {name} finished.") } else { said });
        }
        if stop_reason == Some("refusal") { return KiroResult::new(KiroState::Failed, format!("{name} declined this request.")); }
        stream.outcome(0, false, "")
    }

    fn explain(&self, message: &str) -> String {
        let lower = message.to_lowercase();
        if ["sign in", "signed in", "log in", "login", "unauthenticated", "unauthorized", "authentication"].iter().any(|k| lower.contains(k)) {
            return format!("{} needs you to sign in. {}", self.name(), agents::sign_in_hint(self.tool));
        }
        if crate::stream::units(message) > 600 { format!("{}…", crate::stream::head_units(message, 599)) } else { message.to_owned() }
    }

    /// Sets the model, effort and access the settings ask for, where the agent offers
    /// them and they differ. What it offers after that goes to Settings.
    fn configure(&self, sid: &str, mut offered: Vec<AcpOption>, o: &AgentOptions, ct: &Cancel) -> Result<Vec<AcpOption>, CallErr> {
        fn find(offered: &[AcpOption], category: Option<&str>, ids: &[&str]) -> Option<AcpOption> {
            offered.iter().find(|x| category.is_some() && x.category.as_deref() == category)
                .or_else(|| offered.iter().find(|x| ids.contains(&x.id.as_str()))).cloned()
        }
        let set = |offered: &mut Vec<AcpOption>, option: Option<AcpOption>, value: Option<&str>| -> Result<(), CallErr> {
            let (Some(option), Some(value)) = (option, value) else { return Ok(()) };
            if option.current.as_deref() == Some(value) { return Ok(()); }
            if !option.has(value) { hover_core::log::line(&format!("acp {}: {}={} isn't offered", self.name(), option.id, value)); return Ok(()); }
            let params = o_(vec![("sessionId", st(sid)), ("configId", st(&option.id)), ("value", st(value))]);
            match self.call("session/set_config_option", params, Some(ct), Some(Duration::from_secs(30))) {
                Ok(r) => { if let Some(now) = options(&r).filter(|n| !n.is_empty()) { *offered = now; } Ok(()) }
                Err(CallErr::Acp(m)) => { hover_core::log::line(&format!("acp {}: {}={} refused - {m}", self.name(), option.id, value)); Ok(()) }
                Err(e) => Err(e),
            }
        };
        let f = find(&offered, Some("model"), &["model"]);
        set(&mut offered, f, o.model.as_deref())?;
        // An effort list can appear only once a model is picked (Kiro's does).
        let f = find(&offered, Some("thought_level"), &["effortLevel", "reasoning_effort", "effort"]);
        set(&mut offered, f, o.effort.as_deref())?;
        // Asking needs the agent to ask Hover: each tool is put where it sends every call
        // it would stop for as session/request_permission, and Hover's own rules
        // (ask::needs_asking) decide which reach the user. What each offers (checked
        // against their sources, Sep 2026):
        // - Kiro (v3): the autopilot option; off, everything past its built-in defaults
        //   (workspace reads, read-only git) asks.
        // - Codex (codex-acp): the mode option. agent-full-access never asks; "agent" is
        //   Auto review, where Codex's own reviewer approves what it thinks safe and
        //   Hover would rarely hear of it; workspace-write asks for writes outside the
        //   folder and the network; read-only asks for every write and command. Ask
        //   always takes read-only (Hover then allows reads itself), Ask first
        //   workspace-write, as Codex's own "Auto" preset does.
        // - Cursor (agent acp): asks unless started with --force; its modes are agent,
        //   plan and ask. So it asks either way, and Full answers yes (permission()).
        let asks = !o.read_only && o.approval != AgentApproval::Autopilot;
        match self.tool {
            AgentTool::Kiro => {
                let f = find(&offered, None, &["autopilot"]);
                set(&mut offered, f, Some(if o.read_only || asks { "off" } else { "on" }))?;
                let f = find(&offered, Some("mode"), &["mode"]);
                set(&mut offered, f, Some(o.agent.as_deref().unwrap_or("vibe")))?;
            }
            AgentTool::Codex => {
                // codex-acp 1.13 dropped workspace-write, and its read-only became that
                // preset ("Ask for approval": asks for outside the folder and the
                // network). So Ask first takes whichever of the two is there.
                let f = find(&offered, Some("mode"), &["mode"]);
                let ask_first = if f.as_ref().is_some_and(|m| m.has("workspace-write")) { "workspace-write" } else { "read-only" };
                let mode = if o.read_only { "read-only" } else if !asks { "agent-full-access" }
                    else if o.approval == AgentApproval::Always { "read-only" } else { ask_first };
                set(&mut offered, f, Some(mode))?;
            }
            AgentTool::Cursor => { let f = find(&offered, Some("mode"), &["mode"]); set(&mut offered, f, Some(if o.read_only { "ask" } else { "agent" }))?; }
            // Antigravity (T3 Code's mapping): "yolo" never asks; "default" asks for edits,
            // commands and anything outside the folder (it reads the workspace itself), so
            // Hover's rules decide, and Read only refuses what isn't a read. Never
            // "auto_edit": its edits would bypass Ask first.
            AgentTool::Agy => {
                let f = find(&offered, Some("mode"), &["mode"]);
                set(&mut offered, f, Some(if !o.read_only && !asks { "yolo" } else { "default" }))?;
            }
            // OpenCode and Claude Code run their own ways (opencode.rs, claude.rs), never as ACP servers.
            AgentTool::OpenCode | AgentTool::Claude => {}
            // Only old chats have this tool (an agent of the user's own, gone from Hover); it has no host.
            AgentTool::Custom => {}
        }
        if !offered.is_empty() { self.raise_seen(&offered); }
        Ok(offered)
    }

    /// Read only allows reading and refuses the rest. Otherwise what the approval setting
    /// leaves alone is allowed, and the rest goes to the user, unless they trusted it
    /// earlier in the session. A stopped run withdraws the question.
    fn permission(&self, turn: Option<&Turn>, sid: Option<&str>, p: &Json) -> Json {
        let cancelled = || o(vec![("outcome", st("cancelled"))]);
        let (Some(turn), Some(Json::Arr(opts))) = (turn, p.get("options")) else { return cancelled() };
        let pick = |kinds: &[&str]| -> Option<String> {
            kinds.iter().find_map(|k| opts.iter().find(|x| s(x, "kind").unwrap_or("").starts_with(k) && s(x, "optionId").is_some()).and_then(|x| s(x, "optionId")).map(str::to_owned))
        };
        let selected = |option: Option<String>| option.map_or_else(cancelled, |id| o(vec![("outcome", st("selected")), ("optionId", st(&id))]));
        let allow = || selected(pick(&["allow_once", "allow"]));
        let reject = || selected(pick(&["reject_once", "reject"]));

        let call = p.get("toolCall").cloned().unwrap_or(Json::Null);
        let kind = s(&call, "kind").unwrap_or("other").to_owned();
        if turn.deny_all { turn.refused.store(true, Ordering::SeqCst); return reject(); }
        if turn.options.read_only {
            if matches!(kind.as_str(), "read" | "search" | "fetch" | "think") { return allow(); }
            turn.refused.store(true, Ordering::SeqCst);
            return reject();
        }
        let (question, outside) = ask::describe(&call, &kind, &turn.folder);
        if !ask::needs_asking(turn.options.approval, &kind, outside) { return allow(); }
        let key = ask::key(&question);
        if let Some(sid) = sid {
            let t = self.trusted.lock().unwrap();
            if t.get(sid).is_some_and(|k| k.contains("*") || k.contains(&key)) { return allow(); }
        }
        let asking = self.asking.lock().unwrap().clone();
        let (Some(asking), Some(sid)) = (asking, sid) else { return reject() };

        let (tx, rx) = mpsc::channel::<Option<AskAnswer>>();
        let t2 = tx.clone();
        let _stop = turn.token.on_cancel(move || { let _ = t2.send(None); });
        asking(sid, question, &turn.token, Box::new(move |a| { let _ = tx.send(Some(a)); }));
        let answer = match rx.recv() { Ok(Some(a)) if !turn.token.is_cancelled() => a, _ => return cancelled() };
        // Trust lasts the session and is Hover's: Hover answers the same call itself from
        // then on. The tool's own "always" is only picked where it too is for the
        // session. Cursor's allow-always writes a lasting rule into the user's own
        // ~/.cursor/cli-config.json, and Kiro's can change a Kiro setting
        // (setting_key); a click in the notch must never do that.
        let trust_option = || if self.tool == AgentTool::Codex { pick(&["allow_always", "allow"]) } else { pick(&["allow_once", "allow"]) };
        match answer {
            AskAnswer::Allow => allow(),
            AskAnswer::Trust | AskAnswer::TrustAll => {
                let k = if answer == AskAnswer::Trust { key } else { "*".into() };
                self.trusted.lock().unwrap().entry(sid.to_owned()).or_default().insert(k);
                selected(trust_option())
            }
            AskAnswer::Deny => reject(),
        }
    }

    fn raise_seen(&self, offered: &[AcpOption]) {
        // Which models the tool listed, and whether it ran in the sandbox: what to read when a list looks short.
        let ids: Vec<&str> = offered.iter().find(|o| o.category.as_deref() == Some("model")).map(|o| o.choices.iter().map(|c| c.value.as_str()).collect()).unwrap_or_default();
        hover_core::log::line(&format!("acp {}: offered {} model{}: {}; sandbox {}", self.name(), ids.len(), if ids.len() == 1 { "" } else { "s" }, ids.join(", "), if sandbox::active() { "on" } else { "off" }));
        for f in self.seen.lock().unwrap().iter() { f(self.tool, offered); }
    }

    // MARK: The process

    fn start(self: &Arc<Self>, ct: &Cancel) -> Result<(), CallErr> {
        let _g = self.gate.lock().unwrap();
        if ct.is_cancelled() { return Err(CallErr::Cancelled); }
        if self.link.lock().unwrap().is_some() { return Ok(()); }
        let name = self.name();
        let link = match (self.connect)() {
            Err(e) => return Err(CallErr::Acp(format!("{name} couldn’t start: {e}"))),
            Ok(None) => return Err(CallErr::Acp(format!("{name} isn’t installed. {}", agents::install_hint(self.tool)))),
            Ok(Some(l)) => l,
        };
        let gen = self.gens.fetch_add(1, Ordering::SeqCst) + 1;
        let Link { to_agent, from_agent, kill, errors } = link;
        let live = Arc::new(Live { gen, writer: Mutex::new(to_agent), kill, errors });
        *self.link.lock().unwrap() = Some(live);
        self.session_options.lock().unwrap().clear();
        self.session_mcp.lock().unwrap().clear();
        self.ready.lock().unwrap().clear();
        let me: Weak<Host> = Arc::downgrade(self);
        std::thread::Builder::new().name(format!("acp-{}", self.tool.id())).spawn(move || read(me, from_agent, gen)).expect("a reader thread");
        let init = o_(vec![
            ("protocolVersion", Json::int(1)),
            ("clientCapabilities", o_(vec![("fs", o_(vec![("readTextFile", Json::Bool(false)), ("writeTextFile", Json::Bool(false))])), ("terminal", Json::Bool(false))])),
            ("clientInfo", o_(vec![("name", st("hover")), ("version", st("1"))])),
        ]);
        match self.call("initialize", init, Some(ct), Some(Duration::from_secs(60))) {
            Ok(r) => {
                let load = r.get("agentCapabilities").filter(|c| matches!(c, Json::Obj(_))).and_then(|c| c.get("loadSession")) == Some(&Json::Bool(true));
                self.can_load.store(load, Ordering::SeqCst);
                let targets = r.get("agentCapabilities").and_then(|c| c.get("_meta")).and_then(|m| m.get("kiro")).and_then(|k| k.get("executionTargets"));
                self.can_cloud.store(matches!(targets, Some(Json::Arr(t)) if t.iter().any(|x| x.as_str() == Some("cloud-sandbox"))), Ordering::SeqCst);
                let image = r.get("agentCapabilities").and_then(|c| c.get("promptCapabilities")).and_then(|p| p.get("image")) == Some(&Json::Bool(true));
                self.can_image.store(image, Ordering::SeqCst);
                let list = r.get("agentCapabilities").and_then(|c| c.get("sessionCapabilities")).and_then(|c| c.get("list")).is_some_and(|l| !l.is_null() && l != &Json::Bool(false));
                self.can_list.store(list, Ordering::SeqCst);
                *self.kiro_caps.lock().unwrap() = r.get("agentCapabilities").and_then(|c| c.get("_meta")).and_then(|m| m.get("kiro")).cloned().unwrap_or(Json::Null);
                let methods: Vec<String> = match r.get("authMethods") { Some(Json::Arr(m)) => m.iter().filter_map(|x| s(x, "id").map(str::to_owned)).collect(), _ => vec![] };
                // Antigravity's server makes no session until a sign-in method is picked
                // ("Authentication required", -32000), so it is signed in at once, as T3
                // Code does: an API key in the environment, else Google's own sign-in, which
                // the server runs itself (a browser, back to it on this PC's loopback) and
                // which returns at once when it already has a token.
                if self.tool == AgentTool::Agy {
                    let method = if std::env::var_os("GEMINI_API_KEY").is_some_and(|k| !k.is_empty()) { "gemini-api-key" } else { "oauth-personal" };
                    if methods.iter().any(|m| m == method) {
                        hover_core::log::line(&format!("acp {name}: signing in ({method})"));
                        if let Err(e) = self.call("authenticate", o_(vec![("methodId", st(method))]), Some(ct), Some(Duration::from_secs(600))) {
                            hover_core::log::line(&format!("acp {name}: sign-in ({method}) failed - {e}"));
                            self.shutdown("didn't sign in");
                            return Err(e);
                        }
                    }
                }
                Ok(())
            }
            Err(e) => { self.shutdown("didn't start"); Err(e) }
        }
    }

    fn shutdown(&self, why: &str) {
        let mut l = self.link.lock().unwrap();
        let Some(link) = l.take() else { return };
        self.idle.fetch_add(1, Ordering::SeqCst);
        // Cleared and failed before the link's lock goes: once it does, the next process
        // can start, and its calls and options must not go with this one.
        self.session_options.lock().unwrap().clear();
        self.session_mcp.lock().unwrap().clear();
        self.ready.lock().unwrap().clear();
        self.fail(CallErr::Gone(format!("{} stopped.", self.name())));
        drop(l);
        hover_core::log::line(&format!("acp {}: {why}", self.name()));
        (link.kill)();
    }

    fn gone(&self, gen: u64) {
        let (link, why) = {
            let mut l = self.link.lock().unwrap();
            if l.as_ref().is_none_or(|x| x.gen != gen) { return; }
            let link = l.take().unwrap();
            self.idle.fetch_add(1, Ordering::SeqCst);
            let text = strip_ansi(&(link.errors)());
            let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
            let why = lines[lines.len().saturating_sub(2)..].join(" / ");
            let tail = if why.is_empty() { String::new() } else { format!(" {}", lines[lines.len().saturating_sub(2)..].join("\n")) };
            // As in shutdown: under the lock, or a process started meanwhile (a reply
            // right after an idle shutdown) had its calls failed by this one's exit.
            self.session_options.lock().unwrap().clear();
            self.session_mcp.lock().unwrap().clear();
            self.ready.lock().unwrap().clear();
        self.ready.lock().unwrap().clear();
            self.fail(CallErr::Gone(format!("{} stopped unexpectedly.{tail}", self.name())));
            (link, why)
        };
        hover_core::log::line(&format!("acp {}: exited - {why}", self.name()));
        (link.kill)();
    }

    fn fail(&self, e: CallErr) {
        for (_, tx) in self.pending.lock().unwrap().drain() { let _ = tx.send(Msg::Reply(Err(e.clone()))); }
    }

    fn schedule_idle(self: &Arc<Self>, after: Duration) {
        let gen = self.idle.fetch_add(1, Ordering::SeqCst) + 1;
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            if let Some(h) = me.upgrade() {
                if h.idle.load(Ordering::SeqCst) == gen && h.busy.load(Ordering::SeqCst) == 0 { h.shutdown("idle"); }
            }
        });
    }

    // MARK: JSON-RPC

    fn send(&self, message: &Json) -> Result<(), CallErr> {
        let link = self.link.lock().unwrap().clone().ok_or_else(|| CallErr::Gone(format!("{} stopped.", self.name())))?;
        let mut bytes = message.compact().into_bytes();
        bytes.push(b'\n');
        let mut w = link.writer.lock().unwrap();
        w.write_all(&bytes).and_then(|_| w.flush()).map_err(|_| CallErr::Gone(format!("{} stopped.", self.name())))
    }

    fn notify(&self, method: &str, params: Json) { let _ = self.send(&o_(vec![("jsonrpc", st("2.0")), ("method", st(method)), ("params", params)])); }

    fn begin_call(&self, method: &str, params: Json) -> Result<(i64, mpsc::Receiver<Msg>), CallErr> {
        let id = self.ids.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if let Err(e) = self.send(&o_(vec![("jsonrpc", st("2.0")), ("id", Json::int(id)), ("method", st(method)), ("params", params)])) {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        Ok((id, rx))
    }

    fn call(&self, method: &str, params: Json, ct: Option<&Cancel>, timeout: Option<Duration>) -> Result<Json, CallErr> {
        if ct.is_some_and(Cancel::is_cancelled) { return Err(CallErr::Cancelled); }
        let (id, rx) = self.begin_call(method, params)?;
        let tx = self.pending.lock().unwrap().get(&id).cloned();
        let _reg = ct.map(|c| {
            let tx = tx.clone();
            c.on_cancel(move || { if let Some(tx) = tx { let _ = tx.send(Msg::Reply(Err(CallErr::Cancelled))); } })
        });
        let got = match timeout { Some(t) => rx.recv_timeout(t), None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected) };
        self.pending.lock().unwrap().remove(&id);
        match got {
            Ok(Msg::Reply(r)) => r,
            Ok(Msg::CancelAsked) => Err(CallErr::Cancelled),
            Err(RecvTimeoutError::Timeout) => Err(CallErr::Acp(format!("{} didn’t answer ({method}).", self.name()))),
            Err(RecvTimeoutError::Disconnected) => Err(CallErr::Gone(format!("{} stopped.", self.name()))),
        }
    }

    fn handle(self: &Arc<Self>, line: &str) {
        let Ok(m @ Json::Obj(_)) = json::parse(line) else { return };
        let method = s(&m, "method");
        let id = m.get("id").filter(|i| matches!(i, Json::Num(_) | Json::Str(_)));
        let Some(method) = method else {
            // An answer to one of ours. An id that isn't a whole number is no one's (C#
            // threw there and its reader stopped for good).
            let Some(n) = id.and_then(|i| i.i64().ok()) else { return };
            let Some(tx) = self.pending.lock().unwrap().remove(&n) else { return };
            let r = match m.get("error") {
                Some(err @ Json::Obj(_)) => Err(CallErr::Acp(s(err, "message").map_or_else(|| format!("{} reported an error.", self.name()), str::to_owned))),
                _ => Ok(m.get("result").cloned().unwrap_or(Json::Null)),
            };
            let _ = tx.send(Msg::Reply(r));
            return;
        };
        let p = m.get("params").cloned().unwrap_or(Json::Null);
        let psid = s(&p, "sessionId").map(str::to_owned);
        if method == "session/update" {
            let kind = p.get("update").and_then(|u| u.get("_meta")).and_then(|m| m.get("kiro")).and_then(|k| s(k, "kind"));
            if kind == Some("context_usage") { if let Some(sid) = &psid { self.ready.lock().unwrap().insert(sid.clone()); } }
        }
        let turn = psid.as_ref().and_then(|sid| self.turns.lock().unwrap().get(sid).cloned());
        if let Some(id) = id {
            if method == "session/request_permission" {
                // Answered on its own thread: the user may take minutes, and every other
                // session's news comes down this same pipe meanwhile.
                let (me, id, sid) = (self.clone(), id.clone(), psid.clone());
                std::thread::Builder::new().name("acp-permission".into()).spawn(move || {
                    let outcome = me.permission(turn.as_deref(), sid.as_deref(), &p);
                    let _ = me.send(&o_(vec![("jsonrpc", st("2.0")), ("id", id), ("result", o_(vec![("outcome", outcome)]))]));
                }).expect("a thread for the question");
            } else {
                let _ = self.send(&o_(vec![("jsonrpc", st("2.0")), ("id", id.clone()), ("error", o_(vec![("code", Json::int(-32601)), ("message", st("Not supported by Hover."))]))]));
            }
            return;
        }
        // Read even while a loaded conversation's replay is muted: servers start with
        // the session, and one that didn't is news about this turn, not the past.
        if method == "_kiro/mcp/status" {
            if let Some(t) = &turn { self.mcp_status(t, &p); }
            return;
        }
        let Some(turn) = turn else { return };
        if turn.muted.load(Ordering::SeqCst) {
            // Attaching: the replay is read for the turn that was cut off; otherwise it is ignored.
            if method == "session/update" {
                if let Some(rp) = turn.replay.lock().unwrap().as_mut() { rp.feed(line, p.get("update")); }
            }
            return;
        }
        match method {
            "session/update" => {
                *turn.last_update.lock().unwrap() = std::time::Instant::now();
                turn.live.fetch_add(1, Ordering::SeqCst);
                let (phase, events) = { let mut st = turn.stream.lock().unwrap(); (st.feed(line), st.drain()) };
                if let (Some(p), Some(f)) = (phase, &turn.progress) { f(p); }
                if let Some(f) = &turn.events { for e in events { f(e); } }
                if let Some(u) = p.get("update") {
                    if s(u, "sessionUpdate") == Some("config_option_update") {
                        if let Some(now) = options(u).filter(|n| !n.is_empty()) {
                            self.session_options.lock().unwrap().insert(psid.clone().unwrap(), now.clone());
                            self.raise_seen(&now);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// An MCP server that didn't start is said in the chat, as a failed step, and the
    /// turn goes on. 2.x could end the turn for it (KiroRequireMcp), which threw away a
    /// task that may never have needed that server.
    fn mcp_status(&self, turn: &Turn, p: &Json) {
        let Some(Json::Arr(servers)) = p.get("servers") else { return };
        for sv in servers {
            if !matches!(s(sv, "status"), Some("failed" | "error")) { continue; }
            let server = s(sv, "name").filter(|n| !n.trim().is_empty()).unwrap_or("unnamed").trim().to_owned();
            {
                // Kiro may report every server again on each change: one step per server.
                let mut failed = turn.mcp_failed.lock().unwrap();
                if failed.contains(&server) { continue; }
                failed.push(server.clone());
            }
            hover_core::log::line(&format!("acp {}: MCP server {server} didn't start", self.name()));
            let step = KiroStep::new(&format!("hover-mcp-{server}"), "other", &format!("Started MCP server {server}"), None, "failed");
            if let Some(f) = &turn.events { f(KiroEvent { step: Some(step), ..Default::default() }); }
        }
    }
}

fn o_(props: Vec<(&str, Json)>) -> Json { o(props) }

/// The line under the answer naming the MCP servers that didn't start, if any. Names
/// are code, so one with Markdown in it ("my_server") reads as it is.
fn mcp_note(failed: &[String]) -> Option<String> {
    if failed.is_empty() { return None; }
    let names = failed.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
    Some(if failed.len() == 1 { format!("MCP server {names} didn’t start, so its tools weren’t available.") }
        else { format!("MCP servers {names} didn’t start, so their tools weren’t available.") })
}

/// Lines as StreamReader.ReadLine splits them (\n, \r\n or \r), bad UTF-8 replaced.
fn read(me: Weak<Host>, from: Box<dyn std::io::Read + Send>, gen: u64) {
    let mut r = BufReader::new(from);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let Some(h) = me.upgrade() else { return };
        for line in text.trim_end_matches('\n').split('\r') {
            if line.starts_with('{') { h.handle(line); }
        }
    }
    if let Some(h) = me.upgrade() { h.gone(gen); }
}

/// The configOptions of a session/new, session/load or set_config_option answer.
fn options(r: &Json) -> Option<Vec<AcpOption>> {
    let Some(Json::Arr(list)) = r.get("configOptions") else { return None };
    let mut all = vec![];
    for x in list {
        let Some(id) = s(x, "id") else { continue };
        let mut choices = vec![];
        if let Some(Json::Arr(opts)) = x.get("options") {
            for c in opts {
                // Flat, or in named groups of their own.
                if let Some(v) = s(c, "value") {
                    choices.push(AcpChoice { value: v.into(), name: s(c, "name").unwrap_or(v).into(), levels: None });
                } else if let Some(Json::Arr(inner)) = c.get("options") {
                    for g in inner {
                        if let Some(v) = s(g, "value") { choices.push(AcpChoice { value: v.into(), name: s(g, "name").unwrap_or(v).into(), levels: None }); }
                    }
                }
            }
        }
        all.push(AcpOption { id: id.into(), category: s(x, "category").map(str::to_owned), current: s(x, "currentValue").map(str::to_owned), choices });
    }
    Some(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_options_read_flat_and_grouped() {
        let r = json::parse(r#"{"configOptions":[{"id":"model","category":"model","currentValue":"a","options":[{"value":"a","name":"A"},{"group":"g","options":[{"value":"b"}]}]},{"category":"x"}]}"#).unwrap();
        let o = options(&r).unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].choices, vec![AcpChoice { value: "a".into(), name: "A".into(), levels: None }, AcpChoice { value: "b".into(), name: "b".into(), levels: None }]);
        assert!(options(&Json::Null).is_none());
    }
}
