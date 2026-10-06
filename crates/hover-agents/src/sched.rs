//! Saved tasks that run by themselves: once, on a repeat, or when a webhook calls. A task is a project folder, a
//! prompt, an agent, a workspace rule, an access level, a schedule and a time zone. It runs the way a task started by
//! hand runs (same sessions, same limits, same worktrees, same questions to the user) and leaves a record of each run.
//!
//! Rules kept here:
//! - Due times come from the schedule and a stored time zone (`Tz`): the computer's own (with its daylight saving) or
//!   a fixed offset. Daily times are wall-clock times, so they stay 09:00 through a clock change.
//! - A run that was due while Hover wasn't running is made up *once* when Hover is back, with a note saying so; a task never
//!   gets a backlog. One that is still going when its next time comes is skipped, with a note.
//! - The same due time is never dispatched twice (each run records the time it was for), and a disabled or removed task
//!   cannot be started by a wake-up that was already on its way (`Wake::current`).
//! - A scheduled run asks the user as any run does. Nobody being at the screen never answers for them: the question waits.
//!   Access defaults to “ask first”.
//! - A webhook (webhook.rs) starts the same run with the fields the user chose from what it sent. What it sends grants no
//!   access the task doesn't already have.

use crate::cancel::Cancel;
use crate::orch::Env;
use crate::session::{KiroSessions, Target};
use crate::wake::{now_ms, Timer, Wake};
use crate::workspace::{self, Choice};
use hover_core::ext::SessionExt;
use hover_core::json::Json;
use hover_core::store::Sealed;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

/// Later than this and a due run counts as missed (sleep, Hover closed).
pub const GRACE_MS: i64 = 120_000;
const KEEP_RUNS: usize = 30;
const MAX_WAITING: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tz { Local, Fixed(i32) }

