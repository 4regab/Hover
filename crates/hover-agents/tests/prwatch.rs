//! Pull request watches: the owner is told once and only when it is idle; one poll serves every watch of an address; a stopped
//! owner is never woken; limits and failures pause or end a watch with the reason; the cursor survives a restart.

use hover_agents::prwatch::{Check, Events, Note, PollErr, Poller, Snapshot, WState, Watcher};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::wake::{now_ms, Wake};
use hover_core::crypto::Crypto;
use hover_core::ext::{OrchLink, SessionExt};
use hover_core::model::{AgentTool, KiroState};
use hover_core::store::Sealed;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const URL: &str = "https://github.com/acme/app/pull/12";

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

struct Fake { now: Mutex<Result<Snapshot, PollErr>>, calls: AtomicUsize }
impl Poller for Fake {
    fn poll(&self, _: &str) -> Result<Snapshot, PollErr> { self.calls.fetch_add(1, Ordering::SeqCst); self.now.lock().unwrap().clone() }
    fn me(&self) -> Option<String> { Some("me".into()) }
}

fn snap(checks: &[(&str, &str)], notes: &[(&str, &str, &str)], state: &str) -> Snapshot {
    Snapshot { number: 12, title: "Fix login".into(), state: state.into(), mergeable: "MERGEABLE".into(), updated: format!("{checks:?}{notes:?}"),
        checks: checks.iter().map(|(n, b)| Check { name: (*n).into(), bucket: (*b).into() }).collect(),
        notes: notes.iter().map(|(i, a, t)| Note { id: (*i).into(), author: (*a).into(), text: (*t).into(), kind: "comment".into() }).collect() }
}

struct Rig { k: KiroSessions, w: Arc<Watcher>, wake: Arc<Wake>, poll: Arc<Fake>, folder: String, hold: Arc<Mutex<bool>>, seen: Arc<Mutex<Vec<String>>>, root: PathBuf, crypto: Arc<Crypto>, stop: Arc<Mutex<Vec<String>>> }

fn rig(name: &str) -> Rig {
    let root = std::env::temp_dir().join(format!("hover-prwatch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("p");
    std::fs::create_dir_all(&folder).unwrap();
    let crypto = Arc::new(Crypto::with_key([2; 32]));
    let (hold, seen): (Arc<Mutex<bool>>, Arc<Mutex<Vec<String>>>) = Default::default();
    let (h2, s2) = (hold.clone(), seen.clone());
    let k = KiroSessions::new(move |_| -> RunTask {
        let (h2, s2) = (h2.clone(), s2.clone());
        Arc::new(move |a: RunArgs| { s2.lock().unwrap().push(a.prompt.clone()); while *h2.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); } KiroResult::new(KiroState::Completed, "ok") })
    }, None);
    let wake = Wake::new(None);
    let poll = Arc::new(Fake { now: Mutex::new(Ok(snap(&[("build", "pending")], &[], "OPEN"))), calls: AtomicUsize::new(0) });
    let stop: Arc<Mutex<Vec<String>>> = Default::default();
    let st2 = stop.clone();
    let w = Watcher::new(k.clone(), wake.clone(), poll.clone(), Some(Sealed::in_dir(&root.join("st"), "watches", crypto.clone())), move |key| st2.lock().unwrap().iter().any(|s| s == key));
    Rig { k, w, wake, poll, folder: folder.to_string_lossy().into_owned(), hold, seen, root, crypto, stop }
}

impl Rig {
    fn task(&self) -> KiroSession {
        let s = self.k.start(AgentTool::Kiro, &self.folder, "Open the pull request", vec![]).unwrap();
        wait_for("the task", || self.k.get(s.id).is_some_and(|x| !x.busy()));
        self.k.get(s.id).unwrap()
    }
    fn set(&self, s: Result<Snapshot, PollErr>) { *self.poll.now.lock().unwrap() = s; }
}

/// Runs one round for the address through the real timer path.
fn round(r: &Rig) {
    let calls = r.poll.calls.load(Ordering::SeqCst);
    r.wake.set("watch", URL, now_ms() - 1, "");
    r.wake.start();
    wait_for("the poll", || r.poll.calls.load(Ordering::SeqCst) > calls);
    std::thread::sleep(Duration::from_millis(80));
}

