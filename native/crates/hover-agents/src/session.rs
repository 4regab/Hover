//! Owl/KiroSession.cs: a session (a folder, the first prompt and every reply, each a
//! turn) and all of them (KiroSessions: at most three running across the tools, the
//! newest six kept at desks, every one in the history until deleted). The C# lives on
//! the UI thread; here the sessions sit behind one lock, runs go on threads of their
//! own, and `changed` and `ended` are raised with the lock released, off any thread.

use crate::cancel::Cancel;
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
}

impl KiroTurn {
    pub fn new(prompt: &str, images: Vec<String>) -> KiroTurn {
        KiroTurn { prompt: prompt.into(), images, steps: vec![], result: None, queued: false, started_at: Stamp::DEFAULT, woke_at: None, ended_at: None }
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
}

/// String.Split('\n', RemoveEmptyEntries | TrimEntries).FirstOrDefault().
pub(crate) fn first_line(s: &str) -> &str { s.split('\n').map(str::trim).find(|l| !l.is_empty()).unwrap_or("") }

impl KiroSession {
    /// A new session with no turns (the C# constructor).
    pub fn new(tool: AgentTool) -> KiroSession {
        KiroSession { id: IDS.fetch_add(1, Ordering::SeqCst) + 1, tool, state: KiroState::Idle, phase: KiroPhase::Starting, folder: String::new(), turns: vec![],
            kiro_id: None, context: None, seat: 0, bot: 0, key: hover_core::guid_n(), deleted: false }
    }

    pub fn busy(&self) -> bool { self.state == KiroState::Running }
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
                ended_at: t.ended_at }).collect(),
            updated: now,
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
        for t in &s.turns {
            let mut turn = KiroTurn::new(&t.prompt, t.images.clone());
            turn.started_at = t.started_at;
            turn.woke_at = t.woke_at;
            turn.ended_at = Some(t.ended_at.unwrap_or(t.started_at));
            turn.steps = t.steps.clone();
            turn.result = Some(KiroResult::new(t.state.unwrap_or(KiroState::Cancelled), t.text.clone().unwrap_or_else(|| "Stopped when Hover closed.".into())));
            self.turns.push(turn);
        }
        self.state = self.turns.last().map_or(KiroState::Cancelled, |t| t.result.as_ref().unwrap().state);
    }
}

fn usable(text: &str, images: &[String]) -> bool { !text.trim().is_empty() || !images.is_empty() }

struct Slot { s: KiroSession, cancel: Option<Cancel>, run: RunTask }

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
    pub fn get(&self, id: i32) -> Option<KiroSession> { self.0.inner.lock().unwrap().all.iter().find(|x| x.s.id == id).map(|x| x.s.clone()) }
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
    pub fn start(&self, tool: AgentTool, folder: &str, prompt: &str, images: Vec<String>) -> Option<KiroSession> {
        let mut g = self.0.inner.lock().unwrap();
        let running = g.all.iter().filter(|x| x.s.busy()).count();
        if running >= MAX_RUNNING || !crate::usable_folder(Some(folder)) || !usable(prompt, &images) { return None; }
        if !Self::free_desk(&mut g) { return None; }
        let mut s = KiroSession::new(tool);
        Self::seat(&g, &mut s);
        s.folder = folder.into();
        s.turns.push(KiroTurn::new(prompt.trim(), images));
        let id = s.id;
        g.all.push(Slot { s, cancel: None, run: (self.0.make)(tool) });
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
        let ct = Cancel::new();
        slot.cancel = Some(ct.clone());
        let args_base = (slot.s.folder.clone(), slot.s.turns[ti].text(), slot.s.kiro_id.clone());
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
        t.queued = slot.s.busy();
        let queued = t.queued;
        slot.s.turns.push(t);
        let begun = if queued { None } else { Some(self.begin(&mut g, id)) };
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
        g.all.push(Slot { s, cancel: None, run: (self.0.make)(saved.tool) });
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

    /// Stops the turn that runs; replies waiting behind it are not sent.
    pub fn stop(&self, id: i32) {
        let c = self.0.inner.lock().unwrap().all.iter().find(|x| x.s.id == id && x.s.busy()).and_then(|x| x.cancel.clone());
        if let Some(c) = c { c.cancel(); }
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
        f(slot, now)
    };
    Some((ks, r))
}

/// KiroSession.Go: one turn, on its own thread.
fn go(me: Weak<Shared>, id: i32, ti: usize, run: RunTask, ct: Cancel, (folder, prompt, resume): (String, String, Option<String>)) {
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
            if let Some(step) = e.step {
                let t = &mut slot.s.turns[ti];
                match t.steps.iter().position(|x| x.id == step.id) { Some(i) => t.steps[i] = step, None => t.steps.push(step) }
                if t.woke_at.is_none() { t.woke_at = Some(now); }
            }
        }) { ks.raise(vec![Note::Changed]); }
    });
    let args = RunArgs { folder, prompt, progress, ct: ct.clone(), resume, events };
    let mut r = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(args))) {
        Ok(r) => r,
        Err(p) => KiroResult::new(KiroState::Failed, p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "The run failed.".into())),
    };
    if ct.is_cancelled() && r.state != KiroState::Completed { r.state = KiroState::Cancelled; }
    let Some((ks, (snap, next))) = with(&me, id, |slot, now| {
        slot.cancel = None;
        let t = &mut slot.s.turns[ti];
        t.result = Some(r.clone());
        t.ended_at = Some(now);
        slot.s.state = r.state;
        let secs = now.secs_since(&slot.s.turns[ti].started_at);
        hover_core::log::line(&format!("{} run {} turn {} {} after {:.0}s (exit {})", slot.s.tool.id(), slot.s.id, ti + 1, slot.s.state.name().to_lowercase(), secs,
            r.exit_code.map_or("-".into(), |c| c.to_string())));
        // A stop drops the replies that were waiting; otherwise the next one goes.
        let mut next = slot.s.turns.iter().any(|t| t.queued);
        if next && r.state == KiroState::Cancelled {
            for q in slot.s.turns.iter_mut().filter(|t| t.queued) {
                q.queued = false;
                q.started_at = now;
                q.ended_at = Some(now);
                q.result = Some(KiroResult::new(KiroState::Cancelled, "Not sent: the run before it was stopped."));
            }
            next = false;
        }
        (slot.s.clone(), next)
    }) else { return };
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