impl Tz {
    pub fn offset_min(self, utc_ms: i64) -> i64 {
        match self {
            Tz::Fixed(m) => m as i64,
            Tz::Local => hover_core::time::local_offset_min(hover_core::time::Stamp::from_unix_ms(utc_ms, hover_core::time::Kind::Utc).utc_ticks()),
        }
    }
    pub fn name(self) -> String { match self { Tz::Local => "local".into(), Tz::Fixed(m) => format!("{}{:02}:{:02}", if m < 0 { '-' } else { '+' }, m.abs() / 60, m.abs() % 60) } }
    pub fn parse(s: &str) -> Option<Tz> {
        if s == "local" { return Some(Tz::Local); }
        let (sign, rest) = (s.chars().next()?, &s[1..]);
        let (h, m) = rest.split_once(':')?;
        let v = h.parse::<i32>().ok()? * 60 + m.parse::<i32>().ok()?;
        match sign { '+' => Some(Tz::Fixed(v)), '-' => Some(Tz::Fixed(-v)), _ => None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Schedule {
    /// Only when run now or called by a webhook.
    Manual,
    Once { at: i64 },
    /// Every so many minutes after the last time it was due.
    Every { minutes: u32 },
    /// At this wall-clock time on the days whose bit is set (bit 0 is Sunday).
    Daily { hour: u8, minute: u8, days: u8 },
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

/// (year, month, day) of a count of days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// 0 is Sunday.
fn weekday(days: i64) -> u8 { (days + 4).rem_euclid(7) as u8 }

const DAY_MS: i64 = 86_400_000;

/// The first time after `after` the schedule is due, with `offset_of(utc_ms)` giving the zone's offset in minutes then.
pub fn next_due(s: Schedule, after: i64, offset_of: &dyn Fn(i64) -> i64) -> Option<i64> {
    match s {
        Schedule::Manual => None,
        Schedule::Once { at } => (at > after).then_some(at),
        Schedule::Every { minutes } => Some(after + minutes.max(1) as i64 * 60_000),
        Schedule::Daily { hour, minute, days } => {
            if days & 0x7f == 0 { return None; }
            let today = (after + offset_of(after) * 60_000).div_euclid(DAY_MS);
            for d in 0..=8 {
                if days & (1 << weekday(today + d)) == 0 { continue; }
                let wall = (today + d) * DAY_MS + (hour as i64 * 60 + minute as i64) * 60_000;
                // Wall clock to UTC: the offset in force at that moment, found by one correction. A time that doesn't exist
                // (the hour skipped in spring) falls on the next minute that does; one that happens twice is taken at its first.
                let first = wall - offset_of(after) * 60_000;
                let utc = wall - offset_of(first) * 60_000;
                if utc > after { return Some(utc); }
            }
            None
        }
    }
}

/// “2026-11-02 09:30” as that wall-clock time in `tz`, in ms since 1970. None when it isn’t a real date and time.
pub fn parse_in(text: &str, tz: Tz) -> Option<i64> {
    let (date, time) = text.trim().split_once(' ')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (h, mi) = time.split_once(':').and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)))?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || !(0..24).contains(&h) || !(0..60).contains(&mi) || !(1970..=2200).contains(&y) { return None; }
    let wall = days_from_civil(y, m, day) * DAY_MS + (h * 60 + mi) * 60_000;
    if civil_from_days(wall.div_euclid(DAY_MS)) != (y, m, day) { return None; }
    let first = wall - tz.offset_min(wall) * 60_000;
    Some(wall - tz.offset_min(first) * 60_000)
}

/// `parse_in` for the computer’s own zone.
pub fn parse_local(text: &str) -> Option<i64> { parse_in(text, Tz::Local) }

pub fn civil_text(utc_ms: i64, tz: Tz) -> String {
    let local = utc_ms + tz.offset_min(utc_ms) * 60_000;
    let (y, m, d) = civil_from_days(local.div_euclid(DAY_MS));
    let mins = local.rem_euclid(DAY_MS) / 60_000;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", mins / 60, mins % 60)
}

// MARK: Tasks

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlap { /** skip a run while the last is still going */ Skip, /** start it anyway */ Allow }

/// A webhook that starts a task.
#[derive(Clone, Debug, PartialEq)]
pub struct Hook {
    pub enabled: bool,
    /// Only these events (GitHub’s `X-GitHub-Event`); none means any.
    pub events: Vec<String>,
    /// What of the delivery goes into the prompt: paths into its JSON, like `/pull_request/title`. Nothing else does.
    pub fields: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunState { Started, Done, Failed, Skipped, Waiting }

impl RunState {
    const NAMES: [&'static str; 5] = ["started", "done", "failed", "skipped", "waiting"];
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    fn parse(s: &str) -> RunState { [RunState::Started, RunState::Done, RunState::Failed, RunState::Skipped, RunState::Waiting][Self::NAMES.iter().position(|n| *n == s).unwrap_or(2)] }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    /// What it was for: the due time (or, for a webhook, the delivery id), which makes a second dispatch of it visible.
    pub due: i64,
    pub why: String,
    pub started: i64,
    pub session: Option<String>,
    pub state: RunState,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub prompt: String,
    /// A provider id (`kiro`, `custom:…`).
    pub provider: String,
    /// `own` (a worktree per run, when the folder is a Git project) or `folder`.
    pub workspace: String,
    pub access: String,
    pub schedule: Schedule,
    pub tz: Tz,
    pub enabled: bool,
    pub overlap: Overlap,
    pub hook: Option<Hook>,
    pub runs: Vec<RunRecord>,
    pub created: i64,
}

impl Task {
    fn to_json(&self) -> Json {
        let (kind, a, b, c) = match self.schedule { Schedule::Manual => ("manual", 0, 0, 0), Schedule::Once { at } => ("once", at, 0, 0), Schedule::Every { minutes } => ("every", minutes as i64, 0, 0), Schedule::Daily { hour, minute, days } => ("daily", hour as i64, minute as i64, days as i64) };
        let hook = self.hook.as_ref().map_or(Json::Null, |h| Json::obj(vec![("Enabled", Json::Bool(h.enabled)), ("Events", Json::Arr(h.events.iter().map(Json::str).collect())), ("Fields", Json::Arr(h.fields.iter().map(Json::str).collect()))]));
        Json::obj(vec![("Id", Json::str(&self.id)), ("Name", Json::str(&self.name)), ("Folder", Json::str(&self.folder)), ("Prompt", Json::str(&self.prompt)), ("Provider", Json::str(&self.provider)),
            ("Workspace", Json::str(&self.workspace)), ("Access", Json::str(&self.access)), ("Kind", Json::str(kind)), ("A", Json::int(a)), ("B", Json::int(b)), ("C", Json::int(c)), ("Tz", Json::str(self.tz.name())),
            ("Enabled", Json::Bool(self.enabled)), ("Overlap", Json::str(if self.overlap == Overlap::Skip { "skip" } else { "allow" })), ("Hook", hook), ("Created", Json::int(self.created)),
            ("Runs", Json::Arr(self.runs.iter().map(|r| Json::obj(vec![("Due", Json::int(r.due)), ("Why", Json::str(&r.why)), ("Started", Json::int(r.started)), ("Session", Json::opt_str_of(r.session.as_deref())), ("State", Json::str(r.state.name())), ("Note", Json::str(&r.note))])).collect()))])
    }