#[test]
fn a_failed_check_wakes_the_idle_owner_once_and_the_instruction_comes_with_it() {
    let r = rig("once");
    let t = r.task();
    let id = r.w.watch(&t.key, URL, Events::all(), "Fix what failed and push.").unwrap();
    round(&r);
    assert_eq!(r.w.of(&t.key)[0].label, "#12 Fix login");
    assert_eq!(r.seen.lock().unwrap().len(), 1, "the first look only records");
    r.set(Ok(snap(&[("build", "fail")], &[], "OPEN")));
    round(&r);
    wait_for("the message", || r.k.get(t.id).is_some_and(|x| x.turns.len() == 2 && x.turns[1].result.is_some()));
    let msg = r.k.get(t.id).unwrap().turns[1].prompt.clone();
    assert!(msg.contains(&format!("hover-watch:{id}:1")) && msg.contains("The check “build” failed.") && msg.contains("What to do about it: Fix what failed and push."), "{msg}");
    // Looking again at the same state tells nobody anything.
    round(&r);
    round(&r);
    assert_eq!(r.k.get(t.id).unwrap().turns.len(), 2);
    assert!(r.w.of(&t.key)[0].pending.is_empty() && r.w.of(&t.key)[0].sent == 1);
}

#[test]
fn news_while_the_owner_works_waits_and_goes_as_one_message_when_it_is_done() {
    let r = rig("busy");
    let t = r.task();
    r.w.watch(&t.key, URL, Events::all(), "").unwrap();
    round(&r);
    *r.hold.lock().unwrap() = true;
    assert!(r.k.reply(t.id, "work [busy]", vec![]));
    wait_for("the busy turn", || r.k.get(t.id).unwrap().busy());
    r.set(Ok(snap(&[("build", "fail")], &[("c1", "bob", "|needs a test")], "OPEN")));
    round(&r);
    assert_eq!(r.k.get(t.id).unwrap().turns.len(), 2, "not interrupted");
    assert_eq!(r.w.of(&t.key)[0].pending.len(), 3, "it waits: {:?}", r.w.of(&t.key)[0].pending);
    *r.hold.lock().unwrap() = false;
    wait_for("one message after the turn", || r.k.get(t.id).is_some_and(|x| x.turns.len() == 3 && x.turns[2].result.is_some()));
    let msg = r.k.get(t.id).unwrap().turns[2].prompt.clone();
    assert!(msg.contains("The check “build” failed.") && msg.contains("A new comment from bob: needs a test") && msg.contains("All checks finished: 0 passed, 1 failed."), "{msg}");
    assert!(r.w.of(&t.key)[0].pending.is_empty());
}

#[test]
fn a_stopped_owner_is_never_woken_its_watches_end_and_a_helper_inherits_nothing() {
    let r = rig("stop");
    let t = r.task();
    // A helper of the owner (it carries the owner as its parent) has no watch.
    let child = r.k.start_bound(AgentTool::Codex, &r.folder, "help", vec![], None, None, SessionExt { orch: Some(OrchLink { parent: Some(t.key.clone()), root: Some(t.key.clone()), depth: 1, ..Default::default() }), ..Default::default() }).unwrap();
    wait_for("the helper", || r.k.get(child.id).is_some_and(|x| !x.busy()));
    r.w.watch(&t.key, URL, Events::all(), "").unwrap();
    round(&r);
    assert!(r.w.of(&child.key).is_empty(), "a child does not inherit the parent’s watch");
    // Stopping the child leaves the parent’s watch alone.
    *r.hold.lock().unwrap() = true;
    assert!(r.k.reply(child.id, "more", vec![]));
    wait_for("busy", || r.k.get(child.id).unwrap().busy());
    r.k.stop(child.id);
    *r.hold.lock().unwrap() = false;
    assert_eq!(r.w.of(&t.key)[0].state, WState::Active);
    // Stopping the owner ends its watch, and its timer goes with it.
    *r.hold.lock().unwrap() = true;
    assert!(r.k.reply(t.id, "work", vec![]));
    wait_for("busy", || r.k.get(t.id).unwrap().busy());
    r.stop.lock().unwrap().push(t.key.clone());
    r.k.stop(t.id);
    *r.hold.lock().unwrap() = false;
    wait_for("the watch to end", || matches!(r.w.of(&t.key)[0].state, WState::Ended(_)));
    assert!(r.wake.get("watch", URL).is_none());
    r.set(Ok(snap(&[("build", "fail")], &[], "OPEN")));
    r.w.deliver(&t.key);
    std::thread::sleep(Duration::from_millis(100));
    assert!(!r.seen.lock().unwrap().iter().any(|p| p.contains("hover-watch")), "nothing was sent to a stopped task");
}

