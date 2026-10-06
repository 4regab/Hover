//! Owl/KiroSession.cs: a session (a folder, the first prompt and every reply, each a
//! turn) and all of them (KiroSessions: at most three running across the tools, the
//! newest six kept at desks, every one in the history until deleted). The C# lives on
//! the UI thread; here the sessions sit behind one lock, runs go on threads of their
//! own, and `changed` and `ended` are raised with the lock released, off any thread.

use crate::ask::{AgentAsk, Answers, AskAnswer};
use crate::cancel::{Cancel, Registration};
use crate::checkpoint::Checkpoints;
use crate::stream::{KiroEvent, KiroPhase, KiroResult};
use hover_core::ext::{Chip, Fork, Handoff, Lineage, Native, Returned, SessionExt, TurnExt};
use hover_core::history::{AgentHistory, SavedSession, SavedTurn};
use hover_core::model::{AgentTool, KiroState, KiroStep};
use hover_core::time::Stamp;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// What a turn that was running when Hover closed reads as, once the session is brought back.
pub const CLOSED_TEXT: &str = "Stopped when Hover closed.";
/// What a Kiro Web session's last turn reads as when it was opened while still working there,
/// until Hover has followed it to its end (KiroSessions::adopt_cloud).
pub const STILL_WORKING_TEXT: &str = "Kiro Web was still working on this when Hover opened it.";

pub const MAX_RUNNING: usize = 3;
pub const MAX_KEPT: usize = 6;

/// Where KiroSessions::rewind puts a chat back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rewind {
    /// To just after the answer to turn N: the folder as that turn left it, the turns after it gone.
    After(usize),
    /// To just before turn N, which is sent again at once: the folder as it was before it ran.
    Before(usize),
}

/// What a run gets: KiroSession.RunTask's arguments.
pub struct RunArgs {
    pub folder: String,
    pub prompt: String,
    pub progress: Box<dyn Fn(KiroPhase) + Send + Sync>,
    pub ct: Cancel,
    pub resume: Option<String>,
    pub events: Box<dyn Fn(KiroEvent) + Send + Sync>,
    /// The session's own tool access (AgentOptions::with_access); None keeps the tool's.
    pub access: Option<String>,
    /// The session's key: what the agent browser's server is tagged with, so its calls
    /// reach this session's browser. None for a run that is no session's (voice's routing).
    pub tag: Option<String>,
    /// A Kiro Web session's repos (KiroSession::cloud); None runs on this computer.
    pub cloud: Option<Vec<String>>,
}

/// Runs one turn and blocks until it ends; a panic reads as a failure.
pub type RunTask = Arc<dyn Fn(RunArgs) -> KiroResult + Send + Sync>;

/// One prompt and what came of it.
#[derive(Clone, Debug, PartialEq)]
pub struct KiroTurn {
    pub prompt: String,
    /// Pictures pasted with the prompt, as files the agent can read.
    pub images: Vec<String>,
    pub steps: Vec<KiroStep>,
    pub result: Option<KiroResult>,
    /// Sent while the turn before still ran; it starts when that one ends.
    pub queued: bool,
    pub started_at: Stamp,
    /// When the agent first did something other than start up.
    pub woke_at: Option<Stamp>,
    pub ended_at: Option<Stamp>,
    /// What the turn cost, in the tool's credits, when it says (Kiro does).
    pub credits: Option<f64>,
    /// The project folder's checkpoints (checkpoint.rs) from before the turn ran and from after it;
    /// None where none could be taken (no git, a folder too broad, too slow).
    pub before: Option<String>,
    pub after: Option<String>,
    /// Names this message in queue edits, so an edit, a move or a send-now reaches that message and no other.
    pub uid: String,
    /// What was attached besides words (context.rs), kept with the message in drafts, the queue and the history.
    pub chips: Vec<Chip>,
    /// A provider switch asked for with this message: it happens when the message is sent, not before.
    pub switch_to: Option<String>,
}

impl KiroTurn {
    pub fn new(prompt: &str, images: Vec<String>) -> KiroTurn {
        KiroTurn { prompt: prompt.into(), images, steps: vec![], result: None, queued: false, started_at: Stamp::DEFAULT, woke_at: None, ended_at: None, credits: None, before: None, after: None,
            uid: hover_core::guid_n(), chips: vec![], switch_to: None }
    }

    /// What the agent is sent: the prompt, then what is attached (context.rs), then the pictures' paths
    /// for it to look at. Kiro gets the pictures themselves from these lines (acp.rs, ATTACHED).
    pub fn text(&self) -> String {
        let head = if self.prompt.is_empty() && !self.images.is_empty() { "Look at the attached image.".to_owned() } else { self.prompt.clone() };
        let mut out = head;
        if !self.chips.is_empty() { out = format!("{out}\n\n{}", crate::context::render(&self.chips)); }
        if !self.images.is_empty() { out = format!("{out}\n\n{}", self.images.iter().map(|p| format!("{}{p}", crate::acp::ATTACHED)).collect::<Vec<_>>().join("\n")); }
        out
    }
}

static IDS: AtomicI32 = AtomicI32::new(0);

/// A session as it stands: a copy, for views to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct KiroSession {
    pub id: i32,
    pub tool: AgentTool,
    pub state: KiroState,
    pub phase: KiroPhase,
    pub folder: String,
    /// Oldest first; queued replies at the end.
    pub turns: Vec<KiroTurn>,
    /// The tool's id for the conversation, once the first turn has told it.
    pub kiro_id: Option<String>,
    pub context: Option<f64>,
    pub seat: usize,
    pub bot: usize,
    /// The session's lasting name, in the history.
    pub key: String,
    pub deleted: bool,
    /// The tool access picked when the session started; None keeps the tool's setting.
    pub access: Option<String>,
    /// Runs in Kiro's cloud (Kiro Web): the GitHub repos it was given, empty for an empty
    /// workspace. None runs on this computer.
    pub cloud: Option<Vec<String>>,
    /// Where the task works (a worktree of its own, or the folder itself) and the links orchestration adds.
    pub ext: SessionExt,
    /// The replies waiting are held: a stop or a restart leaves them, and only `resume_queue` or a new
    /// message from the user sends them.
    pub held: bool,
    /// What the agent is waiting on the user for, oldest first.
    pub asks: Vec<AgentAsk>,
    /// Asked to stop or pause; the turn hasn't ended yet (the tool hasn't said).
    pub stopping: bool,
    /// Goes up with every change to the session: a view that drew it at this number
    /// needn't copy or lay it out again.
    pub rev: u64,
}

/// String.Split('\n', RemoveEmptyEntries | TrimEntries).FirstOrDefault().
pub(crate) fn first_line(s: &str) -> &str { s.split('\n').map(str::trim).find(|l| !l.is_empty()).unwrap_or("") }

impl KiroSession {
    /// A new session with no turns (the C# constructor).
    pub fn new(tool: AgentTool) -> KiroSession {
        KiroSession { id: IDS.fetch_add(1, Ordering::SeqCst) + 1, tool, state: KiroState::Idle, phase: KiroPhase::Starting, folder: String::new(), turns: vec![],
            kiro_id: None, context: None, seat: 0, bot: 0, key: hover_core::guid_n(), deleted: false, access: None, cloud: None, ext: SessionExt::default(), held: false, asks: vec![], stopping: false, rev: 0 }
    }

    /// A copy without what only the chat reads: the answers' text, and the steps'
    /// changes and output. What the notch, the desks and the panels draw is all there.
    pub fn light(&self) -> KiroSession {
        KiroSession {
            turns: self.turns.iter().map(|t| KiroTurn {
                prompt: t.prompt.clone(), images: t.images.clone(),
                steps: t.steps.iter().map(|x| KiroStep { id: x.id.clone(), kind: x.kind.clone(), title: x.title.clone(), target: x.target.clone(), status: x.status.clone(),
                    added: x.added, removed: x.removed, diff: None, output: None, exit: x.exit, ms: x.ms,
                    // Only a subagent's input (its name is in it); the rest is for the desk's panels, which read the whole session.
                    input: x.input.clone().filter(|_| crate::state::is_subagent(x)), log: None }).collect(),
                result: t.result.as_ref().map(|r| KiroResult { state: r.state, text: String::new(), exit_code: r.exit_code, unconfirmed: r.unconfirmed }),
                queued: t.queued, started_at: t.started_at, woke_at: t.woke_at, ended_at: t.ended_at, credits: t.credits,
                before: t.before.clone(), after: t.after.clone(), uid: t.uid.clone(), chips: t.chips.clone(), switch_to: t.switch_to.clone(),
            }).collect(),
            folder: self.folder.clone(), kiro_id: self.kiro_id.clone(), key: self.key.clone(), access: self.access.clone(), cloud: self.cloud.clone(), ext: self.ext.clone(), asks: self.asks.clone(),
            ..*self
        }
    }

    pub fn busy(&self) -> bool { self.state == KiroState::Running }
    /// The question in front: the oldest one waiting.
    pub fn asking(&self) -> Option<&AgentAsk> { self.asks.first() }
    pub fn waiting(&self) -> bool { !self.asks.is_empty() }
    /// The turn running now, or the last one that ran.
    pub fn current(&self) -> Option<&KiroTurn> { self.turns.iter().rev().find(|t| !t.queued) }
    pub fn prompt(&self) -> &str { self.turns.first().map_or("", |t| &t.prompt) }
    pub fn result(&self) -> Option<&KiroResult> { self.current().and_then(|t| t.result.as_ref()) }

    /// The prompt's first line, short enough for a label.
    pub fn title(&self) -> String { crate::stream::clip_to(first_line(self.prompt()), 60) }

    /// The session as the history keeps it.
    pub fn snapshot(&self, now: Stamp) -> SavedSession {
        SavedSession {
            key: self.key.clone(), tool: self.tool, folder: self.folder.clone(), title: self.title(), acp_id: self.kiro_id.clone(), context: self.context,
            turns: self.turns.iter().map(|t| SavedTurn { prompt: t.prompt.clone(), images: t.images.clone(), steps: t.steps.clone(),
                state: t.result.as_ref().map(|r| r.state), text: t.result.as_ref().map(|r| r.text.clone()), started_at: t.started_at, woke_at: t.woke_at,
                ended_at: t.ended_at, credits: t.credits, before: t.before.clone(), after: t.after.clone(),
                ext: TurnExt { queued: t.queued, uid: t.queued.then(|| t.uid.clone()), chips: t.chips.clone(), switch_to: t.switch_to.clone() } }).collect(),
            updated: now,
            access: self.access.clone(),
            cloud: self.cloud.clone(),
            ext: self.ext.clone(),
        }
    }

