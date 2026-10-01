//! Owl/KiroSession.cs: a session (a folder, the first prompt and every reply, each a
//! turn) and all of them (KiroSessions: at most three running across the tools, the
//! newest six kept at desks, every one in the history until deleted). The C# lives on
//! the UI thread; here the sessions sit behind one lock, runs go on threads of their
//! own, and `changed` and `ended` are raised with the lock released, off any thread.

use crate::ask::{AgentAsk, Answers, AskAnswer};
use crate::cancel::{Cancel, Registration};
use crate::stream::{KiroEvent, KiroPhase, KiroResult};
use hover_core::history::{AgentHistory, SavedSession, SavedTurn};
use hover_core::model::{AgentTool, KiroState, KiroStep};
use hover_core::time::Stamp;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub const MAX_RUNNING: usize = 3;
pub const MAX_KEPT: usize = 6;

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
}

impl KiroTurn {
    pub fn new(prompt: &str, images: Vec<String>) -> KiroTurn {
        KiroTurn { prompt: prompt.into(), images, steps: vec![], result: None, queued: false, started_at: Stamp::DEFAULT, woke_at: None, ended_at: None, credits: None }
    }

    /// What the agent is sent: the prompt, then the pictures' paths for it to look at.
    pub fn text(&self) -> String {
        if self.images.is_empty() { return self.prompt.clone(); }
        let head = if self.prompt.is_empty() { "Look at the attached image." } else { &self.prompt };
        format!("{head}\n\n{}", self.images.iter().map(|p| format!("Attached image (read it from this file): {p}")).collect::<Vec<_>>().join("\n"))
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
            kiro_id: None, context: None, seat: 0, bot: 0, key: hover_core::guid_n(), deleted: false, access: None, asks: vec![], stopping: false, rev: 0 }
    }

    /// A copy without what only the chat reads: the answers' text, and the steps'
    /// changes and output. What the notch, the desks and the panels draw is all there.
    pub fn light(&self) -> KiroSession {
        KiroSession {
            turns: self.turns.iter().map(|t| KiroTurn {
                prompt: t.prompt.clone(), images: t.images.clone(),
                steps: t.steps.iter().map(|x| KiroStep { id: x.id.clone(), kind: x.kind.clone(), title: x.title.clone(), target: x.target.clone(), status: x.status.clone(),
                    added: x.added, removed: x.removed, diff: None, output: None, exit: x.exit, ms: x.ms }).collect(),
                result: t.result.as_ref().map(|r| KiroResult { state: r.state, text: String::new(), exit_code: r.exit_code, unconfirmed: r.unconfirmed }),
                queued: t.queued, started_at: t.started_at, woke_at: t.woke_at, ended_at: t.ended_at, credits: t.credits,
            }).collect(),
            folder: self.folder.clone(), kiro_id: self.kiro_id.clone(), key: self.key.clone(), access: self.access.clone(), asks: self.asks.clone(),
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
                ended_at: t.ended_at, credits: t.credits }).collect(),
            updated: now,
            access: self.access.clone(),
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
        for t in &s.turns {
            let mut turn = KiroTurn::new(&t.prompt, t.images.clone());
            turn.started_at = t.started_at;
            turn.woke_at = t.woke_at;
            turn.ended_at = Some(t.ended_at.unwrap_or(t.started_at));
            turn.credits = t.credits;
            turn.steps = t.steps.clone();
            turn.result = Some(KiroResult::new(t.state.unwrap_or(KiroState::Cancelled), t.text.clone().unwrap_or_else(|| "Stopped when Hover closed.".into())));
            self.turns.push(turn);
        }
        self.state = self.turns.last().map_or(KiroState::Cancelled, |t| t.result.as_ref().unwrap().state);
    }
}

fn usable(text: &str, images: &[String]) -> bool { !text.trim().is_empty() || !images.is_empty() }

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
struct Slot { s: KiroSession, cancel: Option<Cancel>, run: RunTask, asks: Vec<Pending>, pausing: bool }

