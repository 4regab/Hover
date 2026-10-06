//! Agent orchestration: Hover's own record of who asked whom for help, above the providers.
//!
//! A lead task may, once the user has switched delegation on for it, start *helpers*: ordinary Hover
//! sessions on any ready provider, each with a brief, maybe a role, and the lead's permissions or
//! fewer. Hover keeps the tree (runs, attempts, saved results, receipts) in its own sealed file; a
//! provider's thread ids and the live processes are separate things, named apart:
//!
//! - *conversation*: a session, by its lasting key (what the history keeps);
//! - *run*: one helper job (`r-…`), with its parent, root, depth, brief and result;
//! - *attempt*: one try of a run on a provider, with that provider's own thread id;
//! - *live session*: the number of a session at a desk, never saved.
//!
//! The lead's agent reaches this through a small MCP server (`servers`, Unix only for now: it rides the
//! same relay and socket as Hover's browser). Each call is checked against the live session: delegation
//! on, a turn running, not stopped. A call from a session whose turn is over is refused as stale.
//!
//! Promises kept here, each with a test:
//! - a retry with the same request id makes no second helper (receipts);
//! - waiting that times out does not cancel the helper, and a wait parks the lead so a full house of
//!   waiting leads can't starve their helpers;
//! - a result reaches a lead once: by its own wait/result call, else as one message when its turn is
//!   over, never to a lead the user stopped and never twice (a marker in the message settles doubt);
//! - Stop reaches helpers, their helpers and their queued starts; late news from them wakes nobody;
//! - helpers get equal or narrower access; a writing helper gets its own worktree (workspace.rs);
//! - after a restart, no run is left pretending to work.
//!
//! Nothing here is written to the log except ids and states: briefs, results and tokens stay out of it.

use crate::browser;
use crate::cancel::Cancel;
use crate::computer_use::McpServer;
use crate::session::{KiroSession, KiroSessions};
use crate::stream::KiroResult;
use crate::workspace::{self, Choice};
use hover_core::ext::{OrchLink, SessionExt};
use hover_core::json::{self, Json};
use hover_core::model::{AgentTool, DelegationLimits, KiroState};
use hover_core::store::Sealed;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

pub const SERVER_NAME: &str = "hover-helpers";
/// What a lead is told when the switch is off.
pub const OFF: &str = "Delegation is off for this task. The user can switch it on for the task.";
/// The longest result one call hands back; the rest is fetched with an offset.
pub const PAGE: usize = 20_000;
/// The longest result kept with a run (the helper's own session keeps everything).
const KEEP: usize = 400_000;

// MARK: What the host tells us

/// A provider as the lead sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Provider {
    /// What the lead writes: `kiro`, `codex`, `cursor`, `opencode`, `claude`, or `custom:<id>`.
    pub id: String,
    pub name: String,
    pub tool: AgentTool,
    /// The custom provider's instance id.
    pub instance: Option<String>,
    pub ready: bool,
    pub hint: String,
    /// Can run read-only, resume a conversation, and call tools of its own (so can lead).
    pub read_only: bool,
    pub resume: bool,
    pub leads: bool,
}

/// What the orchestrator asks of the app around it.
pub trait Env: Send + Sync {
    /// The providers that could take a job, ready or not (a blocking look at the disk and the tools' sign-in).
    fn providers(&self) -> Vec<Provider>;
    /// The access a session really has: `full`, `risky`, `always` or `read`.
    fn access_of(&self, s: &KiroSession) -> String;
    fn limits(&self) -> DelegationLimits;
    /// Where worktrees for writing helpers go.
    fn worktrees(&self) -> PathBuf;
}

/// Widest first. A helper's access is never above its lead's.
fn rank(a: &str) -> u8 { match a { "read" | "none" => 0, "always" => 1, "risky" => 2, _ => 3 } }

/// The narrower of the two.
pub fn narrow(lead: &str, want: Option<&str>) -> String {
    match want.filter(|w| ["read", "always", "risky", "full"].contains(w)) {
        Some(w) if rank(w) <= rank(lead) => w.to_owned(),
        _ => lead.to_owned(),
    }
}

/// The host's real answers: the tools on this computer, the user's settings, Hover's data folder.
pub struct SystemEnv { settings: Arc<hover_core::settings::Settings>, extra: Mutex<Vec<Arc<dyn Fn() -> Vec<Provider> + Send + Sync>>> }

impl SystemEnv {
    pub fn new(settings: Arc<hover_core::settings::Settings>) -> SystemEnv { SystemEnv { settings, extra: Mutex::new(vec![]) } }

    /// More providers (the user's custom agents), asked for at each listing.
    pub fn add(&self, f: impl Fn() -> Vec<Provider> + Send + Sync + 'static) { self.extra.lock().unwrap().push(Arc::new(f)); }
}

impl Env for SystemEnv {
    fn providers(&self) -> Vec<Provider> {
        let mut all: Vec<Provider> = AgentTool::ALL.iter().map(|&t| {
            let c = crate::runtime::caps(t);
            let ready = crate::agents::check(t, false);
            Provider { id: t.id().into(), name: t.name().into(), tool: t, instance: None, ready: ready.ok(), hint: ready.hint, read_only: c.read_only, resume: c.resume,
                // OpenCode's one shared server can't hand each session its own MCP server.
                leads: t != AgentTool::OpenCode }
        }).collect();
        for f in self.extra.lock().unwrap().clone() { all.extend(f()); }
        all
    }

    fn access_of(&self, s: &KiroSession) -> String {
        s.access.clone().unwrap_or_else(|| self.settings.agent_options(s.tool).access_id(crate::agents::read_only_works(s.tool)).to_owned())
    }

    fn limits(&self) -> DelegationLimits { self.settings.delegation() }