    /// A new session carrying on a saved one; a turn cut short by Hover closing reads
    /// as stopped.
    pub fn restore(&mut self, s: &SavedSession) {
        if self.state != KiroState::Idle || !self.turns.is_empty() { return; }
        self.key = s.key.clone();
        self.tool = s.tool;
        self.folder = s.folder.clone();
        self.kiro_id = s.acp_id.clone();
        self.context = s.context;
        self.access = s.access.clone();
        self.cloud = s.cloud.clone();
        self.ext = s.ext.clone();
        for t in &s.turns {
            let mut turn = KiroTurn::new(&t.prompt, t.images.clone());
            turn.started_at = t.started_at;
            turn.woke_at = t.woke_at;
            turn.ended_at = Some(t.ended_at.unwrap_or(t.started_at));
            turn.credits = t.credits;
            turn.before = t.before.clone();
            turn.after = t.after.clone();
            turn.steps = t.steps.clone();
            turn.chips = t.ext.chips.clone();
            turn.switch_to = t.ext.switch_to.clone();
            if t.ext.queued {
                // A reply that was waiting when Hover closed is still waiting, and held: a saved message alone
                // does not start a task again.
                turn.queued = true;
                turn.uid = t.ext.uid.clone().unwrap_or(turn.uid);
                turn.ended_at = None;
                self.held = true;
            } else {
                turn.result = Some(KiroResult::new(t.state.unwrap_or(KiroState::Cancelled), t.text.clone().unwrap_or_else(|| CLOSED_TEXT.into())));
            }
            self.turns.push(turn);
        }
        self.state = self.turns.iter().rev().find_map(|t| t.result.as_ref()).map_or(KiroState::Cancelled, |r| r.state);
    }
}

fn usable(text: &str, images: &[String]) -> bool { !text.trim().is_empty() || !images.is_empty() }

/// A provider a conversation can move to: a built-in tool, or one of the user's custom agents.
#[derive(Clone, Debug, PartialEq)]
pub struct Target { pub id: String, pub tool: AgentTool, pub instance: Option<String> }

impl Target {
    /// `kiro`, `codex`, … or `custom:<id>`. Nothing is looked up: whether it is ready is the caller's to know.
    pub fn parse(id: &str) -> Option<Target> {
        if let Some(c) = id.strip_prefix("custom:").filter(|c| !c.is_empty()) { return Some(Target { id: id.into(), tool: AgentTool::Custom, instance: Some(c.into()) }); }
        AgentTool::parse(Some(id)).map(|t| Target { id: id.into(), tool: t, instance: None })
    }
}

/// The provider a session is with now, as `Target::parse` names it.
pub fn provider_id(s: &KiroSession) -> String {
    match (&s.tool, &s.ext.provider) { (AgentTool::Custom, Some(c)) => format!("custom:{c}"), (t, _) => t.id().to_owned() }
}

/// What a provider switch did.
#[derive(Clone, Debug, PartialEq)]
pub struct Switched {
    /// `native` (the provider’s own conversation, resumed), `portable` (a new one, started from an account of this) or `fresh`
    /// (nothing had been said yet).
    pub mode: &'static str,
    pub carried: usize,
    pub omitted: usize,
    pub notes: Vec<String>,
}

/// A message to an agent: its words, pictures and chips (context.rs), and a provider switch asked for with it
/// (applied when it is sent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Msg { pub text: String, pub images: Vec<String>, pub chips: Vec<Chip>, pub switch_to: Option<String> }

impl Msg {
    pub fn text(text: &str) -> Msg { Msg { text: text.into(), ..Default::default() } }
    fn ok(&self) -> bool { usable(&self.text, &self.images) || !self.chips.is_empty() }
}

/// Why a queue edit didn't happen.
#[derive(Clone, Debug, PartialEq)]
pub enum QueueError {
    /// No such session or message.
    Gone,
    /// The message began to send while it was being edited. The text the edit carried comes back, so it can go
    /// into the composer instead of being lost.
    Started(Msg),
    Invalid(String),
}

/// What send-now did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendNow {
    /// Nothing was running: the message started.
    Started,
    /// A run was going: it is being stopped (through the tool, not by force), and the message goes once the tool confirms.
    Steering,
}

type Reply = Box<dyn FnOnce(AskAnswer) + Send>;
type QuestionReply = Box<dyn FnOnce(Answers) + Send>;

/// Where an answer goes: a tool call's Allow or Deny, or a question's picks.
enum Answer { Call(Reply), Question(QuestionReply) }

impl Answer {
    /// Turned down (or withdrawn): Deny, or no answers.
    fn deny(self) { match self { Answer::Call(f) => f(AskAnswer::Deny), Answer::Question(f) => f(None) } }
}

/// A question waiting: where its answer goes, and the stop that withdraws it.
struct Pending { id: String, reply: Answer, _stop: Option<Registration> }

/// pausing: the turn was cancelled by Pause, so the replies queued behind it go once it ends.
/// usage: Kiro's last reported context (percent) that no compaction has answered yet.
/// parked: the run is waiting on its helpers (orch.rs), so it holds no place among the tasks that run at once.
struct Slot { s: KiroSession, cancel: Option<Cancel>, run: RunTask, asks: Vec<Pending>, pausing: bool, note: Option<String>, usage: Option<f64>, parked: bool }

impl Slot {
    fn new(s: KiroSession, run: RunTask) -> Slot {
        let usage = s.context.filter(|_| s.tool == AgentTool::Kiro);
        Slot { s, cancel: None, run, asks: vec![], pausing: false, note: None, usage, parked: false }
    }

    /// Runs and takes a place among the tasks that run at once.
    fn counts(&self) -> bool { self.s.busy() && !self.parked }

    /// KiroSession.DenyAll: every question it left has nobody to answer it now. The
    /// replies go once the lock is released.
    fn deny_all(&mut self) -> Vec<Answer> {
        self.s.asks.clear();
        self.s.rev += 1;
        self.asks.drain(..).map(|p| p.reply).collect()
    }
}

struct Inner { all: Vec<Slot>, selected: Option<i32> }

type Changed = Arc<dyn Fn() + Send + Sync>;
type Ended = Arc<dyn Fn(&KiroSession, &KiroResult) + Send + Sync>;
type Stopped = Arc<dyn Fn(&KiroSession) + Send + Sync>;
type CustomRunner = Arc<dyn Fn(&str) -> Option<RunTask> + Send + Sync>;

struct Shared {
    inner: Mutex<Inner>,
    make: Box<dyn Fn(AgentTool) -> RunTask + Send + Sync>,
    history: Option<Arc<AgentHistory>>,
    now: Box<dyn Fn() -> Stamp + Send + Sync>,
    changed: Mutex<Vec<Changed>>,
    ended: Mutex<Vec<Ended>>,
    checkpoints: Mutex<Option<Arc<Checkpoints>>>,
    /// Called when a run is asked to stop (Stop, Pause, delete, quit).
    stops: Mutex<Vec<Stopped>>,
    /// The runner of a custom agent, by its id (custom.rs). Without one, a custom conversation says its agent isn't set up.
    custom: Mutex<Option<CustomRunner>>,
    /// Where auto compact's percent comes from (None while it is off); without one, settings.json.
    compact: Mutex<Option<Arc<dyn Fn() -> Option<u8> + Send + Sync>>>,
    /// Whether a Kiro turn stopped by a busy model is continued (None while unset; the setting is then read from settings.json).
    retry_busy: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    /// How many run at once: MAX_RUNNING, unless the host says otherwise (the Mac's Settings).
    limit: AtomicUsize,
}

/// Every session the office knows about, shared by the notch and the app window.
#[derive(Clone)]
pub struct KiroSessions(Arc<Shared>);

enum Note { Changed, Ended(KiroSession, KiroResult) }

impl KiroSessions {
    /// make gives each new or woken session the runner for its tool.
    pub fn new(make: impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, history: Option<Arc<AgentHistory>>) -> KiroSessions {
        KiroSessions::with_clock(make, history, Stamp::now)
    }

    pub fn with_clock(make: impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, history: Option<Arc<AgentHistory>>,
        now: impl Fn() -> Stamp + Send + Sync + 'static) -> KiroSessions {
        KiroSessions(Arc::new(Shared { inner: Mutex::new(Inner { all: vec![], selected: None }), make: Box::new(make), history, now: Box::new(now),
            changed: Mutex::new(vec![]), ended: Mutex::new(vec![]), checkpoints: Mutex::new(None), stops: Mutex::new(vec![]), custom: Mutex::new(None), compact: Mutex::new(None), retry_busy: Mutex::new(None), limit: AtomicUsize::new(MAX_RUNNING) }))
    }

