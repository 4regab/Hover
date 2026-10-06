//! Watching a pull request for a task: when something that needs attention happens, the task that owns the watch
//! is told, once, with enough to act on.
//!
//! - **What wakes it.** A check failing; the required checks all finishing; a review or comment from someone else, new or
//!   edited; a merge conflict; the pull request being merged or closed. The user picks the events and writes what the task
//!   should do about them. Everything the task does happens under the access it already has.
//! - **Once.** A cursor (which failures, reviews and comments were already reported) is saved with the watch, so nothing is
//!   reported twice, not after a restart either, and the first look at a pull request only records how it stands. Items the user's
//!   own account wrote are not news to the task that wrote them.
//! - **One poll per pull request.** Watches of the same address share a single `gh` call per round, by one timer per address.
//!   A quiet pull request is looked at less and less often (1 to 10 minutes). GitHub's rate limit pauses the watch with the time to
//!   try again; a pull request that can't be found or read three times in a row ends it, with the reason.
//! - **No restarts of stopped work.** The owner is told only when it is idle, finished well and wasn't stopped; news that comes
//!   while it works waits and goes as one message when it is done. Stopping, deleting or putting away the owner ends its watches.
//!   A helper the owner started gets no watch of its own and inherits none.
//! - **Passing checks authorize nothing.** The message says so; merging is the user's.

use crate::session::{KiroSessions, Msg};
use crate::wake::{now_ms, Timer, Wake};
use hover_core::json::{self, Json};
use hover_core::model::KiroState;
use hover_core::store::Sealed;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

const KIND: &str = "watch";

#[derive(Clone, Debug, PartialEq)]
pub struct Events { pub failed: bool, pub done: bool, pub review: bool, pub conflict: bool, pub closed: bool }

impl Events {
    pub fn all() -> Events { Events { failed: true, done: true, review: true, conflict: true, closed: true } }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cursor {
    pub primed: bool,
    pub failed: Vec<String>,
    pub done: bool,
    pub conflict: bool,
    /// Review and comment ids with a fingerprint of their text, so an edit is seen.
    pub seen: BTreeMap<String, String>,
    pub sig: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WState { Active, Paused(String), Ended(String) }

#[derive(Clone, Debug, PartialEq)]
pub struct Watch {
    pub id: String,
    /// The task that owns it (a session key). Nothing else inherits it.
    pub session: String,
    pub url: String,
    pub events: Events,
    pub instruction: String,
    /// The user's own GitHub login, whose reviews and comments are not news.
    pub me: Option<String>,
    pub cursor: Cursor,
    pub pending: Vec<String>,
    pub state: WState,
    pub last_checked: i64,
    pub next_check: i64,
    pub quiet: u32,
    pub fails: u32,
    pub sent: u32,
    pub label: String,
}

// MARK: What a poll sees

#[derive(Clone, Debug, PartialEq)]
pub struct Check { pub name: String, /** `pass`, `fail`, `pending` or `skip` */ pub bucket: String }

#[derive(Clone, Debug, PartialEq)]
pub struct Note { pub id: String, pub author: String, pub text: String, pub kind: String }

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot { pub number: i64, pub title: String, pub state: String, pub mergeable: String, pub checks: Vec<Check>, pub notes: Vec<Note>, pub updated: String }

#[derive(Clone, Debug, PartialEq)]
pub enum PollErr {
    /// Try again at this time (ms), when GitHub said.
    RateLimited(Option<i64>),
    /// It can't be found or read: wrong address, no access, signed out.
    Lost(String),
    Other(String),
}

pub trait Poller: Send + Sync {
    fn poll(&self, url: &str) -> Result<Snapshot, PollErr>;
    /// The signed-in login, if known.
    fn me(&self) -> Option<String> { None }
}

fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
    format!("{h:016x}")
}

/// Reads `gh pr view --json …`’s answer.
pub fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    let v = json::parse(text).map_err(|e| format!("gh’s answer couldn’t be read: {e}"))?;
    let s = |x: &Json, k: &str| x.get(k).and_then(Json::as_str).unwrap_or("").to_owned();
    let login = |x: &Json| x.get("author").and_then(|a| a.get("login")).and_then(Json::as_str).unwrap_or("").to_owned();
    let mut checks = vec![];
    if let Some(Json::Arr(list)) = v.get("statusCheckRollup") {
        for c in list {
            let name = { let n = s(c, "name"); if n.is_empty() { s(c, "context") } else { n } };
            let (status, conclusion, state) = (s(c, "status"), s(c, "conclusion"), s(c, "state"));
            let bucket = if !state.is_empty() {
                match state.as_str() { "SUCCESS" => "pass", "PENDING" | "EXPECTED" => "pending", _ => "fail" }
            } else if status != "COMPLETED" && !status.is_empty() { "pending" }
            else { match conclusion.as_str() { "SUCCESS" => "pass", "NEUTRAL" | "SKIPPED" => "skip", "" => "pending", _ => "fail" } };
            checks.push(Check { name, bucket: bucket.into() });
        }
    }
    let mut notes = vec![];
    for (key, kind) in [("reviews", "review"), ("comments", "comment")] {
        if let Some(Json::Arr(list)) = v.get(key) {
            for n in list {
                let id = { let i = s(n, "id"); if i.is_empty() { format!("{kind}:{}", fnv(&format!("{}{}", s(n, "createdAt"), s(n, "submittedAt")))) } else { i } };
                let text = format!("{}|{}", s(n, "state"), s(n, "body"));
                notes.push(Note { id: format!("{kind}:{id}"), author: login(n), text, kind: kind.into() });
            }
        }
    }
    Ok(Snapshot { number: v.get("number").and_then(|n| n.i64().ok()).unwrap_or(0), title: s(&v, "title"), state: s(&v, "state"), mergeable: s(&v, "mergeable"), checks, notes, updated: s(&v, "updatedAt") })
}

