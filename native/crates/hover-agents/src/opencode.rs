//! Services/OpenCodeHost.cs: OpenCode as T3 Code runs it. One Hover-owned
//! "opencode serve" on 127.0.0.1, a port the system picks, no mDNS, and a password made
//! for that process only (in its environment, sent as Basic auth, never on a command
//! line or in a URL). Every request names the session's folder, so one server serves
//! every folder with that folder's opencode config, agents, skills and MCP servers.
//! OpenCode keeps its own providers (API keys, cloud sign-ins, local models); Hover
//! never sees them.
//!
//! A turn follows T3's order: subscribe to the events first, then send the prompt
//! (prompt_async, with a message id Hover makes), and count the turn done only once the
//! server went idle after it saw that message or went busy for it. An idle left over
//! from before, or from another session, never ends it. When the event stream drops,
//! the turn reconnects and reads the session's state and messages back, since missed
//! events aren't replayed. A prompt whose sending can't be confirmed is looked up by
//! its id, never sent twice.
//!
//! Shut down after the idle time in its settings, like the ACP tools. The C# is async;
//! here a turn blocks its own thread, and the event stream, the watch, each approval
//! and each question have threads of their own.

use crate::acp::{Asking, Events, Progress};
use crate::agents;
use crate::ask::{self, AgentAsk, AgentQuestion, Answers, AskAnswer};
use crate::cancel::Cancel;
use crate::http::{escape_data, Client, HttpErr};
use crate::proc::strip_ansi;
use crate::stream::{clip_to, head_units, tool_phase, units, KiroEvent, KiroPhase, KiroResult};
use hover_core::json::{self, Json};
use hover_core::model::{AcpChoice, AcpOption, AgentApproval, AgentOptions, AgentTool, KiroState, KiroStep};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

/// OpenCodeHost.Questioning: asks the user a question the agent has (ask.questions),
/// for OpenCode's session id named first. The answer is each question's picked labels,
/// in order; none when the user skipped it.
pub type Questioning = Arc<dyn Fn(&str, AgentAsk, &Cancel, Box<dyn FnOnce(Answers) + Send>) + Send + Sync>;
type Seen = Box<dyn Fn(AgentTool, &[AcpOption]) + Send + Sync>;

/// A running OpenCode server: where it listens, the password it was started with, how
/// to end it, what it last printed, and a channel that closes or speaks when it exits.
pub struct OpenCodeLink {
    pub url: String,
    pub password: String,
    pub kill: Box<dyn Fn() + Send + Sync>,
    pub errors: Box<dyn Fn() -> String + Send + Sync>,
    pub exited: Option<mpsc::Receiver<()>>,
}

type Connect = Box<dyn Fn(&Cancel, &Timeouts) -> Result<Option<OpenCodeLink>, OcErr> + Send + Sync>;

/// How long a startup, a prompt's sending and the first event wait. T3 waits 30 s for
/// the server; the C# measured 39 s on a cold start (its plugins load first), so 90.
#[derive(Clone, Debug)]
pub struct Timeouts { pub start: Duration, pub send: Duration, pub connect: Duration, pub stop_grace: Duration, pub quiet: Duration }

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { start: Duration::from_secs(90), send: Duration::from_secs(10), connect: Duration::from_secs(10), stop_grace: Duration::from_secs(8), quiet: Duration::from_secs(20) }
    }
}

/// OpenCodeError (a status when the server answered), a failed connection
/// (HttpRequestException), or the run's stop.
#[derive(Debug, Clone)]
pub enum OcErr { Oc(Option<u16>, String), Net(String), Cancelled }

const NAME: &str = "OpenCode";

fn s<'a>(e: Option<&'a Json>, name: &str) -> Option<&'a str> { e?.get(name).and_then(Json::as_str) }
fn num(e: Option<&Json>, name: &str) -> Option<f64> { match e?.get(name) { Some(v @ Json::Num(_)) => v.f64().ok(), _ => None } }
fn st(v: &str) -> Json { Json::str(v) }
fn is_true(e: Option<&Json>, name: &str) -> bool { e.and_then(|e| e.get(name)) == Some(&Json::Bool(true)) }
fn is_false(e: Option<&Json>, name: &str) -> bool { e.and_then(|e| e.get(name)) == Some(&Json::Bool(false)) }
fn cap(t: &str) -> String { let mut c = t.chars(); c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str()) }
fn log(t: &str) { hover_core::log::line(&format!("opencode: {t}")); }

/// Shared answer slot a thread waits on (TaskCompletionSource).
struct Done { r: Mutex<Option<KiroResult>>, cv: Condvar }

impl Done {
    fn set(&self, r: KiroResult) -> bool {
        let mut g = self.r.lock().unwrap();
        if g.is_some() { return false; }
        *g = Some(r);
        self.cv.notify_all();
        true
    }
    fn is_set(&self) -> bool { self.r.lock().unwrap().is_some() }
    fn wait(&self) -> KiroResult { self.cv.wait_while(self.r.lock().unwrap(), |r| r.is_none()).unwrap().clone().unwrap() }
    /// Waits up to `d`; true once it is set.
    fn wait_for(&self, d: Duration) -> bool { self.cv.wait_timeout_while(self.r.lock().unwrap(), d, |r| r.is_none()).unwrap().0.is_some() }
}

/// The answer text, by message, in order, from this turn's assistant messages.
#[derive(Default)]
struct Said {
    messages: Vec<String>,
    part_order: HashMap<String, Vec<String>>,
    text: HashMap<String, String>,
    roles: HashMap<String, String>,
    steps: HashMap<String, KiroStep>,
    began: HashMap<String, Instant>,
}

/// One turn: the session it is in, the message it sent, and what it has seen.
struct Turn {
    folder: String,
    options: AgentOptions,
    progress: Option<Progress>,
    events: Option<Events>,
    token: Cancel,
    sid: Mutex<String>,
    message_id: Mutex<String>,
    /// This session and the subagents' sessions it started.
    related: Mutex<HashSet<String>>,
    done: Done,
    connected: (Mutex<bool>, Condvar),
    /// Ends the event stream, the watch and the reads back when the turn ends.
    stream: Cancel,
    accepted: AtomicBool, user_seen: AtomicBool, busy_seen: AtomicBool, stopping: AtomicBool, refused: AtomicBool, idle_early: AtomicBool, connects: AtomicBool,
    error: Mutex<Option<String>>,
    retry: Mutex<Option<String>>,
    last_event: Mutex<Instant>,
    idle_confirms: AtomicUsize,
    /// Requests being asked about now, and those answered, so neither is asked twice.
    open: Mutex<HashMap<String, Cancel>>,
    resolved: Mutex<HashSet<String>>,
    said: Mutex<Said>,
    phase: Mutex<KiroPhase>,
    context: Mutex<Option<f64>>,
}

impl Turn {
    fn sid(&self) -> String { self.sid.lock().unwrap().clone() }
    fn mid(&self) -> String { self.message_id.lock().unwrap().clone() }
    fn flag(f: &AtomicBool) -> bool { f.load(Ordering::SeqCst) }

    fn said(&self) -> String {
        let g = self.said.lock().unwrap();
        for m in g.messages.iter().rev() {
            if let Some(parts) = g.part_order.get(m) {
                let t: String = parts.iter().map(|p| g.text.get(p).map_or("", String::as_str)).collect();
                let t = t.trim();
                if !t.is_empty() { return t.to_owned(); }
            }
        }
        String::new()
    }

    fn set_phase(&self, p: KiroPhase) {
        {
            let mut g = self.phase.lock().unwrap();
            if *g == p { return; }
            *g = p;
        }
        if let Some(f) = &self.progress { f(p); }
    }

    fn stopped(&self) -> KiroResult {
        let said = self.said();
        KiroResult::new(KiroState::Cancelled, if said.is_empty() { format!("Stopped before {NAME} finished.") } else { said })
    }

    fn cancel_open(&self) { let open: Vec<Cancel> = self.open.lock().unwrap().values().cloned().collect(); for c in open { c.cancel(); } }
}

struct Live { gen: u64, client: Client, kill: Box<dyn Fn() + Send + Sync>, errors: Box<dyn Fn() -> String + Send + Sync> }

struct Host {
    options: Box<dyn Fn() -> AgentOptions + Send + Sync>,
    connect: Connect,
    t: Timeouts,
    gate: Mutex<()>,
    live: Mutex<Option<Arc<Live>>>,
    gens: AtomicU64,
    turns: Mutex<HashMap<String, Arc<Turn>>>,
    trusted: Mutex<HashMap<String, HashSet<String>>>,
    inventory: Mutex<HashMap<String, Json>>,
    stuck: Mutex<HashSet<String>>,
    last_model: Mutex<HashMap<String, String>>,
    busy: AtomicUsize,
    idle: AtomicU64,
    seen: Mutex<Vec<Seen>>,
    asking: Mutex<Option<Asking>>,
    questioning: Mutex<Option<Questioning>>,
    reconciling: Mutex<()>,
}

/// OpenCode's runtime: shared by every OpenCode session.
#[derive(Clone)]
pub struct OpenCodeHost(Arc<Host>);

impl OpenCodeHost {
    /// OpenCode as Agents finds and starts it.
    pub fn new(options: impl Fn() -> AgentOptions + Send + Sync + 'static) -> OpenCodeHost {
        OpenCodeHost::build(options, Box::new(launch), Timeouts::default())
    }

    /// With the server given (tests hand in a stand-in), and shorter waits.
    pub fn with_connect(options: impl Fn() -> AgentOptions + Send + Sync + 'static, connect: impl Fn() -> Option<OpenCodeLink> + Send + Sync + 'static, t: Timeouts) -> OpenCodeHost {
        OpenCodeHost::build(options, Box::new(move |_, _| Ok(connect())), t)
    }