    fn from_json(v: &Json) -> hover_core::json::Result<Task> {
        let s = |x: &Json, k: &str| x.get(k).and_then(Json::as_str).unwrap_or("").to_owned();
        let n = |x: &Json, k: &str| x.get(k).and_then(|y| y.i64().ok()).unwrap_or(0);
        let strs = |x: Option<&Json>| -> Vec<String> { match x { Some(Json::Arr(a)) => a.iter().filter_map(|y| y.as_str().map(str::to_owned)).collect(), _ => vec![] } };
        let schedule = match s(v, "Kind").as_str() { "once" => Schedule::Once { at: n(v, "A") }, "every" => Schedule::Every { minutes: n(v, "A").max(1) as u32 },
            "daily" => Schedule::Daily { hour: n(v, "A").clamp(0, 23) as u8, minute: n(v, "B").clamp(0, 59) as u8, days: n(v, "C") as u8 }, _ => Schedule::Manual };
        let hook = v.get("Hook").filter(|h| !h.is_null()).map(|h| Hook { enabled: h.get("Enabled").and_then(|b| b.bool().ok()).unwrap_or(false), events: strs(h.get("Events")), fields: strs(h.get("Fields")) });
        let runs = match v.get("Runs") { Some(Json::Arr(a)) => a.iter().map(|r| RunRecord { due: n(r, "Due"), why: s(r, "Why"), started: n(r, "Started"), session: r.get("Session").and_then(Json::as_str).map(str::to_owned), state: RunState::parse(&s(r, "State")), note: s(r, "Note") }).collect(), _ => vec![] };
        Ok(Task { id: s(v, "Id"), name: s(v, "Name"), folder: s(v, "Folder"), prompt: s(v, "Prompt"), provider: s(v, "Provider"), workspace: s(v, "Workspace"), access: s(v, "Access"), schedule,
            tz: Tz::parse(&s(v, "Tz")).unwrap_or(Tz::Local), enabled: v.get("Enabled").and_then(|b| b.bool().ok()).unwrap_or(false),
            overlap: if s(v, "Overlap") == "allow" { Overlap::Allow } else { Overlap::Skip }, hook, runs, created: n(v, "Created") })
    }
}

/// What the user fills in.
#[derive(Clone, Debug)]
pub struct NewTask { pub name: String, pub folder: String, pub prompt: String, pub provider: String, pub workspace: String, pub access: String, pub schedule: Schedule, pub tz: Tz, pub hook: Option<Hook> }

/// Where the timers are being run.
#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub executor: bool, pub owner: String, pub tasks: usize, pub enabled: usize }

struct Waiting { task: String, due: i64, why: String, extra: String, note: String }

pub struct Scheduler {
    me: Weak<Scheduler>,
    tasks: Mutex<Vec<Task>>,
    waiting: Mutex<VecDeque<Waiting>>,
    pub wake: Arc<Wake>,
    sessions: KiroSessions,
    env: Arc<dyn Env>,
    doc: Option<Sealed>,
    executor: Mutex<Option<String>>,
    /// Run only what asks nothing (the background service): a task that asks before it acts waits for the app.
    service: std::sync::atomic::AtomicBool,
    hold: std::sync::atomic::AtomicBool,
}

const KIND: &str = "task";

impl Scheduler {
    pub fn new(sessions: KiroSessions, env: Arc<dyn Env>, wake: Arc<Wake>, doc: Option<Sealed>) -> Arc<Scheduler> {
        let tasks = doc.as_ref().and_then(Sealed::read).and_then(|v| match v.get("Tasks") { Some(Json::Arr(a)) => Some(a.iter().filter_map(|t| Task::from_json(t).ok()).collect::<Vec<_>>()), _ => None }).unwrap_or_default();
        let s = Arc::new_cyclic(|me| Scheduler { me: me.clone(), tasks: Mutex::new(tasks), waiting: Mutex::new(VecDeque::new()), wake: wake.clone(), sessions: sessions.clone(), env, doc, executor: Mutex::new(None),
            service: Default::default(), hold: Default::default() });
        let w = s.me.clone();
        wake.on(KIND, move |t, late| { if let Some(s) = w.upgrade() { s.fire(t, late); } });
        let w = s.me.clone();
        sessions.on_ended(move |ses, r| { if let Some(s) = w.upgrade() { s.ended(ses, r.state); } });
        let w = s.me.clone();
        sessions.on_changed(move || { if let Some(s) = w.upgrade() { if !s.waiting.lock().unwrap().is_empty() { s.pump(); } } });
        s
    }

