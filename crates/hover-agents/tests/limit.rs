//! A task stopped by the provider's usage limit: shown as limited with the provider's reason and reset time, resumed once at the user's
//! choice, ahead of anything held, and never by a stale timer, a stopped task, or a different kind of failure.

use hover_agents::limit::{Limits, Mode};
use hover_agents::session::{KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::wake::{now_ms, Wake};
use hover_core::model::{AgentTool, KiroState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

struct Rig { k: KiroSessions, l: Arc<Limits>, wake: Arc<Wake>, folder: String, seen: Arc<Mutex<Vec<String>>>, reset_in_ms: Arc<Mutex<i64>>, stopped: Arc<Mutex<bool>>, auto: Arc<Mutex<bool>>, gate: Arc<Mutex<bool>> }

/// Agents that hit the limit on a prompt containing "limited", say they are overloaded on "overloaded", and otherwise finish.
/// While `gate` is shut a limited turn stays running, so a test can queue behind it without racing the turn's end.
fn rig(name: &str) -> Rig {
    let folder = std::env::temp_dir().join(format!("hover-limit-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let (seen, reset_in_ms): (Arc<Mutex<Vec<String>>>, Arc<Mutex<i64>>) = (Default::default(), Arc::new(Mutex::new(600)));
    let gate: Arc<Mutex<bool>> = Default::default();
    let (s2, r2, g2) = (seen.clone(), reset_in_ms.clone(), gate.clone());
    let k = KiroSessions::new(move |_| -> RunTask {
        let (s2, r2, g2) = (s2.clone(), r2.clone(), g2.clone());
        Arc::new(move |a: RunArgs| {
            s2.lock().unwrap().push(a.prompt.clone());
            if a.prompt.contains("limited") { while *g2.lock().unwrap() { std::thread::sleep(Duration::from_millis(5)); } }
            if a.prompt.contains("limited") { let at = (now_ms() + *r2.lock().unwrap()) / 1000 + 1; return KiroResult::new(KiroState::Failed, &format!("You've hit your usage limit. (resets_at: {at})")); }
            if a.prompt.contains("nolimit") { return KiroResult::new(KiroState::Failed, "You've hit your weekly limit."); }
            if a.prompt.contains("overloaded") { return KiroResult::new(KiroState::Failed, "The model is overloaded. Try again in 2 minutes."); }
            KiroResult::new(KiroState::Completed, "ok")
        })
    }, None);
    let wake = Wake::new(None);
    let (stopped, auto): (Arc<Mutex<bool>>, Arc<Mutex<bool>>) = Default::default();
    let (st2, au2) = (stopped.clone(), auto.clone());
    let l = Limits::new(k.clone(), wake.clone(), move || *au2.lock().unwrap(), move |_| *st2.lock().unwrap());
    l.set_slack(0);
    wake.start();
    Rig { k, l, wake, folder: folder.to_string_lossy().into_owned(), seen, reset_in_ms, stopped, auto, gate }
}

fn done(k: &KiroSessions, id: i32, n: usize) { wait_for("the turn", || k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == n && s.turns[n - 1].result.is_some())); }

/// The session's key once its limit is recorded: that happens in the turn's end hook, just after the
/// turn shows as ended, so a loaded machine could look in between.
fn limited(r: &Rig, id: i32) -> String {
    let key = r.k.get(id).unwrap().key;
    wait_for("the limit to be recorded", || r.l.of(&key).is_some());
    key
}

#[test]
fn a_real_limit_is_shown_with_the_providers_reason_and_time_and_nothing_else_is_taken_for_one() {
    let r = rig("shown");
    let s = r.k.start(AgentTool::Codex, &r.folder, "limited work", vec![]).unwrap();
    done(&r.k, s.id, 1);
    let key = limited(&r, s.id);
    let l = r.l.of(&key).unwrap();
    assert!(l.limit.reason.contains("usage limit") && l.limit.reset_at.is_some_and(|t| t > now_ms() - 2000) && l.mode == Mode::Off, "{l:?}");
    assert!(r.wake.get("resume", &key).is_none(), "shown, nothing scheduled, until the user chooses");
    // No reset time: shown, and a resume can't be scheduled, but a retry by hand works.
    let s2 = r.k.start(AgentTool::Codex, &r.folder, "nolimit here", vec![]).unwrap();
    done(&r.k, s2.id, 1);
    let key2 = limited(&r, s2.id);
    assert_eq!(r.l.of(&key2).unwrap().limit.reset_at, None);
    assert!(r.l.arm(&key2).unwrap_err().contains("didn’t say when"));
    // A busy model and an ordinary failure are not limits.
    let s3 = r.k.start(AgentTool::Codex, &r.folder, "overloaded now", vec![]).unwrap();
    done(&r.k, s3.id, 1);
    assert!(r.l.of(&r.k.get(s3.id).unwrap().key).is_none());
    assert!(r.l.arm("nobody").is_err());
}

#[test]
fn resume_at_reset_sends_one_continue_ahead_of_whatever_was_held_and_only_once() {
    let r = rig("resume");
    *r.reset_in_ms.lock().unwrap() = 1500;
    // The limited turn runs until the follow-up is queued behind it (an instant fake ended first, now and then).
    *r.gate.lock().unwrap() = true;
    let s = r.k.start(AgentTool::Codex, &r.folder, "limited work", vec![]).unwrap();
    wait_for("the run", || r.k.get(s.id).unwrap().busy());
    // A follow-up waits behind the limited turn; the limit holds it, so it doesn't meet the same wall.
    assert!(r.k.reply(s.id, "follow-up question", vec![]));
    *r.gate.lock().unwrap() = false;
    wait_for("the hold", || r.k.get(s.id).is_some_and(|x| !x.busy() && x.held));
    assert_eq!(r.seen.lock().unwrap().len(), 1, "the follow-up was not sent into the limit");
    let key = limited(&r, s.id);
    let at = r.l.arm(&key).unwrap();
    assert!(r.wake.get("resume", &key).is_some_and(|t| t.due >= at));
    wait_for("the continuation and then the follow-up", || r.seen.lock().unwrap().len() == 3);
    let seen = r.seen.lock().unwrap().clone();
    assert_eq!((seen[1].as_str(), seen[2].as_str()), ("continue", "follow-up question"), "continued work goes first");
    assert!(r.l.of(&key).is_none(), "one limit, one continuation");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.seen.lock().unwrap().len(), 3, "nothing more");
    assert_eq!(r.k.get(s.id).unwrap().access, None, "the continuation asks for no new permissions");
}

#[test]
fn new_work_a_stop_a_cancel_and_a_snooze_each_keep_a_stale_resume_from_happening() {
    let r = rig("stale");
    // A reset well ahead, so a slow machine still has the new work in before it.
    *r.reset_in_ms.lock().unwrap() = 4000;
    // New work before the reset: the limit and its timer go.
    let a = r.k.start(AgentTool::Codex, &r.folder, "limited a", vec![]).unwrap();
    done(&r.k, a.id, 1);
    let ka = limited(&r, a.id);
    r.l.arm(&ka).unwrap();
    assert!(r.k.reply(a.id, "something else now", vec![]));
    done(&r.k, a.id, 2);
    wait_for("the new work to end the limit", || r.l.of(&ka).is_none() && r.wake.get("resume", &ka).is_none());
    assert!(!r.seen.lock().unwrap().iter().any(|p| p == "continue"), "the user went on; nothing was resumed");
    // Cancelled by hand.
    let b = r.k.start(AgentTool::Codex, &r.folder, "limited b", vec![]).unwrap();
    done(&r.k, b.id, 1);
    let kb = limited(&r, b.id);
    r.l.arm(&kb).unwrap();
    r.l.cancel(&kb);
    assert!(r.wake.get("resume", &kb).is_none() && r.l.of(&kb).is_none());
    // Snoozed: shown later, nothing sent.
    let c = r.k.start(AgentTool::Codex, &r.folder, "limited c", vec![]).unwrap();
    done(&r.k, c.id, 1);
    let kc = limited(&r, c.id);
    r.l.snooze(&kc, now_ms() + 3_600_000);
    assert!(matches!(r.l.of(&kc).unwrap().mode, Mode::Snoozed(_)) && r.wake.get("resume", &kc).is_none());
    // A task the user stopped: the timer finds it stopped and ends.
    let d = r.k.start(AgentTool::Codex, &r.folder, "limited d", vec![]).unwrap();
    done(&r.k, d.id, 1);
    let kd = limited(&r, d.id);
    // Due at once: the timer finds the task stopped (a fixed sleep past a whole-second reset missed it now and then).
    *r.stopped.lock().unwrap() = true;
    r.l.arm(&kd).unwrap();
    r.wake.set("resume", &kd, now_ms(), "");
    wait_for("the stopped task's timer to end it", || r.l.of(&kd).is_none());
    assert!(!r.seen.lock().unwrap().iter().any(|p| p == "continue"), "no continuation for any of them");
}

#[test]
fn the_default_arms_a_new_limit_only_when_the_user_turned_it_on_and_an_overdue_resume_still_happens() {
    let r = rig("auto");
    *r.auto.lock().unwrap() = true;
    // Far enough ahead that the limit is still there to look at on a slow machine.
    *r.reset_in_ms.lock().unwrap() = 2000;
    let s = r.k.start(AgentTool::Codex, &r.folder, "limited x", vec![]).unwrap();
    done(&r.k, s.id, 1);
    let key = limited(&r, s.id);
    assert_eq!(r.l.of(&key).map(|l| l.mode), Some(Mode::Auto));
    wait_for("the automatic continuation", || r.seen.lock().unwrap().iter().any(|p| p == "continue"));
    // A timer left from before (Hover was closed over the reset) is overdue when it fires, and still resumes.
    let r2 = rig("overdue");
    let s = r2.k.start(AgentTool::Codex, &r2.folder, "limited y", vec![]).unwrap();
    done(&r2.k, s.id, 1);
    let key = limited(&r2, s.id);
    r2.l.arm(&key).unwrap();
    r2.wake.set("resume", &key, now_ms() - 3_600_000, "");
    wait_for("the overdue resume", || r2.seen.lock().unwrap().iter().any(|p| p == "continue"));
}