    fn build(options: impl Fn() -> AgentOptions + Send + Sync + 'static, connect: Connect, t: Timeouts) -> OpenCodeHost {
        OpenCodeHost(Arc::new(Host {
            options: Box::new(options), connect, t, gate: Mutex::new(()), live: Mutex::new(None), gens: AtomicU64::new(0), turns: Default::default(),
            trusted: Default::default(), inventory: Default::default(), stuck: Default::default(), last_model: Default::default(),
            busy: AtomicUsize::new(0), idle: AtomicU64::new(0), seen: Mutex::new(vec![]), asking: Mutex::new(None), questioning: Mutex::new(None),
            reconciling: Mutex::new(()),
        }))
    }

    pub fn tool(&self) -> AgentTool { AgentTool::OpenCode }
    /// The server is up.
    pub fn alive(&self) -> bool { self.0.live.lock().unwrap().is_some() }
    pub fn on_options_seen(&self, f: impl Fn(AgentTool, &[AcpOption]) + Send + Sync + 'static) { self.0.seen.lock().unwrap().push(Box::new(f)); }
    pub fn set_asking(&self, f: Asking) { *self.0.asking.lock().unwrap() = Some(f); }
    pub fn set_questioning(&self, f: Questioning) { *self.0.questioning.lock().unwrap() = Some(f); }
    pub fn shutdown(&self, why: &str) { self.0.end(None, why, "OpenCode stopped."); }

    /// Runs one turn: a new conversation, or the one resume names. Never fails outright.
    /// Blocks: run it off the UI thread.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&self, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        self.0.run(folder, prompt, progress, ct, resume, events, access)
    }

    pub fn runner(&self) -> crate::session::RunTask {
        let h = self.clone();
        Arc::new(move |a: crate::session::RunArgs| h.run(&a.folder, &a.prompt, Some(a.progress), &a.ct, a.resume.as_deref(), Some(a.events), a.access.as_deref()))
    }
}

impl Host {
    // MARK: A run

    #[allow(clippy::too_many_arguments)]
    fn run(self: &Arc<Self>, folder: &str, prompt: &str, progress: Option<Progress>, ct: &Cancel, resume: Option<&str>, events: Option<Events>, access: Option<&str>) -> KiroResult {
        if !crate::usable_folder(Some(folder)) { return KiroResult::new(KiroState::Failed, "That folder isn’t there any more. Choose another one."); }
        if prompt.trim().is_empty() { return KiroResult::new(KiroState::Failed, format!("Tell {NAME} what to do first.")); }
        let o = (self.options)().with_access(access);
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.idle.fetch_add(1, Ordering::SeqCst);
        let turn = Arc::new(Turn {
            folder: folder.into(), options: o.clone(), progress, events, token: ct.clone(), sid: Mutex::new(String::new()), message_id: Mutex::new(String::new()),
            related: Default::default(), done: Done { r: Mutex::new(None), cv: Condvar::new() }, connected: (Mutex::new(false), Condvar::new()), stream: Cancel::new(),
            accepted: AtomicBool::new(false), user_seen: AtomicBool::new(false), busy_seen: AtomicBool::new(false), stopping: AtomicBool::new(false),
            refused: AtomicBool::new(false), idle_early: AtomicBool::new(false), connects: AtomicBool::new(false),
            error: Mutex::new(None), retry: Mutex::new(None), last_event: Mutex::new(Instant::now()), idle_confirms: AtomicUsize::new(0),
            open: Default::default(), resolved: Default::default(), said: Default::default(), phase: Mutex::new(KiroPhase::Starting), context: Mutex::new(None),
        });
        let mut pump: Option<std::thread::JoinHandle<()>> = None;
        let r = match self.turn(prompt, ct, resume, &o, &turn, &mut pump) {
            Ok(r) => r,
            Err(OcErr::Cancelled) => turn.stopped(),
            Err(OcErr::Oc(_, m)) => if ct.is_cancelled() { turn.stopped() } else { KiroResult::new(KiroState::Failed, explain(&m)) },
            Err(OcErr::Net(m)) => if ct.is_cancelled() { turn.stopped() } else { KiroResult::new(KiroState::Failed, format!("Couldn’t reach OpenCode: {m}")) },
        };
        // finally
        turn.done.set(turn.stopped());
        turn.stream.cancel();
        turn.cancel_open();
        let sid = turn.sid();
        if !sid.is_empty() {
            let mut t = self.turns.lock().unwrap();
            if t.get(&sid).is_some_and(|x| Arc::ptr_eq(x, &turn)) { t.remove(&sid); }
        }
        if let Some(p) = pump { let _ = p.join(); }
        if self.busy.fetch_sub(1, Ordering::SeqCst) == 1 && self.live.lock().unwrap().is_some() {
            self.schedule_idle(Duration::from_secs(60 * (self.options)().idle_minutes.max(1) as u64));
        }
        r
    }

    fn turn(self: &Arc<Self>, prompt: &str, ct: &Cancel, resume: Option<&str>, o: &AgentOptions, turn: &Arc<Turn>, pump: &mut Option<std::thread::JoinHandle<()>>)
        -> Result<KiroResult, OcErr> {
        let folder = turn.folder.as_str();
        if let Some(p) = &turn.progress { p(KiroPhase::Starting); }
        self.start(ct)?;
        let inv = self.inventory_of(folder, ct)?;
        let model = match pick_model(&inv, o.model.as_deref()) { Ok(m) => m, Err(e) => return Ok(KiroResult::new(KiroState::Failed, e)) };
        let agent = o.agent.clone();
        let agents = match self.get("/agent", Some(folder), Some(ct), None)? { a @ Json::Arr(_) => a, _ => Json::Arr(vec![]) };
        if let Some(a) = &agent {
            if !agents_of(&agents).any(|x| s(Some(x), "name") == Some(a)) {
                return Ok(KiroResult::new(KiroState::Failed, format!("OpenCode has no agent named “{a}” for this folder. Pick another in Settings → OpenCode.")));
            }
        }
        let default_agent = match &agent { Some(a) => a.clone(), None => s(Some(&self.get("/config", Some(folder), Some(ct), None)?), "default_agent").unwrap_or("build").to_owned() };
        let rules = rules(o, &agents, &default_agent);

        if let Some(r) = resume.filter(|r| !r.is_empty()) {
            if self.stuck.lock().unwrap().contains(r) {
                // The last stop wasn't confirmed: a busy session isn't sent more.
                let status = self.get("/session/status", Some(folder), Some(ct), None)?;
                if matches!(s(status.get(r), "type"), Some("busy" | "retry")) {
                    return Ok(KiroResult::new(KiroState::Failed, "OpenCode is still stopping the last run of this conversation. Try again in a moment."));
                }
                self.stuck.lock().unwrap().remove(r);
            }
            match self.get(&format!("/session/{}", escape_data(r)), Some(folder), Some(ct), None) {
                Err(OcErr::Oc(Some(404), _)) => {
                    // No quiet new conversation in its place: the transcript stays, the user decides.
                    return Ok(KiroResult::new(KiroState::Failed, "OpenCode no longer has this conversation, so it can’t carry on from here. The chat above is kept. Start a new task to go on."));
                }
                Err(e) => return Err(e),
                Ok(_) => {}
            }
            // Resuming skips session create, so the rules are set again.
            self.send("PATCH", &format!("/session/{}", escape_data(r)), Some(folder), Some(Json::obj(vec![("permission", rules)])), Some(ct), None)?;
            *turn.sid.lock().unwrap() = r.to_owned();
        } else {
            let created = self.send("POST", "/session", Some(folder), Some(Json::obj(vec![("title", st(&title(prompt))), ("permission", rules)])), Some(ct), None)?;
            *turn.sid.lock().unwrap() = s(Some(&created), "id").ok_or_else(|| OcErr::Oc(None, "OpenCode didn’t start a session.".into()))?.to_owned();
        }
        let sid = turn.sid();
        turn.related.lock().unwrap().insert(sid.clone());
        self.turns.lock().unwrap().insert(sid.clone(), turn.clone());
        if let Some(e) = &turn.events { e(KiroEvent { session_id: Some(sid.clone()), ..Default::default() }); }

        // Events first, then the prompt: nothing it does is missed.
        let (me, t2) = (self.clone(), turn.clone());
        *pump = Some(std::thread::Builder::new().name("opencode-events".into()).spawn(move || me.pump(&t2)).expect("a thread for the events"));
        {
            let (m, cv) = &turn.connected;
            let until = Instant::now() + self.t.connect;
            let mut g = m.lock().unwrap();
            while !*g && !ct.is_cancelled() && Instant::now() < until {
                g = cv.wait_timeout(g, Duration::from_millis(50)).unwrap().0;
            }
            if !*g {
                if ct.is_cancelled() { return Err(OcErr::Cancelled); }
                return Ok(KiroResult::new(KiroState::Failed, "OpenCode’s event stream didn’t connect. Try again."));
            }
        }
        { let (me, t2) = (self.clone(), turn.clone()); std::thread::spawn(move || me.recover_asks(&t2)); }

        *turn.message_id.lock().unwrap() = new_message_id();
        let mut body = vec![("messageID", st(&turn.mid())), ("parts", Json::Arr(vec![Json::obj(vec![("type", st("text")), ("text", st(prompt.trim()))])]))];
        if let Some(m) = &model {
            body.push(("model", Json::obj(vec![("providerID", st(&m.provider)), ("modelID", st(&m.model))])));
            // Only a variant this model has; never one made up.
            if let Some(v) = o.effort.as_ref().filter(|v| m.variants.contains(v)) { body.push(("variant", st(v))); }
        }
        if let Some(a) = &agent { body.push(("agent", st(a))); }
        let (me, t2) = (Arc::downgrade(self), turn.clone());
        let _reg = ct.on_cancel(move || { if let Some(h) = me.upgrade() { std::thread::spawn(move || h.stop(&t2)); } });
        self.submit(turn, Json::obj(body))?;
        Ok(turn.done.wait())
    }