    /// This process is the one that runs timers (`who`), or isn't. The timers of enabled tasks are set, and anything that came due while
    /// nobody ran them is made up once by the wake thread.
    pub fn arm_all(&self, owner: Option<&str>) {
        *self.executor.lock().unwrap() = owner.map(str::to_owned);
        if owner.is_none() { return; }
        let now = now_ms();
        for t in self.list().iter().filter(|t| t.enabled) {
            match self.wake.get(KIND, &t.id) {
                Some(_) => {} // kept from before: overdue or not, it fires now and the catch-up rule applies
                None => if let Some(due) = next_due(t.schedule, now, &|ms| t.tz.offset_min(ms)) { self.wake.set(KIND, &t.id, due, ""); }
            }
        }
        self.wake.start();
        self.requeue();
    }

    /// This copy runs only tasks that never ask (access `full` or `read`); the others wait, with a note, for the app.
    pub fn set_service_mode(&self, on: bool) { self.service.store(on, std::sync::atomic::Ordering::SeqCst); }

    /// Stops starting runs (a handover to the app is under way). Runs already going finish.
    pub fn hold_starts(&self, on: bool) { self.hold.store(on, std::sync::atomic::Ordering::SeqCst); }

    /// Runs that were waiting for a place or for the app when the last owner stopped are started by this one.
    fn requeue(&self) {
        let waiting: Vec<(String, i64, String)> = self.list().iter().filter(|t| t.enabled).flat_map(|t| t.runs.iter().filter(|r| r.state == RunState::Waiting && r.session.is_none() && !r.why.starts_with("hook:")).map(|r| (t.id.clone(), r.due, r.why.clone()))).collect();
        let mut w = self.waiting.lock().unwrap();
        for (task, due, why) in waiting { if !w.iter().any(|x| x.task == task && x.due == due && x.why == why) && w.len() < MAX_WAITING { w.push_back(Waiting { task, due, why, extra: String::new(), note: String::new() }); } }
        drop(w);
        self.pump();
    }

    pub fn status(&self) -> Status {
        let t = self.tasks.lock().unwrap();
        let owner = self.executor.lock().unwrap().clone();
        Status { executor: owner.is_some(), owner: owner.unwrap_or_default(), tasks: t.len(), enabled: t.iter().filter(|t| t.enabled).count() }
    }

    pub fn list(&self) -> Vec<Task> { self.tasks.lock().unwrap().clone() }
    pub fn get(&self, id: &str) -> Option<Task> { self.tasks.lock().unwrap().iter().find(|t| t.id == id).cloned() }