/// The real poller: the GitHub CLI the desk already uses.
pub struct GhPoller(pub Arc<crate::github::GitHubCli>);

impl Poller for GhPoller {
    fn poll(&self, url: &str) -> Result<Snapshot, PollErr> {
        let Some(exe) = self.0.exe() else { return Err(PollErr::Lost("The GitHub CLI (gh) isn’t installed.".into())) };
        let r = crate::github::run(&exe, None, std::time::Duration::from_secs(30), 4 << 20, &["pr", "view", url, "--json", "number,title,state,mergeable,statusCheckRollup,reviews,comments,updatedAt"], None, self.0.env());
        if r.ok() { return parse_snapshot(&r.out).map_err(PollErr::Other); }
        let e = r.err.to_lowercase();
        if e.contains("rate limit") || e.contains("secondary rate") || e.contains("http 429") { return Err(PollErr::RateLimited(None)); }
        if ["could not resolve", "not found", "http 404", "gh auth login", "not logged", "http 401", "http 403", "no pull requests found"].iter().any(|k| e.contains(k)) { return Err(PollErr::Lost(crate::desk::gh_reason(&r.err))); }
        Err(PollErr::Other(r.err.lines().next().unwrap_or("gh failed").trim().to_owned()))
    }

    fn me(&self) -> Option<String> { self.0.check(false).user }
}

// MARK: What changed

/// The news in `snap` for `events`, and the cursor after it. The first look only records how things stand.
pub fn diff(w: &Watch, snap: &Snapshot) -> (Vec<String>, Cursor) {
    let mut c = w.cursor.clone();
    let mut news = vec![];
    let failing: Vec<String> = { let mut f: Vec<String> = snap.checks.iter().filter(|x| x.bucket == "fail").map(|x| x.name.clone()).collect(); f.sort(); f.dedup(); f };
    let all_done = !snap.checks.is_empty() && snap.checks.iter().all(|x| x.bucket != "pending");
    let conflicting = snap.mergeable == "CONFLICTING";
    let mine = |a: &str| w.me.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(a));
    if c.primed {
        if w.events.failed { for f in failing.iter().filter(|f| !c.failed.contains(f)) { news.push(format!("The check “{f}” failed.")); } }
        if w.events.done && all_done && !c.done {
            let (pass, fail) = (snap.checks.iter().filter(|x| x.bucket == "pass").count(), failing.len());
            news.push(if fail == 0 { format!("All {} checks finished and passed. (That does not authorize merging.)", snap.checks.len()) } else { format!("All checks finished: {pass} passed, {fail} failed.") });
        }
        if w.events.conflict && conflicting && !c.conflict { news.push("The pull request now has a merge conflict with its base branch.".into()); }
        if w.events.review {
            for n in &snap.notes {
                if mine(&n.author) { continue; }
                let fp = fnv(&n.text);
                match c.seen.get(&n.id) {
                    None => news.push(format!("A new {} from {}: {}", n.kind, n.author, crate::handoff::clip(n.text.split_once('|').map_or(n.text.as_str(), |x| x.1).trim(), 300))),
                    Some(old) if *old != fp => news.push(format!("{} edited a {}: {}", n.author, n.kind, crate::handoff::clip(n.text.split_once('|').map_or(n.text.as_str(), |x| x.1).trim(), 300))),
                    _ => {}
                }
            }
        }
        if w.events.closed && matches!(snap.state.as_str(), "MERGED" | "CLOSED") { news.push(format!("The pull request was {}.", snap.state.to_lowercase())); }
    }
    c.primed = true;
    c.failed = failing;
    c.done = all_done;
    c.conflict = conflicting;
    for n in &snap.notes { c.seen.insert(n.id.clone(), fnv(&n.text)); }
    c.sig = fnv(&format!("{}|{}|{:?}|{}", snap.updated, snap.state, snap.checks.iter().map(|x| (&x.name, &x.bucket)).collect::<Vec<_>>(), snap.notes.len()));
    (news, c)
}