    /// Sends the prompt once. When the answer is lost (a timeout, a dropped
    /// connection), the message is looked up by its id instead of sent again.
    fn submit(self: &Arc<Self>, turn: &Arc<Turn>, body: Json) -> Result<(), OcErr> {
        let sid = turn.sid();
        match self.send("POST", &format!("/session/{}/prompt_async", escape_data(&sid)), Some(&turn.folder), Some(body), Some(&turn.token), Some(self.t.send)) {
            Ok(_) => {
                turn.accepted.store(true, Ordering::SeqCst);
                // It may have gone idle before the answer to the prompt came back.
                if Turn::flag(&turn.idle_early) { self.reconcile_later(turn, "idle while sending"); }
                return Ok(());
            }
            Err(OcErr::Oc(Some(code), m)) if (400..500).contains(&code) => {
                // Refused outright: it wasn't taken.
                turn.done.set(KiroResult::new(KiroState::Failed, explain(&m)));
                return Ok(());
            }
            Err(OcErr::Cancelled) => return Err(OcErr::Cancelled),
            Err(_) if turn.token.is_cancelled() => return Err(OcErr::Cancelled),
            Err(OcErr::Oc(_, m) | OcErr::Net(m)) => log(&format!("prompt for {sid} unconfirmed - {m}")),
        }
        if self.message_exists(turn) {
            turn.accepted.store(true, Ordering::SeqCst);
            turn.user_seen.store(true, Ordering::SeqCst);
            self.reconcile_later(turn, "prompt found");
            return Ok(());
        }
        turn.done.set(KiroResult::new(KiroState::Failed,
            "Hover couldn’t confirm OpenCode got the task, and it isn’t in the conversation, so it wasn’t sent again. Send it again when you’re ready."));
        Ok(())
    }

    fn message_exists(&self, turn: &Turn) -> bool {
        let (sid, mid) = (turn.sid(), turn.mid());
        match self.get(&format!("/session/{}/message/{}", escape_data(&sid), escape_data(&mid)), Some(&turn.folder), None, Some(Duration::from_secs(5))) {
            Ok(m) => s(m.get("info"), "id") == Some(mid.as_str()),
            Err(_) => false,
        }
    }

    /// Stop: OpenCode is asked to abort, questions still open are withdrawn, and the
    /// turn ends as stopped once it goes idle, or after a grace period. One that is
    /// still busy then is marked, and not sent more until it has stopped.
    fn stop(self: &Arc<Self>, turn: &Arc<Turn>) {
        let sid = turn.sid();
        if Turn::flag(&turn.stopping) || sid.is_empty() { return; }
        turn.stopping.store(true, Ordering::SeqCst);
        turn.cancel_open();
        if let Err(e) = self.send("POST", &format!("/session/{}/abort", escape_data(&sid)), Some(&turn.folder), None, None, Some(Duration::from_secs(5))) {
            log(&format!("abort {sid} - {}", err_text(&e)));
        }
        if !turn.done.wait_for(self.t.stop_grace) {
            self.stuck.lock().unwrap().insert(sid.clone());
            log(&format!("{sid} didn't stop within {:.0}s", self.t.stop_grace.as_secs_f64()));
            // Only this run uses the server: end it, and with it the run.
            if self.busy.load(Ordering::SeqCst) == 1 { self.end(None, "didn't stop when asked", "OpenCode stopped."); }
            turn.done.set(turn.stopped());
        }
    }

    // MARK: Models, variants, agents

    /// The folder's providers and models, read once per server and folder, and passed on
    /// as the tool's offers: every model with its own variants, and the agents.
    fn inventory_of(&self, folder: &str, ct: &Cancel) -> Result<Json, OcErr> {
        if let Some(k) = self.inventory.lock().unwrap().get(folder) { return Ok(k.clone()); }
        let inv = match self.get("/config/providers", Some(folder), Some(ct), None)? { v @ Json::Obj(_) => v, _ => Json::Obj(vec![]) };
        let agents = match self.get("/agent", Some(folder), Some(ct), None)? { v @ Json::Arr(_) => v, _ => Json::Arr(vec![]) };
        self.inventory.lock().unwrap().insert(folder.into(), inv.clone());
        let offered = offers(&inv, &agents);
        for f in self.seen.lock().unwrap().iter() { f(AgentTool::OpenCode, &offered); }
        Ok(inv)
    }

    // MARK: The event stream

