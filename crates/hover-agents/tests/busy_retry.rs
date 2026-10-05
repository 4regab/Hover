//! Kiro's "continue when high usage encountered": a turn that fails because the model has
//! too many users is continued at once, in the same turn, until it works or is stopped.

use hover_agents::session::{KiroSessions, RunArgs, RunTask};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::model::{AgentTool, KiroState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-busy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

/// A runner that is busy `busy` times, then answers. It records each prompt and resume id.
fn runner(busy: usize, said: &'static str) -> (impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, Arc<Mutex<Vec<(String, Option<String>)>>>) {
    let seen: Arc<Mutex<Vec<(String, Option<String>)>>> = Default::default();
    let s2 = seen.clone();
    let make = move |_| -> RunTask {
        let s3 = s2.clone();
        Arc::new(move |a: RunArgs| {
            let n = { let mut g = s3.lock().unwrap(); g.push((a.prompt.clone(), a.resume.clone())); g.len() };
            (a.events)(KiroEvent { session_id: Some("s1".into()), ..Default::default() });
            if n <= busy { KiroResult::new(KiroState::Failed, said) } else { KiroResult::new(KiroState::Completed, "done") }
        })
    };
    (make, seen)
}

fn ended(k: &KiroSessions, id: i32) -> hover_agents::session::KiroSession {
    let t = Instant::now();
    while k.get(id).unwrap().busy() && t.elapsed() < Duration::from_secs(15) { std::thread::sleep(Duration::from_millis(20)); }
    k.get(id).unwrap()
}

#[test]
fn a_busy_model_is_continued_in_the_same_turn_until_it_answers() {
    let f = folder("on");
    let (make, seen) = runner(2, "Too many requests, please wait before trying again.");
    let k = KiroSessions::new(make, None);
    k.set_retry_when_busy(|| true);
    let s = k.start(AgentTool::Kiro, &f, "fix the bug", vec![]).unwrap();
    let s = ended(&k, s.id);
    assert_eq!(s.state, KiroState::Completed);
    assert_eq!(s.turns.len(), 1, "no extra turns in the chat");
    let calls = seen.lock().unwrap().clone();
    // The first try had no conversation yet, so it is sent as it was; then it continues the one Kiro named.
    assert_eq!(calls, [("fix the bug".into(), None), ("continue".into(), Some("s1".into())), ("continue".into(), Some("s1".into()))]);
    let step = s.turns[0].steps.iter().find(|x| x.id == "hover-retry").expect("its one quiet step");
    assert_eq!((step.title.as_str(), step.status.as_str()), ("Retrying after high demand (2)", "completed"));
}

#[test]
fn off_or_another_failure_or_another_tool_is_not_continued() {
    let f = folder("off");
    // Setting off.
    let (make, seen) = runner(1, "The model you've selected is experiencing a high volume of traffic.");
    let k = KiroSessions::new(make, None);
    k.set_retry_when_busy(|| false);
    let s = ended(&k, k.start(AgentTool::Kiro, &f, "a", vec![]).unwrap().id);
    assert_eq!((s.state, seen.lock().unwrap().len()), (KiroState::Failed, 1));
    // On, but a different failure.
    let (make, seen) = runner(1, "Kiro isn't signed in.");
    let k = KiroSessions::new(make, None);
    k.set_retry_when_busy(|| true);
    let s = ended(&k, k.start(AgentTool::Kiro, &f, "a", vec![]).unwrap().id);
    assert_eq!((s.state, seen.lock().unwrap().len()), (KiroState::Failed, 1));
    // On, and busy, but another tool's.
    let (make, seen) = runner(1, "Too many requests, please wait before trying again.");
    let k = KiroSessions::new(make, None);
    k.set_retry_when_busy(|| true);
    let s = ended(&k, k.start(AgentTool::Codex, &f, "a", vec![]).unwrap().id);
    assert_eq!((s.state, seen.lock().unwrap().len()), (KiroState::Failed, 1));
}

#[test]
fn stop_ends_a_model_that_stays_busy() {
    let f = folder("stop");
    let (make, seen) = runner(usize::MAX, "Too many requests, please wait before trying again.");
    let k = KiroSessions::new(make, None);
    k.set_retry_when_busy(|| true);
    let id = k.start(AgentTool::Kiro, &f, "a", vec![]).unwrap().id;
    let t = Instant::now();
    while seen.lock().unwrap().len() < 3 && t.elapsed() < Duration::from_secs(10) { std::thread::sleep(Duration::from_millis(20)); }
    assert!(seen.lock().unwrap().len() >= 3, "it keeps going on its own");
    k.stop(id);
    let s = ended(&k, id);
    assert_eq!(s.state, KiroState::Cancelled);
    let n = seen.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(seen.lock().unwrap().len(), n, "nothing is sent after Stop");
}