/// Seconds to the next look at a quiet pull request: a minute, then longer, to ten.
pub fn interval_ms(quiet: u32) -> i64 { 60_000 * (1 + quiet.min(9) as i64) }

/// After this many failures in a row, try again after this long (an hour at most).
pub fn backoff_ms(fails: u32) -> i64 { (60_000i64 << fails.min(6)).min(3_600_000) }

// MARK: The watcher

#[derive(Default)]
struct St { watches: Vec<Watch>, seq: u64 }

pub struct Watcher {
    me: Weak<Watcher>,
    st: Mutex<St>,
    wake: Arc<Wake>,
    sessions: KiroSessions,
    poller: Arc<dyn Poller>,
    doc: Option<Sealed>,
    stopped: Box<dyn Fn(&str) -> bool + Send + Sync>,
}

impl Watcher {
    /// `stopped` says whether a session was stopped by the user (orch.rs).
    pub fn new(sessions: KiroSessions, wake: Arc<Wake>, poller: Arc<dyn Poller>, doc: Option<Sealed>, stopped: impl Fn(&str) -> bool + Send + Sync + 'static) -> Arc<Watcher> {
        let watches = doc.as_ref().and_then(Sealed::read).map(|v| load(&v)).unwrap_or_default();
        let w = Arc::new_cyclic(|me| Watcher { me: me.clone(), st: Mutex::new(St { watches, seq: 0 }), wake: wake.clone(), sessions: sessions.clone(), poller, doc, stopped: Box::new(stopped) });
        let me = w.me.clone();
        wake.on(KIND, move |t, _| { if let Some(w) = me.upgrade() { w.round(t); } });
        let me = w.me.clone();
        sessions.on_stop(move |s| { if let Some(w) = me.upgrade() { w.end_for(&s.key, "The task was stopped, put away or deleted."); } });
        let me = w.me.clone();
        sessions.on_ended(move |s, r| { if r.state == KiroState::Completed { if let Some(w) = me.upgrade() { w.deliver(&s.key); } } });
        w
    }

    /// Sets the timers of the active watches (this process is the one that runs timers).
    pub fn arm_all(&self) {
        let urls: Vec<String> = { let g = self.st.lock().unwrap(); let mut u: Vec<String> = g.watches.iter().filter(|w| w.state == WState::Active).map(|w| w.url.clone()).collect(); u.sort(); u.dedup(); u };
        for u in urls { if self.wake.get(KIND, &u).is_none() { self.wake.set(KIND, &u, now_ms() + 2_000, ""); } }
    }

    fn save(&self, g: &St) {
        if let Some(d) = &self.doc { let _ = d.write(&dump(&g.watches)); }
    }

    pub fn list(&self) -> Vec<Watch> { self.st.lock().unwrap().watches.clone() }
    pub fn of(&self, session: &str) -> Vec<Watch> { self.st.lock().unwrap().watches.iter().filter(|w| w.session == session).cloned().collect() }