#[test]
fn two_watches_of_one_pull_request_share_one_poll_and_a_merged_one_ends() {
    let r = rig("shared");
    let a = r.task();
    let b = r.k.start(AgentTool::Codex, &r.folder, "second task", vec![]).unwrap();
    wait_for("second", || r.k.get(b.id).is_some_and(|x| !x.busy()));
    r.w.watch(&a.key, URL, Events::all(), "").unwrap();
    r.w.watch(&r.k.get(b.id).unwrap().key, URL, Events::all(), "").unwrap();
    let before = r.poll.calls.load(Ordering::SeqCst);
    round(&r);
    assert_eq!(r.poll.calls.load(Ordering::SeqCst) - before, 1, "one gh call for both");
    r.set(Ok(snap(&[("build", "pass")], &[], "MERGED")));
    round(&r);
    wait_for("both told", || r.k.get(a.id).unwrap().turns.len() == 2 && r.k.get(b.id).unwrap().turns.len() == 2);
    assert!(r.k.get(a.id).unwrap().turns[1].prompt.contains("The pull request was merged."));
    assert!(r.w.list().iter().all(|w| matches!(&w.state, WState::Ended(m) if m.contains("merged"))));
    assert!(r.wake.get("watch", URL).is_none(), "no timer for an address nobody watches");
}

#[test]
fn a_rate_limit_pauses_with_a_time_and_lost_pull_requests_end_the_watch_after_three_tries() {
    let r = rig("limits");
    let t = r.task();
    r.w.watch(&t.key, URL, Events::all(), "").unwrap();
    r.set(Err(PollErr::RateLimited(Some(now_ms() + 600_000))));
    round(&r);
    let w = r.w.of(&t.key)[0].clone();
    assert!(w.next_check > now_ms() + 500_000, "the next look is at the limit’s end");
    assert_eq!(r.wake.get("watch", URL).map(|t| t.due > now_ms() + 500_000), Some(true));
    r.wake.cancel("watch", URL);
    r.set(Err(PollErr::Lost("No pull request was found at that address.".into())));
    for _ in 0..3 { round(&r); }
    let w = r.w.of(&t.key)[0].clone();
    assert!(matches!(&w.state, WState::Ended(m) if m.contains("Stopped after 3 tries") && m.contains("No pull request was found")), "{:?}", w.state);
    assert_eq!(r.k.get(t.id).unwrap().turns.len(), 1, "errors are not news for the task");
    assert!(r.w.watch(&t.key, "https://example.com/x", Events::all(), "").unwrap_err().contains("GitHub pull request"));
    assert!(r.w.watch("nobody", URL, Events::all(), "").unwrap_err().contains("isn’t open"));
}

#[test]
fn the_cursor_and_what_waits_survive_a_restart_so_nothing_is_reported_twice() {
    let r = rig("restart");
    let t = r.task();
    r.w.watch(&t.key, URL, Events::all(), "").unwrap();
    round(&r);
    r.set(Ok(snap(&[("build", "fail")], &[], "OPEN")));
    *r.hold.lock().unwrap() = true;
    assert!(r.k.reply(t.id, "busy [busy]", vec![]));
    wait_for("busy", || r.k.get(t.id).unwrap().busy());
    round(&r);
    assert!(!r.w.of(&t.key)[0].pending.is_empty());
    let again = Watcher::new(r.k.clone(), Wake::new(None), r.poll.clone(), Some(Sealed::in_dir(&r.root.join("st"), "watches", r.crypto.clone())), |_| false);
    let w = again.of(&t.key)[0].clone();
    assert!(w.cursor.primed && w.cursor.failed == ["build"] && !w.pending.is_empty(), "{w:?}");
    *r.hold.lock().unwrap() = false;
}