impl Slot {
    fn new(s: KiroSession, run: RunTask) -> Slot { Slot { s, cancel: None, run, asks: vec![], pausing: false } }

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

struct Shared {
    inner: Mutex<Inner>,
    make: Box<dyn Fn(AgentTool) -> RunTask + Send + Sync>,
    history: Option<Arc<AgentHistory>>,
    now: Box<dyn Fn() -> Stamp + Send + Sync>,
    changed: Mutex<Vec<Changed>>,
    ended: Mutex<Vec<Ended>>,
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
            changed: Mutex::new(vec![]), ended: Mutex::new(vec![]) }))
    }

    pub fn history(&self) -> Option<&Arc<AgentHistory>> { self.0.history.as_ref() }
    pub fn now(&self) -> Stamp { (self.0.now)() }

    /// Any session changed, or one came or went. Off any thread.
    pub fn on_changed(&self, f: impl Fn() + Send + Sync + 'static) { self.0.changed.lock().unwrap().push(Arc::new(f)); }
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
    pub fn running(&self) -> usize { self.0.inner.lock().unwrap().all.iter().filter(|x| x.s.busy()).count() }
    pub fn can_start(&self) -> bool { self.running() < MAX_RUNNING }
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
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.s.busy()).count();
        if running >= MAX_RUNNING || !crate::usable_folder(Some(folder)) || !usable(prompt, &images) { return None; }
        if !Self::free_desk(&mut g) { return None; }
        let mut s = KiroSession::new(tool);
        Self::seat(&g, &mut s);
        s.folder = folder.into();
        s.access = access.map(str::to_owned);
        s.turns.push(KiroTurn::new(prompt.trim(), images));
        let id = s.id;
        g.all.push(Slot::new(s, (self.0.make)(tool)));
        let begun = self.begin(&mut g, id);
        g.selected = Some(id);
        let snap = g.all.iter().find(|x| x.s.id == id).unwrap().s.clone();
        drop(g);
        self.save(&snap);
        self.raise(vec![Note::Changed, Note::Changed]);
        begun();
        Some(snap)
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
        let args_base = (slot.s.folder.clone(), slot.s.turns[ti].text(), slot.s.kiro_id.clone(), slot.s.access.clone());
        let run = slot.run.clone();
        let me = Arc::downgrade(&self.0);
        Box::new(move || {
            std::thread::Builder::new().name("agent-turn".into()).spawn(move || go(me, id, ti, run, ct, args_base)).expect("a thread for the turn");
        })
    }

    /// A reply. While a turn runs it waits and starts when that one ends. False when the
    /// session isn't here or hasn't started, there is nothing to send, or it would start
    /// a fourth run.
    pub fn reply(&self, id: i32, text: &str, images: Vec<String>) -> bool {
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.s.busy()).count();
        let Some(slot) = g.all.iter_mut().find(|x| x.s.id == id) else { return false };
        if !slot.s.busy() && running >= MAX_RUNNING { return false; }
        if slot.s.state == KiroState::Idle || !usable(text, &images) { return false; }
        let mut t = KiroTurn::new(text.trim(), images);
        // Replies left queued (behind a stop that wasn't confirmed) go first, in order.
        let start_now = !slot.s.busy();
        t.queued = slot.s.busy() || slot.s.turns.iter().any(|t| t.queued);
        slot.s.turns.push(t);
        slot.s.rev += 1;
        let begun = if start_now { Some(self.begin(&mut g, id)) } else { None };
        let snap = g.all.iter().find(|x| x.s.id == id).unwrap().s.clone();
        drop(g);
        self.raise(vec![Note::Changed]);
        if let Some(b) = begun { b(); }
        self.save(&snap);
        true
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
        g.all.push(Slot::new(s, (self.0.make)(saved.tool)));
        drop(g);
        self.raise(vec![Note::Changed]);
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
        g.all.remove(i);
        if g.selected == Some(id) { g.selected = None; }
        drop(g);
        self.raise(vec![Note::Changed]);
    }

    /// The user deleted a session: a run is stopped, and it leaves the office and the history.
    pub fn delete(&self, key: &str) {
        let mut g = self.0.inner.lock().unwrap();
        if let Some(i) = g.all.iter().position(|x| x.s.key == key) {
            let mut slot = g.all.remove(i);
            slot.s.deleted = true;
            if slot.s.busy() { if let Some(c) = &slot.cancel { c.cancel(); } }
            if g.selected == Some(slot.s.id) { g.selected = None; }
        }
        drop(g);
        if let Some(h) = &self.0.history { h.delete(key); }
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

    pub fn stop_all(&self) {
        let cs: Vec<Cancel> = self.0.inner.lock().unwrap().all.iter().filter(|x| x.s.busy()).filter_map(|x| x.cancel.clone()).collect();
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

/// KiroSession.Go: one turn, on its own thread.
fn go(me: Weak<Shared>, id: i32, ti: usize, run: RunTask, ct: Cancel, (folder, prompt, resume, access): (String, String, Option<String>, Option<String>)) {
    let (m1, m2) = (me.clone(), me.clone());
    let progress = Box::new(move |p: KiroPhase| {
        if let Some((ks, true)) = with(&m1, id, |slot, now| {
            if !slot.s.busy() || slot.s.phase == p { return false; }
            slot.s.phase = p;
            if p != KiroPhase::Starting { let t = &mut slot.s.turns[ti]; if t.woke_at.is_none() { t.woke_at = Some(now); } }
            true
        }) { ks.raise(vec![Note::Changed]); }
    });
    let events = Box::new(move |e: KiroEvent| {
        if let Some((ks, ())) = with(&m2, id, |slot, now| {
            if let Some(i) = e.session_id { slot.s.kiro_id = Some(i); }
            if let Some(c) = e.context { slot.s.context = Some(c); }
            if let Some(c) = e.credits { slot.s.turns[ti].credits = Some(c); }
            if let Some(step) = e.step {
                let t = &mut slot.s.turns[ti];
                match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
                if t.woke_at.is_none() { t.woke_at = Some(now); }
            }
        }) { ks.raise(vec![Note::Changed]); }
    });
    let args = RunArgs { folder, prompt, progress, ct: ct.clone(), resume, events, access };
    let mut r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(args))) {
        Ok(r) => r,
        Err(p) => KiroResult::new(KiroState::Failed, p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "The run failed.".into())),
    };
    if ct.is_cancelled() && r.state != KiroState::Completed && !r.unconfirmed { r.state = KiroState::Cancelled; }
    let Some((ks, (snap, next, denied))) = with(&me, id, |slot, now| {
        slot.cancel = None;
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
        let mut next = slot.s.turns.iter().any(|t| t.queued);
        if next && r.unconfirmed && pausing {
            next = false;
        } else if next && !pausing && (r.state == KiroState::Cancelled || r.unconfirmed) {
            for q in slot.s.turns.iter_mut().filter(|t| t.queued) {
                q.queued = false;
                q.started_at = now;
                q.ended_at = Some(now);
                q.result = Some(KiroResult::new(KiroState::Cancelled, "Not sent: the run before it was stopped."));
            }
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