    fn worktrees(&self) -> PathBuf { hover_core::paths::support().join("worktrees") }
}

// MARK: Records

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState { Queued, Running, Done, Failed, Cancelled }

impl RunState {
    const NAMES: [&'static str; 5] = ["queued", "running", "done", "failed", "cancelled"];
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    fn parse(s: &str) -> RunState { [RunState::Queued, RunState::Running, RunState::Done, RunState::Failed, RunState::Cancelled][Self::NAMES.iter().position(|n| *n == s).unwrap_or(3)] }
    pub fn finished(self) -> bool { !matches!(self, RunState::Queued | RunState::Running) }
}

/// What became of telling the lead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery { Pending, /** read by the lead's own call */ Taken, /** sent as a message */ Sent, /** the lead was stopped or is gone */ Suppressed }

impl Delivery {
    const NAMES: [&'static str; 4] = ["pending", "taken", "sent", "suppressed"];
    fn name(self) -> &'static str { Self::NAMES[self as usize] }
    fn parse(s: &str) -> Delivery { [Delivery::Pending, Delivery::Taken, Delivery::Sent, Delivery::Suppressed][Self::NAMES.iter().position(|n| *n == s).unwrap_or(0)] }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attempt { pub id: String, pub session: String, /** the provider's own thread id */ pub thread: Option<String>, pub state: String, pub started: i64, pub ended: Option<i64> }

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: String,
    /// The session that asked (its key), the one at the top of the tree, and how deep this run is (1 for a lead's helper).
    pub parent: String,
    pub root: String,
    pub depth: u32,
    pub provider: String,
    pub role: Option<String>,
    pub brief: String,
    /// The access it was given, after narrowing.
    pub access: String,
    pub state: RunState,
    pub result: Option<String>,
    pub note: Option<String>,
    /// The helper's session (its key), once it has one.
    pub session: Option<String>,
    pub attempts: Vec<Attempt>,
    pub delivery: Delivery,
    pub created: i64,
    pub ended: Option<i64>,
}

/// An ordinary thread a lead started through the server (not a helper: nothing is handed back on its own).
#[derive(Clone, Debug, PartialEq)]
pub struct Thread { pub id: String, pub owner: String, pub session: String }

fn now_ms() -> i64 { hover_core::time::Stamp::now().unix_ms() }
fn new_id(p: &str) -> String { format!("{p}-{}", hover_core::guid_n().chars().take(10).collect::<String>()) }

#[derive(Default)]
struct State {
    runs: Vec<Run>,
    /// (asking session, request id) to the run it made: a retry finds the first answer.
    receipts: Vec<(String, String, String)>,
    threads: Vec<Thread>,
    /// Sessions the user stopped, with the number of turns they had then: they are stopped until a newer turn begins.
    stopped: Vec<(String, usize)>,
    /// Runs being started right now, so two pumps never start one twice.
    starting: HashSet<String>,
    /// Waits in progress, by run id.
    waiting: Vec<(String, String)>,
}

fn opt(s: &Option<String>) -> Json { Json::opt_str_of(s.as_deref()) }

impl Run {
    fn to_json(&self) -> Json {
        Json::obj(vec![("Id", Json::str(&self.id)), ("Parent", Json::str(&self.parent)), ("Root", Json::str(&self.root)), ("Depth", Json::int(self.depth as i64)),
            ("Provider", Json::str(&self.provider)), ("Role", opt(&self.role)), ("Brief", Json::str(&self.brief)), ("Access", Json::str(&self.access)),
            ("State", Json::str(self.state.name())), ("Result", opt(&self.result)), ("Note", opt(&self.note)), ("Session", opt(&self.session)),
            ("Attempts", Json::Arr(self.attempts.iter().map(|a| Json::obj(vec![("Id", Json::str(&a.id)), ("Session", Json::str(&a.session)), ("Thread", opt(&a.thread)),
                ("State", Json::str(&a.state)), ("Started", Json::int(a.started)), ("Ended", a.ended.map_or(Json::Null, Json::int))])).collect())),
            ("Delivery", Json::str(self.delivery.name())), ("Created", Json::int(self.created)), ("Ended", self.ended.map_or(Json::Null, Json::int))])
    }

    fn from_json(v: &Json) -> json::Result<Run> {
        let s = |k: &str| -> json::Result<String> { Ok(v.get(k).map(Json::opt_str).transpose()?.flatten().unwrap_or_default()) };
        let o = |k: &str| -> json::Result<Option<String>> { Ok(v.get(k).map(Json::opt_str).transpose()?.flatten()) };
        let i = |x: &Json, k: &str| -> json::Result<i64> { Ok(x.get(k).map(Json::i64).transpose()?.unwrap_or(0)) };
        Ok(Run {
            id: s("Id")?, parent: s("Parent")?, root: s("Root")?, depth: v.get("Depth").map(Json::i32).transpose()?.unwrap_or(1).max(0) as u32, provider: s("Provider")?, role: o("Role")?,
            brief: s("Brief")?, access: s("Access")?, state: RunState::parse(&s("State")?), result: o("Result")?, note: o("Note")?, session: o("Session")?,
            attempts: v.get("Attempts").map(|a| a.opt_list(|x| Ok(Attempt { id: x.get("Id").map(Json::opt_str).transpose()?.flatten().unwrap_or_default(),
                session: x.get("Session").map(Json::opt_str).transpose()?.flatten().unwrap_or_default(), thread: x.get("Thread").map(Json::opt_str).transpose()?.flatten(),
                state: x.get("State").map(Json::opt_str).transpose()?.flatten().unwrap_or_default(), started: i(x, "Started")?,
                ended: x.get("Ended").filter(|e| !e.is_null()).map(Json::i64).transpose()? }))).transpose()?.flatten().unwrap_or_default(),
            delivery: Delivery::parse(&s("Delivery")?), created: i(v, "Created")?, ended: v.get("Ended").filter(|e| !e.is_null()).map(Json::i64).transpose()?,
        })
    }
}

impl State {
    fn to_json(&self) -> Json {
        let three = |(a, b, c): &(String, String, String)| Json::obj(vec![("Session", Json::str(a)), ("Request", Json::str(b)), ("Run", Json::str(c))]);
        Json::obj(vec![("Runs", Json::Arr(self.runs.iter().map(Run::to_json).collect())), ("Receipts", Json::Arr(self.receipts.iter().map(three).collect())),
            ("Threads", Json::Arr(self.threads.iter().map(|t| Json::obj(vec![("Id", Json::str(&t.id)), ("Owner", Json::str(&t.owner)), ("Session", Json::str(&t.session))])).collect())),
            ("Stopped", Json::Arr(self.stopped.iter().map(|(k, n)| Json::obj(vec![("Session", Json::str(k)), ("Turns", Json::int(*n as i64))])).collect()))])
    }

    fn from_json(v: &Json) -> json::Result<State> {
        let txt = |x: &Json, k: &str| -> json::Result<String> { Ok(x.get(k).map(Json::opt_str).transpose()?.flatten().unwrap_or_default()) };
        let mut st = State::default();
        if let Some(l) = v.get("Runs") { st.runs = l.opt_list(Run::from_json)?.unwrap_or_default(); }
        if let Some(l) = v.get("Receipts") { st.receipts = l.opt_list(|x| Ok((txt(x, "Session")?, txt(x, "Request")?, txt(x, "Run")?)))?.unwrap_or_default(); }
        if let Some(l) = v.get("Threads") { st.threads = l.opt_list(|x| Ok(Thread { id: txt(x, "Id")?, owner: txt(x, "Owner")?, session: txt(x, "Session")? }))?.unwrap_or_default(); }
        if let Some(l) = v.get("Stopped") { st.stopped = l.opt_list(|x| Ok((txt(x, "Session")?, x.get("Turns").map(Json::i32).transpose()?.unwrap_or(0).max(0) as usize)))?.unwrap_or_default(); }
        Ok(st)
    }
}

/// Writes the latest state to disk on a thread of its own, so no caller (the UI thread included) waits on the disk.
struct Writer { tx: Mutex<mpsc::Sender<String>>, pending: Arc<(Mutex<usize>, Condvar)> }

impl Writer {
    fn new(doc: Sealed) -> Writer {
        let (tx, rx) = mpsc::channel::<String>();
        let pending = Arc::new((Mutex::new(0usize), Condvar::new()));
        let p = pending.clone();
        std::thread::Builder::new().name("orch-store".into()).spawn(move || {
            while let Ok(mut text) = rx.recv() {
                let mut n = 1;
                while let Ok(newer) = rx.try_recv() { text = newer; n += 1; }
                match json::parse(&text) { Ok(v) => { if let Err(e) = doc.write(&v) { hover_core::log::line(&format!("orch: save failed - {e}")); } } Err(_) => {} }
                *p.0.lock().unwrap() -= n;
                p.1.notify_all();
            }
        }).expect("a thread for the orchestration record");
        Writer { tx: Mutex::new(tx), pending }
    }