    fn pump(self: &Arc<Self>, turn: &Arc<Turn>) {
        let mut backoff = Duration::from_millis(250);
        let watch = { let (me, t2) = (self.clone(), turn.clone()); std::thread::spawn(move || me.watch(&t2)) };
        while !turn.done.is_set() && !turn.stream.is_cancelled() {
            let client = self.live.lock().unwrap().as_ref().map(|l| l.client.clone());
            let got = match client {
                None => Err(OcErr::Oc(None, "OpenCode stopped.".into())),
                Some(c) => self.read_stream(&c, turn),
            };
            match got {
                Ok(()) => backoff = Duration::from_millis(250),
                Err(_) if turn.stream.is_cancelled() => break,
                Err(e) => {
                    if self.live.lock().unwrap().is_none() {
                        turn.done.set(if Turn::flag(&turn.stopping) { turn.stopped() } else { KiroResult::new(KiroState::Failed, "OpenCode stopped unexpectedly.") });
                        break;
                    }
                    log(&format!("event stream for {} dropped - {}", turn.sid(), err_text(&e)));
                }
            }
            if turn.done.is_set() || turn.stream.is_cancelled() { break; }
            // Reconnecting: the bot keeps its pose, and the state is read back once in.
            if turn.done.wait_for(backoff) || turn.stream.is_cancelled() { break; }
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
        let _ = watch.join();
    }

    fn read_stream(self: &Arc<Self>, c: &Client, turn: &Arc<Turn>) -> Result<(), OcErr> {
        let mut res = c.open("GET", &url("/event", Some(&turn.folder)), None, None, Some(&turn.stream)).map_err(|e| http_err(e, "GET", "/event"))?;
        if !(200..300).contains(&res.status) { return Err(OcErr::Oc(Some(res.status), format!("OpenCode’s event stream answered {}.", res.status))); }
        let mut data = String::new();
        loop {
            let line = match res.line() { Ok(Some(l)) => l, Ok(None) => break, Err(e) => return Err(http_err(e, "GET", "/event")) };
            if line.is_empty() {
                if !data.is_empty() { self.handle(turn, &data); data.clear(); }
                continue;
            }
            if let Some(d) = line.strip_prefix("data:") {
                if !data.is_empty() { data.push('\n'); }
                data.push_str(d.trim_start());
                // A runaway event can't grow without end.
                if data.len() > 8 * 1024 * 1024 { data.clear(); }
            }
        }
        if !data.is_empty() { self.handle(turn, &data); }
        Ok(())
    }

    fn handle(self: &Arc<Self>, turn: &Arc<Turn>, data: &str) {
        let Ok(ev @ Json::Obj(_)) = json::parse(data) else { return };
        let Some(kind) = s(Some(&ev), "type").map(str::to_owned) else { return };
        *turn.last_event.lock().unwrap() = Instant::now();
        if kind == "server.connected" {
            let again = turn.connects.swap(true, Ordering::SeqCst);
            let (m, cv) = &turn.connected;
            *m.lock().unwrap() = true;
            cv.notify_all();
            // Events missed while away aren't sent again: read the state back.
            if again { self.reconcile_later(turn, "reconnected"); }
            return;
        }
        let p = match ev.get("properties") { Some(p @ Json::Obj(_)) => p.clone(), _ => Json::Obj(vec![]) };
        self.apply(turn, &kind, &p);
    }

    /// One event, as it touches this turn. Parts and deltas are merged by their ids, so a
    /// delta and the whole part after it never say the same words twice.
    fn apply(self: &Arc<Self>, turn: &Arc<Turn>, kind: &str, p: &Json) {
        let sid = s(Some(p), "sessionID").map(str::to_owned);
        let related = |x: &Option<String>| x.as_ref().is_some_and(|x| turn.related.lock().unwrap().contains(x));
        match kind {
            "session.created" | "session.updated" => {
                let info = p.get("info");
                if let (Some(parent), Some(child)) = (s(info, "parentID"), s(info, "id")) {
                    let mut r = turn.related.lock().unwrap();
                    if r.contains(parent) { r.insert(child.to_owned()); }
                }
                return;
            }
            "permission.asked" => { if related(&sid) { self.permission_later(turn, p.clone()); } return; }
            "question.asked" => { if related(&sid) { self.question_later(turn, p.clone()); } return; }
            "permission.replied" | "question.replied" | "question.rejected" => {
                // Answered elsewhere (another client) or by Hover: its card goes.
                if let Some(rid) = s(Some(p), "requestID") {
                    turn.resolved.lock().unwrap().insert(rid.to_owned());
                    if let Some(c) = turn.open.lock().unwrap().remove(rid) { c.cancel(); }
                }
                return;
            }
            _ => {}
        }
        if sid.as_deref() != Some(turn.sid().as_str()) { return; }
        match kind {
            "message.updated" => {
                let Some(msg) = p.get("info").filter(|m| matches!(m, Json::Obj(_))) else { return };
                let Some(mid) = s(Some(msg), "id") else { return };
                let role = s(Some(msg), "role").unwrap_or("");
                turn.said.lock().unwrap().roles.insert(mid.into(), role.into());
                if role == "user" && mid == turn.mid() { turn.user_seen.store(true, Ordering::SeqCst); }
                if role == "assistant" && mine(turn, mid, s(Some(msg), "parentID")) {
                    { let mut g = turn.said.lock().unwrap(); if !g.messages.iter().any(|m| m == mid) { g.messages.push(mid.into()); } }
                    if let Some(err @ Json::Obj(_)) = msg.get("error") {
                        if let Some(why) = error_text(Some(err)) { if s(Some(err), "name") != Some("MessageAbortedError") { *turn.error.lock().unwrap() = Some(why); } }
                    }
                    self.usage(turn, msg);
                }
            }
            "message.part.updated" => { if let Some(part @ Json::Obj(_)) = p.get("part") { self.part(turn, part); } }
            "message.part.delta" => {
                if s(Some(p), "field") != Some("text") { return; }
                let (Some(pid), Some(delta)) = (s(Some(p), "partID"), s(Some(p), "delta").filter(|d| !d.is_empty())) else { return };
                {
                    let mut g = turn.said.lock().unwrap();
                    let Some(t) = g.text.get_mut(pid) else { return };
                    t.push_str(delta);
                }
                turn.set_phase(KiroPhase::Writing);
            }
            "session.status" => {
                let status = p.get("status");
                match s(status, "type") {
                    Some("busy") => { if !turn.mid().is_empty() { turn.busy_seen.store(true, Ordering::SeqCst); } turn.idle_confirms.store(0, Ordering::SeqCst); }
                    Some("retry") => { if !turn.mid().is_empty() { turn.busy_seen.store(true, Ordering::SeqCst); } *turn.retry.lock().unwrap() = s(status, "message").map(str::to_owned); }
                    Some("idle") => self.idle_seen(turn),
                    _ => {}
                }
            }
            "session.idle" => self.idle_seen(turn),
            "session.error" => {
                let error = p.get("error");
                if s(error, "name") == Some("MessageAbortedError") {
                    if Turn::flag(&turn.stopping) { turn.done.set(turn.stopped()); }
                    return;
                }
                if !Turn::flag(&turn.accepted) { return; }
                let e = error_text(error).or_else(|| turn.retry.lock().unwrap().clone()).unwrap_or_else(|| "OpenCode reported an error.".into());
                *turn.error.lock().unwrap() = Some(e.clone());
                turn.done.set(if Turn::flag(&turn.stopping) { turn.stopped() } else { KiroResult::new(KiroState::Failed, explain(&e)) });
            }
            _ => {}
        }
    }

    /// Only an idle after the server took this prompt (it saw the message, or went busy
    /// for it) ends the turn. Anything else is looked into instead.
    fn idle_seen(self: &Arc<Self>, turn: &Arc<Turn>) {
        let seen = Turn::flag(&turn.user_seen) || Turn::flag(&turn.busy_seen);
        if !Turn::flag(&turn.accepted) { if !turn.mid().is_empty() && seen { turn.idle_early.store(true, Ordering::SeqCst); } return; }
        if Turn::flag(&turn.stopping) { turn.done.set(turn.stopped()); return; }
        if seen { finish(turn); return; }
        self.reconcile_later(turn, "idle before the prompt was seen");
    }

    fn part(self: &Arc<Self>, turn: &Arc<Turn>, part: &Json) {
        let (Some(id), Some(mid)) = (s(Some(part), "id"), s(Some(part), "messageID")) else { return };
        let role = turn.said.lock().unwrap().roles.get(mid).cloned();
        if role.as_deref() == Some("user") || mid == turn.mid() || !mine(turn, mid, None) { return; }
        { let mut g = turn.said.lock().unwrap(); if !g.messages.iter().any(|m| m == mid) { g.messages.push(mid.into()); } }
        match s(Some(part), "type") {
            Some("text") => {
                if is_true(Some(part), "synthetic") { return; }
                {
                    let mut g = turn.said.lock().unwrap();
                    let order = g.part_order.entry(mid.into()).or_default();
                    if !order.iter().any(|x| x == id) { order.push(id.into()); }
                    // The whole part replaces what the deltas built: never twice.
                    g.text.insert(id.into(), s(Some(part), "text").unwrap_or("").into());
                }
                turn.set_phase(KiroPhase::Writing);
            }
            Some("reasoning") => turn.set_phase(KiroPhase::Thinking),
            Some("tool") => self.tool_part(turn, part),
            Some("step-finish") => { if let Some(t @ Json::Obj(_)) = part.get("tokens") { self.tokens(turn, t, None); } }
            _ => {}
        }
    }

    fn tool_part(&self, turn: &Turn, part: &Json) {
        let tool = s(Some(part), "tool").unwrap_or("tool");
        // The question itself shows as the question's card.
        if tool == "question" { return; }
        let Some(call) = s(Some(part), "callID") else { return };
        let state = part.get("state").filter(|x| matches!(x, Json::Obj(_)));
        let status = match s(state, "status") { Some("completed") => "completed", Some("error") => "failed", _ => "in_progress" };
        let input = state.and_then(|x| x.get("input")).filter(|x| matches!(x, Json::Obj(_)));
        let kind = kind_of(tool);
        let target = ["filePath", "path", "command", "pattern", "url", "query"].iter().find_map(|k| s(input, k)).map(str::to_owned);
        let title = s(state, "title").filter(|t| !t.is_empty()).map_or_else(|| cap(tool), str::to_owned);
        let next = {
            let mut g = turn.said.lock().unwrap();
            let known = match g.steps.get(call) {
                Some(k) => k.clone(),
                None => { g.began.insert(call.into(), Instant::now()); KiroStep::new(call, kind, &title, target.clone(), status) }
            };
            let mut next = KiroStep { status: status.into(), title: title.clone(), target: known.target.clone().or(target), ..known.clone() };
            if kind == "edit" {
                if let Some((a, r, d)) = input.and_then(change) { next.added = a; next.removed = r; next.diff = Some(d); }
            }
            if kind == "execute" && status != "in_progress" {
                if let Some(out) = s(state, "output").or_else(|| s(state, "error")) {
                    next.output = crate::stream::output_of(&Json::obj(vec![("rawOutput", st(out))])).0;
                }
                if let Some(exit) = num(state.and_then(|x| x.get("metadata")), "exit") { next.exit = Some(exit as i32); }
            }
            if status != "in_progress" && known.ms.is_none() {
                if let Some(t0) = g.began.get(call) { next.ms = Some(t0.elapsed().as_secs_f64() * 1000.0); }
            }
            if g.steps.contains_key(call) && next == known { return; }
            g.steps.insert(call.into(), next.clone());
            next
        };
        if status == "in_progress" { if let Some(p) = tool_phase(Some(kind), Some(&title)) { turn.set_phase(p); } }
        if let Some(e) = &turn.events { e(KiroEvent { step: Some(next), ..Default::default() }); }
    }

    fn usage(&self, turn: &Turn, msg: &Json) {
        if let (Some(t @ Json::Obj(_)), Some(pid), Some(mid)) = (msg.get("tokens"), s(Some(msg), "providerID"), s(Some(msg), "modelID")) {
            self.tokens(turn, t, Some(format!("{pid}/{mid}")));
        }
    }

    /// How full the context is: the tokens of the last request over the model's window.
    fn tokens(&self, turn: &Turn, tokens: &Json, model: Option<String>) {
        let sid = turn.sid();
        let model = match model {
            Some(m) => { self.last_model.lock().unwrap().insert(sid.clone(), m.clone()); m }
            None => match self.last_model.lock().unwrap().get(&sid) { Some(m) => m.clone(), None => return },
        };
        let Some(inv) = self.inventory.lock().unwrap().get(&turn.folder).cloned() else { return };
        let Ok(Some(m)) = pick_model(&inv, Some(&model)) else { return };
        let Some(limit) = m.limit.filter(|l| *l > 0.0) else { return };
        // As C#'s double? sums: any part missing leaves no total.
        let cache = tokens.get("cache");
        let used = match (num(Some(tokens), "input"), num(Some(tokens), "output"), num(cache, "read"), num(cache, "write")) {
            (Some(a), Some(b), Some(c), Some(d)) => a + b + c + d,
            _ => return,
        };
        if used <= 0.0 { return; }
        let pct = (used * 100.0 / limit).clamp(0.0, 100.0);
        {
            let mut c = turn.context.lock().unwrap();
            if c.is_some_and(|old| (old - pct).abs() < 0.5) { return; }
            *c = Some(pct);
        }
        if let Some(e) = &turn.events { e(KiroEvent { context: Some(pct), ..Default::default() }); }
    }

    // MARK: Looking into the state

    /// No event for a while: the state and messages are read from the server.
    fn watch(self: &Arc<Self>, turn: &Arc<Turn>) {
        loop {
            if turn.done.wait_for(Duration::from_secs(3)) || turn.stream.is_cancelled() { return; }
            if Turn::flag(&turn.accepted) && turn.last_event.lock().unwrap().elapsed() > self.t.quiet { self.reconcile(turn, "quiet"); }
        }
    }

    fn reconcile_later(self: &Arc<Self>, turn: &Arc<Turn>, why: &'static str) {
        let (me, t2) = (self.clone(), turn.clone());
        std::thread::spawn(move || me.reconcile(&t2, why));
    }

    /// What the server says now: busy carries on; idle with this prompt in the
    /// conversation ends the turn with the messages read back; a prompt that never
    /// arrived, after a few looks, fails rather than hang.
    fn reconcile(self: &Arc<Self>, turn: &Arc<Turn>, why: &str) {
        let Ok(_g) = self.reconciling.try_lock() else { return };
        if turn.done.is_set() || !Turn::flag(&turn.accepted) { return; }
        let sid = turn.sid();
        log(&format!("reading {sid} back ({why})"));
        let r = (|| -> Result<(), OcErr> {
            let status = self.get("/session/status", Some(&turn.folder), Some(&turn.stream), Some(Duration::from_secs(5)))?;
            let kind = s(status.get(&sid), "type").unwrap_or("idle");
            if matches!(kind, "busy" | "retry") {
                turn.busy_seen.store(true, Ordering::SeqCst);
                turn.idle_confirms.store(0, Ordering::SeqCst);
                *turn.last_event.lock().unwrap() = Instant::now();
                return Ok(());
            }
            if !Turn::flag(&turn.user_seen) && self.message_exists(turn) { turn.user_seen.store(true, Ordering::SeqCst); }
            self.read_back(turn)?;
            self.recover_asks(turn);
            let user = Turn::flag(&turn.user_seen);
            if user && (Turn::flag(&turn.busy_seen) || turn.idle_confirms.fetch_add(1, Ordering::SeqCst) + 1 >= 2) { finish(turn); return Ok(()); }
            if !user && turn.idle_confirms.fetch_add(1, Ordering::SeqCst) + 1 >= 5 {
                turn.done.set(KiroResult::new(KiroState::Failed, "OpenCode took the task but never started it. Send it again when you’re ready."));
            }
            Ok(())
        })();
        if let Err(e) = r { log(&format!("couldn't read {sid} back - {}", err_text(&e))); }
    }

    /// This turn's messages from the server, merged by id with what the events built.
    fn read_back(self: &Arc<Self>, turn: &Arc<Turn>) -> Result<(), OcErr> {
        let sid = turn.sid();
        let Json::Arr(messages) = self.get(&format!("/session/{}/message", escape_data(&sid)), Some(&turn.folder), Some(&turn.stream), Some(Duration::from_secs(10)))? else { return Ok(()) };
        for m in messages.iter().filter(|m| matches!(m, Json::Obj(_))) {
            let Some(info @ Json::Obj(_)) = m.get("info") else { continue };
            self.apply(turn, "message.updated", &Json::obj(vec![("sessionID", st(&sid)), ("info", info.clone())]));
            if let Some(Json::Arr(parts)) = m.get("parts") {
                for part in parts.iter().filter(|p| matches!(p, Json::Obj(_))) { self.part(turn, part); }
            }
        }
        Ok(())
    }

    /// Requests left waiting (from before a reconnect, or from a run Hover wasn't
    /// watching) are asked about now; the ones already open or answered aren't.
    fn recover_asks(self: &Arc<Self>, turn: &Arc<Turn>) {
        let r = (|| -> Result<(), OcErr> {
            let related = |x: &Json| s(Some(x), "sessionID").is_some_and(|s| turn.related.lock().unwrap().contains(s));
            if let Json::Arr(list) = self.get("/permission", Some(&turn.folder), Some(&turn.stream), Some(Duration::from_secs(5)))? {
                for p in list.into_iter().filter(|p| matches!(p, Json::Obj(_)) && related(p)) { self.permission_later(turn, p); }
            }
            if let Json::Arr(list) = self.get("/question", Some(&turn.folder), Some(&turn.stream), Some(Duration::from_secs(5)))? {
                for q in list.into_iter().filter(|q| matches!(q, Json::Obj(_)) && related(q)) { self.question_later(turn, q); }
            }
            Ok(())
        })();
        if let Err(e) = r { log(&format!("couldn't read waiting requests - {}", err_text(&e))); }
    }

    // MARK: Approvals and questions

    fn permission_later(self: &Arc<Self>, turn: &Arc<Turn>, req: Json) {
        let (me, t2) = (self.clone(), turn.clone());
        std::thread::Builder::new().name("opencode-permission".into()).spawn(move || me.permission(&t2, &req)).expect("a thread for the approval");
    }

    fn question_later(self: &Arc<Self>, turn: &Arc<Turn>, req: Json) {
        let (me, t2) = (self.clone(), turn.clone());
        std::thread::Builder::new().name("opencode-question".into()).spawn(move || me.question(&t2, &req)).expect("a thread for the question");
    }

    /// Holds a request open while it is asked about: its own stop, which the turn's stop
    /// and an answer from elsewhere set. None when it is open or answered already.
    fn open_request(&self, turn: &Turn, id: &str) -> Option<(Cancel, crate::cancel::Registration)> {
        if turn.resolved.lock().unwrap().contains(id) { return None; }
        let cts = Cancel::new();
        {
            let mut open = turn.open.lock().unwrap();
            if open.contains_key(id) { return None; }
            open.insert(id.into(), cts.clone());
        }
        let c2 = cts.clone();
        let reg = turn.token.on_cancel(move || c2.cancel());
        Some((cts, reg))
    }

    /// Read only turns every request down (its rules should leave none). Otherwise what
    /// OpenCode asks is asked of the user: its rules already let through what the access
    /// allows, and a request it sends on purpose is never answered yes for the user.
    /// Trust is Hover's, for this session, and each yes is OpenCode's "once": its
    /// "always" can outlast the session.
    fn permission(self: &Arc<Self>, turn: &Arc<Turn>, req: &Json) {
        let Some(id) = s(Some(req), "id").map(str::to_owned) else { return };
        let Some((cts, _reg)) = self.open_request(turn, &id) else { return };
        let sid = turn.sid();
        let ask = describe(req, &turn.folder);
        let key = ask::key(&ask);
        let mut message: Option<&str> = None;
        let trusted = self.trusted.lock().unwrap().get(&sid).is_some_and(|t| t.contains("*") || t.contains(&key));
        let asking = self.asking.lock().unwrap().clone();
        let reply = if turn.options.read_only {
            message = Some("Hover has OpenCode set to read only.");
            turn.refused.store(true, Ordering::SeqCst);
            "reject"
        } else if trusted {
            "once"
        } else if let Some(asking) = asking {
            let (tx, rx) = mpsc::channel::<Option<AskAnswer>>();
            let t2 = tx.clone();
            let _stop = cts.on_cancel(move || { let _ = t2.send(None); });
            asking(&sid, ask, &cts, Box::new(move |a| { let _ = tx.send(Some(a)); }));
            match rx.recv() {
                Ok(Some(a)) if !cts.is_cancelled() => match a {
                    AskAnswer::Allow => "once",
                    AskAnswer::Trust => { self.trusted.lock().unwrap().entry(sid.clone()).or_default().insert(key); "once" }
                    AskAnswer::TrustAll => { self.trusted.lock().unwrap().entry(sid.clone()).or_default().insert("*".into()); "once" }
                    AskAnswer::Deny => "reject",
                },
                _ => { message = Some("Stopped."); "reject" }
            }
        } else {
            "reject"
        };
        // Answered elsewhere meanwhile: nothing to send.
        if !turn.resolved.lock().unwrap().insert(id.clone()) { turn.open.lock().unwrap().remove(&id); return; }
        let mut body = vec![("reply", st(reply))];
        if let Some(m) = message { body.push(("message", st(m))); }
        if let Err(e) = self.send("POST", &format!("/permission/{}/reply", escape_data(&id)), Some(&turn.folder), Some(Json::obj(body)), None, Some(Duration::from_secs(10))) {
            log(&format!("permission {id} - {}", err_text(&e)));
        }
        turn.open.lock().unwrap().remove(&id);
    }

    /// A question goes to the user as it is; the answer is theirs, never made up. A
    /// skipped or withdrawn one is rejected, which OpenCode tells the agent.
    fn question(self: &Arc<Self>, turn: &Arc<Turn>, req: &Json) {
        let Some(id) = s(Some(req), "id").map(str::to_owned) else { return };
        let Some((cts, _reg)) = self.open_request(turn, &id) else { return };
        let questions = questions_of(req);
        let mut answers: Answers = None;
        let questioning = self.questioning.lock().unwrap().clone();
        if let (Some(first), Some(q)) = (questions.first(), questioning) {
            let ask = AgentAsk { id: id.clone(), kind: "question".into(), title: first.header.clone(), command: None, path: None, preview: None,
                added: 0, removed: 0, reason: first.question.clone(), danger: false, questions: Some(questions.clone()) };
            let (tx, rx) = mpsc::channel::<Option<Answers>>();
            let t2 = tx.clone();
            let _stop = cts.on_cancel(move || { let _ = t2.send(None); });
            q(&turn.sid(), ask, &cts, Box::new(move |a| { let _ = tx.send(Some(a)); }));
            if let Ok(Some(a)) = rx.recv() { if !cts.is_cancelled() { answers = a; } }
        }
        if !turn.resolved.lock().unwrap().insert(id.clone()) { turn.open.lock().unwrap().remove(&id); return; }
        let sent = match answers.filter(|a| !a.is_empty()) {
            Some(a) => self.send("POST", &format!("/question/{}/reply", escape_data(&id)), Some(&turn.folder),
                Some(Json::obj(vec![("answers", Json::Arr(a.iter().map(|x| Json::Arr(x.iter().map(|l| st(l)).collect())).collect()))])), None, Some(Duration::from_secs(10))),
            None => self.send("POST", &format!("/question/{}/reject", escape_data(&id)), Some(&turn.folder), None, None, Some(Duration::from_secs(10))),
        };
        if let Err(e) = sent { log(&format!("question {id} - {}", err_text(&e))); }
        turn.open.lock().unwrap().remove(&id);
    }

    // MARK: The process

    fn start(self: &Arc<Self>, ct: &Cancel) -> Result<(), OcErr> {
        let _g = self.gate.lock().unwrap();
        if ct.is_cancelled() { return Err(OcErr::Cancelled); }
        if self.live.lock().unwrap().is_some() { return Ok(()); }
        let link = (self.connect)(ct, &self.t)?.ok_or_else(|| OcErr::Oc(None, format!("OpenCode isn’t installed. {}", agents::install_hint(AgentTool::OpenCode))))?;
        let client = Client::new(&link.url, "opencode", &link.password).ok_or_else(|| OcErr::Oc(None, format!("OpenCode listened on {}, which Hover can’t reach.", link.url)))?;
        let gen = self.gens.fetch_add(1, Ordering::SeqCst) + 1;
        let OpenCodeLink { url: at, kill, errors, exited, .. } = link;
        *self.live.lock().unwrap() = Some(Arc::new(Live { gen, client, kill, errors }));
        self.inventory.lock().unwrap().clear();
        if let Some(rx) = exited {
            let me = Arc::downgrade(self);
            std::thread::spawn(move || { let _ = rx.recv(); if let Some(h) = me.upgrade() { h.gone(gen); } });
        }
        // Its health and version before any task: an API Hover wasn't checked against is
        // a clear error, not a strange failure later.
        let health = (|| -> Result<(), OcErr> {
            let h = self.get("/global/health", None, Some(ct), Some(Duration::from_secs(5)))?;
            let version = s(Some(&h), "version").unwrap_or("").to_owned();
            let v = agents::parse_version(version.split('-').next().unwrap_or(""));
            if !is_true(Some(&h), "healthy") || v.is_none() { return Err(OcErr::Oc(None, "OpenCode’s server didn’t say it was healthy.".into())); }
            if v.unwrap() < agents::parse_version(agents::OPENCODE_MIN_VERSION).unwrap() {
                return Err(OcErr::Oc(None, format!("OpenCode {version} is too old for Hover. {}", agents::install_hint(AgentTool::OpenCode))));
            }
            log(&format!("server {version} at {}", at.trim_start_matches("http://").trim_end_matches('/')));
            Ok(())
        })();
        if health.is_err() { self.end(None, "didn't start", "OpenCode stopped."); }
        health
    }

    /// The server exited on its own: the runs using it fail and say why.
    fn gone(&self, gen: u64) {
        let text = match self.live.lock().unwrap().as_ref() { Some(l) if l.gen == gen => strip_ansi(&(l.errors)()), _ => return };
        let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        let why = &lines[lines.len().saturating_sub(2)..];
        let failure = format!("OpenCode stopped unexpectedly.{}", if why.is_empty() { String::new() } else { format!(" {}", why.join("\n")) });
        self.end(Some(gen), &format!("exited - {}", why.join(" / ")), &failure);
    }

    fn end(&self, only: Option<u64>, why: &str, failure: &str) {
        let live = {
            let mut l = self.live.lock().unwrap();
            if only.is_some_and(|g| l.as_ref().is_none_or(|x| x.gen != g)) { return; }
            match l.take() { Some(x) => x, None => return }
        };
        self.idle.fetch_add(1, Ordering::SeqCst);
        log(why);
        self.inventory.lock().unwrap().clear();
        let turns: Vec<Arc<Turn>> = self.turns.lock().unwrap().values().cloned().collect();
        for t in turns {
            t.done.set(if Turn::flag(&t.stopping) { t.stopped() } else { KiroResult::new(KiroState::Failed, failure) });
            t.stream.cancel();
        }
        (live.kill)();
    }

    fn schedule_idle(self: &Arc<Self>, after: Duration) {
        let gen = self.idle.fetch_add(1, Ordering::SeqCst) + 1;
        let me: Weak<Host> = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            if let Some(h) = me.upgrade() {
                if h.idle.load(Ordering::SeqCst) == gen && h.busy.load(Ordering::SeqCst) == 0 { h.end(None, "idle", "OpenCode stopped."); }
            }
        });
    }

    // MARK: HTTP

    fn get(&self, path: &str, folder: Option<&str>, ct: Option<&Cancel>, timeout: Option<Duration>) -> Result<Json, OcErr> {
        self.send("GET", path, folder, None, ct, Some(timeout.unwrap_or(Duration::from_secs(30))))
    }

    /// One request; an answer that isn't JSON (or none) reads as null.
    fn send(&self, method: &str, path: &str, folder: Option<&str>, body: Option<Json>, ct: Option<&Cancel>, timeout: Option<Duration>) -> Result<Json, OcErr> {
        let client = self.live.lock().unwrap().as_ref().map(|l| l.client.clone()).ok_or_else(|| OcErr::Oc(None, "OpenCode stopped.".into()))?;
        let body = body.map(|b| b.compact());
        let (status, text) = client.call(method, &url(path, folder), body.as_deref(), Some(timeout.unwrap_or(Duration::from_secs(30))), ct)
            .map_err(|e| http_err(e, method, path))?;
        if !(200..300).contains(&status) {
            let n = json::parse(&text).ok();
            let message = s(n.as_ref().and_then(|n| n.get("data")), "message").or_else(|| s(n.as_ref(), "message"))
                .or_else(|| s(n.as_ref().and_then(|n| n.get("error")), "message")).map(str::to_owned);
            return Err(OcErr::Oc(Some(status), message.unwrap_or_else(|| format!("OpenCode answered {status} to {method} {path}."))));
        }
        if text.is_empty() { return Ok(Json::Null); }
        Ok(json::parse(&text).unwrap_or(Json::Null))
    }
}

