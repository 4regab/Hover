//! A Kiro Web turn cut off (the connection dropped, or Hover closed on it) is attached to
//! again and carries on in the same turn. The session logic against a stubbed runner.

use hover_agents::acp::{ATTACH_NOTHING, ATTACH_PROMPT};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::crypto::Crypto;
use hover_core::history::{AgentHistory, SavedSession, SavedTurn};
use hover_core::model::{AgentTool, KiroState};
use hover_core::time::Stamp;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOST: &str = "The connection to the cloud session was lost before the turn finished. Please try again.";

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-reattach-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A runner whose own prompt is lost, and whose attach says `attach`.
fn runner(attach: KiroResult) -> (impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, Seen) {
    let seen: Seen = Default::default();
    let s2 = seen.clone();
    let make = move |_| -> RunTask {
        let (s3, attach) = (s2.clone(), attach.clone());
        Arc::new(move |a: RunArgs| {
            s3.lock().unwrap().push((a.prompt.clone(), a.resume.clone()));
            (a.events)(KiroEvent { session_id: Some("k1".into()), ..Default::default() });
            if a.prompt == ATTACH_PROMPT { attach.clone() } else { KiroResult::new(KiroState::Failed, LOST) }
        })
    };
    (make, seen)
}

fn ended(k: &KiroSessions, id: i32) -> KiroSession {
    let t = Instant::now();
    while k.get(id).unwrap().busy() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(20)); }
    k.get(id).unwrap()
}

#[test]
fn a_lost_connection_is_attached_to_again_and_the_turn_carries_on() {
    let (make, seen) = runner(KiroResult::new(KiroState::Completed, "The real answer."));
    let k = KiroSessions::new(make, None);
    let s = ended(&k, k.start_in(AgentTool::Kiro, &folder("drop"), "task", vec![], None, Some(vec![])).unwrap().id);
    assert_eq!((s.state, s.turns.len()), (KiroState::Completed, 1), "the error is replaced in the same turn");
    assert_eq!(s.turns[0].result.as_ref().unwrap().text, "The real answer.");
    assert_eq!(*seen.lock().unwrap(), [("task".to_owned(), None), (ATTACH_PROMPT.to_owned(), Some("k1".to_owned()))]);
    let step = s.turns[0].steps.iter().find(|x| x.id == "hover-reconnect").expect("its quiet step");
    assert_eq!((step.title.as_str(), step.status.as_str()), ("Reconnecting to the cloud session (1)", "completed"));
}

#[test]
fn a_cloud_session_with_nothing_new_keeps_its_first_failure() {
    let (make, seen) = runner(KiroResult::new(KiroState::Failed, ATTACH_NOTHING));
    let k = KiroSessions::new(make, None);
    let s = ended(&k, k.start_in(AgentTool::Kiro, &folder("nothing"), "task", vec![], None, Some(vec![])).unwrap().id);
    assert_eq!((s.state, s.turns[0].result.as_ref().unwrap().text.as_str()), (KiroState::Failed, LOST));
    assert_eq!(seen.lock().unwrap().len(), 2, "one attach, not eight");
}

#[test]
fn a_turn_running_when_hover_closed_is_attached_to_at_start() {
    let f = folder("start");
    let crypto = Arc::new(Crypto::with_key([1; 32]));
    let dir = std::path::PathBuf::from(&f).join("agents");
    let history = Arc::new(AgentHistory::new(dir, crypto));
    let turn = |prompt: &str, state: Option<KiroState>, text: Option<&str>| SavedTurn { prompt: prompt.into(), images: vec![], steps: vec![], state, text: text.map(str::to_owned),
        started_at: Stamp::now(), woke_at: None, ended_at: None, credits: None, before: None, after: None };
    let saved = |key: &str, cloud: Option<Vec<String>>, last: Option<KiroState>| SavedSession { key: key.into(), tool: AgentTool::Kiro, folder: f.clone(), title: key.into(), acp_id: Some("k1".into()), context: None,
        turns: vec![turn("done", Some(KiroState::Completed), Some("ok")), turn("cut", last, None)], updated: Stamp::now(), access: Some("full".into()), cloud };
    history.save(&saved("cutoff", Some(vec![]), None));
    history.save(&saved("finished", Some(vec![]), Some(KiroState::Completed)));
    history.save(&saved("local", None, None));
    history.flush();
    let (make, seen) = runner(KiroResult::new(KiroState::Completed, "Picked up where it was."));
    let k = KiroSessions::new(make, Some(history));
    k.reattach_cut_off();
    let t = Instant::now();
    while seen.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); }
    let all = loop {
        let all = k.all();
        if all.iter().all(|s| !s.busy()) && !all.is_empty() || t.elapsed() > Duration::from_secs(10) { break all; }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(all.len(), 1, "only the cloud session that was cut off comes back: {:?}", all.iter().map(|s| s.key.clone()).collect::<Vec<_>>());
    let s = &all[0];
    assert_eq!((s.key.as_str(), s.state, s.turns.len()), ("cutoff", KiroState::Completed, 2));
    assert_eq!(s.turns[1].result.as_ref().unwrap().text, "Picked up where it was.");
    assert_eq!(*seen.lock().unwrap(), [(ATTACH_PROMPT.to_owned(), Some("k1".to_owned()))], "attached, nothing prompted");
}