    fn put(&self, text: String) { *self.pending.0.lock().unwrap() += 1; let _ = self.tx.lock().unwrap().send(text); }

    fn flush(&self) { let g = self.pending.0.lock().unwrap(); let _ = self.pending.1.wait_timeout_while(g, Duration::from_secs(10), |n| *n > 0).unwrap(); }
}

// MARK: The orchestrator

pub struct Orch {
    me: Weak<Orch>,
    sessions: KiroSessions,
    env: Arc<dyn Env>,
    st: Mutex<State>,
    cv: Condvar,
    out: Option<Writer>,
    listeners: Mutex<Vec<Arc<dyn Fn(&str) + Send + Sync>>>,
}

static GLOBAL: OnceLock<Weak<Orch>> = OnceLock::new();

/// A run as the lead, the desk card and the MCP tools see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub run: String,
    pub state: RunState,
    pub provider: String,
    pub role: Option<String>,
    pub parent: String,
    pub session: Option<String>,
    pub access: String,
    pub result: Option<String>,
    pub note: Option<String>,
    pub delivery: Delivery,
}

fn info(r: &Run) -> Info {
    Info { run: r.id.clone(), state: r.state, provider: r.provider.clone(), role: r.role.clone(), parent: r.parent.clone(), session: r.session.clone(), access: r.access.clone(),
        result: r.result.clone(), note: r.note.clone(), delivery: r.delivery }
}

/// What a lead asks for.
#[derive(Clone, Debug, Default)]
pub struct Delegate { pub provider: String, pub brief: String, pub role: Option<String>, pub access: Option<String>, pub request: Option<String> }

impl Orch {
    /// Starts the orchestrator over the sessions: loads the saved record, settles what a restart left
    /// unfinished, and watches the sessions for ends, stops and freed places.
    pub fn new(sessions: KiroSessions, env: Arc<dyn Env>, doc: Option<Sealed>) -> Arc<Orch> {
        let st = doc.as_ref().and_then(Sealed::read).and_then(|v| match State::from_json(&v) { Ok(s) => Some(s), Err(e) => { hover_core::log::line(&format!("orch: record unreadable - {e}")); None } }).unwrap_or_default();
        let o = Arc::new_cyclic(|me| Orch { me: me.clone(), sessions: sessions.clone(), env, st: Mutex::new(st), cv: Condvar::new(), out: doc.map(Writer::new), listeners: Mutex::new(vec![]) });
        o.recover();
        let w = o.me.clone();
        sessions.on_ended(move |s, r| { if let Some(o) = w.upgrade() { o.ended(s, r); } });
        let w = o.me.clone();
        sessions.on_stop(move |s| { if let Some(o) = w.upgrade() { o.stopped(s); } });
        let w = o.me.clone();
        sessions.on_changed(move || { if let Some(o) = w.upgrade() { if o.has_queued() { o.pump(); } } });
        o
    }

    /// Makes this the orchestrator the agents' MCP servers talk to.
    pub fn install(self: &Arc<Orch>) { let _ = GLOBAL.set(Arc::downgrade(self)); }

    /// A run is waiting for a place. Cheap: the change hook asks it on every change of every session.
    fn has_queued(&self) -> bool { self.st.lock().unwrap().runs.iter().any(|r| r.state == RunState::Queued && r.session.is_none()) }

    pub fn flush(&self) { if let Some(w) = &self.out { w.flush(); } }