    fn save(&self, t: &[Task]) {
        if let Some(d) = &self.doc { if let Err(e) = d.write(&Json::obj(vec![("Tasks", Json::Arr(t.iter().map(Task::to_json).collect()))])) { hover_core::log::line(&format!("tasks: save failed - {e}")); } }
    }

    fn validate(n: &NewTask) -> Result<(), String> {
        if n.name.trim().is_empty() { return Err("Give the task a name.".into()); }
        if n.prompt.trim().is_empty() { return Err("Say what the task should do.".into()); }
        if Target::parse(&n.provider).is_none() { return Err("Pick an agent for the task.".into()); }
        if !["read", "always", "risky", "full"].contains(&n.access.as_str()) { return Err("Pick how much the agent may do without asking.".into()); }
        if !crate::usable_folder(Some(&n.folder)) { return Err("The folder isn’t there.".into()); }
        if let Schedule::Every { minutes } = n.schedule { if minutes < 5 { return Err("A repeat can be no closer than every 5 minutes.".into()); } }
        if let Schedule::Daily { days, hour, minute } = n.schedule { if days & 0x7f == 0 || hour > 23 || minute > 59 { return Err("Pick at least one day and a real time.".into()); } }
        Ok(())
    }

    pub fn add(&self, n: NewTask) -> Result<String, String> {
        Self::validate(&n)?;
        let id = format!("tk-{}", hover_core::guid_n().chars().take(10).collect::<String>());
        let t = Task { id: id.clone(), name: n.name.trim().into(), folder: n.folder, prompt: n.prompt, provider: n.provider, workspace: if n.workspace == "folder" { "folder".into() } else { "own".into() },
            access: n.access, schedule: n.schedule, tz: n.tz, enabled: true, overlap: Overlap::Skip, hook: n.hook, runs: vec![], created: now_ms() };
        { let mut g = self.tasks.lock().unwrap(); g.push(t.clone()); self.save(&g); }
        self.arm(&t);
        Ok(id)
    }

    pub fn update(&self, id: &str, n: NewTask) -> Result<(), String> {
        Self::validate(&n)?;
        let t = {
            let mut g = self.tasks.lock().unwrap();
            let t = g.iter_mut().find(|t| t.id == id).ok_or("That task isn’t there.")?;
            (t.name, t.folder, t.prompt, t.provider, t.workspace, t.access, t.schedule, t.tz, t.hook) = (n.name.trim().into(), n.folder, n.prompt, n.provider, if n.workspace == "folder" { "folder".into() } else { "own".into() }, n.access, n.schedule, n.tz, n.hook);
            let t = t.clone();
            self.save(&g);
            t
        };
        self.arm(&t);
        Ok(())
    }

    /// Pauses or resumes. Pausing takes the timer away, so a wake-up already on its way finds itself stale.
    pub fn set_enabled(&self, id: &str, on: bool) -> bool {
        let t = { let mut g = self.tasks.lock().unwrap(); let Some(t) = g.iter_mut().find(|t| t.id == id) else { return false }; t.enabled = on; let t = t.clone(); self.save(&g); t };
        self.arm(&t);
        true
    }

    pub fn remove(&self, id: &str) {
        self.wake.cancel(KIND, id);
        self.waiting.lock().unwrap().retain(|w| w.task != id);
        let mut g = self.tasks.lock().unwrap();
        g.retain(|t| t.id != id);
        self.save(&g);
    }

    fn arm(&self, t: &Task) {
        if self.executor.lock().unwrap().is_none() { return; }
        if !t.enabled { self.wake.cancel(KIND, &t.id); return; }
        match next_due(t.schedule, now_ms(), &|ms| t.tz.offset_min(ms)) { Some(due) => { self.wake.set(KIND, &t.id, due, ""); } None => self.wake.cancel(KIND, &t.id) }
    }