    /// Starts watching `url` for the task `session`. One watch per task and address.
    pub fn watch(&self, session: &str, url: &str, events: Events, instruction: &str) -> Result<String, String> {
        if !(url.starts_with("https://github.com/") && url.contains("/pull/")) { return Err("That isn’t a GitHub pull request address.".into()); }
        if self.sessions.find(session).is_none() { return Err("That task isn’t open.".into()); }
        let id = {
            let mut g = self.st.lock().unwrap();
            if let Some(w) = g.watches.iter_mut().find(|w| w.session == session && w.url == url) {
                // Watching again restarts an ended watch and changes what it looks for; what was already reported stays reported.
                (w.events, w.instruction, w.state, w.fails) = (events, instruction.trim().into(), WState::Active, 0);
                let id = w.id.clone();
                self.save(&g);
                id
            } else {
                g.seq += 1;
                let id = format!("w-{}", hover_core::guid_n().chars().take(10).collect::<String>());
                g.watches.push(Watch { id: id.clone(), session: session.into(), url: url.into(), events, instruction: instruction.trim().into(), me: self.poller.me(), cursor: Cursor::default(), pending: vec![], state: WState::Active, last_checked: 0, next_check: 0, quiet: 0, fails: 0, sent: 0, label: url.rsplit('/').next().map(|n| format!("#{n}")).unwrap_or_default() });
                self.save(&g);
                id
            }
        };
        self.wake.set(KIND, url, now_ms() + 500, "");
        Ok(id)
    }

    pub fn unwatch(&self, id: &str) {
        let url = { let mut g = self.st.lock().unwrap(); let url = g.watches.iter().find(|w| w.id == id).map(|w| w.url.clone()); g.watches.retain(|w| w.id != id); self.save(&g); url };
        if let Some(u) = url { self.retime(&u); }
    }

    /// Ends every watch of a session (stopped, deleted, put away), and whatever waited to be sent.
    pub fn end_for(&self, session: &str, why: &str) {
        let urls: Vec<String> = {
            let mut g = self.st.lock().unwrap();
            let mut u = vec![];
            for w in g.watches.iter_mut().filter(|w| w.session == session && !matches!(w.state, WState::Ended(_))) { w.state = WState::Ended(why.into()); w.pending.clear(); u.push(w.url.clone()); }
            self.save(&g);
            u
        };
        for u in urls { self.retime(&u); }
    }

    /// The timer for an address exists while some watch of it is active.
    fn retime(&self, url: &str) {
        let active = self.st.lock().unwrap().watches.iter().any(|w| w.url == url && w.state == WState::Active);
        if !active { self.wake.cancel(KIND, url); }
    }

    /// One look at an address, for every watch of it.
    fn round(&self, t: &Timer) {
        if !self.wake.current(KIND, &t.key, t.gen) { return; }
        let url = t.key.clone();
        let mine: Vec<Watch> = self.st.lock().unwrap().watches.iter().filter(|w| w.url == url && w.state == WState::Active).cloned().collect();
        if mine.is_empty() { return; }
        let result = self.poller.poll(&url);
        let now = now_ms();
        let mut next = now + interval_ms(0);
        let mut quiet_min = u32::MAX;
        let mut touched = vec![];
        {
            let mut g = self.st.lock().unwrap();
            for w in g.watches.iter_mut().filter(|w| w.url == url && w.state == WState::Active) {
                w.last_checked = now;
                match &result {
                    Ok(snap) => {
                        w.fails = 0;
                        if !snap.title.is_empty() { w.label = format!("#{} {}", snap.number, crate::stream::clip_to(&snap.title, 50)); }
                        let (news, cursor) = diff(w, snap);
                        let changed = cursor.sig != w.cursor.sig;
                        w.quiet = if changed { 0 } else { (w.quiet + 1).min(9) };
                        w.cursor = cursor;
                        if !news.is_empty() { w.pending.extend(news); touched.push(w.session.clone()); }
                        if w.events.closed && matches!(snap.state.as_str(), "MERGED" | "CLOSED") { w.state = WState::Ended(format!("The pull request was {}.", snap.state.to_lowercase())); }
                        quiet_min = quiet_min.min(w.quiet);
                    }
                    Err(PollErr::RateLimited(at)) => {
                        // A limit is not a failure of the watch: it waits for the limit to lift (fifteen minutes when GitHub gave no time).
                        let retry = at.unwrap_or(now + 900_000);
                        w.state = WState::Paused(format!("GitHub’s rate limit was reached. Trying again at {}.", crate::sched::civil_text(retry, crate::sched::Tz::Local)));
                        next = next.max(retry);
                    }
                    Err(PollErr::Lost(why)) => {
                        w.fails += 1;
                        if w.fails >= 3 { w.state = WState::Ended(format!("Stopped after 3 tries: {why}")); } else { next = now + backoff_ms(w.fails); }
                    }
                    Err(PollErr::Other(why)) => {
                        w.fails += 1;
                        if w.fails >= 5 { w.state = WState::Ended(format!("Stopped after 5 tries: {why}")); } else { next = now + backoff_ms(w.fails); }
                    }
                }
            }
            if result.is_ok() && quiet_min != u32::MAX { next = now + interval_ms(quiet_min); }
            // A paused (rate limited) watch is looked at again at its time, as an active one.
            for w in g.watches.iter_mut().filter(|w| w.url == url && matches!(w.state, WState::Paused(_))) { w.next_check = next; }
            for w in g.watches.iter_mut().filter(|w| w.url == url && w.state == WState::Active) { w.next_check = next; }
            self.save(&g);
        }
        let again = self.st.lock().unwrap().watches.iter().any(|w| w.url == url && matches!(w.state, WState::Active | WState::Paused(_)));
        if again {
            // A paused watch comes back when its time does.
            { let mut g = self.st.lock().unwrap(); for w in g.watches.iter_mut().filter(|w| w.url == url && matches!(w.state, WState::Paused(_))) { w.state = WState::Active; } self.save(&g); }
            self.wake.set(KIND, &url, next, "");
        }
        touched.sort();
        touched.dedup();
        for s in touched { self.deliver(&s); }
    }