fn http_err(e: HttpErr, method: &str, path: &str) -> OcErr {
    match e {
        HttpErr::Cancelled => OcErr::Cancelled,
        HttpErr::Timeout => OcErr::Oc(None, format!("OpenCode didn’t answer ({method} {path}).")),
        HttpErr::Io(m) => OcErr::Net(m),
    }
}

fn err_text(e: &OcErr) -> String { match e { OcErr::Oc(_, m) | OcErr::Net(m) => m.clone(), OcErr::Cancelled => "stopped".into() } }

fn url(path: &str, folder: Option<&str>) -> String {
    match folder { None => path.into(), Some(f) => format!("{path}{}directory={}", if path.contains('?') { "&" } else { "?" }, escape_data(f)) }
}

fn agents_of(agents: &Json) -> impl Iterator<Item = &Json> {
    match agents { Json::Arr(a) => a.iter(), _ => [].iter() }.filter(|a| matches!(a, Json::Obj(_)))
}

/// An assistant message answers this turn's prompt, or came after it.
fn mine(turn: &Turn, message_id: &str, parent_id: Option<&str>) -> bool {
    let mid = turn.mid();
    !mid.is_empty() && (parent_id == Some(mid.as_str()) || message_id > mid.as_str())
}

fn finish(turn: &Turn) {
    let mut said = turn.said();
    if let Some(err) = turn.error.lock().unwrap().clone() { turn.done.set(KiroResult::new(KiroState::Failed, explain(&err))); return; }
    let refused = Turn::flag(&turn.refused);
    if refused && said.is_empty() {
        turn.done.set(KiroResult::new(KiroState::Failed, "OpenCode wanted to change files or run a command, and it is set to read only (Settings → OpenCode)."));
        return;
    }
    // The model's own words can claim it did what read only refused.
    if refused { said.push_str("\n\n*Hover has OpenCode set to read only, so the changes or commands it tried were refused.*"); }
    turn.done.set(KiroResult::new(KiroState::Completed, if said.is_empty() { "Done. OpenCode didn’t leave a summary.".into() } else { said }));
}