    fn record(&self, id: &str, r: RunRecord) {
        let mut g = self.tasks.lock().unwrap();
        if let Some(t) = g.iter_mut().find(|t| t.id == id) {
            if let Some(i) = t.runs.iter().position(|x| x.due == r.due && x.why == r.why) { t.runs[i] = r; } else { t.runs.push(r); }
            let extra = t.runs.len().saturating_sub(KEEP_RUNS);
            t.runs.drain(..extra);
        }
        self.save(&g);
    }

    /// A timer is due.
    fn fire(&self, timer: &Timer, late: i64) {
        if !self.wake.current(KIND, &timer.key, timer.gen) { return; }
        let Some(t) = self.get(&timer.key).filter(|t| t.enabled) else { return };
        let missed = late > GRACE_MS;
        let note = if missed { format!("Missed: it was due {} and Hover wasn’t running it. Made up once, now.", civil_text(timer.due, t.tz)) } else { String::new() };
        self.dispatch(&t, timer.due, "due", &note, "");
        if matches!(t.schedule, Schedule::Once { .. }) { self.set_enabled(&t.id, false); } else { self.arm(&t); }
    }

    /// Runs a task now (the user’s button): the same path as a due run.
    pub fn run_now(&self, id: &str) -> Result<(), String> {
        let t = self.get(id).ok_or("That task isn’t there.")?;
        self.dispatch(&t, now_ms(), "now", "Run by hand.", "");
        Ok(())
    }

    /// A webhook call for the task, with the words its chosen fields made.
    pub fn trigger(&self, id: &str, delivery: &str, extra: &str) -> Result<(), String> {
        let t = self.get(id).filter(|t| t.enabled).ok_or("That task isn’t there or is paused.")?;
        if self.waiting.lock().unwrap().len() >= MAX_WAITING { return Err("Too many runs are waiting.".into()); }
        self.dispatch(&t, now_ms(), &format!("hook:{delivery}"), "Started by a webhook.", extra);
        Ok(())
    }

    fn dispatch(&self, t: &Task, due: i64, why: &str, note: &str, extra: &str) {
        // The same due time is never dispatched twice.
        // (A webhook’s delivery is named by its id, whatever time it arrives.)
        if t.runs.iter().any(|r| r.why == why && (why.starts_with("hook:") || r.due == due) && r.state != RunState::Waiting) { return; }
        if t.overlap == Overlap::Skip {
            let busy = t.runs.iter().rev().find_map(|r| r.session.as_ref()).and_then(|k| self.sessions.find(k)).is_some_and(|s| s.busy());
            if busy { self.record(&t.id, RunRecord { due, why: why.into(), started: now_ms(), session: None, state: RunState::Skipped, note: "Skipped: the last run was still going.".into() }); return; }
        }
        self.record(&t.id, RunRecord { due, why: why.into(), started: now_ms(), session: None, state: RunState::Waiting, note: note.into() });
        let mut w = self.waiting.lock().unwrap();
        if w.len() >= MAX_WAITING { drop(w); self.record(&t.id, RunRecord { due, why: why.into(), started: now_ms(), session: None, state: RunState::Skipped, note: "Skipped: too many runs were waiting for a place.".into() }); return; }
        w.push_back(Waiting { task: t.id.clone(), due, why: why.into(), extra: extra.into(), note: note.into() });
        drop(w);
        self.pump();
    }