    /// Called with a session key for each session in a stopped tree (watchers and continuations end there).
    pub fn on_tree_stop(&self, f: impl Fn(&str) + Send + Sync + 'static) { self.listeners.lock().unwrap().push(Arc::new(f)); }

    fn save(&self, s: &State) { if let Some(w) = &self.out { w.put(s.to_json().compact()); } }

    /// A restart leaves nothing pretending: a run that was going is failed with the reason; a result the
    /// lead never heard of is delivered once if the lead finished well and wasn't stopped.
    fn recover(&self) {
        let mut g = self.st.lock().unwrap();
        let live: HashSet<String> = self.sessions.all().into_iter().filter(KiroSession::busy).map(|s| s.key).collect();
        let t = now_ms();
        let mut n = 0;
        for r in g.runs.iter_mut().filter(|r| !r.state.finished() && !r.session.as_ref().is_some_and(|k| live.contains(k))) {
            r.state = RunState::Failed;
            r.note = Some("Hover closed before this helper finished.".into());
            r.ended = Some(t);
            if let Some(a) = r.attempts.last_mut() { if a.ended.is_none() { a.state = "failed".into(); a.ended = Some(t); } }
            n += 1;
        }
        if n > 0 { hover_core::log::line(&format!("orch: {n} run(s) were cut off when Hover closed")); }
        self.save(&g);
    }

    /// The lead's results that were never delivered, once the sessions are up. Call it after the app has started.
    pub fn deliver_pending(&self) {
        let parents: HashSet<String> = self.st.lock().unwrap().runs.iter().filter(|r| r.state.finished() && r.delivery == Delivery::Pending).map(|r| r.parent.clone()).collect();
        for p in parents { self.deliver_to(&p); }
    }

    // MARK: Checks

    /// The lead and its link, when its credentials are good: it is there, running a turn, with delegation on and not stopped.
    fn lead(&self, key: &str) -> Result<(KiroSession, OrchLink), String> {
        let s = self.sessions.find(key).ok_or("This task isn’t open any more, so it can’t ask for helpers.")?;
        if !s.busy() { return Err("This task’s turn is over, so these credentials are no longer valid.".into()); }
        if self.is_stopped(key, s.turns.len()) { return Err("This task was stopped.".into()); }
        let link = s.ext.orch.clone().filter(|l| l.delegation).ok_or(OFF)?;
        Ok((s, link))
    }

    fn is_stopped(&self, key: &str, turns: usize) -> bool { self.st.lock().unwrap().stopped.iter().any(|(k, n)| k == key && turns <= *n) }

    pub fn stopped_now(&self, key: &str) -> bool { self.sessions.find(key).is_some_and(|s| self.is_stopped(key, s.turns.len())) }

    // MARK: Delegating

    pub fn providers(&self) -> Vec<Provider> { self.env.providers() }

    /// Starts (or finds, on a retry) a helper. The run is saved first; the helper starts when a place is free.
    pub fn delegate(&self, caller: &str, d: Delegate) -> Result<Info, String> {
        let (lead, link) = self.lead(caller)?;
        if d.brief.trim().is_empty() { return Err("A helper needs a brief: say what it should do.".into()); }
        let limits = self.env.limits();
        let depth = link.depth + 1;
        if depth > limits.max_depth { return Err(format!("Helpers can go {} level{} deep here, and this would be level {depth}. Do the work yourself, or ask the user to raise the limit.", limits.max_depth, if limits.max_depth == 1 { "" } else { "s" })); }
        let providers = self.env.providers();
        let p = providers.iter().find(|p| p.id == d.provider).ok_or_else(|| format!("There is no provider called “{}”. Available: {}.", d.provider,
            providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")))?;
        if !p.ready { return Err(format!("{} isn’t available: {}", p.name, p.hint)); }
        let lead_access = self.env.access_of(&lead);
        let access = narrow(&lead_access, d.access.as_deref());
        if access == "read" && !p.read_only { return Err(format!("{} can’t run read-only here, so it can’t be a read-only helper.", p.name)); }
        let root = link.root.clone().unwrap_or_else(|| caller.to_owned());
        let id = {
            let mut g = self.st.lock().unwrap();
            if let Some(req) = d.request.as_deref().filter(|r| !r.is_empty()) {
                if let Some((_, _, run)) = g.receipts.iter().find(|(s, r, _)| s == caller && r == req) {
                    return g.runs.iter().find(|r| &r.id == run).map(info).ok_or_else(|| "That request was answered, but its run is gone.".to_owned());
                }
            }
            let mine: Vec<&Run> = g.runs.iter().filter(|r| r.root == root).collect();
            let threads = g.threads.iter().filter(|t| t.owner == root).count();
            if mine.len() + threads >= limits.max_helpers as usize { return Err(format!("This task has used its {} helpers. Wait for their results, or ask the user to raise the limit.", limits.max_helpers)); }
            let going = mine.iter().filter(|r| !r.state.finished()).count();
            if going >= limits.max_parallel as usize { return Err(format!("{going} helper{} already working (the limit is {}). Wait for one to finish, then ask again.", if going == 1 { " is" } else { "s are" }, limits.max_parallel)); }
            let id = new_id("r");
            let note = d.access.as_deref().filter(|w| ["read", "always", "risky", "full"].contains(w) && *w != access).map(|_| format!("Access was narrowed to {access}: a helper never has more than the task that asked."));
            g.runs.push(Run { id: id.clone(), parent: caller.to_owned(), root, depth, provider: p.id.clone(), role: d.role.clone().filter(|r| !r.trim().is_empty()), brief: d.brief.clone(), access,
                state: RunState::Queued, result: None, note, session: None, attempts: vec![], delivery: Delivery::Pending, created: now_ms(), ended: None });
            if let Some(req) = d.request.filter(|r| !r.is_empty()) { g.receipts.push((caller.to_owned(), req, id.clone())); }
            self.save(&g);
            id
        };
        hover_core::log::line(&format!("orch: {caller} asks {} for run {id}", p.id));
        self.pump();
        self.run_info(&id).ok_or_else(|| "The helper could not be recorded.".to_owned())
    }

    fn run_info(&self, id: &str) -> Option<Info> { self.st.lock().unwrap().runs.iter().find(|r| r.id == id).map(info) }

    /// Starts every queued run that can start now. Each start (a worktree may be made) runs on a thread of its own.
    pub fn pump(&self) {
        let todo: Vec<String> = {
            let mut g = self.st.lock().unwrap();
            let ids: Vec<String> = g.runs.iter().filter(|r| r.state == RunState::Queued && r.session.is_none() && !g.starting.contains(&r.id)).map(|r| r.id.clone()).collect();
            for i in &ids { g.starting.insert(i.clone()); }
            ids
        };
        for id in todo {
            let Some(me) = self.me.upgrade() else { return };
            let spawned = std::thread::Builder::new().name("orch-start".into()).spawn(move || { me.start_run(&id); });
            if spawned.is_err() { self.st.lock().unwrap().starting.clear(); }
        }
    }

    fn start_run(&self, id: &str) {
        let done = |o: &Orch| { o.st.lock().unwrap().starting.remove(id); };
        let Some(run) = self.st.lock().unwrap().runs.iter().find(|r| r.id == id).cloned() else { return done(self) };
        if run.state != RunState::Queued { return done(self); }
        let Some(lead) = self.sessions.find(&run.parent) else { self.finish(id, RunState::Cancelled, None, Some("The task that asked for this helper is gone.")); return done(self) };
        if self.is_stopped(&run.parent, lead.turns.len()) { self.finish(id, RunState::Cancelled, None, Some("The task that asked was stopped before this helper started.")); return done(self); }
        let Some(p) = self.env.providers().into_iter().find(|p| p.id == run.provider) else { self.finish(id, RunState::Failed, None, Some("The provider is no longer available.")); return done(self) };
        if !p.ready { self.finish(id, RunState::Failed, None, Some(&format!("{} isn’t available: {}", p.name, p.hint))); return done(self); }
        // A place first: nothing is made for a helper that has nowhere to run yet.
        if !self.sessions.can_start() { return done(self); }
        let read_only = run.access == "read";
        let cloud = lead.cloud.is_some();
        let prepared = if read_only || cloud { Ok(workspace::Prepared { folder: lead.folder.clone(), binding: lead.ext.workspace.clone(), note: None }) }
            else { workspace::prepare(&lead.folder, &Choice::Own { base: None }, run.role.as_deref().unwrap_or(&run.brief), &self.env.worktrees(), false, false, &Cancel::new()) };
        let prepared = match prepared { Ok(x) => x, Err(e) => { self.finish(id, RunState::Failed, None, Some(&format!("No workspace for the helper: {e}"))); return done(self); } };
        let mut note = run.note.clone();
        if !read_only && prepared.binding.as_ref().is_some_and(|b| !b.is_worktree()) {
            note = Some(format!("{} The helper shares the task’s folder.", prepared.note.clone().unwrap_or_default()).trim().to_owned());
        }
        let depth = run.depth;
        let link = OrchLink { delegation: p.leads && depth < self.env.limits().max_depth, run: Some(run.id.clone()), parent: Some(run.parent.clone()), root: Some(run.root.clone()), depth };
        let ext = SessionExt { workspace: prepared.binding.clone(), orch: Some(link), provider: p.instance.clone(), ..Default::default() };
        let prompt = brief_prompt(&run);
        match self.sessions.start_bound(p.tool, &prepared.folder, &prompt, vec![], Some(&run.access), None, ext) {
            Some(s) => {
                let mut g = self.st.lock().unwrap();
                if let Some(r) = g.runs.iter_mut().find(|r| r.id == id) {
                    r.state = RunState::Running;
                    r.session = Some(s.key.clone());
                    r.note = note;
                    r.attempts.push(Attempt { id: new_id("a"), session: s.key.clone(), thread: None, state: "running".into(), started: now_ms(), ended: None });
                }
                g.starting.remove(id);
                self.save(&g);
                drop(g);
                self.cv.notify_all();
                hover_core::log::line(&format!("orch: run {id} started on {}", p.id));
            }
            // No place, or the folder is held: it stays queued and the next change tries again.
            None => done(self),
        }
    }

    /// Settles a run that ended without a session result (or before it had a session).
    fn finish(&self, id: &str, state: RunState, result: Option<String>, note: Option<&str>) {
        let mut g = self.st.lock().unwrap();
        let Some(r) = g.runs.iter_mut().find(|r| r.id == id) else { return };
        if r.state.finished() { return; }
        r.state = state;
        r.result = result.map(|t| clip(&t));
        if let Some(n) = note { r.note = Some(n.to_owned()); }
        r.ended = Some(now_ms());
        self.save(&g);
        drop(g);
        self.cv.notify_all();
        self.after_run(id);
    }

    // MARK: Waiting, reading, cancelling

    fn authorized(&self, caller: &str, run: &Run) -> Result<(), String> {
        if run.parent == caller || run.root == caller { Ok(()) } else { Err("That helper belongs to another task.".into()) }
    }

    /// Waits up to `timeout` for a run to finish. Running out of time does not stop the helper: the answer
    /// says it is still working, and the lead may ask again. While it waits, the lead holds no place among the tasks that run.
    pub fn wait(&self, caller: &str, run_id: &str, timeout: Duration) -> Result<Info, String> {
        {
            let g = self.st.lock().unwrap();
            let r = g.runs.iter().find(|r| r.id == run_id).ok_or("There is no helper with that id.")?;
            self.authorized(caller, r)?;
        }
        self.lead(caller)?;
        let deadline = Instant::now() + timeout;
        self.sessions.park(caller, true);
        self.st.lock().unwrap().waiting.push((caller.to_owned(), run_id.to_owned()));
        let out = loop {
            let g = self.st.lock().unwrap();
            let r = g.runs.iter().find(|r| r.id == run_id).cloned();
            let Some(r) = r else { break Err("That helper is gone.".to_owned()) };
            if r.state.finished() { break Ok(info(&r)); }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { break Ok(info(&r)); }
            let (g2, _) = self.cv.wait_timeout(g, left.min(Duration::from_millis(200))).unwrap();
            drop(g2);
            if self.stopped_now(caller) { break Err("This task was stopped.".into()); }
        };
        {
            let mut g = self.st.lock().unwrap();
            if let Some(i) = g.waiting.iter().position(|(c, r)| c == caller && r == run_id) { g.waiting.remove(i); }
            // Read by the lead's own call: nothing more to send.
            if let Ok(i) = &out { if i.state.finished() { if let Some(r) = g.runs.iter_mut().find(|r| r.id == run_id) { if r.delivery == Delivery::Pending { r.delivery = Delivery::Taken; } } } }
            self.save(&g);
        }
        if !self.st.lock().unwrap().waiting.iter().any(|(c, _)| c == caller) { self.sessions.park(caller, false); }
        out
    }

    /// What a run has so far; a finished run's result counts as read.
    pub fn result(&self, caller: &str, run_id: &str) -> Result<Info, String> {
        let mut g = self.st.lock().unwrap();
        let r = g.runs.iter_mut().find(|r| r.id == run_id).ok_or("There is no helper with that id.")?;
        self.authorized(caller, r)?;
        if r.state.finished() && r.delivery == Delivery::Pending { r.delivery = Delivery::Taken; }
        let i = info(r);
        self.save(&g);
        Ok(i)
    }

    /// Cancels a helper and everything it started. A shared provider process is not ended for it.
    pub fn cancel(&self, caller: &str, run_id: &str) -> Result<Info, String> {
        {
            let g = self.st.lock().unwrap();
            let r = g.runs.iter().find(|r| r.id == run_id).ok_or("There is no helper with that id.")?;
            self.authorized(caller, r)?;
        }
        self.cancel_run(run_id);
        self.run_info(run_id).ok_or_else(|| "That helper is gone.".to_owned())
    }

    fn cancel_run(&self, id: &str) {
        let (session, queued) = {
            let g = self.st.lock().unwrap();
            let Some(r) = g.runs.iter().find(|r| r.id == id) else { return };
            (r.session.clone(), r.state == RunState::Queued && r.session.is_none())
        };
        if queued { self.finish(id, RunState::Cancelled, None, Some("Cancelled before it started.")); return; }
        if let Some(s) = session.and_then(|k| self.sessions.find(&k)) {
            // The helper's own stop hook reaches its helpers in turn.
            self.sessions.stop(s.id);
        }
    }

    // MARK: Ends and stops

    /// A session's turn ended.
    fn ended(&self, s: &KiroSession, r: &KiroResult) {
        let run = self.st.lock().unwrap().runs.iter().find(|x| x.session.as_deref() == Some(&s.key) && !x.state.finished()).map(|x| x.id.clone());
        if let Some(id) = run {
            let (state, note) = match r.state {
                KiroState::Completed => (RunState::Done, None),
                KiroState::Cancelled => (RunState::Cancelled, Some("The helper was stopped.")),
                _ => (RunState::Failed, Some("The helper couldn’t finish.")),
            };
            {
                let mut g = self.st.lock().unwrap();
                if let Some(x) = g.runs.iter_mut().find(|x| x.id == id) {
                    x.state = state;
                    x.result = Some(clip(&r.text));
                    if let Some(n) = note { x.note = Some(n.to_owned()); }
                    x.ended = Some(now_ms());
                    if let Some(a) = x.attempts.last_mut() { a.state = state.name().into(); a.ended = x.ended; a.thread = s.kiro_id.clone(); }
                }
                self.save(&g);
            }
            self.cv.notify_all();
            self.after_run(&id);
        }
        // A lead whose turn is over hears about results that came in while it was busy.
        if r.state == KiroState::Completed { self.deliver_to(&s.key); }
        // A place may have freed for a queued helper.
        if self.has_queued() { self.pump(); }
    }

    /// A run finished: tell its lead, if that is still right.
    fn after_run(&self, id: &str) {
        let Some(parent) = self.st.lock().unwrap().runs.iter().find(|r| r.id == id).map(|r| r.parent.clone()) else { return };
        if self.has_queued() { self.pump(); }
        self.deliver_to(&parent);
    }

    /// Sends the lead one message with every finished result it hasn't been told, if it isn't busy and
    /// wasn't stopped; marks them sent only when the message went. A result already in the lead's history
    /// (a send whose record was lost) is marked, not sent again.
    pub fn deliver_to(&self, lead_key: &str) {
        let pending: Vec<Run> = self.st.lock().unwrap().runs.iter().filter(|r| r.parent == lead_key && r.state.finished() && r.delivery == Delivery::Pending).cloned().collect();
        if pending.is_empty() { return; }
        let Some(lead) = self.sessions.find(lead_key).or_else(|| self.sessions.wake(lead_key)) else {
            // Gone for good (deleted): nobody to tell. A lead merely without a desk yet keeps its results waiting.
            if self.sessions.saved(lead_key).is_none() { self.mark(&pending, Delivery::Suppressed); }
            return;
        };
        let suppress = self.is_stopped(lead_key, lead.turns.len()) || lead.state == KiroState::Cancelled && !lead.busy();
        if suppress { self.mark(&pending, Delivery::Suppressed); return; }
        if lead.busy() { return; }
        if lead.state != KiroState::Completed { self.mark(&pending, Delivery::Suppressed); return; }
        let (seen, fresh): (Vec<&Run>, Vec<&Run>) = pending.iter().partition(|r| lead.turns.iter().any(|t| t.prompt.contains(&marker(&r.id))));
        self.mark(&seen.into_iter().cloned().collect::<Vec<_>>(), Delivery::Sent);
        if fresh.is_empty() { return; }
        let text = fresh.iter().map(|r| report(r)).collect::<Vec<_>>().join("\n\n---\n\n");
        if self.sessions.reply(lead.id, &text, vec![]) { self.mark(&fresh.into_iter().cloned().collect::<Vec<_>>(), Delivery::Sent); }
    }

    fn mark(&self, runs: &[Run], d: Delivery) {
        if runs.is_empty() { return; }
        let mut g = self.st.lock().unwrap();
        for r in g.runs.iter_mut().filter(|x| runs.iter().any(|y| y.id == x.id) && x.delivery == Delivery::Pending) { r.delivery = d; }
        self.save(&g);
    }

    /// A session was stopped (or deleted): it stays stopped until a newer turn begins; its helpers, their
    /// helpers and the threads it started are stopped; nothing they report later wakes it.
    fn stopped(&self, s: &KiroSession) {
        let mut keys = vec![s.key.clone()];
        let mut todo = vec![s.key.clone()];
        let mut runs = vec![];
        {
            let mut g = self.st.lock().unwrap();
            while let Some(k) = todo.pop() {
                let turns = self.sessions.find(&k).map_or(usize::MAX, |x| x.turns.len());
                g.stopped.retain(|(x, _)| x != &k);
                g.stopped.push((k.clone(), turns));
                let kids: Vec<Run> = g.runs.iter().filter(|r| r.parent == k && !r.state.finished()).cloned().collect();
                let owned: Vec<String> = g.threads.iter().filter(|t| t.owner == k).map(|t| t.session.clone()).collect();
                for r in kids { if let Some(cs) = &r.session { todo.push(cs.clone()); keys.push(cs.clone()); } runs.push(r.id); }
                for t in owned { todo.push(t.clone()); keys.push(t); }
            }
            self.save(&g);
        }
        for id in &runs { self.cancel_run(id); }
        // Threads the lead started are ordinary sessions: stopping them is the same call.
        for k in keys.iter().skip(1) { if let Some(x) = self.sessions.find(k) { if x.busy() { self.sessions.stop(x.id); } } }
        let ls = self.listeners.lock().unwrap().clone();
        for k in &keys { for f in &ls { f(k); } }
        self.cv.notify_all();
    }

    // MARK: What the desk shows

    /// The helpers a session started, oldest first: who, on what, in what state, with what result.
    pub fn helpers_of(&self, key: &str) -> Vec<Info> { self.st.lock().unwrap().runs.iter().filter(|r| r.parent == key).map(info).collect() }

    /// Helpers still working for a lead (queued or running): what "waiting on 2 helpers" counts.
    pub fn pending_for(&self, key: &str) -> usize { self.st.lock().unwrap().runs.iter().filter(|r| r.parent == key && !r.state.finished()).count() }

    /// The run a helper session is, if it is one.
    pub fn run_of(&self, session_key: &str) -> Option<Info> { self.st.lock().unwrap().runs.iter().find(|r| r.session.as_deref() == Some(session_key)).map(info) }

    /// Switches delegation on or off for a task (a lead: depth 0). Allowed while it runs or not.
    pub fn enable(&self, key: &str, on: bool) -> bool {
        self.sessions.update_ext(key, |e| {
            let l = e.orch.get_or_insert_with(OrchLink::default);
            l.delegation = on;
        })
    }

    // MARK: Threads a lead launches

    pub fn thread_launch(&self, caller: &str, provider: &str, prompt: &str, access: Option<&str>) -> Result<String, String> {
        let (lead, link) = self.lead(caller)?;
        if prompt.trim().is_empty() { return Err("A thread needs a first message.".into()); }
        let limits = self.env.limits();
        let root = link.root.clone().unwrap_or_else(|| caller.to_owned());
        let providers = self.env.providers();
        let p = providers.iter().find(|p| p.id == provider).ok_or_else(|| format!("There is no provider called “{provider}”."))?;
        if !p.ready { return Err(format!("{} isn’t available: {}", p.name, p.hint)); }
        {
            let g = self.st.lock().unwrap();
            if g.runs.iter().filter(|r| r.root == root).count() + g.threads.iter().filter(|t| t.owner == root).count() >= limits.max_helpers as usize { return Err("This task has used its helpers and threads.".into()); }
        }
        let access = narrow(&self.env.access_of(&lead), access);
        let ext = SessionExt { workspace: lead.ext.workspace.clone(), orch: Some(OrchLink { delegation: false, run: None, parent: Some(caller.to_owned()), root: Some(root.clone()), depth: link.depth + 1 }), provider: p.instance.clone(), ..Default::default() };
        let s = self.sessions.start_bound(p.tool, &lead.folder, prompt, vec![], Some(&access), None, ext).ok_or("No place is free to start a thread now. Try again when a task is done.")?;
        let id = new_id("t");
        let mut g = self.st.lock().unwrap();
        g.threads.push(Thread { id: id.clone(), owner: root, session: s.key });
        self.save(&g);
        Ok(id)
    }

    fn owned_thread(&self, caller: &str, id: &str) -> Result<KiroSession, String> {
        let (_, link) = self.lead(caller)?;
        let root = link.root.unwrap_or_else(|| caller.to_owned());
        let t = self.st.lock().unwrap().threads.iter().find(|t| t.id == id).cloned().ok_or("There is no thread with that id.")?;
        if t.owner != root { return Err("That thread belongs to another task. Having its id does not let you read or change it.".into()); }
        self.sessions.find(&t.session).ok_or_else(|| "That thread isn’t open any more.".to_owned())
    }

    /// The thread's turns from `from`, each prompt and answer, up to `max` characters in all.
    pub fn thread_read(&self, caller: &str, id: &str, from: usize, max: usize) -> Result<(String, usize, bool), String> {
        let s = self.owned_thread(caller, id)?;
        let mut out = String::new();
        let mut next = from;
        for (i, t) in s.turns.iter().enumerate().skip(from) {
            let piece = format!("[{i}] You: {}\n{}\n\n", t.prompt, t.result.as_ref().map_or("(working…)".to_owned(), |r| format!("Thread: {}", r.text)));
            if !out.is_empty() && out.chars().count() + piece.chars().count() > max { return Ok((out, next, true)); }
            out += &piece;
            next = i + 1;
        }
        Ok((out, next, false))
    }

    pub fn thread_send(&self, caller: &str, id: &str, text: &str) -> Result<(), String> {
        let s = self.owned_thread(caller, id)?;
        if self.sessions.reply(s.id, text, vec![]) { Ok(()) } else { Err("The thread couldn’t take that message now (no place is free, or it is empty).".into()) }
    }

    pub fn thread_interrupt(&self, caller: &str, id: &str) -> Result<(), String> {
        let s = self.owned_thread(caller, id)?;
        if s.busy() { self.sessions.stop(s.id); Ok(()) } else { Err("That thread isn’t working.".into()) }
    }
}

fn clip(t: &str) -> String { if t.chars().count() <= KEEP { t.to_owned() } else { t.chars().take(KEEP).collect() } }

/// The marker that lets Hover see a result was already sent.
fn marker(run: &str) -> String { format!("hover-run:{run}") }

/// The helper’s first message: the brief, the role, and how its answer is used. Nothing of the lead’s conversation.
fn brief_prompt(r: &Run) -> String {
    let role = r.role.as_deref().map(|x| format!("Your role: {x}.\n")).unwrap_or_default();
    format!("[Hover helper task] Another agent asked you to help with one job.\n{role}Access: {}.\n\n{}\n\nWhen you are done, write what you did and what you found as your final message: it is handed back to the agent that asked.", r.access, r.brief.trim())
}

/// A finished run as a message to its lead.
fn report(r: &Run) -> String {
    let who = format!("{}{}", r.provider, r.role.as_deref().map(|x| format!(", {x}")).unwrap_or_default());
    let body = r.result.as_deref().unwrap_or("(no result)");
    let shown: String = body.chars().take(8000).collect();
    let more = if body.chars().count() > 8000 { format!("\n(Cut at 8000 characters. Fetch the rest with task_result, run_id {}, offset 8000.)", r.id) } else { String::new() };
    format!("[Hover] A helper finished ({marker}; {who}; {state}).{note}\n\n{shown}{more}", marker = marker(&r.id), state = r.state.name(), note = r.note.as_deref().map(|n| format!(" {n}")).unwrap_or_default())
}

// MARK: The MCP server

fn text_prop(desc: &str) -> Json { Json::obj(vec![("type", Json::str("string")), ("description", Json::str(desc))]) }
fn int_prop(desc: &str) -> Json { Json::obj(vec![("type", Json::str("integer")), ("description", Json::str(desc))]) }

fn tool(name: &str, description: &str, props: Vec<(&str, Json)>, required: &[&str]) -> Json {
    Json::obj(vec![("name", Json::str(name)), ("description", Json::str(description)),
        ("inputSchema", Json::obj(vec![("type", Json::str("object")), ("properties", Json::obj(props)), ("required", Json::Arr(required.iter().map(|r| Json::str(*r)).collect())), ("additionalProperties", Json::Bool(false))]))])
}

pub const INSTRUCTIONS: &str = "Hover lets you ask other coding agents for help. Call list_providers to see who is available, delegate_task with a clear \
brief (they do not see this conversation), then wait_for_task or task_result for the answer. A helper has your access or less. Waiting that times out does not \
stop the helper: ask again. Give each delegate_task a request_id so a retry never starts a second helper.";

pub fn tools() -> Json {
    Json::Arr(vec![
        tool("list_providers", "Which agents can be asked for help, whether each is ready, and what it can do.", vec![], &[]),
        tool("delegate_task", "Ask another agent to do one job. Returns a run_id at once; the helper works on its own.",
            vec![("provider", text_prop("A provider id from list_providers.")), ("brief", text_prop("What to do, with everything the helper needs. It cannot see your conversation.")),
                ("role", text_prop("Optional role, for example reviewer.")), ("access", text_prop("Optional: read, always, risky or full. Never more than you have.")),
                ("request_id", text_prop("Your own id for this request; the same id again returns the same helper."))], &["provider", "brief"]),
        tool("wait_for_task", "Wait for a helper to finish, up to timeout_secs (default 30, at most 120). If it is still working, say so; it keeps working.",
            vec![("run_id", text_prop("The run_id from delegate_task.")), ("timeout_secs", int_prop("How long to wait.")), ("offset", int_prop("Where in a long result to start."))], &["run_id"]),
        tool("task_result", "What a helper has reported so far, without waiting. A long result is read in pages with offset.",
            vec![("run_id", text_prop("The run_id.")), ("offset", int_prop("Where to start."))], &["run_id"]),
        tool("cancel_task", "Stop a helper and anything it started.", vec![("run_id", text_prop("The run_id."))], &["run_id"]),
        tool("launch_thread", "Start an ordinary Hover thread on a provider with a first message. You can read it, message it and interrupt it; nothing is handed back on its own.",
            vec![("provider", text_prop("A provider id.")), ("prompt", text_prop("The first message.")), ("access", text_prop("Optional, never more than you have."))], &["provider", "prompt"]),
        tool("read_thread", "Read a thread you launched, from turn `from`, in bounded pages.", vec![("thread_id", text_prop("The thread id.")), ("from", int_prop("First turn.")), ("max_chars", int_prop("Default 20000."))], &["thread_id"]),
        tool("send_to_thread", "Send a message to a thread you launched.", vec![("thread_id", text_prop("The thread id.")), ("text", text_prop("The message."))], &["thread_id", "text"]),
        tool("interrupt_thread", "Stop what a thread you launched is doing.", vec![("thread_id", text_prop("The thread id."))], &["thread_id"]),
    ])
}

fn reply_ok(id: &Json, result: Json) -> Json { Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("result", result)]) }
fn reply_err(id: &Json, code: i64, m: &str) -> Json { Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("error", Json::obj(vec![("code", Json::int(code)), ("message", Json::str(m))]))]) }
fn said(text: &str, error: bool) -> Json { Json::obj(vec![("content", Json::Arr(vec![Json::obj(vec![("type", Json::str("text")), ("text", Json::str(text))])])), ("isError", Json::Bool(error))]) }

fn arg_s(a: &Json, k: &str) -> Option<String> { a.get(k).and_then(Json::as_str).map(str::to_owned).filter(|s| !s.is_empty()) }
fn arg_n(a: &Json, k: &str) -> Option<usize> { a.get(k).and_then(|v| v.i64().ok()).map(|n| n.max(0) as usize) }

/// One run as text for the lead: its state, note, and a page of its result.
fn describe(i: &Info, offset: usize) -> String {
    let mut s = format!("run_id: {}\nstate: {}\nprovider: {}{}\naccess: {}", i.run, i.state.name(), i.provider, i.role.as_deref().map(|r| format!(" ({r})")).unwrap_or_default(), i.access);
    if let Some(n) = &i.note { s += &format!("\nnote: {n}"); }
    match (&i.result, i.state.finished()) {
        (Some(r), true) => {
            let total = r.chars().count();
            let page: String = r.chars().skip(offset).take(PAGE).collect();
            s += &format!("\nresult_chars: {total}\n\n{page}");
            if offset + PAGE < total { s += &format!("\n\n(More: call task_result with offset {}.)", offset + PAGE); }
        }
        (_, true) => s += "\n\n(no result)",
        _ => s += "\n\nStill working. It keeps working; call wait_for_task or task_result again.",
    }
    s
}

impl Orch {
    /// One MCP message from the lead whose key is `caller`: the reply, or none for a notification. A call
    /// that waits blocks here, so the server runs each message on a thread of its own.
    pub fn answer(&self, caller: &str, m: &Json) -> Option<Json> {
        let id = m.get("id")?;
        let params = m.get("params");
        match m.get("method").and_then(Json::as_str) {
            Some("initialize") => Some(reply_ok(id, Json::obj(vec![
                ("protocolVersion", Json::str(params.and_then(|p| p.get("protocolVersion")).and_then(Json::as_str).unwrap_or("2025-06-18"))),
                ("capabilities", Json::obj(vec![("tools", Json::obj(vec![("listChanged", Json::Bool(false))]))])),
                ("serverInfo", Json::obj(vec![("name", Json::str(SERVER_NAME)), ("title", Json::str("Hover helpers")), ("version", Json::str("1.0"))])),
                ("instructions", Json::str(INSTRUCTIONS))]))),
            Some("ping") => Some(reply_ok(id, Json::obj(vec![]))),
            Some("tools/list") => Some(reply_ok(id, Json::obj(vec![("tools", tools())]))),
            Some("tools/call") => {
                let name = params.and_then(|p| p.get("name")).and_then(Json::as_str).unwrap_or("");
                let args = match params.and_then(|p| p.get("arguments")) { Some(a @ Json::Obj(_)) => a.clone(), _ => Json::obj(vec![]) };
                if !matches!(tools(), Json::Arr(all) if all.iter().any(|t| t.get("name").and_then(Json::as_str) == Some(name))) { return Some(reply_err(id, -32602, &format!("Unknown tool {name}."))); }
                Some(reply_ok(id, match self.call(caller, name, &args) { Ok(t) => said(&t, false), Err(e) => said(&e, true) }))
            }
            other => Some(reply_err(id, -32601, &format!("Method {} isn’t supported.", other.unwrap_or("")))),
        }
    }

