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
use crate::proc::{strip_ansi, Link};
use crate::stream::{KiroEvent, KiroPhase, KiroResult, KiroStream};
use hover_core::json::{self, Json};
use hover_core::model::{AcpChoice, AcpOption, AgentApproval, AgentOptions, AgentTool, KiroState};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub type Progress = Box<dyn Fn(KiroPhase) + Send + Sync>;
pub type Events = Box<dyn Fn(KiroEvent) + Send + Sync>;
type Connect = Box<dyn Fn() -> std::io::Result<Option<Link>> + Send + Sync>;
type Seen = Box<dyn Fn(AgentTool, &[AcpOption]) + Send + Sync>;
/// AcpHost.Asking: asks the user about a tool call for the ACP session named first;
/// the token ends when the run is stopped. The answer goes to the reply, from any thread.
pub type Asking = Arc<dyn Fn(&str, AgentAsk, &Cancel, Box<dyn FnOnce(AskAnswer) + Send>) + Send + Sync>;

#[derive(Debug, Clone)]
enum CallErr {
    Acp(String),
    Gone(String),
    Cancelled,
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
    refused: AtomicBool,
    mcp_failed: Mutex<Option<String>>,
}

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
}

#[derive(Clone)]
pub struct AcpHost(Arc<Host>);

fn s<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }
fn o(props: Vec<(&str, Json)>) -> Json { Json::obj(props) }
fn st(v: &str) -> Json { Json::str(v) }

impl AcpHost {
    /// The tool as Agents finds and starts it.
    pub fn new(tool: AgentTool, options: impl Fn() -> AgentOptions + Send + Sync + 'static) -> AcpHost {
        AcpHost::with_connect(tool, options, move || match agents::exe(tool) {
            None => Ok(None),
            Some(exe) => crate::proc::launch(&exe, agents::arguments(tool), &[]).map(Some),
        })
    }

    pub fn with_connect(tool: AgentTool, options: impl Fn() -> AgentOptions + Send + Sync + 'static,
        connect: impl Fn() -> std::io::Result<Option<Link>> + Send + Sync + 'static) -> AcpHost {
        AcpHost(Arc::new(Host {
            tool, options: Box::new(options), connect: Box::new(connect), gate: Mutex::new(()), link: Mutex::new(None), gens: AtomicU64::new(0),
            pending: Mutex::new(HashMap::new()), turns: Mutex::new(HashMap::new()), session_options: Mutex::new(HashMap::new()),
            can_load: AtomicBool::new(false), ids: AtomicI64::new(0), busy: AtomicUsize::new(0), idle: AtomicU64::new(0), seen: Mutex::new(vec![]),
            asking: Mutex::new(None), trusted: Mutex::new(HashMap::new()),
        }))
    }

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
        self.0.run(folder, prompt, progress, ct, resume, events, None)
    }

    /// run, with the session's own tool access (AgentOptions::with_access).
    #[allow(clippy::too_many_arguments)]
    pub fn run_as(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access)
    }

    /// Where a question goes. Without one, whatever the settings say should be asked
    /// about is turned down.
    pub fn set_asking(&self, f: Asking) { *self.0.asking.lock().unwrap() = Some(f); }

    /// Ends the tool's process now. Runs still going fail; the next one starts it again.
    pub fn shutdown(&self, why: &str) { self.0.shutdown(why) }

    /// The session's runner for this tool (OwlApp.Kiro's make: Agents[tool].Run).
    pub fn runner(&self) -> crate::session::RunTask {
        let h = self.clone();
        Arc::new(move |a: crate::session::RunArgs| h.run_as(&a.folder, &a.prompt, Some(a.progress), &a.ct, a.resume.as_deref(), Some(a.events), a.access.as_deref()))
    }
}

impl Host {
    fn name(&self) -> &'static str { self.tool.name() }