    /// Starts waiting runs while places are free. Each start runs on a thread of its own (a worktree may be made).
    pub fn pump(&self) {
        loop {
            if !self.sessions.can_start() || self.hold.load(std::sync::atomic::Ordering::SeqCst) { return; }
            let Some(w) = self.waiting.lock().unwrap().pop_front() else { return };
            let Some(me) = self.me.upgrade() else { return };
            let _ = std::thread::Builder::new().name("task-start".into()).spawn(move || me.start(w));
            // One at a time per call: the next place is counted again when this one has taken its turn.
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn start(&self, w: Waiting) {
        let fail = |note: String| self.record(&w.task, RunRecord { due: w.due, why: w.why.clone(), started: now_ms(), session: None, state: RunState::Failed, note });
        let Some(t) = self.get(&w.task).filter(|t| t.enabled || w.why == "now") else { return };
        let Some(to) = Target::parse(&t.provider) else { return fail("The agent isn’t known.".into()) };
        // The service never answers for the user: a task that asks waits (as it is, Waiting) until the app is open.
        if self.service.load(std::sync::atomic::Ordering::SeqCst) && matches!(t.access.as_str(), "risky" | "always") {
            self.record(&w.task, RunRecord { due: w.due, why: w.why, started: now_ms(), session: None, state: RunState::Waiting, note: "It asks before it acts, so it starts when Hover is open.".into() });
            return;
        }
        if let Some(p) = self.env.providers().into_iter().find(|p| p.id == t.provider) { if !p.ready { return fail(format!("{} isn’t available: {}", p.name, p.hint)); } }
        let title = t.name.clone();
        let choice = if t.workspace == "folder" { Choice::Folder } else { Choice::Own { base: None } };
        let prepared = match workspace::prepare(&t.folder, &choice, &title, &self.env.worktrees(), t.access == "read", false, &Cancel::new()) { Ok(p) => p, Err(e) => return fail(format!("No workspace: {e}")) };
        // No two writers in one folder.
        if prepared.binding.as_ref().is_none_or(|b| !b.is_worktree()) && t.access != "read" && self.sessions.all().iter().any(|s| s.busy() && workspace::overlaps(&s.folder, &prepared.folder)) {
            self.record(&w.task, RunRecord { due: w.due, why: w.why.clone(), started: now_ms(), session: None, state: RunState::Skipped, note: "Skipped: another task is writing in that folder.".into() });
            return;
        }
        let prompt = if w.extra.is_empty() { t.prompt.clone() } else { format!("{}\n\n{}", t.prompt, w.extra) };
        let ext = SessionExt { workspace: prepared.binding.clone(), provider: to.instance.clone(), ..Default::default() };
        match self.sessions.start_bound(to.tool, &prepared.folder, &prompt, vec![], Some(&t.access), None, ext) {
            Some(s) => self.record(&w.task, RunRecord { due: w.due, why: w.why, started: now_ms(), session: Some(s.key), state: RunState::Started, note: [w.note.clone(), prepared.note.unwrap_or_default()].into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join(" ") }),
            None => { self.waiting.lock().unwrap().push_front(w); }
        }
    }

    fn ended(&self, s: &crate::session::KiroSession, state: hover_core::model::KiroState) {
        let mut g = self.tasks.lock().unwrap();
        let mut hit = false;
        for t in g.iter_mut() {
            for r in t.runs.iter_mut().filter(|r| r.session.as_deref() == Some(&s.key) && r.state == RunState::Started) {
                r.state = if state == hover_core::model::KiroState::Completed { RunState::Done } else { RunState::Failed };
                hit = true;
            }
        }
        if hit { self.save(&g); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, m: i64, d: i64, h: i64, mi: i64) -> i64 { days_from_civil(y, m, d) * DAY_MS + (h * 60 + mi) * 60_000 }
    fn zero(_: i64) -> i64 { 0 }

    /// A zone like the US Eastern one: UTC-5, UTC-4 from 2026-03-08 07:00 UTC to 2026-11-01 06:00 UTC.
    fn eastern(ms: i64) -> i64 { if (utc(2026, 3, 8, 7, 0)..utc(2026, 11, 1, 6, 0)).contains(&ms) { -240 } else { -300 } }

    #[test]
    fn dates_and_weekdays_are_right() {
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 6)), (2026, 10, 6));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(weekday(days_from_civil(2026, 10, 6)), 2, "a Tuesday");
        assert_eq!(weekday(0), 4, "1970-01-01 was a Thursday");
    }

    #[test]
    fn one_off_and_repeating_times_follow_the_schedule() {
        let now = utc(2026, 10, 6, 12, 0);
        assert_eq!(next_due(Schedule::Manual, now, &zero), None);
        assert_eq!(next_due(Schedule::Once { at: now + 5 }, now, &zero), Some(now + 5));
        assert_eq!(next_due(Schedule::Once { at: now - 5 }, now, &zero), None, "a one-off in the past does not come back");
        assert_eq!(next_due(Schedule::Every { minutes: 90 }, now, &zero), Some(now + 90 * 60_000));
    }