fn explain(message: &str) -> String {
    let lower = message.to_lowercase();
    if ["api key", "unauthorized", "authentication", "not authenticated"].iter().any(|k| lower.contains(k)) {
        return format!("{message}\n\n{}", agents::sign_in_hint(AgentTool::OpenCode));
    }
    if units(message) > 600 { format!("{}…", head_units(message, 599)) } else { message.to_owned() }
}

fn error_text(error: Option<&Json>) -> Option<String> {
    let e = error?;
    s(e.get("data"), "message").or_else(|| s(Some(e), "message")).or_else(|| s(Some(e), "name")).map(str::to_owned)
}

fn title(prompt: &str) -> String {
    let line = crate::session::first_line(prompt);
    clip_to(if line.is_empty() { "Hover task" } else { line }, 60)
}

/// The picked model: its provider and id as OpenCode names them, its variants and its
/// context window.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick { pub provider: String, pub model: String, pub variants: Vec<String>, pub limit: Option<f64> }

/// The model the settings name, exactly as OpenCode names it ("provider/model", where
/// the model part may itself have slashes). No model: OpenCode's default. Err says why.
pub fn pick_model(inv: &Json, wanted: Option<&str>) -> Result<Option<Pick>, String> {
    let Some(wanted) = wanted else { return Ok(None) };
    if let (Some(slash), Some(Json::Arr(providers))) = (wanted.find('/').filter(|i| *i > 0), inv.get("providers")) {
        let (pid, mid) = (&wanted[..slash], &wanted[slash + 1..]);
        for p in providers {
            if s(Some(p), "id") == Some(pid) {
                if let Some(model @ Json::Obj(_)) = p.get("models").and_then(|m| m.get(mid)) {
                    return Ok(Some(Pick { provider: pid.into(), model: mid.into(), variants: variants(model), limit: num(model.get("limit"), "context") }));
                }
            }
        }
    }
    Err(format!("OpenCode doesn’t offer “{wanted}” any more. Pick another model in the model menu."))
}

fn variants(model: &Json) -> Vec<String> { match model.get("variants") { Some(Json::Obj(v)) => v.iter().map(|(k, _)| k.clone()).collect(), _ => vec![] } }

/// The tool's offers: every model with its own variants, and the agents a task can use.
pub fn offers(inv: &Json, agents: &Json) -> Vec<AcpOption> {
    let mut models = vec![];
    if let Some(Json::Arr(providers)) = inv.get("providers") {
        for p in providers.iter().filter(|p| matches!(p, Json::Obj(_))) {
            let (Some(pid), Some(Json::Obj(list))) = (s(Some(p), "id"), p.get("models")) else { continue };
            let pname = s(Some(p), "name").unwrap_or(pid);
            for (mid, model) in list {
                if !matches!(model, Json::Obj(_)) { continue; }
                models.push(AcpChoice { value: format!("{pid}/{mid}"), name: format!("{} · {pname}", s(Some(model), "name").unwrap_or(mid)), levels: Some(variants(model)) });
            }
        }
    }
    let modes = agents_of(agents)
        .filter(|a| matches!(s(Some(a), "mode"), Some("primary" | "all")) && !is_true(Some(a), "hidden") && s(Some(a), "name").is_some_and(|n| !n.is_empty()))
        .map(|a| { let n = s(Some(a), "name").unwrap(); AcpChoice::new(n, &cap(n)) }).collect();
    vec![
        AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: models },
        AcpOption { id: "agent".into(), category: Some("mode".into()), current: None, choices: modes },
    ]
}