    #[allow(clippy::too_many_arguments)]
    fn run(self: &Arc<Self>, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        let name = self.name();
        if !crate::usable_folder(Some(folder)) { return KiroResult::new(KiroState::Failed, "That folder isn’t there any more. Choose another one."); }
        if prompt.trim().is_empty() { return KiroResult::new(KiroState::Failed, format!("Tell {name} what to do first.")); }
        let o = (self.options)().with_access(access);
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.idle.fetch_add(1, Ordering::SeqCst);
        let turn = Arc::new(Turn { stream: Mutex::new(KiroStream::new(name)), progress, events, options: o.clone(), folder: folder.into(), token: ct.clone(), muted: AtomicBool::new(false),
            refused: AtomicBool::new(false), mcp_failed: Mutex::new(None) });
        let mut sid: Option<String> = None;
        let r = self.turn(folder, prompt, ct, resume, &o, &turn, &mut sid);
        let result = match r {
            Ok(r) => r,
            Err(CallErr::Cancelled) => self.finish(&turn, Some("cancelled"), true),
            Err(CallErr::Acp(m)) => KiroResult::new(KiroState::Failed, self.explain(&m)),
            Err(CallErr::Gone(m)) => if ct.is_cancelled() { self.finish(&turn, Some("cancelled"), true) } else { KiroResult::new(KiroState::Failed, m) },
        };
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
    fn turn(self: &Arc<Self>, folder: &str, prompt: &str, ct: &Cancel, resume: Option<&str>, o: &AgentOptions, turn: &Arc<Turn>, sid: &mut Option<String>)
        -> Result<KiroResult, CallErr> {
        let name = self.name();
        if let Some(p) = &turn.progress { p(KiroPhase::Starting); }
        self.start(ct)?;
        let mut offered: Option<Vec<AcpOption>> = None;
        if let Some(r) = resume.filter(|r| !r.is_empty()) {
            let known = self.session_options.lock().unwrap().get(r).cloned();
            if let Some(k) = known {
                *sid = Some(r.into());
                offered = Some(k);
            } else if self.can_load.load(Ordering::SeqCst) {
                turn.muted.store(true, Ordering::SeqCst);
                self.turns.lock().unwrap().insert(r.into(), turn.clone());
                let params = o_(vec![("sessionId", st(r)), ("cwd", st(folder)), ("mcpServers", Json::Arr(vec![]))]);
                match self.call("session/load", params, Some(ct), Some(Duration::from_secs(120))) {
                    Ok(res) => { *sid = Some(r.into()); offered = options(&res); }
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
            let res = self.call("session/new", o_(vec![("cwd", st(folder)), ("mcpServers", Json::Arr(vec![]))]), Some(ct), Some(Duration::from_secs(120)))?;
            *sid = Some(s(&res, "sessionId").ok_or_else(|| CallErr::Acp(format!("{name} didn’t start a session.")))?.to_owned());
            offered = options(&res);
        }
        let id = sid.clone().unwrap();
        self.turns.lock().unwrap().insert(id.clone(), turn.clone());
        if let Some(e) = &turn.events { e(KiroEvent { session_id: Some(id.clone()), ..Default::default() }); }
        let configured = self.configure(&id, offered.unwrap_or_default(), o, ct)?;
        self.session_options.lock().unwrap().insert(id.clone(), configured);

        let params = o_(vec![("sessionId", st(&id)), ("prompt", Json::Arr(vec![o_(vec![("type", st("text")), ("text", st(prompt.trim()))])]))]);
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
                        if self.busy.load(Ordering::SeqCst) == 1 { self.shutdown("didn't stop when asked"); }
                        return Ok(self.finish(turn, Some("cancelled"), true));
                    }
                },
                Err(_) => break Err(CallErr::Gone(format!("{name} stopped."))),
            }
        }?;
        Ok(self.finish(turn, s(&reply, "stopReason"), ct.is_cancelled()))
    }

    fn finish(&self, t: &Turn, stop_reason: Option<&str>, cancelled: bool) -> KiroResult {
        let name = self.name();
        if let Some(server) = t.mcp_failed.lock().unwrap().clone() {
            return KiroResult::new(KiroState::Failed, format!("An MCP server {name} depends on ({server}) didn’t start."));
        }
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
            // OpenCode runs as its own server (opencode.rs), never as an ACP one.
            AgentTool::OpenCode => {}
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

    fn raise_seen(&self, offered: &[AcpOption]) { for f in self.seen.lock().unwrap().iter() { f(self.tool, offered); } }

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
                Ok(())
            }
            Err(e) => { self.shutdown("didn't start"); Err(e) }
        }
    }

    fn shutdown(&self, why: &str) {
        let Some(link) = self.link.lock().unwrap().take() else { return };
        self.idle.fetch_add(1, Ordering::SeqCst);
        hover_core::log::line(&format!("acp {}: {why}", self.name()));
        self.session_options.lock().unwrap().clear();
        self.fail(CallErr::Gone(format!("{} stopped.", self.name())));
        (link.kill)();
    }

    fn gone(&self, gen: u64) {
        let link = {
            let mut l = self.link.lock().unwrap();
            if l.as_ref().is_none_or(|x| x.gen != gen) { return; }
            l.take().unwrap()
        };
        self.idle.fetch_add(1, Ordering::SeqCst);
        self.session_options.lock().unwrap().clear();
        let text = strip_ansi(&(link.errors)());
        let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        let why = &lines[lines.len().saturating_sub(2)..];
        hover_core::log::line(&format!("acp {}: exited - {}", self.name(), why.join(" / ")));
        let tail = if why.is_empty() { String::new() } else { format!(" {}", why.join("\n")) };
        self.fail(CallErr::Gone(format!("{} stopped unexpectedly.{tail}", self.name())));
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
        let Some(turn) = turn.filter(|t| !t.muted.load(Ordering::SeqCst)) else { return };
        match method {
            "session/update" => {
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
            "_kiro/mcp/status" if turn.options.require_mcp => {
                if let Some(Json::Arr(servers)) = p.get("servers") {
                    for sv in servers {
                        if matches!(s(sv, "status"), Some("failed" | "error")) && turn.mcp_failed.lock().unwrap().is_none() {
                            *turn.mcp_failed.lock().unwrap() = Some(s(sv, "name").unwrap_or("one").to_owned());
                            self.notify("session/cancel", o_(vec![("sessionId", st(psid.as_deref().unwrap_or("")))]));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn o_(props: Vec<(&str, Json)>) -> Json { o(props) }

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
