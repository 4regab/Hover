//! Saved tasks: they run the way a task started by hand runs, once per due time, with the user's access, a note when a run was
//! missed, and nothing started by a wake-up that was already on its way when the task was paused or removed.

use hover_agents::orch::{Env, Provider};
use hover_agents::sched::{civil_text, Hook, NewTask, RunState, Schedule, Scheduler, Tz};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::wake::{now_ms, Wake};
use hover_core::crypto::Crypto;
use hover_core::model::{AgentTool, DelegationLimits, KiroState};
use hover_core::store::Sealed;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

struct E { root: PathBuf }
impl Env for E {
    fn providers(&self) -> Vec<Provider> {
        vec![Provider { id: "kiro".into(), name: "Kiro".into(), tool: AgentTool::Kiro, instance: None, ready: true, hint: String::new(), read_only: true, resume: true, leads: true },
            Provider { id: "codex".into(), name: "Codex".into(), tool: AgentTool::Codex, instance: None, ready: false, hint: "sign in first".into(), read_only: true, resume: true, leads: true }]
    }
    fn access_of(&self, _: &KiroSession) -> String { "full".into() }
    fn limits(&self) -> DelegationLimits { Default::default() }
    fn worktrees(&self) -> PathBuf { self.root.join("worktrees") }
}

struct Rig { k: KiroSessions, s: Arc<Scheduler>, wake: Arc<Wake>, folder: String, seen: Arc<Mutex<Vec<(String, Option<String>)>>>, hold: Arc<Mutex<bool>>, root: PathBuf, crypto: Arc<Crypto> }

fn rig(name: &str) -> Rig {
    let root = std::env::temp_dir().join(format!("hover-sched-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let crypto = Arc::new(Crypto::with_key([1; 32]));
    let seen: Arc<Mutex<Vec<(String, Option<String>)>>> = Default::default();
    let hold = Arc::new(Mutex::new(false));
    let (s2, h2) = (seen.clone(), hold.clone());
    let k = KiroSessions::new(move |_| -> RunTask {
        let (s2, h2) = (s2.clone(), h2.clone());
        Arc::new(move |a: RunArgs| {
            s2.lock().unwrap().push((a.prompt.clone(), a.access.clone()));
            while *h2.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); }
            KiroResult::new(KiroState::Completed, "done")
        })
    }, None);
    let wake = Wake::new(Some(Sealed::in_dir(&root.join("state"), "wake", crypto.clone())));
    let s = Scheduler::new(k.clone(), Arc::new(E { root: root.clone() }), wake.clone(), Some(Sealed::in_dir(&root.join("state"), "tasks", crypto.clone())));
    Rig { k, s, wake, folder: folder.to_string_lossy().into_owned(), seen, hold, root, crypto }
}

fn new_task(r: &Rig, schedule: Schedule) -> NewTask {
    NewTask { name: "Nightly check".into(), folder: r.folder.clone(), prompt: "Run the checks and report.".into(), provider: "kiro".into(), workspace: "folder".into(), access: "risky".into(), schedule, tz: Tz::Fixed(0), hook: None }
}