/// The session's rules for the tool access picked. OpenCode applies the last rule that
/// matches, so the agent's own deny rules (the user's config and the agent's, Plan's
/// no-edit for one) go after Hover's and always win: Full never undoes a deny. Read
/// only is enforced by the server, not by trusting the agent's name.
pub fn rules(o: &AgentOptions, agents: &Json, agent: &str) -> Json {
    let mut r: Vec<(String, String, String)> = vec![];
    let mut add = |p: &str, pat: &str, a: &str| r.push((p.into(), pat.into(), a.into()));
    if o.read_only {
        // Everything but reading asks, and Hover turns every ask down in read only
        // (permission), so the server runs no edit, command, subagent, MCP or custom
        // tool, and nothing outside the folder. They are asked about rather than denied:
        // a deny hides the tool, and OpenCode's free models refused a request whose
        // tools didn't look like OpenCode's own.
        add("*", "*", "ask");
        for p in ["read", "glob", "grep", "list", "lsp", "codesearch", "webfetch", "websearch", "todoread", "todowrite", "skill", "question"] { add(p, "*", "allow"); }
        add("read", "*.env", "deny");
        add("read", "*.env.*", "deny");
        for p in ["edit", "bash", "task", "external_directory", "doom_loop"] { add(p, "*", "ask"); }
    } else if o.approval == AgentApproval::Autopilot {
        add("*", "*", "allow");
        add("external_directory", "*", "allow");
        // OpenCode's own safety stop for a tool called over and over stays.
        add("doom_loop", "*", "ask");
    } else {
        // T3's Supervised set, with edits in the folder let through for Ask first.
        add("*", "*", "ask");
        for p in ["read", "glob", "grep", "list", "lsp", "skill", "todoread", "todowrite", "question"] { add(p, "*", "allow"); }
        add("read", "*.env", "ask");
        add("read", "*.env.*", "ask");
        add("read", "*.env.example", "allow");
        add("edit", "*", if o.approval == AgentApproval::Risky { "allow" } else { "ask" });
        for p in ["bash", "webfetch", "websearch", "codesearch", "external_directory", "doom_loop", "task"] { add(p, "*", "ask"); }
    }
    if let Some(Json::Arr(own)) = agents_of(agents).find(|a| s(Some(a), "name") == Some(agent)).and_then(|a| a.get("permission")) {
        let list: Vec<&Json> = own.iter().filter(|x| matches!(x, Json::Obj(_))).collect();
        // Only a deny that is the agent's own last word: OpenCode's defaults deny the
        // question tool and allow it again further down, and that allow wins.
        for (i, x) in list.iter().enumerate() {
            let (Some("deny"), Some(p), Some(pat)) = (s(Some(x), "action"), s(Some(x), "permission"), s(Some(x), "pattern")) else { continue };
            let undone = list[i + 1..].iter().any(|y| s(Some(y), "action") != Some("deny") && matches(p, s(Some(y), "permission")) && matches(pat, s(Some(y), "pattern")));
            if !undone { add(p, pat, "deny"); }
        }
    }
    Json::Arr(r.into_iter().map(|(p, pat, a)| Json::obj(vec![("permission", st(&p)), ("pattern", st(&pat)), ("action", st(&a))])).collect())
}

/// OpenCode's wildcard: * is any run of characters (newlines too), the rest is literal.
pub fn matches(value: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern else { return false };
    let (v, p): (Vec<char>, Vec<char>) = (value.chars().collect(), pattern.chars().collect());
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while i < v.len() {
        if j < p.len() && p[j] != '*' && p[j] == v[i] { i += 1; j += 1; }
        else if j < p.len() && p[j] == '*' { star = Some(j); mark = i; j += 1; }
        else if let Some(st) = star { j = st + 1; mark += 1; i = mark; }
        else { return false; }
    }
    while j < p.len() && p[j] == '*' { j += 1; }
    j == p.len()
}

/// OpenCode's tools as ACP's kinds, which the office draws.
pub fn kind_of(tool: &str) -> &'static str {
    match tool {
        "read" => "read",
        "write" | "edit" | "multiedit" | "patch" | "apply_patch" => "edit",
        "bash" | "shell" => "execute",
        "glob" | "grep" | "list" | "codesearch" => "search",
        "webfetch" | "websearch" => "fetch",
        "todowrite" | "todoread" => "think",
        _ => "other",
    }
}

/// An edit's lines added and removed, from its old and new text.
fn change(input: &Json) -> Option<(i32, i32, String)> {
    let old = s(Some(input), "oldString");
    let new = s(Some(input), "newString").or_else(|| s(Some(input), "content"))?;
    crate::stream::diff_of(&Json::obj(vec![("content", Json::Arr(vec![Json::obj(vec![("type", st("diff")), ("oldText", Json::opt_str_of(old)), ("newText", st(new))])]))]))
}

fn questions_of(req: &Json) -> Vec<AgentQuestion> {
    let Some(Json::Arr(list)) = req.get("questions") else { return vec![] };
    list.iter().filter(|q| matches!(q, Json::Obj(_))).map(|q| AgentQuestion {
        header: s(Some(q), "header").unwrap_or("Question").into(),
        question: s(Some(q), "question").unwrap_or("").into(),
        options: match q.get("options") {
            Some(Json::Arr(o)) => o.iter().filter(|x| matches!(x, Json::Obj(_)))
                .map(|x| (s(Some(x), "label").unwrap_or("").to_owned(), s(Some(x), "description").unwrap_or("").to_owned())).filter(|x| !x.0.is_empty()).collect(),
            _ => vec![],
        },
        multiple: is_true(Some(q), "multiple"),
        custom: !is_false(Some(q), "custom"),
    }).collect()
}

/// OpenCode's permission request as the notch and the office show it.
pub fn describe(req: &Json, folder: &str) -> AgentAsk {
    let permission = s(Some(req), "permission").unwrap_or("tool");
    let meta = req.get("metadata").filter(|m| matches!(m, Json::Obj(_)));
    let pattern = match req.get("patterns") { Some(Json::Arr(p)) => p.iter().filter_map(Json::as_str).find(|x| !x.is_empty()).map(str::to_owned), _ => None };
    let id = s(Some(req), "id").map_or_else(hover_core::guid_n, str::to_owned);
    let (mut kind, mut title) = ("other", cap(permission));
    let (mut command, mut path, mut preview): (Option<String>, Option<String>, Option<String>) = (None, None, None);
    let (mut added, mut removed) = (0i32, 0i32);
    let m = |k: &str| s(meta, k).map(str::to_owned);
    match permission {
        "bash" => { kind = "execute"; title = "Run a command".into(); command = m("command").or(pattern.clone()); }
        "edit" => {
            kind = "edit";
            title = "Edit a file".into();
            path = m("filepath").or_else(|| m("filePath")).or(pattern.clone());
            if let Some(diff) = m("diff") {
                let d = diff.replace('\r', "");
                let changed: Vec<&str> = d.split('\n').filter(|l| (l.starts_with('+') && !l.starts_with("+++")) || (l.starts_with('-') && !l.starts_with("---"))).collect();
                added = changed.iter().filter(|l| l.starts_with('+')).count() as i32;
                removed = changed.iter().filter(|l| l.starts_with('-')).count() as i32;
                preview = Some(changed.iter().take(6).map(|l| format!("{} {}", &l[..1], clip_to(l[1..].trim(), 110))).collect::<Vec<_>>().join("\n"));
            }
        }
        "webfetch" | "websearch" | "codesearch" => { kind = "fetch"; title = "Use the network".into(); command = m("url").or_else(|| m("query")).or(pattern.clone()); }
        "read" => { kind = "read"; title = "Read a file".into(); path = m("filePath").or(pattern.clone()); }
        "external_directory" => { title = "Work outside the folder".into(); path = pattern.clone(); }
        "task" => { title = "Start a subagent".into(); command = pattern.clone(); }
        "doom_loop" => { title = "Repeat the same tool call".into(); }
        _ => {}
    }
    let mut outside = permission == "external_directory";
    if let Some(p) = path.clone().filter(|p| !p.is_empty()) {
        let full = if crate::fully_qualified(&p) { ask::full(&p) } else { ask::full(&format!("{folder}/{}", p.trim_end_matches('*'))) };
        let sep = if cfg!(windows) { '\\' } else { '/' };
        let root = format!("{}{sep}", ask::full(folder).trim_end_matches(['\\', '/']));
        if full.len() >= root.len() && full.is_char_boundary(root.len()) && full[..root.len()].to_lowercase() == root.to_lowercase() {
            path = Some(full[root.len()..].replace('\\', "/"));
        } else {
            outside = true;
        }
    }
    let danger = permission == "doom_loop" || command.as_deref().is_some_and(ask::destructive);
    let n = added + removed;
    let mut reason: String = match kind {
        "execute" => if danger { "Can delete or overwrite things".into() } else if command.as_deref().is_some_and(ask::network) { "Installs packages or uses the network".into() } else { "Runs a command".into() },
        "edit" => if outside { "Edits a file outside the folder".into() } else if n > 0 { format!("Changes {n} line{}", if n == 1 { "" } else { "s" }) } else { "Edits a file".into() },
        "fetch" => "Uses the network".into(),
        "read" => "Reads a file your OpenCode rules protect".into(),
        _ => match permission {
            "external_directory" => "Reaches outside the folder".into(),
            "doom_loop" => "OpenCode saw it call the same tool again and again".into(),
            "task" => "Hands part of the task to a subagent".into(),
            _ => "Uses a tool".into(),
        },
    };
    if outside && !matches!(kind, "edit" | "other") { reason.push_str(" · outside the folder"); }
    AgentAsk { id, kind: kind.into(), title, command: command.filter(|c| !c.is_empty()).map(|c| clip_to(&c, 400)), path, preview, added, removed, reason, danger, questions: None }
}

/// A message id in OpenCode's own form (msg_, 12 hex digits of time, 14 random
/// characters), later than any before it, so the server orders it last. As T3 makes it.
pub fn new_message_id() -> String {
    static LAST: Mutex<(u64, u64)> = Mutex::new((0, 0));
    let (ms, n) = {
        let mut g = LAST.lock().unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let ms = if now <= g.0 { g.0 } else { *g = (now, 0); now };
        g.1 += 1;
        (ms, g.1)
    };
    let time = (ms.wrapping_mul(0x1000).wrapping_add(n)) & 0xFFFF_FFFF_FFFF;
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut r = [0u8; 14];
    getrandom::fill(&mut r).expect("the system has no randomness");
    format!("msg_{time:012x}{}", r.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect::<String>())
}