    /// Tells the owner what waits, in one message, when it is idle, finished well and wasn't stopped. News that comes while it
    /// works waits; a stopped owner's news is dropped with its watches.
    pub fn deliver(&self, session: &str) {
        let batch: Vec<(String, String, Vec<String>, String)> = {
            let g = self.st.lock().unwrap();
            g.watches.iter().filter(|w| w.session == session && !w.pending.is_empty()).map(|w| (w.id.clone(), w.label.clone(), w.pending.clone(), w.instruction.clone())).collect()
        };
        if batch.is_empty() { return; }
        let Some(s) = self.sessions.find(session) else { return };
        if (self.stopped)(session) { self.end_for(session, "The task was stopped."); return; }
        if s.busy() || s.state != KiroState::Completed { return; }
        let mut text = String::new();
        for (id, label, news, instruction) in &batch {
            let seq = self.st.lock().unwrap().watches.iter().find(|w| &w.id == id).map_or(0, |w| w.sent + 1);
            text += &format!("[Hover] Pull request {label} (hover-watch:{id}:{seq}):\n{}\n", news.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n"));
            if !instruction.is_empty() { text += &format!("What to do about it: {instruction}\n"); }
            text += "\n";
        }
        if self.sessions.reply_msg(s.id, Msg::text(text.trim_end())) {
            let mut g = self.st.lock().unwrap();
            for (id, _, news, _) in &batch { if let Some(w) = g.watches.iter_mut().find(|w| &w.id == id) { w.pending.drain(..news.len().min(w.pending.len())); w.sent += 1; } }
            self.save(&g);
        }
    }
}

fn dump(ws: &[Watch]) -> Json {
    let strs = |v: &[String]| Json::Arr(v.iter().map(Json::str).collect());
    Json::obj(vec![("Watches", Json::Arr(ws.iter().map(|w| Json::obj(vec![
        ("Id", Json::str(&w.id)), ("Session", Json::str(&w.session)), ("Url", Json::str(&w.url)), ("Instruction", Json::str(&w.instruction)), ("Me", Json::opt_str_of(w.me.as_deref())), ("Label", Json::str(&w.label)),
        ("Events", Json::obj(vec![("Failed", Json::Bool(w.events.failed)), ("Done", Json::Bool(w.events.done)), ("Review", Json::Bool(w.events.review)), ("Conflict", Json::Bool(w.events.conflict)), ("Closed", Json::Bool(w.events.closed))])),
        ("Cursor", Json::obj(vec![("Primed", Json::Bool(w.cursor.primed)), ("Failed", strs(&w.cursor.failed)), ("Done", Json::Bool(w.cursor.done)), ("Conflict", Json::Bool(w.cursor.conflict)), ("Sig", Json::str(&w.cursor.sig)),
            ("Seen", Json::Obj(w.cursor.seen.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect()))])),
        ("Pending", strs(&w.pending)),
        ("State", match &w.state { WState::Active => Json::str("active"), WState::Paused(r) => Json::str(format!("paused:{r}")), WState::Ended(r) => Json::str(format!("ended:{r}")) }),
        ("LastChecked", Json::int(w.last_checked)), ("NextCheck", Json::int(w.next_check)), ("Quiet", Json::int(w.quiet as i64)), ("Fails", Json::int(w.fails as i64)), ("Sent", Json::int(w.sent as i64)),
    ])).collect()))])
}

fn load(v: &Json) -> Vec<Watch> {
    let Some(Json::Arr(list)) = v.get("Watches") else { return vec![] };
    let s = |x: &Json, k: &str| x.get(k).and_then(Json::as_str).unwrap_or("").to_owned();
    let n = |x: &Json, k: &str| x.get(k).and_then(|y| y.i64().ok()).unwrap_or(0);
    let b = |x: &Json, k: &str| x.get(k).and_then(|y| y.bool().ok()).unwrap_or(false);
    let strs = |x: Option<&Json>| -> Vec<String> { match x { Some(Json::Arr(a)) => a.iter().filter_map(|y| y.as_str().map(str::to_owned)).collect(), _ => vec![] } };
    list.iter().map(|w| {
        let e = w.get("Events").cloned().unwrap_or(Json::Null);
        let c = w.get("Cursor").cloned().unwrap_or(Json::Null);
        let state = s(w, "State");
        Watch { id: s(w, "Id"), session: s(w, "Session"), url: s(w, "Url"), instruction: s(w, "Instruction"), me: w.get("Me").and_then(Json::as_str).map(str::to_owned), label: s(w, "Label"),
            events: Events { failed: b(&e, "Failed"), done: b(&e, "Done"), review: b(&e, "Review"), conflict: b(&e, "Conflict"), closed: b(&e, "Closed") },
            cursor: Cursor { primed: b(&c, "Primed"), failed: strs(c.get("Failed")), done: b(&c, "Done"), conflict: b(&c, "Conflict"), sig: s(&c, "Sig"),
                seen: match c.get("Seen") { Some(Json::Obj(o)) => o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect(), _ => BTreeMap::new() } },
            pending: strs(w.get("Pending")),
            state: if let Some(r) = state.strip_prefix("paused:") { WState::Paused(r.into()) } else if let Some(r) = state.strip_prefix("ended:") { WState::Ended(r.into()) } else { WState::Active },
            last_checked: n(w, "LastChecked"), next_check: n(w, "NextCheck"), quiet: n(w, "Quiet") as u32, fails: n(w, "Fails") as u32, sent: n(w, "Sent") as u32 }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(me: Option<&str>) -> Watch {
        Watch { id: "w1".into(), session: "s".into(), url: "u".into(), events: Events::all(), instruction: String::new(), me: me.map(str::to_owned), cursor: Cursor::default(), pending: vec![], state: WState::Active,
            last_checked: 0, next_check: 0, quiet: 0, fails: 0, sent: 0, label: String::new() }
    }

    fn snap(checks: &[(&str, &str)], notes: &[(&str, &str, &str)], mergeable: &str, state: &str) -> Snapshot {
        Snapshot { number: 12, title: "T".into(), state: state.into(), mergeable: mergeable.into(),
            checks: checks.iter().map(|(n, b)| Check { name: (*n).into(), bucket: (*b).into() }).collect(),
            notes: notes.iter().map(|(id, a, t)| Note { id: (*id).into(), author: (*a).into(), text: (*t).into(), kind: "comment".into() }).collect(), updated: "t0".into() }
    }

    #[test]
    fn the_first_look_only_records_and_later_looks_report_each_change_once() {
        let mut watch = w(Some("me"));
        let s0 = snap(&[("build", "pending"), ("lint", "pass")], &[("comment:1", "bob", "|hello")], "MERGEABLE", "OPEN");
        let (news, c) = diff(&watch, &s0);
        assert!(news.is_empty(), "the first look is only a baseline: {news:?}");
        watch.cursor = c;
        // The build fails.
        let s1 = snap(&[("build", "fail"), ("lint", "pass")], &[("comment:1", "bob", "|hello")], "MERGEABLE", "OPEN");
        let (news, c) = diff(&watch, &s1);
        assert_eq!(news, ["The check “build” failed.", "All checks finished: 1 passed, 1 failed."]);
        watch.cursor = c;
        assert!(diff(&watch, &s1).0.is_empty(), "the same state is not news again");
        // A retry passes, then fails again: that is news again.
        let ok = snap(&[("build", "pass"), ("lint", "pass")], &[("comment:1", "bob", "|hello")], "MERGEABLE", "OPEN");
        let (news, c) = diff(&watch, &ok);
        assert!(news.is_empty(), "already told that all finished; the pass is not a new round: {news:?}");
        watch.cursor = c;
        let (news, _) = diff(&watch, &s1);
        assert_eq!(news, ["The check “build” failed."]);
    }

    #[test]
    fn reviews_and_comments_from_others_count_new_and_edited_but_your_own_do_not() {
        let mut watch = w(Some("me"));
        watch.cursor = diff(&watch, &snap(&[], &[("comment:1", "bob", "|first")], "MERGEABLE", "OPEN")).1;
        let s = snap(&[], &[("comment:1", "bob", "|first"), ("comment:2", "alice", "|please fix the name"), ("comment:3", "me", "|my own note")], "MERGEABLE", "OPEN");
        let (news, c) = diff(&watch, &s);
        assert_eq!(news, ["A new comment from alice: please fix the name"]);
        watch.cursor = c;
        let edited = snap(&[], &[("comment:1", "bob", "|first, edited"), ("comment:2", "alice", "|please fix the name")], "MERGEABLE", "OPEN");
        assert_eq!(diff(&watch, &edited).0, ["bob edited a comment: first, edited"]);
        // Events the user didn't pick are not reported.
        watch.events.review = false;
        assert!(diff(&watch, &edited).0.is_empty());
    }

    #[test]
    fn a_conflict_and_the_end_of_the_pull_request_are_reported_once() {
        let mut watch = w(None);
        watch.cursor = diff(&watch, &snap(&[], &[], "MERGEABLE", "OPEN")).1;
        let (news, c) = diff(&watch, &snap(&[], &[], "CONFLICTING", "OPEN"));
        assert_eq!(news, ["The pull request now has a merge conflict with its base branch."]);
        watch.cursor = c;
        assert!(diff(&watch, &snap(&[], &[], "CONFLICTING", "OPEN")).0.is_empty());
        assert_eq!(diff(&watch, &snap(&[], &[], "MERGEABLE", "MERGED")).0, ["The pull request was merged."]);
    }

    #[test]
    fn gh_answers_are_read_as_checks_reviews_and_comments() {
        let s = parse_snapshot(r#"{"number":12,"title":"Fix login","state":"OPEN","mergeable":"CONFLICTING","updatedAt":"2026-10-06T10:00:00Z",
          "statusCheckRollup":[{"__typename":"CheckRun","name":"build","status":"COMPLETED","conclusion":"FAILURE"},{"__typename":"CheckRun","name":"lint","status":"IN_PROGRESS","conclusion":""},
            {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED"},{"__typename":"StatusContext","context":"ci/old","state":"SUCCESS"}],
          "reviews":[{"id":"R1","author":{"login":"bob"},"state":"CHANGES_REQUESTED","body":"no","submittedAt":"x"}],
          "comments":[{"id":"C1","author":{"login":"amy"},"body":"hi","createdAt":"y"}]}"#).unwrap();
        assert_eq!((s.number, s.state.as_str(), s.mergeable.as_str()), (12, "OPEN", "CONFLICTING"));
        assert_eq!(s.checks.iter().map(|c| (c.name.as_str(), c.bucket.as_str())).collect::<Vec<_>>(), [("build", "fail"), ("lint", "pending"), ("docs", "skip"), ("ci/old", "pass")]);
        assert_eq!(s.notes.iter().map(|n| (n.id.as_str(), n.author.as_str(), n.kind.as_str())).collect::<Vec<_>>(), [("review:R1", "bob", "review"), ("comment:C1", "amy", "comment")]);
        assert!(parse_snapshot("not json").is_err());
    }

    #[test]
    fn looking_slows_down_when_quiet_and_backs_off_after_trouble() {
        assert_eq!((interval_ms(0), interval_ms(3), interval_ms(50)), (60_000, 240_000, 600_000));
        assert_eq!((backoff_ms(1), backoff_ms(2), backoff_ms(20)), (120_000, 240_000, 3_600_000));
    }
}