    /// Kiro's auto compact: `at` says, at each prompt, the context percent that calls for a
    /// `/compact` first, or None while it is off. Without one the setting is read from
    /// settings.json at each prompt.
    pub fn set_auto_compact(&self, at: impl Fn() -> Option<u8> + Send + Sync + 'static) { *self.0.compact.lock().unwrap() = Some(Arc::new(at)); }

    /// Kiro's "continue when high usage encountered": `on` says, after each stopped turn, whether to
    /// continue it. Without one the setting is read from settings.json each time.
    pub fn set_retry_when_busy(&self, on: impl Fn() -> bool + Send + Sync + 'static) { *self.0.retry_busy.lock().unwrap() = Some(Arc::new(on)); }

    pub fn history(&self) -> Option<&Arc<AgentHistory>> { self.0.history.as_ref() }
    /// Keep the project folder before and after every turn from now on (checkpoint.rs).
    pub fn set_checkpoints(&self, c: Arc<Checkpoints>) { *self.0.checkpoints.lock().unwrap() = Some(c); }
    pub fn checkpoints(&self) -> Option<Arc<Checkpoints>> { self.0.checkpoints.lock().unwrap().clone() }
    pub fn now(&self) -> Stamp { (self.0.now)() }

    /// Any session changed, or one came or went. Off any thread.
    pub fn on_changed(&self, f: impl Fn() + Send + Sync + 'static) { self.0.changed.lock().unwrap().push(Arc::new(f)); }
    /// Where a custom agent's runner comes from (custom.rs), by the agent's id.
    pub fn set_custom(&self, f: impl Fn(&str) -> Option<RunTask> + Send + Sync + 'static) { *self.0.custom.lock().unwrap() = Some(Arc::new(f)); }

    /// The runner for a new or woken session: its tool's, or its custom agent's.
    fn run_for(&self, tool: AgentTool, provider: Option<&str>) -> RunTask {
        if tool != AgentTool::Custom { return (self.0.make)(tool); }
        let f = self.0.custom.lock().unwrap().clone();
        match provider.and_then(|p| f.as_ref().and_then(|f| f(p))) {
            Some(r) => r,
            None => Arc::new(|_| KiroResult::new(KiroState::Failed, "This conversation’s agent isn’t set up any more. Add it again in Settings → Agents to carry on; the conversation is kept.")),
        }
    }

    /// A run was asked to stop (Stop or Pause, a delete, Hover quitting), with the lock released. Off any thread.
    pub fn on_stop(&self, f: impl Fn(&KiroSession) + Send + Sync + 'static) { self.0.stops.lock().unwrap().push(Arc::new(f)); }
    fn stopping(&self, s: &KiroSession) { let cbs = self.0.stops.lock().unwrap().clone(); for f in cbs { f(s); } }

    /// Takes a run out of the count of tasks that run at once while it waits on its helpers, or puts it back.
    /// A full house of waiting parents can then never keep their helpers from starting.
    pub fn park(&self, key: &str, on: bool) {
        let changed = { let mut g = self.0.inner.lock().unwrap(); g.all.iter_mut().find(|x| x.s.key == key).is_some_and(|x| std::mem::replace(&mut x.parked, on) != on) };
        if changed { self.raise(vec![Note::Changed]); }
    }

    /// Changes a session's links (ext) and keeps it in the history. False when it isn't at a desk.
    pub fn update_ext(&self, key: &str, f: impl FnOnce(&mut SessionExt)) -> bool {
        let snap = {
            let mut g = self.0.inner.lock().unwrap();
            let Some(x) = g.all.iter_mut().find(|x| x.s.key == key) else { return false };
            f(&mut x.s.ext);
            x.s.rev += 1;
            x.s.clone()
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        true
    }

    /// The session with this lasting key, if it is at a desk.
    pub fn find(&self, key: &str) -> Option<KiroSession> { self.0.inner.lock().unwrap().all.iter().find(|x| x.s.key == key).map(|x| x.s.clone()) }

    /// A turn ended. Off any thread.
    pub fn on_ended(&self, f: impl Fn(&KiroSession, &KiroResult) + Send + Sync + 'static) { self.0.ended.lock().unwrap().push(Arc::new(f)); }

    fn raise(&self, notes: Vec<Note>) {
        for n in notes {
            match n {
                Note::Changed => { let cbs = self.0.changed.lock().unwrap().clone(); for f in cbs { f(); } }
                Note::Ended(s, r) => { let cbs = self.0.ended.lock().unwrap().clone(); for f in cbs { f(&s, &r); } }
            }
        }
    }

    /// Oldest first.
    pub fn all(&self) -> Vec<KiroSession> { self.0.inner.lock().unwrap().all.iter().map(|x| x.s.clone()).collect() }
    /// Oldest first, each without its answers' text and its steps' changes and output
    /// (KiroSession::light): for what redraws often, the notch, the desks, the panels.
    pub fn all_light(&self) -> Vec<KiroSession> { self.0.inner.lock().unwrap().all.iter().map(|x| x.s.light()).collect() }
    pub fn get(&self, id: i32) -> Option<KiroSession> { self.0.inner.lock().unwrap().all.iter().find(|x| x.s.id == id).map(|x| x.s.clone()) }
    /// The session's change number (KiroSession::rev) and whether it runs, without copying it.
    pub fn rev(&self, id: i32) -> Option<(u64, bool)> { self.0.inner.lock().unwrap().all.iter().find(|x| x.s.id == id).map(|x| (x.s.rev, x.s.busy())) }
    /// The question in front of each session that waits, and how many it has waiting:
    /// what the office draws over the bots' heads, without copying every transcript.
    pub fn asking_now(&self) -> Vec<(i32, AgentAsk, usize)> {
        self.0.inner.lock().unwrap().all.iter().filter_map(|x| x.s.asking().map(|a| (x.s.id, a.clone(), x.s.asks.len()))).collect()
    }
    pub fn running(&self) -> usize { self.0.inner.lock().unwrap().all.iter().filter(|x| x.counts()).count() }
    pub fn can_start(&self) -> bool { self.running() < self.max_running() }
    /// How many tasks run at once (Settings.MaxRunning on a Mac: 1 to MAX_KEPT).
    pub fn max_running(&self) -> usize { self.0.limit.load(Ordering::SeqCst) }
    pub fn set_max_running(&self, n: usize) { self.0.limit.store(n.clamp(1, MAX_KEPT), Ordering::SeqCst); }
    /// The session the office last opened.
    pub fn selected(&self) -> Option<i32> { self.0.inner.lock().unwrap().selected }

    fn save(&self, s: &KiroSession) {
        if let Some(h) = &self.0.history { if !s.deleted && !s.turns.is_empty() { h.save(&s.snapshot(self.now())); } }
    }

    /// A seventh session needs a desk: the oldest finished one gives up its own.
    fn free_desk(g: &mut Inner) -> bool {
        if g.all.len() >= MAX_KEPT {
            if let Some(i) = g.all.iter().position(|x| !x.s.busy()) {
                let id = g.all.remove(i).s.id;
                if g.selected == Some(id) { g.selected = None; }
            }
        }
        g.all.len() < MAX_KEPT
    }

    fn seat(g: &Inner, s: &mut KiroSession) {
        s.seat = (0..MAX_KEPT).find(|i| g.all.iter().all(|x| x.s.seat != *i)).unwrap();
        s.bot = (0..MAX_KEPT).find(|i| g.all.iter().all(|x| x.s.bot != *i)).unwrap();
    }

    /// Starts a task. None, and nothing happens, when three run, the folder or prompt
    /// can't be used, or every desk is busy.
    pub fn start(&self, tool: AgentTool, folder: &str, prompt: &str, images: Vec<String>) -> Option<KiroSession> { self.start_as(tool, folder, prompt, images, None) }

    /// start, with the session's own tool access (AgentOptions::with_access), kept in its history.
    pub fn start_as(&self, tool: AgentTool, folder: &str, prompt: &str, images: Vec<String>, access: Option<&str>) -> Option<KiroSession> {
        self.start_in(tool, folder, prompt, images, access, None)
    }

    /// start_as, in Kiro's cloud when `cloud` names its repos (KiroSession::cloud).
    pub fn start_in(&self, tool: AgentTool, folder: &str, prompt: &str, images: Vec<String>, access: Option<&str>, cloud: Option<Vec<String>>) -> Option<KiroSession> {
        self.start_bound(tool, folder, prompt, images, access, cloud, SessionExt::default())
    }

    /// start_in, in the workspace `ext` names (workspace.rs: `folder` is then the task's worktree). None
    /// also while a checkpoint restore holds that folder.
    #[allow(clippy::too_many_arguments)]
    pub fn start_bound(&self, tool: AgentTool, folder: &str, prompt: &str, images: Vec<String>, access: Option<&str>, cloud: Option<Vec<String>>, ext: SessionExt) -> Option<KiroSession> {
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.counts()).count();
        if running >= self.max_running() || !crate::usable_folder(Some(folder)) || !usable(prompt, &images) || crate::workspace::held(folder).is_some() { return None; }
        if !Self::free_desk(&mut g) { return None; }
        let mut s = KiroSession::new(tool);
        Self::seat(&g, &mut s);
        s.folder = folder.into();
        s.access = access.map(str::to_owned);
        s.cloud = cloud;
        s.ext = ext;
        s.turns.push(KiroTurn::new(prompt.trim(), images));
        let id = s.id;
        let run = self.run_for(tool, s.ext.provider.as_deref());
        g.all.push(Slot::new(s, run));
        let begun = self.begin(&mut g, id);
        g.selected = Some(id);
        let snap = g.all.iter().find(|x| x.s.id == id).unwrap().s.clone();
        drop(g);
        self.save(&snap);
        self.raise(vec![Note::Changed, Note::Changed]);
        begun();
        Some(snap)
    }

    /// Moves the slot's conversation to another provider. Changes nothing until it knows it can: the account for a provider that
    /// starts afresh is made first, and if it doesn't fit, the conversation stays where it is. History is never altered; it gains a
    /// record of the move. Native state of the provider it leaves is kept, so coming back resumes it and brings over only what it missed.
    fn apply_switch(&self, slot: &mut Slot, to: &Target) -> Result<Switched, String> {
        if slot.s.cloud.is_some() { return Err("A Kiro Web task stays with Kiro Web.".into()); }
        let from = provider_id(&slot.s);
        if from == to.id { return Err(format!("It is with {} already.", to.id)); }
        let done = slot.s.turns.iter().filter(|t| !t.queued && t.result.is_some()).count();
        let mut lin = slot.s.ext.lineage.clone().unwrap_or_default();
        if let Some(id) = slot.s.kiro_id.clone() { lin.natives.retain(|n| n.provider != from); lin.natives.push(Native { provider: from.clone(), id, seen: done }); }
        let native = lin.natives.iter().find(|n| n.provider == to.id && n.seen <= done).cloned();
        let (mode, carry, id) = match &native {
            // Its own conversation, resumed; what it missed since is handed over as text.
            Some(n) => {
                let missed = crate::handoff::portable(&slot.s.turns, n.seen, crate::handoff::BUDGET, &slot.s.key,
                    &format!("You are {} again, and this conversation went on without you for {} turn{}. Your own memory of it is intact up to turn {}; the turns you missed follow.", to.id, done - n.seen, if done - n.seen == 1 { "" } else { "s" }, n.seen))?;
                ("native", (n.seen < done).then_some(missed), Some(n.id.clone()))
            }
            None if done == 0 => ("fresh", None, None),
            None => ("portable", Some(crate::handoff::portable(&slot.s.turns, 0, crate::handoff::BUDGET, &slot.s.key,
                &format!("This conversation was with {from} until now, and you ({}) are carrying it on. You have none of it in memory; this is an account of it.", to.id))?), None),
        };
        lin.handoffs.push(Handoff { turn: done, from, to: to.id.clone(), mode: mode.into(), carried: carry.as_ref().map_or(0, |c| c.carried), omitted: carry.as_ref().map_or(0, |c| c.omitted) });
        lin.pending = carry.as_ref().map(|c| c.text.clone()).filter(|t| !t.is_empty());
        slot.s.tool = to.tool;
        slot.s.ext.provider = to.instance.clone();
        slot.s.ext.lineage = Some(lin);
        slot.s.kiro_id = id;
        slot.s.context = None;
        slot.usage = None;
        slot.run = self.run_for(to.tool, to.instance.as_deref());
        slot.s.rev += 1;
        Ok(Switched { mode, carried: carry.as_ref().map_or(0, |c| c.carried), omitted: carry.as_ref().map_or(0, |c| c.omitted), notes: carry.map_or(vec![], |c| c.notes) })
    }

    /// Moves a conversation to another provider now. Not while a run goes on: a switch asked for with a queued message happens when that
    /// message is sent (`Msg::switch_to`), after the work before it.
    pub fn switch_provider(&self, id: i32, to: &Target) -> Result<Switched, String> {
        let (r, snap) = {
            let mut g = self.0.inner.lock().unwrap();
            let slot = g.all.iter_mut().find(|x| x.s.id == id).ok_or("That chat isn't here.")?;
            if slot.s.busy() { return Err("A run is going on. Wait for it, or queue the message with the switch; it happens when that message is sent.".into()); }
            if crate::workspace::held(&slot.s.folder).is_some() { return Err("The folder is in use by a restore. Try again in a moment.".into()); }
            (self.apply_switch(slot, to)?, slot.s.clone())
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        Ok(r)
    }

    /// A new conversation from turn `turn` of another, which stays as it is. The copy holds the turns up to and including it, so the chat reads on;
    /// its agent starts afresh from an account of them (no provider here forks its own conversation). The provider is the caller's choice and
    /// the folder is given separately: branching the conversation and choosing where files are written are two decisions. Only from a turn that ended.
    pub fn fork(&self, key: &str, turn: usize, to: &Target, folder: &str, workspace: Option<hover_core::ext::WorkspaceBinding>) -> Result<KiroSession, String> {
        let src = self.saved(key).ok_or("That conversation isn’t available.")?;
        let t = src.turns.get(turn).ok_or("That message isn’t there.")?;
        if t.ext.queued || t.state.is_none() { return Err("A conversation can be forked only from a turn that has ended.".into()); }
        if src.cloud.is_some() { return Err("A Kiro Web conversation can’t be forked here.".into()); }
        if !crate::usable_folder(Some(folder)) { return Err("The folder isn’t there.".into()); }
        let mut copy = src.clone();
        copy.key = hover_core::guid_n();
        copy.turns.truncate(turn + 1);
        for t in &mut copy.turns { (t.before, t.after) = (None, None); }
        (copy.tool, copy.acp_id, copy.context, copy.folder, copy.cloud) = (to.tool, None, None, folder.into(), None);
        copy.ext = SessionExt { workspace, provider: to.instance.clone(), orch: None, lineage: None };
        let mut s = KiroSession::new(to.tool);
        s.restore(&copy);
        let from = { let mut probe = KiroSession::new(src.tool); probe.ext.provider = src.ext.provider.clone(); provider_id(&probe) };
        let carry = crate::handoff::portable(&s.turns, 0, crate::handoff::BUDGET, &copy.key,
            &format!("This conversation is a fork of another, taken after turn {}. It was with {from}; you ({}) are carrying it on from that point. You have none of it in memory; this is an account of it.", turn + 1, to.id))?;
        s.ext.lineage = Some(Lineage { fork: Some(Fork { key: key.into(), turn }), pending: Some(carry.text).filter(|t| !t.is_empty()),
            handoffs: if from != to.id { vec![Handoff { turn: turn + 1, from, to: to.id.clone(), mode: "portable".into(), carried: carry.carried, omitted: carry.omitted }] } else { vec![] }, ..Default::default() });
        s.ext.provider = to.instance.clone();
        s.held = false;
        let snap = {
            let mut g = self.0.inner.lock().unwrap();
            if !Self::free_desk(&mut g) { return Err("Every desk is busy. Finish or dismiss a task first.".into()); }
            Self::seat(&g, &mut s);
            let snap = s.clone();
            let run = self.run_for(to.tool, to.instance.as_deref());
            g.all.push(Slot::new(s, run));
            g.selected = Some(snap.id);
            snap
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        Ok(snap)
    }

    /// Brings what a conversation found back to another (by default the one it was forked from), as one message to it: words only. No code, file
    /// or branch moves. Done once per state of the fork: the same findings are found already sent (by the mark in the message) and not sent again;
    /// a fork that has gone on since has new findings to send. Records what was moved.
    pub fn bring_findings_back(&self, fork_key: &str, into_key: Option<&str>) -> Result<usize, String> {
        let fork = self.saved(fork_key).ok_or("That conversation isn’t available.")?;
        let lin = fork.ext.lineage.clone().unwrap_or_default();
        let (from_turn, parent) = match (&lin.fork, into_key) {
            (Some(f), None) => (f.turn, f.key.clone()),
            (Some(f), Some(p)) => (if p == f.key { f.turn } else { 0 }, p.to_owned()),
            (None, Some(p)) => (0, p.to_owned()),
            (None, None) => return Err("This conversation wasn’t forked from another, so say where the findings go.".into()),
        };
        let parent_s = self.wake(&parent).ok_or("The conversation to bring them to isn’t available (every desk may be busy).")?;
        let mut probe = KiroSession::new(fork.tool);
        probe.restore(&fork);
        let (text, chars) = crate::handoff::findings(&fork.title, fork_key, &probe.turns, from_turn, 16_000);
        let marker = text.split(|c: char| c == '(' || c == ')').find(|p| p.starts_with("hover-return:")).unwrap_or("").to_owned();
        if !marker.is_empty() && parent_s.turns.iter().any(|t| t.prompt.contains(&marker)) { return Ok(0); }
        let done = probe.turns.iter().enumerate().filter(|(i, t)| *i > from_turn && !t.queued && t.result.is_some()).count();
        if done == 0 { return Err("Nothing was asked in that conversation after the point it was forked at.".into()); }
        let chip = crate::context::thread(fork_key, &fork.title);
        if !self.reply_msg(parent_s.id, Msg { text, chips: vec![chip], ..Default::default() }) { return Err("The message couldn’t be sent now. Try again when a place is free.".into()); }
        self.update_ext(&parent, |e| { e.lineage.get_or_insert_with(Default::default).returned.push(Returned { from: fork_key.into(), turn: done, chars }); });
        Ok(chars)
    }

    /// Marks the next turn running and gives back what starts its thread (run once the
    /// lock is gone).
    fn begin(&self, g: &mut Inner, id: i32) -> Box<dyn FnOnce() + Send> {
        let now = self.now();
        let slot = g.all.iter_mut().find(|x| x.s.id == id).unwrap();
        let ti = slot.s.turns.iter().position(|t| t.queued || t.result.is_none()).unwrap();
        let t = &mut slot.s.turns[ti];
        t.queued = false;
        t.started_at = now;
        slot.s.phase = KiroPhase::Starting;
        slot.s.state = KiroState::Running;
        slot.s.rev += 1;
        let ct = Cancel::new();
        slot.cancel = Some(ct.clone());
        // A provider switch asked for with this message happens now, as it is sent. One that can't be made leaves the conversation where it
        // is, and the message says so to the agent that gets it.
        let mut failed = None;
        if let Some(p) = slot.s.turns[ti].switch_to.take() {
            match Target::parse(&p) {
                Some(to) if to.id != provider_id(&slot.s) => { if let Err(e) = self.apply_switch(slot, &to) { failed = Some(format!("[Hover] The switch to {p} couldn’t be made ({e}) and this message goes to {}.", provider_id(&slot.s))); } }
                Some(_) => {}
                None => failed = Some(format!("[Hover] The switch to “{p}” couldn’t be made: there is no such agent.")),
            }
        }
        // What the agent is told first, once: that its folder and chat went back, or the account of a conversation it now carries on.
        let mut prompt = slot.s.turns[ti].text();
        if let Some(carry) = slot.s.ext.lineage.as_mut().and_then(|l| l.pending.take()) { prompt = format!("{carry}{prompt}"); }
        if let Some(n) = slot.note.take() { prompt = format!("{n}\n\n{prompt}"); }
        if let Some(f) = failed { prompt = format!("{f}\n\n{prompt}"); }
        // A cloud session's files are in its sandbox, not this folder: nothing to keep.
        let cp = self.checkpoints().filter(|_| slot.s.cloud.is_none()).map(|c| (c, slot.s.key.clone()));
        let args_base = (slot.s.folder.clone(), prompt, slot.s.kiro_id.clone(), slot.s.access.clone(), slot.s.cloud.clone());
        let run = slot.run.clone();
        let me = Arc::downgrade(&self.0);
        Box::new(move || {
            std::thread::Builder::new().name("agent-turn".into()).spawn(move || go(me, id, ti, run, ct, cp, args_base, None)).expect("a thread for the turn");
        })
    }

    /// A reply. While a turn runs it waits and starts when that one ends. False when the
    /// session isn't here or hasn't started, there is nothing to send, or it would start
    /// a fourth run.
    pub fn reply(&self, id: i32, text: &str, images: Vec<String>) -> bool { self.reply_msg(id, Msg { text: text.into(), images, ..Default::default() }) }

    /// reply, with chips and a provider switch. A reply from the user also frees a held queue: the oldest waiting message goes first.
    pub fn reply_msg(&self, id: i32, m: Msg) -> bool { self.reply_at(id, m, false) }

    /// reply_msg, but the message goes *ahead* of any that wait: it is the next sent (and starts now when nothing runs). For a
    /// continuation that must finish before the follow-ups held behind it.
    pub fn reply_first(&self, id: i32, m: Msg) -> bool { self.reply_at(id, m, true) }

    fn reply_at(&self, id: i32, m: Msg, front: bool) -> bool {
        let Msg { text, images, chips, switch_to } = m;
        let text = text.as_str();
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.counts()).count();
        let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
        if !slot.s.busy() && running >= self.max_running() { return false; }
        if slot.s.state == KiroState::Idle || !(usable(text, &images) || !chips.is_empty()) || crate::workspace::held(&slot.s.folder).is_some() { return false; }
        let mut t = KiroTurn::new(text.trim(), images);
        t.chips = chips;
        t.switch_to = switch_to;
        slot.s.held = false;
        let at = if front { slot.s.turns.iter().position(|x| x.queued) } else { None };
        // Replies left queued (behind a stop that wasn't confirmed) go first, in order.
        let start_now = !slot.s.busy();
        t.queued = slot.s.busy() || slot.s.turns.iter().any(|t| t.queued);
        match at { Some(i) => slot.s.turns.insert(i, t), None => slot.s.turns.push(t) }
        slot.s.rev += 1;
        let begun = if start_now { Some(self.begin(&mut g, id)) } else { None };
        let snap = g.all.iter().find(|x| x.s.id == id).unwrap().s.clone();
        drop(g);
        self.raise(vec![Note::Changed]);
        if let Some(b) = begun { b(); }
        self.save(&snap);
        true
    }

    /// Puts a chat back to a checkpoint: the project folder as it was there (checkpoint.rs),
    /// and the turns after it gone. Never while a run or a queued reply exists (the agent
    /// could be writing). `Before` sends that turn's message again at once. The agent, which
    /// still remembers everything, is told once with its next message that the folder and
    /// the chat went back; before the very first message it starts a new conversation.
    pub fn rewind(&self, id: i32, to: Rewind) -> Result<(), String> {
        let cp = self.checkpoints().ok_or("Checkpoints need git. Install it, then start a new chat.")?;
        let (key, folder, tree, keep, prompt, images) = {
            let g = self.0.inner.lock().unwrap();
            let slot = g.all.iter().find(|x| x.s.id == id).ok_or("That chat isn't here.")?;
            if slot.s.busy() || slot.s.turns.iter().any(|t| t.queued) { return Err("Stop the run first.".into()); }
            let (i, after) = match to { Rewind::After(i) => (i, true), Rewind::Before(i) => (i, false) };
            let t = slot.s.turns.get(i).ok_or("That message isn't here.")?;
            let tree = if after { &t.after } else { &t.before }.clone().ok_or("No checkpoint was kept there.")?;
            if !after && g.all.iter().filter(|x| x.counts()).count() >= self.max_running() { return Err(format!("{} running. Try again when one is done.", match self.max_running() { 1 => "1 task is".to_owned(), n => format!("{n} tasks are") })); }
            (slot.s.key.clone(), slot.s.folder.clone(), tree, if after { i + 1 } else { i }, t.prompt.clone(), t.images.clone())
        };
        // The folder is held for the whole restore: no task may start or reply in it, or in a folder inside it
        // or around it. A task already running there (an ancestor or a descendant too) stops the restore.
        let hold = crate::workspace::hold(&folder, "A checkpoint restore")?;
        {
            let g = self.0.inner.lock().unwrap();
            if let Some(o) = g.all.iter().find(|x| x.s.id != id && x.s.busy() && x.s.cloud.is_none() && crate::workspace::overlaps(&x.s.folder, &folder)) {
                return Err(format!("Another task is working in {}, which overlaps this folder. Stop it first.", o.s.folder));
            }
            if g.all.iter().find(|x| x.s.id == id).is_some_and(|x| x.s.busy()) { return Err("Stop the run first.".into()); }
        }
        // Files first, off the lock: a big folder takes a while, and the chat stays as it is until it worked.
        cp.restore(&key, &folder, &tree)?;
        let snap = {
            let mut g = self.0.inner.lock().unwrap();
            let slot = g.all.iter_mut().find(|x| x.s.id == id).ok_or("That chat was deleted.")?;
            if slot.s.busy() { return Err("A run started while the files were put back.".into()); }
            slot.s.turns.truncate(keep);
            slot.s.state = slot.s.turns.last().and_then(|t| t.result.as_ref()).map_or(slot.s.state, |r| r.state);
            slot.s.asks.clear();
            slot.s.rev += 1;
            // What the provider remembers is no longer the chat: its own conversation (and any it kept from another move) held the turns
            // just removed.
            if let Some(l) = &mut slot.s.ext.lineage { l.natives.retain(|n| n.seen <= keep); l.pending = None; }
            if keep == 0 {
                slot.s.kiro_id = None;
                slot.s.context = None;
                slot.usage = None;
                slot.note = None;
            } else {
                let what = crate::stream::clip_to(first_line(&slot.s.turns[keep - 1].prompt), 80);
                let told = match to {
                    Rewind::After(_) => format!("The project's files were just put back to how they were right after your reply to “{what}”. Everything that changed after that point was undone, and the later messages were removed from this chat. Carry on from here and don't rely on that later work."),
                    Rewind::Before(_) => "The project's files were just put back to how they were before the next message, and your earlier attempt at it (and anything after it) was undone and removed from this chat. Start it afresh.".to_owned(),
                };
                // A replacement conversation: the agent starts anew from an account of the turns that remain, not from its memory of ones that
                // are gone. If the account won't fit, the old way is kept (it is told, and remembers) and the log says why.
                let intro = format!("{told} This is a new conversation for you: it starts from the account below, not from what you remember of this one.");
                match crate::handoff::portable(&slot.s.turns, 0, crate::handoff::BUDGET, &slot.s.key, &intro) {
                    Ok(c) if !c.text.is_empty() => {
                        slot.s.kiro_id = None;
                        slot.s.context = None;
                        slot.usage = None;
                        slot.note = None;
                        slot.s.ext.lineage.get_or_insert_with(Default::default).pending = Some(c.text);
                    }
                    other => {
                        if let Err(e) = other { hover_core::log::line(&format!("rewind: no replacement conversation - {e}")); }
                        slot.note = Some(format!("[Hover] {told}"));
                    }
                }
            }
            slot.s.clone()
        };
        // The chat is cut: the folder may be used again (the message below starts a turn in it).
        drop(hold);
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        if matches!(to, Rewind::Before(_)) && !self.reply(id, &prompt, images) { return Err("The files are back, but the message couldn't be sent again.".into()); }
        Ok(())
    }
    /// The session a history entry is, at a desk: the one already there, or the saved
    /// one brought back to a free desk. None when it can't be read or every desk is busy.
    pub fn wake(&self, key: &str) -> Option<KiroSession> {
        if let Some(s) = self.all().into_iter().find(|x| x.key == key) { return Some(s); }
        let saved = self.0.history.as_ref()?.load(key)?;
        let mut g = self.0.inner.lock().unwrap();
        if !Self::free_desk(&mut g) { return None; }
        let mut s = KiroSession::new(saved.tool);
        s.restore(&saved);
        Self::seat(&g, &mut s);
        let snap = s.clone();
        let run = self.run_for(saved.tool, saved.ext.provider.as_deref());
        g.all.push(Slot::new(s, run));
        drop(g);
        self.raise(vec![Note::Changed]);
        Some(snap)
    }

    /// Kiro Web sessions that were still working when Hover closed are brought back to desks and
    /// followed on (`reattach`). Reads the history off this thread; only sessions updated in the
    /// last three days, at most the newest few.
    pub fn reattach_cut_off(&self) {
        let Some(h) = self.0.history.clone() else { return };
        let me = self.clone();
        std::thread::Builder::new().name("reattach".into()).spawn(move || {
            let week = me.now();
            let mut keys = vec![];
            for e in h.entries().into_iter().filter(|e| e.tool == AgentTool::Kiro && week.secs_since(&e.updated) < 3.0 * 86400.0).take(10) {
                let Some(s) = h.load(&e.key) else { continue };
                let cut = s.turns.last().is_some_and(|t| t.state.is_none() || t.state == Some(KiroState::Failed) && cut_off_text(t.text.as_deref().unwrap_or(""))
                    || t.state == Some(KiroState::Cancelled) && t.text.as_deref() == Some(STILL_WORKING_TEXT));
                if s.cloud.is_some() && s.acp_id.is_some() && cut { keys.push(e.key); }
            }
            for key in keys {
                let Some(s) = me.wake(&key) else { continue };
                hover_core::log::line(&format!("kiro web: {} was still working when Hover closed; attaching to it", s.title()));
                me.reattach(s.id);
            }
        }).ok();
    }

    /// A Kiro Web session whose last turn was cut off (Hover closed, or the connection was lost)
    /// goes back to running and follows the cloud session on, in that turn. False when it isn't one,
    /// or it is busy, or three already run.
    pub fn reattach(&self, id: i32) -> bool {
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.counts()).count();
        let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
        if slot.s.busy() || running >= self.max_running() || slot.s.cloud.is_none() || slot.s.kiro_id.is_none() { return false; }
        let Some(ti) = slot.s.turns.len().checked_sub(1) else { return false };
        let cut = slot.s.turns[ti].result.as_ref().filter(|r| (r.state == KiroState::Cancelled && (r.text == CLOSED_TEXT || r.text == STILL_WORKING_TEXT)) || cut_off_result(r)).cloned();
        let Some(prior) = cut else { return false };
        let t = &mut slot.s.turns[ti];
        t.result = None;
        t.ended_at = None;
        slot.s.phase = KiroPhase::Starting;
        slot.s.state = KiroState::Running;
        slot.s.rev += 1;
        let ct = Cancel::new();
        slot.cancel = Some(ct.clone());
        let args_base = (slot.s.folder.clone(), crate::acp::ATTACH_PROMPT.to_owned(), slot.s.kiro_id.clone(), slot.s.access.clone(), slot.s.cloud.clone());
        let (run, me) = (slot.run.clone(), Arc::downgrade(&self.0));
        let snap = slot.s.clone();
        drop(g);
        self.raise(vec![Note::Changed]);
        self.save(&snap);
        std::thread::Builder::new().name("agent-turn".into()).spawn(move || go(me, id, ti, run, ct, None, args_base, Some(prior))).expect("a thread for the turn");
        true
    }

    /// A Kiro Web session made outside Hover (Kiro Web, the CLI, another computer), brought to a
    /// desk and kept in the history like any other. `turns` is its conversation as its replay gave
    /// it; when that couldn't be read, it is one turn with its title and why. A last turn still
    /// working in the cloud is followed on. The one already here when it was opened before. None
    /// when every desk is busy.
    pub fn adopt_cloud(&self, kiro_id: &str, title: &str, folder: &str, updated: Option<Stamp>, turns: Result<Vec<crate::acp::CloudTurn>, String>) -> Option<KiroSession> {
        if let Some(s) = self.all().into_iter().find(|s| s.kiro_id.as_deref() == Some(kiro_id)) { return Some(s); }
        let at = updated.unwrap_or_else(|| self.now());
        let title = if title.trim().is_empty() { "Kiro Web session" } else { title.trim() };
        let turn = |prompt: &str, steps: Vec<KiroStep>, state: KiroState, text: String| {
            let mut t = KiroTurn::new(prompt, vec![]);
            (t.started_at, t.ended_at, t.steps, t.result) = (at, Some(at), steps, Some(KiroResult::new(state, text)));
            t
        };
        let mut s = KiroSession::new(AgentTool::Kiro);
        (s.folder, s.kiro_id, s.cloud, s.access) = (folder.into(), Some(kiro_id.into()), Some(vec![]), Some("full".into()));
        let mut running = false;
        match turns {
            Ok(list) if !list.is_empty() => {
                let n = list.len();
                for (i, c) in list.into_iter().enumerate() {
                    let prompt = if c.prompt.is_empty() && i == 0 { title.to_owned() } else { c.prompt };
                    let (state, text) = if i + 1 == n && !c.completed { running = true; (KiroState::Cancelled, STILL_WORKING_TEXT.into()) }
                        else if c.text.is_empty() { (KiroState::Completed, "Done. Kiro didn’t leave a summary.".into()) } else { (KiroState::Completed, c.text) };
                    s.turns.push(turn(&prompt, c.steps, state, text));
                }
            }
            Ok(_) => s.turns.push(turn(title, vec![], KiroState::Completed, "This Kiro Web session has no messages yet.".into())),
            Err(e) => s.turns.push(turn(title, vec![], KiroState::Failed, format!("Couldn’t read this conversation from Kiro Web. {e} Open it there with the cloud button, or reply to carry on."))),
        }
        s.state = s.turns.last().and_then(|t| t.result.as_ref()).map_or(KiroState::Completed, |r| r.state);
        let snap = {
            let mut g = self.0.inner.lock().unwrap();
            if !Self::free_desk(&mut g) { return None; }
            Self::seat(&g, &mut s);
            let snap = s.clone();
            g.all.push(Slot::new(s, (self.0.make)(AgentTool::Kiro)));
            snap
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        if running { self.reattach(snap.id); }
        Some(snap)
    }

    /// The history entry's record, whether or not it is at a desk now.
    pub fn saved(&self, key: &str) -> Option<SavedSession> {
        let here = self.all().into_iter().find(|x| x.key == key);
        match here { Some(s) => Some(s.snapshot(self.now())), None => self.0.history.as_ref()?.load(key) }
    }

    pub fn select(&self, id: Option<i32>) {
        let mut g = self.0.inner.lock().unwrap();
        if id.is_some_and(|i| !g.all.iter().any(|x| x.s.id == i)) || g.selected == id { return; }
        g.selected = id;
        drop(g);
        self.raise(vec![Note::Changed]);
    }

    /// Takes a finished session out of the office; it stays in the history.
    pub fn dismiss(&self, id: i32) {
        let mut g = self.0.inner.lock().unwrap();
        let Some(i) = g.all.iter().position(|x| x.s.id == id && !x.s.busy()) else { return };
        let gone = g.all.remove(i).s;
        if g.selected == Some(id) { g.selected = None; }
        drop(g);
        // Putting a task away ends what was waiting on its behalf (watches, resumes).
        self.stopping(&gone);
        self.raise(vec![Note::Changed]);
    }

    /// The user deleted a session: a run is stopped, and it leaves the office and the history.
    pub fn delete(&self, key: &str) {
        let mut g = self.0.inner.lock().unwrap();
        let mut gone = None;
        if let Some(i) = g.all.iter().position(|x| x.s.key == key) {
            let mut slot = g.all.remove(i);
            slot.s.deleted = true;
            if slot.s.busy() { if let Some(c) = &slot.cancel { c.cancel(); } }
            if g.selected == Some(slot.s.id) { g.selected = None; }
            gone = Some(slot.s.clone());
        }
        drop(g);
        if let Some(s) = &gone { self.stopping(s); }
        if let Some(h) = &self.0.history { h.delete(key); }
        if let Some(c) = self.checkpoints() { c.delete(key); }
        self.raise(vec![Note::Changed]);
    }

    /// Stops the turn that runs; replies waiting behind it are not sent, and a
    /// question it asked is turned down.
    pub fn stop(&self, id: i32) { self.halt(id, false); }

    /// Pause: the turn that runs is cancelled through the tool, the conversation stays,
    /// and once the tool says the turn has ended the next queued reply goes, once. With
    /// none queued the session waits; a later reply carries on the conversation. False
    /// when nothing runs.
    pub fn pause(&self, id: i32) -> bool { self.halt(id, true) }

    fn halt(&self, id: i32, pausing: bool) -> bool {
        let (c, denied, found) = {
            let mut g = self.0.inner.lock().unwrap();
            match g.all.iter_mut().find(|x| x.s.id == id && x.s.busy()) {
                Some(x) => {
                    x.pausing |= pausing;
                    x.s.stopping = true;
                    x.s.rev += 1;
                    (x.cancel.clone(), x.deny_all(), true)
                }
                None => (None, vec![], false),
            }
        };
        for d in denied { d.deny(); }
        if found { self.raise(vec![Note::Changed]); }
        if let Some(c) = c { c.cancel(); }
        if found { if let Some(s) = self.get(id) { self.stopping(&s); } }
        found
    }

    /// A queued reply taken back before it was sent. False when turn `index` isn't a
    /// queued one of that session.
    pub fn cancel_queued(&self, id: i32, index: usize) -> bool {
        let snap = {
            let mut g = self.0.inner.lock().unwrap();
            let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
            if !slot.s.turns.get(index).is_some_and(|t| t.queued) { return false; }
            slot.s.turns.remove(index);
            slot.s.rev += 1;
            slot.s.clone()
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        true
    }

    /// The queued message `uid` of session `id`: its place in `turns`, or why not.
    fn queued_at(slot: &Slot, uid: &str, carried: &Msg) -> Result<usize, QueueError> {
        match slot.s.turns.iter().position(|t| t.uid == uid) {
            None => Err(QueueError::Gone),
            Some(i) if !slot.s.turns[i].queued => Err(QueueError::Started(carried.clone())),
            Some(i) => Ok(i),
        }
    }

    fn queue_change<R>(&self, id: i32, f: impl FnOnce(&mut Slot) -> Result<R, QueueError>) -> Result<R, QueueError> {
        let (r, snap) = {
            let mut g = self.0.inner.lock().unwrap();
            let slot = g.all.iter_mut().find(|x| x.s.id == id).ok_or(QueueError::Gone)?;
            let r = f(slot)?;
            if !slot.s.turns.iter().any(|t| t.queued) { slot.s.held = false; }
            slot.s.rev += 1;
            (r, slot.s.clone())
        };
        self.save(&snap);
        self.raise(vec![Note::Changed]);
        Ok(r)
    }

    /// Replaces a waiting message’s words, pictures and chips. If it began to send meanwhile, the error carries the
    /// text back so it can be put in the composer; nothing of it is lost.
    pub fn edit_queued(&self, id: i32, uid: &str, m: Msg) -> Result<(), QueueError> {
        if !m.ok() { return Err(QueueError::Invalid("A message needs words, a picture or an attachment.".into())); }
        self.queue_change(id, |slot| {
            let i = Self::queued_at(slot, uid, &m)?;
            let t = &mut slot.s.turns[i];
            (t.prompt, t.images, t.chips, t.switch_to) = (m.text.trim().to_owned(), m.images, m.chips, m.switch_to);
            Ok(())
        })
    }

    /// Moves a waiting message to place `to` among the waiting ones (0 is the next to go).
    pub fn move_queued(&self, id: i32, uid: &str, to: usize) -> Result<(), QueueError> {
        self.queue_change(id, |slot| {
            let i = Self::queued_at(slot, uid, &Msg::default())?;
            let first = slot.s.turns.iter().position(|t| t.queued).unwrap_or(i);
            let n = slot.s.turns.iter().filter(|t| t.queued).count();
            let t = slot.s.turns.remove(i);
            slot.s.turns.insert(first + to.min(n - 1), t);
            Ok(())
        })
    }

    /// Takes a waiting message back. The same message twice is `Gone`, not an error for the one that worked.
    pub fn remove_queued(&self, id: i32, uid: &str) -> Result<Msg, QueueError> {
        self.queue_change(id, |slot| {
            let i = Self::queued_at(slot, uid, &Msg::default())?;
            let t = slot.s.turns.remove(i);
            Ok(Msg { text: t.prompt, images: t.images, chips: t.chips, switch_to: t.switch_to })
        })
    }

    /// Sends a waiting message now, ahead of the others. No provider here steers a run that is going (none says so),
    /// so a run in progress is asked to stop the way Pause asks: through the tool, the conversation kept, and the message
    /// goes once the tool confirms. If it never confirms, nothing is sent and no second writer starts. A second click
    /// finds the message already sent.
    pub fn send_now(&self, id: i32, uid: &str) -> Result<SendNow, QueueError> {
        let busy = self.queue_change(id, |slot| {
            let i = Self::queued_at(slot, uid, &Msg::default())?;
            let first = slot.s.turns.iter().position(|t| t.queued).unwrap_or(i);
            let t = slot.s.turns.remove(i);
            slot.s.turns.insert(first, t);
            slot.s.held = false;
            Ok(slot.s.busy())
        })?;
        if busy {
            return if self.halt(id, true) { Ok(SendNow::Steering) } else { Ok(SendNow::Started) };
        }
        self.resume_queue(id).then_some(SendNow::Started).ok_or_else(|| QueueError::Invalid("No place is free to start it now.".into()))
    }

    /// Lets a held queue go: the oldest waiting message starts (when nothing runs and a place is free). The user's own action; a saved
    /// queue, or a stop, never does this by itself.
    pub fn resume_queue(&self, id: i32) -> bool {
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.counts()).count();
        let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
        if !slot.s.turns.iter().any(|t| t.queued) { slot.s.held = false; return false; }
        if slot.s.busy() { slot.s.held = false; return true; }
        if running >= self.max_running() || crate::workspace::held(&slot.s.folder).is_some() { return false; }
        slot.s.held = false;
        let begun = self.begin(&mut g, id);
        let snap = g.all.iter().find(|x| x.s.id == id).unwrap().s.clone();
        drop(g);
        self.raise(vec![Note::Changed]);
        begun();
        self.save(&snap);
        true
    }

    /// KiroSession.Ask: the agent of the session running conversation `sid` on `tool`
    /// asks the user about a tool call. The answer goes to `reply` (off any thread);
    /// with no such session running, or when `ct` is cancelled, it is Deny.
    pub fn ask(&self, tool: AgentTool, sid: &str, ask: AgentAsk, ct: &Cancel, reply: Reply) { self.hold(tool, sid, ask, ct, Answer::Call(reply)) }

    /// KiroSession.AskQuestion: the agent asks the user a question (ask.questions). The
    /// answer is each question's picked labels, in order; none when it was skipped,
    /// withdrawn, or nobody holds it.
    pub fn ask_question(&self, tool: AgentTool, sid: &str, ask: AgentAsk, ct: &Cancel, reply: QuestionReply) {
        if !ask.is_question() { reply(None); return; }
        self.hold(tool, sid, ask, ct, Answer::Question(reply))
    }

    fn hold(&self, tool: AgentTool, sid: &str, ask: AgentAsk, ct: &Cancel, reply: Answer) {
        let mut g = self.0.inner.lock().unwrap();
        let Some(slot) = g.all.iter_mut().find(|x| x.s.tool == tool && x.s.kiro_id.as_deref() == Some(sid) && x.s.busy()) else {
            drop(g);
            reply.deny();
            return;
        };
        hover_core::log::line(&format!("{} run {} asks: {} ({})", tool.id(), slot.s.id, ask.kind, ask.reason));
        let (id, qid) = (slot.s.id, ask.id.clone());
        slot.s.asks.push(ask);
        slot.s.rev += 1;
        slot.asks.push(Pending { id: qid.clone(), reply, _stop: None });
        drop(g);
        let me = Arc::downgrade(&self.0);
        let q2 = qid.clone();
        let reg = ct.on_cancel(move || { if let Some(sh) = me.upgrade() { KiroSessions(sh).answer(id, &q2, AskAnswer::Deny); } });
        {
            let mut g = self.0.inner.lock().unwrap();
            match g.all.iter_mut().find(|x| x.s.id == id).and_then(|x| x.asks.iter_mut().find(|p| p.id == qid)) {
                Some(p) => p._stop = Some(reg),
                None => drop(reg),
            }
        }
        self.raise(vec![Note::Changed]);
    }

    /// KiroSession.Answer: false when the session isn't waiting on that question.
    /// Deny on a question skips it.
    pub fn answer(&self, id: i32, ask_id: &str, answer: AskAnswer) -> bool {
        let reply = {
            let mut g = self.0.inner.lock().unwrap();
            let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
            let Some(i) = slot.asks.iter().position(|p| p.id == ask_id) else { return false };
            let p = slot.asks.remove(i);
            slot.s.asks.retain(|a| a.id != ask_id);
            slot.s.rev += 1;
            // Dropped here, outside the token's own lock: the stop no longer withdraws it.
            (p.reply, p._stop)
        };
        match reply.0 { Answer::Call(f) => f(answer), Answer::Question(f) => f(None) }
        drop(reply.1);
        self.raise(vec![Note::Changed]);
        true
    }

    /// KiroSession.AnswerQuestion: the labels picked (or typed) for each of its
    /// questions. False when it isn't waiting on that one, or the answers don't fit.
    pub fn answer_question(&self, id: i32, ask_id: &str, picked: Vec<Vec<String>>) -> bool {
        let reply = {
            let mut g = self.0.inner.lock().unwrap();
            let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
            let Some(i) = slot.asks.iter().position(|p| p.id == ask_id && matches!(p.reply, Answer::Question(_))) else { return false };
            let n = slot.s.asks.iter().find(|a| a.id == ask_id).and_then(|a| a.questions.as_ref()).map_or(0, Vec::len);
            if picked.len() != n || picked.iter().all(Vec::is_empty) { return false; }
            let p = slot.asks.remove(i);
            slot.s.asks.retain(|a| a.id != ask_id);
            slot.s.rev += 1;
            (p.reply, p._stop)
        };
        if let Answer::Question(f) = reply.0 { f(Some(picked)); }
        drop(reply.1);
        self.raise(vec![Note::Changed]);
        true
    }

    /// Stops every running turn, except Kiro Web's: a cancel would stop the cloud run too, and
    /// the next start follows it on (`reattach_cut_off`).
    pub fn stop_all(&self) {
        let (cs, who): (Vec<Cancel>, Vec<KiroSession>) = {
            let g = self.0.inner.lock().unwrap();
            let busy: Vec<&Slot> = g.all.iter().filter(|x| x.s.busy() && x.s.cloud.is_none()).collect();
            (busy.iter().filter_map(|x| x.cancel.clone()).collect(), busy.iter().map(|x| x.s.clone()).collect())
        };
        for s in &who { self.stopping(s); }
        for c in cs { c.cancel(); }
    }

    /// Something the office shows changed outside a run, like the first-use note.
    pub fn raise_changed(&self) { self.raise(vec![Note::Changed]); }
}

fn with<R>(me: &Weak<Shared>, id: i32, f: impl FnOnce(&mut Slot, Stamp) -> R) -> Option<(KiroSessions, R)> {
    let sh = me.upgrade()?;
    let ks = KiroSessions(sh);
    let now = ks.now();
    let r = {
        let mut g = ks.0.inner.lock().unwrap();
        let slot = g.all.iter_mut().find(|x| x.s.id == id)?;
        slot.s.rev += 1;
        f(slot, now)
    };
    Some((ks, r))
}

/// The step auto compact leaves in the turn it ran before.
const COMPACT_STEP: &str = "hover-compact";
const RETRY_STEP: &str = "hover-retry";
const RECONNECT_STEP: &str = "hover-reconnect";

/// Kiro's own words (and Hover's, when the agent's process ended) for a cloud session whose
/// connection dropped while it worked on. The last two are what Kiro says when this computer
/// loses its internet ("Could not reach the cloud session service…") or drops the link
/// ("The connection dropped before the turn finished…"); both leave the cloud session running.
fn cut_off_text(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("cloud session was lost") || t.contains("connection to the cloud") || (t.contains("connection") && t.contains("lost")) || (t.len() < 40 && t.ends_with(" stopped."))
        || t.contains("could not reach the cloud session") || t.contains("connection dropped before the turn finished")
}

/// A turn that failed only because its connection to the cloud session dropped.
fn cut_off_result(r: &KiroResult) -> bool { r.state == KiroState::Failed && !r.unconfirmed && cut_off_text(&r.text) }

/// The turn's one quiet step while it reconnects.
fn reconnect_step(me: &Weak<Shared>, id: i32, ti: usize, tries: u32, going: bool) {
    let step = KiroStep::new(RECONNECT_STEP, "other", &format!("Reconnecting to the cloud session ({tries})"), None, if going { "in_progress" } else { "completed" });
    if let Some((ks, ())) = with(me, id, |slot, now| {
        let t = &mut slot.s.turns[ti];
        match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
        if t.woke_at.is_none() { t.woke_at = Some(now); }
    }) { ks.raise(vec![Note::Changed]); }
}

/// Kiro's words when the model has too many users (seen in its issue tracker and CLI).
fn busy_message(text: &str) -> bool {
    let t = text.to_lowercase();
    ["high volume of traffic", "high traffic", "high demand", "too many requests", "trouble responding right now", "overloaded"].iter().any(|p| t.contains(p))
}

/// A Kiro turn that failed only because the model is busy, with the setting on and no Stop:
/// it goes again. Short messages only, so an answer that merely mentions these words never loops.
fn busy_again(me: &Weak<Shared>, id: i32, ct: &Cancel, r: &KiroResult) -> bool {
    if ct.is_cancelled() || r.unconfirmed || r.state != KiroState::Failed || r.text.len() > 600 || !busy_message(&r.text) { return false; }
    let Some(sh) = me.upgrade() else { return false };
    if !sh.inner.lock().unwrap().all.iter().any(|x| x.s.id == id && x.s.tool == AgentTool::Kiro) { return false; }
    let f = sh.retry_busy.lock().unwrap().clone();
    match f { Some(f) => f(), None => hover_core::settings::load_model(&hover_core::paths::settings_file()).retry_busy() }
}

/// The turn's one quiet step for it: in progress while the next try starts, done once it has.
fn retry_step(me: &Weak<Shared>, id: i32, ti: usize, tries: u32, going: bool) {
    let step = KiroStep::new(RETRY_STEP, "other", &format!("Retrying after high demand ({tries})"), None, if going { "in_progress" } else { "completed" });
    if let Some((ks, ())) = with(me, id, |slot, now| {
        let t = &mut slot.s.turns[ti];
        match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
        if t.woke_at.is_none() { t.woke_at = Some(now); }
    }) { ks.raise(vec![Note::Changed]); }
}

/// What the compaction step says. `said` is Kiro's answer to /compact.
pub fn compact_title(usage: f64, state: Option<KiroState>, said: &str) -> String {
    let full = format!("{usage:.0}% full");
    match state {
        None => format!("Compacting the conversation ({full})"),
        Some(KiroState::Completed) if said.to_lowercase().contains("nothing to compact") => format!("Nothing to compact yet (the context is {full})"),
        Some(KiroState::Completed) => format!("Compacted the conversation (it was {full})"),
        Some(KiroState::Failed) => format!("Couldn't compact the conversation (it was {full})"),
        Some(_) => format!("Stopped while compacting the conversation ({full})"),
    }
}

/// The percent the setting asks for now: the app's reader, else settings.json.
fn compact_at(sh: &Shared) -> Option<u8> {
    let f = sh.compact.lock().unwrap().clone();
    match f { Some(f) => f(), None => hover_core::settings::load_model(&hover_core::paths::settings_file()).auto_compact() }
}

/// Kiro's last reported context when it calls for a compaction before this prompt: the
/// setting is on and the context is at or past its percent. Taking it is what keeps
/// a session from compacting twice in a row: only a new report from a turn of its own
/// asks for the next one.
fn due_compact(me: &Weak<Shared>, id: i32) -> Option<f64> {
    let sh = me.upgrade()?;
    {
        let g = sh.inner.lock().unwrap();
        let slot = g.all.iter().find(|x| x.s.id == id)?;
        if slot.s.tool != AgentTool::Kiro || slot.usage.is_none() { return None; }
    }
    let at = compact_at(&sh)?;
    let mut g = sh.inner.lock().unwrap();
    let slot = g.all.iter_mut().find(|x| x.s.id == id)?;
    let used = slot.usage.filter(|u| *u >= at as f64)?;
    slot.usage = None;
    Some(used)
}

/// Kiro only: with auto compact on and the context past its percent, a compaction
/// (a run of exactly `/compact`, which acp.rs sends as Kiro's `_kiro/session/compact`) goes
/// first and shows in the turn as one quiet step. Access is the session's own: compacting
/// changes no files. Some(result) when the compaction was stopped, which stops the turn too;
/// one that failed is only said, and the reply goes on.
#[allow(clippy::too_many_arguments)]
fn compact_first(me: &Weak<Shared>, id: i32, ti: usize, run: &RunTask, folder: &str, resume: &Option<String>, access: &Option<String>,
    tag: &Option<String>, ct: &Cancel) -> Option<KiroResult> {
    let used = due_compact(me, id)?;
    let put = |title: String, status: &str, ms: Option<f64>, said: Option<String>| {
        let step = KiroStep { ms, output: said, ..KiroStep::new(COMPACT_STEP, "other", &title, None, status) };
        if let Some((ks, ())) = with(me, id, |slot, now| {
            let t = &mut slot.s.turns[ti];
            match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
            if t.woke_at.is_none() { t.woke_at = Some(now); }
        }) { ks.raise(vec![Note::Changed]); }
    };
    put(compact_title(used, None, ""), "in_progress", None, None);
    let m = me.clone();
    // What it reports is shown, but is no new report to compact on: that comes from the reply.
    let events = Box::new(move |e: KiroEvent| {
        if let Some((ks, ())) = with(&m, id, |slot, _| {
            if let Some(i) = e.session_id { slot.s.kiro_id = Some(i); }
            if let Some(c) = e.context { slot.s.context = Some(c); }
        }) { ks.raise(vec![Note::Changed]); }
    });
    let began = std::time::Instant::now();
    let args = RunArgs { folder: folder.into(), prompt: crate::acp::COMPACT_PROMPT.into(), progress: Box::new(|_| {}), ct: ct.clone(), resume: resume.clone(), events, access: access.clone(), tag: tag.clone(), cloud: None };
    let r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(args))) {
        Ok(r) => r,
        Err(_) => KiroResult::new(KiroState::Failed, "The compaction failed."),
    };
    let ms = Some(began.elapsed().as_secs_f64() * 1000.0);
    let stopped = ct.is_cancelled() || r.unconfirmed || r.state == KiroState::Cancelled;
    let state = if stopped { KiroState::Cancelled } else { r.state };
    hover_core::log::line(&format!("kiro run {id} compact at {used:.0}%: {}", state.name().to_lowercase()));
    // Kiro's own words are kept with the step (not drawn for this kind of step).
    put(compact_title(used, Some(state), &r.text), if state == KiroState::Completed { "completed" } else { "failed" }, ms,
        Some(crate::stream::clip(r.text.trim(), 2000)).filter(|t| !t.is_empty()));
    stopped.then_some(r)
}

/// KiroSession.Go: one turn, on its own thread.
fn go(me: Weak<Shared>, id: i32, ti: usize, run: RunTask, ct: Cancel, cp: Option<(Arc<Checkpoints>, String)>, (folder, prompt, resume, access, cloud): (String, String, Option<String>, Option<String>, Option<Vec<String>>),
    prior: Option<KiroResult>) {
    // The folder as it is before the agent touches it (and again after, below).
    let before = cp.as_ref().and_then(|(c, key)| c.snapshot(key, &folder));
    if before.is_some() { let b = before.clone(); with(&me, id, move |slot, _| slot.s.turns[ti].before = b); }
    let kept_folder = folder.clone();
    let (m1, m2) = (me.clone(), me.clone());
    let progress: Arc<dyn Fn(KiroPhase) + Send + Sync> = Arc::new(move |p: KiroPhase| {
        if let Some((ks, true)) = with(&m1, id, |slot, now| {
            if !slot.s.busy() || slot.s.phase == p { return false; }
            slot.s.phase = p;
            if p != KiroPhase::Starting { let t = &mut slot.s.turns[ti]; if t.woke_at.is_none() { t.woke_at = Some(now); } }
            true
        }) { ks.raise(vec![Note::Changed]); }
    });
    let events: Arc<dyn Fn(KiroEvent) + Send + Sync> = Arc::new(move |e: KiroEvent| {
        if let Some((ks, fresh)) = with(&m2, id, |slot, now| {
            // A Kiro Web session's id is written to disk as soon as Kiro gives it, not when the turn
            // ends: if Hover closes or dies first, the next start can only rejoin it with the id.
            let mut fresh = None;
            if let Some(i) = e.session_id {
                if slot.s.cloud.is_some() && slot.s.kiro_id.as_deref() != Some(i.as_str()) { slot.s.kiro_id = Some(i); fresh = Some(slot.s.clone()); }
                else { slot.s.kiro_id = Some(i); }
            }
            if let Some(c) = e.context { slot.s.context = Some(c); slot.usage = Some(c); }
            if let Some(c) = e.credits { slot.s.turns[ti].credits = Some(c); }
            if let Some(step) = e.step {
                let t = &mut slot.s.turns[ti];
                match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
                if t.woke_at.is_none() { t.woke_at = Some(now); }
            }
            fresh
        }) {
            if let Some(snap) = fresh { ks.save(&snap); }
            ks.raise(vec![Note::Changed]);
        }
    });
    let tag = me.upgrade().and_then(|sh| sh.inner.lock().unwrap().all.iter().find(|x| x.s.id == id).map(|x| x.s.key.clone()));
    // Kiro's cloud compacts its own conversations.
    let stopped = if cloud.is_some() { None } else { compact_first(&me, id, ti, &run, &folder, &resume, &access, &tag, &ct) };
    // One run of the tool, with this prompt and conversation.
    let attempt = |prompt: &str, resume: &Option<String>| -> KiroResult {
        let args = RunArgs { folder: folder.clone(), prompt: prompt.to_owned(), progress: { let p = progress.clone(); Box::new(move |x| p(x)) }, ct: ct.clone(), resume: resume.clone(),
            events: { let e = events.clone(); Box::new(move |x| e(x)) }, access: access.clone(), tag: tag.clone(), cloud: cloud.clone() };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(args))) {
            Ok(r) => r,
            Err(p) => KiroResult::new(KiroState::Failed, p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "The run failed.".into())),
        }
    };
    let (first_prompt, mut prompt, mut resume) = (prompt.clone(), prompt, resume);
    let mut tries = 0u32;
    let mut r = match stopped {
        Some(r) => r,
        None => loop {
            let r = attempt(&prompt, &resume);
            if !busy_again(&me, id, &ct, &r) { break r; }
            tries += 1;
            hover_core::log::line(&format!("kiro run {id} turn {}: the model is busy, continuing (try {tries}): {}", ti + 1, crate::stream::clip(r.text.trim(), 200)));
            retry_step(&me, id, ti, tries, true);
            // ponytail: one second between tries, not none: an instant loop hammers a busy server. It waits in slices so Stop ends it.
            for _ in 0..10 { if ct.is_cancelled() { break; } std::thread::sleep(std::time::Duration::from_millis(100)); }
            if ct.is_cancelled() { break KiroResult::new(KiroState::Cancelled, "Stopped before Kiro finished."); }
            // The conversation it has by now (a first prompt that never got one is sent again, not "continue").
            resume = with(&me, id, |slot, _| slot.s.kiro_id.clone()).and_then(|(_, k)| k);
            prompt = if resume.is_some() { "continue".into() } else { first_prompt.clone() };
            retry_step(&me, id, ti, tries, false);
        },
    };
    // Kiro Web: a cloud turn whose connection dropped, or that Hover closed on, is still working in the
    // cloud. Attach to it again (5 s, 10, 20, 40, then a minute apart, eight tries) and carry on from there;
    // if it sends nothing new or can't be reached, the turn ends as it had.
    if cloud.is_some() && !ct.is_cancelled() && (prior.is_some() || cut_off_result(&r)) {
        let first = prior.is_some();
        let keep = prior.unwrap_or_else(|| r.clone());
        let mut n = 0u32;
        let mut cur = first.then(|| r.clone());
        r = loop {
            let res = match cur.take() {
                Some(x) => x,
                None => {
                    n += 1;
                    reconnect_step(&me, id, ti, n, true);
                    let wait = (5u64 << (n - 1).min(3)).min(60);
                    hover_core::log::line(&format!("kiro run {id} turn {}: reconnecting to the cloud session in {wait} s (try {n})", ti + 1));
                    for _ in 0..wait * 10 { if ct.is_cancelled() { break; } std::thread::sleep(std::time::Duration::from_millis(100)); }
                    if ct.is_cancelled() { break KiroResult::new(KiroState::Cancelled, "Stopped before Kiro finished."); }
                    let now = with(&me, id, |slot, _| slot.s.kiro_id.clone()).and_then(|(_, k)| k).or_else(|| resume.clone());
                    attempt(crate::acp::ATTACH_PROMPT, &now)
                }
            };
            if ct.is_cancelled() { break KiroResult::new(KiroState::Cancelled, "Stopped before Kiro finished."); }
            if res.text == crate::acp::ATTACH_NOTHING { break keep.clone(); }
            let again = res.state == KiroState::Failed && (res.text.starts_with(crate::acp::ATTACH_FAILED) || cut_off_result(&res));
            if again && n < 8 { continue; }
            break if again { keep.clone() } else { res };
        };
        hover_core::log::line(&format!("kiro run {id} turn {}: after {n} reconnects the turn is {}", ti + 1, r.state.name().to_lowercase()));
        if n > 0 { reconnect_step(&me, id, ti, n, false); }
    }
    if ct.is_cancelled() && r.state != KiroState::Completed && !r.unconfirmed { r.state = KiroState::Cancelled; }
    let after = if before.is_some() { cp.as_ref().and_then(|(c, key)| c.snapshot(key, &kept_folder)) } else { None };
    let Some((ks, (snap, next, denied))) = with(&me, id, |slot, now| {
        slot.cancel = None;
        slot.s.turns[ti].after = after;
        let pausing = std::mem::take(&mut slot.pausing);
        slot.s.stopping = false;
        // A question the run left behind has nobody to answer it now.
        let denied = slot.deny_all();
        let t = &mut slot.s.turns[ti];
        t.result = Some(r.clone());
        t.ended_at = Some(now);
        slot.s.state = r.state;
        let secs = now.secs_since(&slot.s.turns[ti].started_at);
        hover_core::log::line(&format!("{} run {} turn {} {} after {:.0}s (exit {})", slot.s.tool.id(), slot.s.id, ti + 1, slot.s.state.name().to_lowercase(), secs,
            r.exit_code.map_or("-".into(), |c| c.to_string())));
        // A stop drops the replies that were waiting; a pause sends the next one, once the
        // tool has said the turn ended. A stop the tool never confirmed sends nothing:
        // what it was doing may still go on (a pause keeps them queued, a stop drops them).
        // Stop holds what is waiting: the replies stay, in order, and go only when the user resumes the queue (or
        // sends a new message). A stop the tool never confirmed sends nothing either.
        let mut next = slot.s.turns.iter().any(|t| t.queued);
        if next && r.unconfirmed && pausing {
            next = false;
            slot.s.held = true;
        } else if next && !pausing && (r.state == KiroState::Cancelled || r.unconfirmed) {
            slot.s.held = true;
            next = false;
        } else if next && r.state == KiroState::Failed && crate::limit::detect(&r.text, Stamp::now().unix_ms(), 0).is_some() {
            // The provider's usage ran out: what waits would only meet the same wall, so it is held until the task is continued.
            slot.s.held = true;
            next = false;
        }
        (slot.s.clone(), next, denied)
    }) else { return };
    for d in denied { d.deny(); }
    ks.save(&snap);
    ks.raise(vec![Note::Changed, Note::Ended(snap, r)]);
    if next {
        let mut g = ks.0.inner.lock().unwrap();
        if g.all.iter().any(|x| x.s.id == id) {
            let b = ks.begin(&mut g, id);
            drop(g);
            ks.raise(vec![Note::Changed]);
            b();
        }
    }
}