/// OpenCodeHost.Launch: "opencode serve" hidden, in the group that goes with Hover, its
/// URL read from what it prints ("listening on http://127.0.0.1:port").
fn launch(ct: &Cancel, t: &Timeouts) -> Result<Option<OpenCodeLink>, OcErr> {
    let Some(exe) = agents::exe(AgentTool::OpenCode) else { return Ok(None) };
    let mut pw = [0u8; 24];
    getrandom::fill(&mut pw).expect("the system has no randomness");
    let password: String = pw.iter().map(|b| format!("{b:02X}")).collect();
    let mut cmd = crate::proc::hidden(&exe, agents::arguments(AgentTool::OpenCode));
    cmd.current_dir(crate::proc::home());
    cmd.env("OPENCODE_SERVER_PASSWORD", &password);
    // The question tool, which Hover answers in the notch and the office.
    cmd.env("OPENCODE_ENABLE_QUESTION_TOOL", "1");
    let g = Arc::new(crate::proc::Group::spawn(cmd).map_err(|e| OcErr::Oc(None, format!("OpenCode couldn’t start: {e}")))?);
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    log(&format!("started (pid {})", g.pid().unwrap_or(0)));
    let tail = Arc::new(Mutex::new(String::new()));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
    // Both pipes are read to the end, so the server never blocks on a full one.
    let keep = |pipe: Option<Box<dyn Read + Send>>| {
        let (tail, ready) = (tail.clone(), ready_tx.clone());
        std::thread::spawn(move || {
            let Some(p) = pipe else { return };
            let mut r = BufReader::new(p);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) { Ok(0) | Err(_) => break, Ok(_) => {} }
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_owned();
                {
                    let mut t = tail.lock().unwrap();
                    t.push_str(&line);
                    t.push('\n');
                    let over = t.len().saturating_sub(8192);
                    if over > 0 { let mut cut = over; while !t.is_char_boundary(cut) { cut += 1; } t.drain(..cut); }
                }
                if let Some(at) = line.to_ascii_lowercase().find("listening on ") {
                    let u = strip_ansi(&line[at + 13..]).trim().to_owned();
                    if u.starts_with("http://") { let _ = ready.send(Ok(u)); }
                }
            }
        });
    };
    keep(stdout.map(|p| Box::new(p) as Box<dyn Read + Send>));
    keep(stderr.map(|p| Box::new(p) as Box<dyn Read + Send>));
    let (exit_tx, exit_rx) = mpsc::channel::<()>();
    {
        let (g, ready) = (g.clone(), ready_tx.clone());
        std::thread::spawn(move || {
            while g.wait_timeout(Duration::from_millis(500)).is_none() {}
            let _ = ready.send(Err("OpenCode stopped before its server started.".into()));
            let _ = exit_tx.send(());
        });
    }
    drop(ready_tx);
    let why = |tail: &Mutex<String>| {
        let t = tail.lock().unwrap();
        let lines: Vec<&str> = t.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        lines[lines.len().saturating_sub(2)..].join(" / ")
    };
    let until = Instant::now() + t.start;
    let got = loop {
        if ct.is_cancelled() { g.kill(); return Err(OcErr::Cancelled); }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() { break Err(format!("OpenCode’s server didn’t start within {:.0} s. {}", t.start.as_secs_f64(), why(&tail))); }
        match ready_rx.recv_timeout(left.min(Duration::from_millis(100))) {
            Ok(Ok(u)) => break Ok(u),
            Ok(Err(m)) => { std::thread::sleep(Duration::from_millis(100)); break Err(format!("{m} {}", why(&tail)).trim().to_owned()); }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err("OpenCode stopped before its server started.".into()),
        }
    };
    let url = match got {
        Ok(u) => u,
        Err(m) => { g.kill(); return Err(OcErr::Oc(None, m)); }
    };
    if !crate::http::host_port(&url).is_some_and(|(h, _)| crate::http::is_loopback(&h)) {
        g.kill();
        return Err(OcErr::Oc(None, format!("OpenCode listened on {url}, not on this PC only.")));
    }
    let (g2, t2) = (g.clone(), tail.clone());
    Ok(Some(OpenCodeLink { url, password, kill: Box::new(move || g2.kill()), errors: Box::new(move || t2.lock().unwrap().clone()), exited: Some(exit_rx) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROVIDERS: &str = r#"{"providers":[{"id":"p","name":"Prov","models":{"a/b":{"id":"a/b","name":"A B","variants":{"low":{},"high":{}},"limit":{"context":1000}},"m":{"id":"m","name":"M","limit":{"context":1000}}}}],"default":{"p":"m"}}"#;
    pub const AGENTS: &str = r#"[{"name":"build","mode":"primary","permission":[{"permission":"*","pattern":"*","action":"allow"},{"permission":"question","pattern":"*","action":"deny"},{"permission":"question","pattern":"*","action":"allow"},{"permission":"bash","pattern":"rm *","action":"deny"}]},
             {"name":"plan","mode":"primary","permission":[{"permission":"edit","pattern":"*","action":"deny"}]},
             {"name":"title","mode":"primary","hidden":true,"permission":[]},
             {"name":"explore","mode":"subagent","permission":[]}]"#;

    fn pairs(r: &Json) -> Vec<(String, String)> {
        r.items().unwrap().iter().map(|x| (s(Some(x), "permission").unwrap().to_owned(), s(Some(x), "action").unwrap().to_owned())).collect()
    }

    /// OpenCodeHostTests.Full_access_keeps_the_agents_denies_and_opencodes_own_loop_stop.
    #[test]
    fn full_access_keeps_the_agents_denies_and_opencodes_own_loop_stop() {
        let agents = json::parse(AGENTS).unwrap();
        let full = pairs(&rules(&AgentOptions::default(), &agents, "plan"));
        let ask = pairs(&rules(&AgentOptions { approval: AgentApproval::Risky, ..Default::default() }, &agents, "build"));
        let p = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(full[0], p("*", "allow"));
        assert!(full.contains(&p("doom_loop", "ask")));
        assert_eq!(full.last().unwrap(), &p("edit", "deny"), "Plan still can't edit under Full");
        assert!(ask.contains(&p("bash", "ask")));
        assert_eq!(ask.iter().rev().find(|x| x.0 == "edit").unwrap(), &p("edit", "allow"), "Ask first lets edits in the folder go ahead");
        assert_eq!(ask.iter().rev().find(|x| x.0 == "question").unwrap(), &p("question", "allow"), "a deny the agent overrules later isn't carried over");
        assert_eq!(ask.last().unwrap(), &p("bash", "deny"));
    }

    /// OpenCodeHostTests.Offers_keep_each_models_own_variants_and_leave_hidden_agents_out.
    #[test]
    fn offers_keep_each_models_own_variants_and_leave_hidden_agents_out() {
        let o = offers(&json::parse(PROVIDERS).unwrap(), &json::parse(AGENTS).unwrap());
        let models = &o.iter().find(|x| x.category.as_deref() == Some("model")).unwrap().choices;
        assert_eq!(models.iter().map(|m| m.value.as_str()).collect::<Vec<_>>(), ["p/a/b", "p/m"]);
        assert_eq!(models[0].levels.as_deref(), Some(&["low".to_string(), "high".to_string()][..]));
        assert_eq!(models[1].levels.as_deref(), Some(&[][..]));
        assert_eq!(models[0].name, "A B · Prov");
        let modes: Vec<&str> = o.iter().find(|x| x.category.as_deref() == Some("mode")).unwrap().choices.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(modes, ["build", "plan"]);
    }

    /// OpenCodeHostTests.Message_ids_are_opencodes_shape_and_keep_rising.
    #[test]
    fn message_ids_are_opencodes_shape_and_keep_rising() {
        let ids: Vec<String> = (0..50).map(|_| new_message_id()).collect();
        for i in &ids {
            assert!(i.len() == 30 && i.starts_with("msg_") && i[4..16].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) && i[16..].bytes().all(|b| b.is_ascii_alphanumeric()), "{i}");
        }
        assert!(ids.windows(2).all(|w| w[0][..16] < w[1][..16]));
    }

    /// OpenCodeHostTests.A_permission_request_reads_as_the_notch_shows_it (the outside
    /// path is this platform's).
    #[test]
    fn a_permission_request_reads_as_the_notch_shows_it() {
        let bash = describe(&json::parse(r#"{"id":"per_1","permission":"bash","patterns":["rm -rf build"],"metadata":{"command":"rm -rf build"},"always":[]}"#).unwrap(), "/p");
        let far = if cfg!(windows) { r"C:\\Windows\\*" } else { "/etc/*" };
        let outside = describe(&json::parse(&format!(r#"{{"id":"per_2","permission":"external_directory","patterns":["{far}"],"metadata":{{}},"always":[]}}"#)).unwrap(), "/p");
        assert_eq!((bash.kind.as_str(), bash.command.as_deref(), bash.danger), ("execute", Some("rm -rf build"), true));
        assert_eq!(outside.reason, "Reaches outside the folder");
        let edit = describe(&json::parse(r#"{"id":"e","permission":"edit","patterns":["src/a.rs"],"metadata":{"filepath":"/p/src/a.rs","diff":"--- a\n+++ b\n-old\n+new\n+more"}}"#).unwrap(), "/p");
        assert_eq!((edit.path.as_deref(), edit.added, edit.removed, edit.preview.as_deref(), edit.reason.as_str()), (Some("src/a.rs"), 2, 1, Some("- old\n+ new\n+ more"), "Changes 3 lines"));
    }

    #[test]
    fn wildcards_and_models() {
        assert!(matches("rm -rf x", Some("rm *")) && matches("anything", Some("*")) && !matches("git", Some("rm *")) && !matches("x", None));
        assert!(matches("a\nb", Some("a*b")) && matches("abc", Some("a*c*")) && !matches("ab", Some("abc")));
        let inv = json::parse(PROVIDERS).unwrap();
        let m = pick_model(&inv, Some("p/a/b")).unwrap().unwrap();
        assert_eq!((m.provider.as_str(), m.model.as_str(), m.variants.len(), m.limit), ("p", "a/b", 2, Some(1000.0)));
        assert!(pick_model(&inv, Some("p/gone")).unwrap_err().contains("p/gone"));
        assert_eq!(pick_model(&inv, None), Ok(None));
        assert_eq!(title("\n  Fix the build  \nmore"), "Fix the build");
        assert_eq!(kind_of("apply_patch"), "edit");
    }
}