#[test]
fn a_task_is_checked_run_by_hand_with_the_users_access_and_recorded() {
    let r = rig("hand");
    let mut bad = new_task(&r, Schedule::Manual);
    bad.name = " ".into();
    assert!(r.s.add(bad).unwrap_err().contains("name"));
    assert!(r.s.add(NewTask { access: "root".into(), ..new_task(&r, Schedule::Manual) }).unwrap_err().contains("how much"));
    assert!(r.s.add(NewTask { schedule: Schedule::Every { minutes: 1 }, ..new_task(&r, Schedule::Manual) }).unwrap_err().contains("every 5 minutes"));
    assert!(r.s.add(NewTask { folder: "/no/such".into(), ..new_task(&r, Schedule::Manual) }).unwrap_err().contains("folder"));
    let id = r.s.add(new_task(&r, Schedule::Manual)).unwrap();
    r.s.run_now(&id).unwrap();
    wait_for("the run", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    assert_eq!(r.seen.lock().unwrap().as_slice(), [("Run the checks and report.".to_owned(), Some("risky".to_owned()))], "the access the task says, not more");
    let run = r.s.get(&id).unwrap().runs[0].clone();
    assert!(run.session.is_some() && run.note.contains("by hand"));
    // A provider that isn't ready fails that run, with the reason.
    let bad = r.s.add(NewTask { provider: "codex".into(), ..new_task(&r, Schedule::Manual) }).unwrap();
    r.s.run_now(&bad).unwrap();
    wait_for("the failure", || r.s.get(&bad).unwrap().runs.iter().any(|x| x.state == RunState::Failed));
    assert!(r.s.get(&bad).unwrap().runs[0].note.contains("Codex isn’t available: sign in first"));
    // The record comes back after a restart.
    let again = Scheduler::new(r.k.clone(), Arc::new(E { root: r.root.clone() }), Wake::new(None), Some(Sealed::in_dir(&r.root.join("state"), "tasks", r.crypto.clone())));
    assert_eq!(again.list().len(), 2);
    assert_eq!(again.get(&id).unwrap().runs[0].state, RunState::Done);
}

#[test]
fn a_due_task_runs_once_on_its_timer_and_a_one_off_is_then_done() {
    let r = rig("timer");
    r.s.arm_all(Some("the app"));
    let id = r.s.add(new_task(&r, Schedule::Once { at: now_ms() + 150 })).unwrap();
    assert!(r.wake.get("task", &id).is_some(), "the timer is set");
    wait_for("the run", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    std::thread::sleep(Duration::from_millis(200));
    let t = r.s.get(&id).unwrap();
    assert_eq!(t.runs.len(), 1, "once");
    assert!(!t.enabled && r.wake.get("task", &id).is_none(), "a one-off is finished");
    assert_eq!(r.seen.lock().unwrap().len(), 1);
}

#[test]
fn without_the_executor_nothing_is_set_or_run_and_the_status_says_so() {
    let r = rig("noexec");
    let id = r.s.add(new_task(&r, Schedule::Every { minutes: 10 })).unwrap();
    assert!(r.wake.get("task", &id).is_none(), "this process is not the one that runs timers");
    assert_eq!((r.s.status().executor, r.s.status().enabled), (false, 1));
    r.s.arm_all(Some("the service"));
    assert!(r.wake.get("task", &id).is_some());
    assert_eq!((r.s.status().executor, r.s.status().owner.as_str()), (true, "the service"));
}

#[test]
fn a_run_missed_while_hover_was_closed_is_made_up_once_and_says_so() {
    let r = rig("missed");
    let id = r.s.add(new_task(&r, Schedule::Daily { hour: 9, minute: 0, days: 0x7f })).unwrap();
    // A timer saved by an earlier run of Hover, found two hours overdue when this one starts.
    let was_due = now_ms() - 2 * 3_600_000;
    r.wake.set("task", &id, was_due, "");
    r.s.arm_all(Some("the app"));
    wait_for("the catch-up", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    let t = r.s.get(&id).unwrap();
    assert_eq!(t.runs.len(), 1, "one run, not one for every day missed");
    assert!(t.runs[0].note.contains("Missed: it was due") && t.runs[0].note.contains(&civil_text(was_due, Tz::Fixed(0))), "{}", t.runs[0].note);
    assert!(r.wake.get("task", &id).is_some_and(|x| x.due > now_ms()), "the next one is set in the future");
}

#[test]
fn a_wakeup_on_its_way_cannot_start_a_paused_or_removed_task_and_overlap_is_skipped() {
    let r = rig("stale");
    r.s.arm_all(Some("the app"));
    let id = r.s.add(new_task(&r, Schedule::Every { minutes: 5 })).unwrap();
    // Due now; then the user pauses before it fires.
    r.wake.set("task", &id, now_ms() + 300, "");
    r.s.set_enabled(&id, false);
    std::thread::sleep(Duration::from_millis(500));
    assert!(r.seen.lock().unwrap().is_empty(), "paused: nothing ran");
    assert!(r.s.get(&id).unwrap().runs.is_empty());
    // Removed: same.
    let gone = r.s.add(new_task(&r, Schedule::Once { at: now_ms() + 300 })).unwrap();
    r.s.remove(&gone);
    std::thread::sleep(Duration::from_millis(500));
    assert!(r.seen.lock().unwrap().is_empty() && r.s.get(&gone).is_none());
    // A run still going when the next is due: the next is skipped, with a note.
    *r.hold.lock().unwrap() = true;
    r.s.set_enabled(&id, true);
    r.s.run_now(&id).unwrap();
    wait_for("the first run", || r.seen.lock().unwrap().len() == 1);
    std::thread::sleep(Duration::from_millis(10));
    r.s.run_now(&id).unwrap();
    wait_for("the skip", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Skipped));
    assert!(r.s.get(&id).unwrap().runs.iter().any(|x| x.note.contains("the last run was still going")));
    assert_eq!(r.seen.lock().unwrap().len(), 1, "no second writer");
    *r.hold.lock().unwrap() = false;
}

#[test]
fn a_webhook_starts_the_same_run_with_only_the_words_it_chose_and_no_more_access() {
    let r = rig("hook");
    let id = r.s.add(NewTask { hook: Some(Hook { enabled: true, events: vec![], fields: vec!["/pull_request/title".into()] }), ..new_task(&r, Schedule::Manual) }).unwrap();
    r.s.trigger(&id, "delivery-1", "Event data:\n- pull_request.title: Fix login").unwrap();
    wait_for("the run", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    let (prompt, access) = r.seen.lock().unwrap()[0].clone();
    assert!(prompt.starts_with("Run the checks and report.\n\nEvent data:\n- pull_request.title: Fix login"), "{prompt}");
    assert_eq!(access.as_deref(), Some("risky"));
    // The same delivery is never run twice.
    r.s.trigger(&id, "delivery-1", "Event data:\n- again").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.seen.lock().unwrap().len(), 1);
    r.s.set_enabled(&id, false);
    assert!(r.s.trigger(&id, "delivery-2", "x").unwrap_err().contains("paused"));
}