    fn call(&self, caller: &str, name: &str, a: &Json) -> Result<String, String> {
        let need = |k: &str| arg_s(a, k).ok_or_else(|| format!("{k} is needed."));
        match name {
            "list_providers" => {
                self.lead(caller)?;
                Ok(self.env.providers().iter().map(|p| format!("{} — {}: {}{}{}", p.id, p.name, if p.ready { "ready".to_owned() } else { format!("not ready ({})", p.hint) },
                    if p.read_only { "; can run read-only" } else { "" }, if p.resume { "; can resume" } else { "" })).collect::<Vec<_>>().join("\n"))
            }
            "delegate_task" => {
                let i = self.delegate(caller, Delegate { provider: need("provider")?, brief: need("brief")?, role: arg_s(a, "role"), access: arg_s(a, "access"), request: arg_s(a, "request_id") })?;
                Ok(describe(&i, 0))
            }
            "wait_for_task" => {
                let secs = arg_n(a, "timeout_secs").unwrap_or(30).clamp(1, 120) as u64;
                Ok(describe(&self.wait(caller, &need("run_id")?, Duration::from_secs(secs))?, arg_n(a, "offset").unwrap_or(0)))
            }
            "task_result" => Ok(describe(&self.result(caller, &need("run_id")?)?, arg_n(a, "offset").unwrap_or(0))),
            "cancel_task" => Ok(describe(&self.cancel(caller, &need("run_id")?)?, 0)),
            "launch_thread" => Ok(format!("thread_id: {}", self.thread_launch(caller, &need("provider")?, &need("prompt")?, arg_s(a, "access").as_deref())?)),
            "read_thread" => {
                let (text, next, more) = self.thread_read(caller, &need("thread_id")?, arg_n(a, "from").unwrap_or(0), arg_n(a, "max_chars").unwrap_or(PAGE).clamp(200, 100_000))?;
                Ok(format!("{text}next_from: {next}{}", if more { "\n(More turns: call read_thread again with from = next_from.)" } else { "" }))
            }
            "send_to_thread" => { self.thread_send(caller, &need("thread_id")?, &need("text")?)?; Ok("Sent.".into()) }
            "interrupt_thread" => { self.thread_interrupt(caller, &need("thread_id")?)?; Ok("Interrupt sent.".into()) }
            _ => Err("Unknown tool.".into()),
        }
    }
}

/// Serves one lead's connection: lines of JSON-RPC in, replies out, each call on its own thread.
fn serve(orch: Weak<Orch>, name: &str, reader: Box<dyn std::io::BufRead + Send>, writer: Box<dyn std::io::Write + Send>) {
    let Some(key) = name.strip_prefix("orch:").map(str::to_owned) else { return };
    let writer = Arc::new(Mutex::new(writer));
    for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
        let Ok(m) = json::parse(&line) else { continue };
        let (orch, key, writer) = (orch.clone(), key.clone(), writer.clone());
        let _ = std::thread::Builder::new().name("orch-answer".into()).spawn(move || {
            let Some(o) = orch.upgrade() else { return };
            if let Some(reply) = o.answer(&key, &m) {
                let mut bytes = reply.compact().into_bytes();
                bytes.push(b'\n');
                let mut w = writer.lock().unwrap();
                use std::io::Write;
                let _ = w.write_all(&bytes).and_then(|_| w.flush());
            }
        });
    }
}

/// Helpers’ MCP server for the session `tag` (its key), when its task has delegation on and the host can
/// serve it (a Unix socket); none otherwise. Handed to the agent with its other MCP servers.
pub fn servers(tag: Option<&str>) -> Vec<McpServer> {
    let Some(tag) = tag.filter(|t| !t.is_empty()) else { return vec![] };
    let Some(o) = GLOBAL.get().and_then(Weak::upgrade) else { return vec![] };
    if !o.sessions.find(tag).is_some_and(|s| s.ext.orch.as_ref().is_some_and(|l| l.delegation)) { return vec![]; }
    let w = Arc::downgrade(&o);
    browser::bridge(&format!("orch:{tag}"), SERVER_NAME, Arc::new(move |name, r, wr| serve(w.clone(), name, r, wr)))
}

/// The agent’s MCP server for helpers can be offered on this computer.
pub fn mcp_supported() -> bool { cfg!(unix) }
