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

fn cloud_turn(prompt: &str, text: &str, completed: bool) -> hover_agents::acp::CloudTurn {
    hover_agents::acp::CloudTurn { prompt: prompt.into(), text: text.into(), steps: vec![], completed }
}

#[test]
fn a_kiro_web_session_from_elsewhere_comes_to_a_desk_with_its_conversation() {
    let f = folder("adopt");
    let (make, seen) = runner(KiroResult::new(KiroState::Completed, "Finished in the cloud."));
    let k = KiroSessions::new(make, None);
    // Finished: every turn, as it was, and nothing is sent.
    let s = k.adopt_cloud("w1", "Fix the footer", &f, None, Ok(vec![cloud_turn("fix it", "Fixed.", true), cloud_turn("and the header", "Both done.", true)])).unwrap();
    let got: Vec<(String, String)> = s.turns.iter().map(|t| (t.prompt.clone(), t.result.as_ref().unwrap().text.clone())).collect();
    assert_eq!(got, [("fix it".to_owned(), "Fixed.".to_owned()), ("and the header".to_owned(), "Both done.".to_owned())]);
    assert_eq!((s.state, s.kiro_id.as_deref(), s.cloud.is_some()), (KiroState::Completed, Some("w1"), true));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(k.adopt_cloud("w1", "again", &f, None, Ok(vec![])).unwrap().id, s.id, "opened twice is the same session");
    // Still working there: it is followed on, in its last turn.
    let r = k.adopt_cloud("w2", "Long task", &f, None, Ok(vec![cloud_turn("go", "", false)])).unwrap();
    let r = ended(&k, r.id);
    assert_eq!((r.state, r.turns.len(), r.turns[0].result.as_ref().unwrap().text.as_str()), (KiroState::Completed, 1, "Finished in the cloud."));
    assert_eq!(*seen.lock().unwrap(), [(ATTACH_PROMPT.to_owned(), Some("w2".to_owned()))]);
    // Couldn't be read: its title, and why.
    let e = k.adopt_cloud("w3", "Broken one", &f, None, Err("Kiro didn’t answer.".into())).unwrap();
    assert_eq!((e.turns.len(), e.turns[0].prompt.as_str(), e.state), (1, "Broken one", KiroState::Failed));
    assert!(e.turns[0].result.as_ref().unwrap().text.contains("Kiro didn’t answer."));
}

/// Kiro's own words when the PC can't reach the cloud (read from its agent server, Oct 2026).
const OFFLINE: &str = "Could not reach the cloud session service. Please check your connection and try again.";
const DROPPED: &str = "The connection dropped before the turn finished. The cloud session kept running — reopen it to continue.";

#[test]
fn every_way_kiro_says_the_connection_went_is_attached_to_again() {
    for words in [OFFLINE, DROPPED] {
        let seen: Seen = Default::default();
        let s2 = seen.clone();
        let make = move |_| -> RunTask {
            let s3 = s2.clone();
            Arc::new(move |a: RunArgs| {
                s3.lock().unwrap().push((a.prompt.clone(), a.resume.clone()));
                (a.events)(KiroEvent { session_id: Some("k1".into()), ..Default::default() });
                if a.prompt == ATTACH_PROMPT { KiroResult::new(KiroState::Completed, "The real answer.") } else { KiroResult::new(KiroState::Failed, words) }
            })
        };
        let k = KiroSessions::new(make, None);
        let s = ended(&k, k.start_in(AgentTool::Kiro, &folder("words"), "task", vec![], None, Some(vec![])).unwrap().id);
        assert_eq!(s.state, KiroState::Completed, "not attached again after: {words}");
    }
}

#[test]
fn closing_hover_does_not_stop_a_kiro_web_turn() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (stopped, release) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (st, rl) = (stopped.clone(), release.clone());
    let make = move |_| -> RunTask {
        let (st, rl) = (st.clone(), rl.clone());
        Arc::new(move |a: RunArgs| {
            (a.events)(KiroEvent { session_id: Some("k1".into()), ..Default::default() });
            let t = Instant::now();
            while !rl.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(5) {
                if a.ct.is_cancelled() { st.store(true, Ordering::SeqCst); break; }
                std::thread::sleep(Duration::from_millis(10));
            }
            KiroResult::new(KiroState::Completed, "x")
        })
    };
    let k = KiroSessions::new(make, None);
    let s = k.start_in(AgentTool::Kiro, &folder("quit"), "task", vec![], None, Some(vec![])).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    k.stop_all();
    std::thread::sleep(Duration::from_millis(300));
    let was_stopped = stopped.load(Ordering::SeqCst);
    release.store(true, Ordering::SeqCst);
    ended(&k, s.id);
    assert!(!was_stopped, "Hover's quit told the cloud turn to stop");
}

#[test]
fn a_kiro_web_sessions_id_is_saved_as_soon_as_kiro_gives_it() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = folder("idsave");
    let history = Arc::new(AgentHistory::new(std::path::PathBuf::from(&f).join("agents"), Arc::new(Crypto::with_key([1; 32]))));
    let release = Arc::new(AtomicBool::new(false));
    let rl = release.clone();
    let make = move |_| -> RunTask {
        let rl = rl.clone();
        Arc::new(move |a: RunArgs| {
            (a.events)(KiroEvent { session_id: Some("k1".into()), ..Default::default() });
            let t = Instant::now();
            while !rl.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(10)); }
            KiroResult::new(KiroState::Completed, "x")
        })
    };
    let k = KiroSessions::new(make, Some(history.clone()));
    let s = k.start_in(AgentTool::Kiro, &f, "task", vec![], None, Some(vec![])).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    history.flush();
    let saved = history.load(&s.key).expect("saved while it runs");
    release.store(true, Ordering::SeqCst);
    ended(&k, s.id);
    assert_eq!(saved.acp_id.as_deref(), Some("k1"), "closed now, Hover could not find the cloud session again");
}