    #[test]
    fn a_daily_time_is_a_wall_clock_time_on_the_days_chosen() {
        // Weekdays at 09:30 in a zone 2 hours east of UTC.
        let east = |_: i64| 120;
        let mon_fri = 0b0111110;
        let tue_noon = utc(2026, 10, 6, 10, 0);
        assert_eq!(next_due(Schedule::Daily { hour: 9, minute: 30, days: mon_fri }, tue_noon, &east), Some(utc(2026, 10, 7, 7, 30)), "today's 09:30 (07:30 UTC) has passed; tomorrow is Wednesday");
        let fri_eve = utc(2026, 10, 9, 20, 0);
        assert_eq!(next_due(Schedule::Daily { hour: 9, minute: 30, days: mon_fri }, fri_eve, &east), Some(utc(2026, 10, 12, 7, 30)), "the weekend is skipped");
        assert_eq!(next_due(Schedule::Daily { hour: 9, minute: 30, days: 0 }, tue_noon, &east), None);
        // Later today when it hasn't come yet.
        assert_eq!(next_due(Schedule::Daily { hour: 23, minute: 0, days: 0x7f }, utc(2026, 10, 6, 10, 0), &east), Some(utc(2026, 10, 6, 21, 0)));
    }

    #[test]
    fn a_daily_time_stays_the_same_on_the_wall_clock_through_a_clock_change() {
        // 09:00 Eastern every day. Spring forward is 2026-03-08.
        let d = Schedule::Daily { hour: 9, minute: 0, days: 0x7f };
        let sat = next_due(d, utc(2026, 3, 7, 15, 0), &eastern).unwrap();
        assert_eq!(sat, utc(2026, 3, 8, 13, 0), "the 8th, 09:00 EDT, is 13:00 UTC");
        let before = next_due(d, utc(2026, 3, 6, 15, 0), &eastern).unwrap();
        assert_eq!(before, utc(2026, 3, 7, 14, 0), "the 7th, 09:00 EST, is 14:00 UTC");
        // Fall back is 2026-11-01: the day after is 09:00 EST again, 14:00 UTC.
        assert_eq!(next_due(d, utc(2026, 11, 1, 15, 0), &eastern).unwrap(), utc(2026, 11, 2, 14, 0));
        // A time that does not exist (02:30 on the spring-forward day) is still due once that day, at the next real minute.
        let skipped = next_due(Schedule::Daily { hour: 2, minute: 30, days: 0x7f }, utc(2026, 3, 8, 0, 0), &eastern).unwrap();
        assert!(skipped > utc(2026, 3, 8, 0, 0) && skipped < utc(2026, 3, 9, 0, 0), "fires once that day");
        // And never twice in a row: asking again after it moves to the next day.
        assert!(next_due(Schedule::Daily { hour: 2, minute: 30, days: 0x7f }, skipped, &eastern).unwrap() >= skipped + 20 * 3_600_000);
    }

    #[test]
    fn zones_are_named_and_read_back() {
        assert_eq!(Tz::Fixed(-330).name(), "-05:30");
        assert_eq!(Tz::parse("+02:00"), Some(Tz::Fixed(120)));
        assert_eq!(Tz::parse("-05:30"), Some(Tz::Fixed(-330)));
        assert_eq!(Tz::parse("local"), Some(Tz::Local));
        assert_eq!(Tz::parse("mars"), None);
        assert_eq!(civil_text(utc(2026, 10, 6, 13, 5), Tz::Fixed(120)), "2026-10-06 15:05");
        assert_eq!(parse_in("2026-10-06 15:05", Tz::Fixed(120)), Some(utc(2026, 10, 6, 13, 5)));
        assert_eq!(parse_in("2026-02-30 10:00", Tz::Fixed(0)), None, "not a real date");
        assert_eq!(parse_in("2026-10-06 25:00", Tz::Fixed(0)), None);
        assert_eq!(parse_in("tomorrow", Tz::Fixed(0)), None);
    }
}
