//! Attaching to a Kiro Web session that went on working in the cloud: the session is loaded
//! again, its replay is read for the turn that was cut off (the part after the user's last
//! message), and what comes next is followed to the turn's end. Against a stand-in agent
//! over in-memory pipes that speaks as Kiro's cloud does.

use hover_agents::acp::{AcpHost, ATTACH_PROMPT};
use hover_agents::cancel::Cancel;
use hover_agents::proc::Link;
use hover_agents::session::RunArgs;
use hover_agents::stream::KiroEvent;
use hover_core::json::{self, Json};
use hover_core::model::{AgentOptions, AgentTool, KiroState};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

type Out = Arc<Mutex<Option<std::io::PipeWriter>>>;

fn say(out: &Out, m: &str) { if let Some(w) = out.lock().unwrap().as_mut() { let _ = writeln!(w, "{m}"); } }

fn update(out: &Out, u: &str) {
    say(out, &format!(r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"c1","update":{u}}}}}"#));
}

/// The conversation as a load replays it: a finished first turn, then a second that was cut off,
/// which `done` says did (or did not) finish while the client was away.
fn replay(out: &Out, done: bool) {
    update(out, r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}"#);
    update(out, r#"{"sessionUpdate":"tool_call","toolCallId":"old1","kind":"read","title":"Read File","status":"completed","locations":[{"path":"old.rs"}]}"#);
    update(out, r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"First answer."}}"#);
    update(out, r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}"#);
    update(out, r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}"#);
    update(out, r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":" one"}}"#);
    update(out, r#"{"sessionUpdate":"tool_call","toolCallId":"cut1","kind":"read","title":"Read File","status":"completed","locations":[{"path":"cut.rs"}]}"#);
    if done {
        update(out, r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Second answer, finished while away."}}"#);
        update(out, r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}"#);
    }
}

fn host(done: bool, live: bool) -> (AcpHost, Arc<Mutex<Vec<Json>>>) { host_with(done, live, false) }

/// `same`: Kiro gives the same sessions whichever source it is asked for.
fn host_with(done: bool, live: bool, same: bool) -> (AcpHost, Arc<Mutex<Vec<Json>>>) {
    let got: Arc<Mutex<Vec<Json>>> = Default::default();
    let g2 = got.clone();
    let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, move || {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        let out: Out = Arc::new(Mutex::new(Some(agent_writes)));
        let (o2, g3) = (out.clone(), g2.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(agent_reads).lines() {
                let Ok(line) = line else { return };
                let m = json::parse(&line).unwrap();
                g3.lock().unwrap().push(m.clone());
                let id = m.get("id").and_then(|i| i.i64().ok());
                let r = match m.get("method").and_then(Json::as_str) {
                    Some("initialize") => Some(r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"list":{}},"_meta":{"kiro":{"executionTargets":["local","cloud-sandbox"]}}}}"#.to_owned()),
                    // For Kiro Web's: two pages, a Kiro Web session and this computer's, then another. For this
                    // computer's: only its own. (`same`: the same for both, with nothing marked.)
                    Some("session/list") => {
                        let p = m.get("params").cloned().unwrap_or(Json::Null);
                        let remote = p.compact().contains("\"remote\"");
                        Some(if same { r#"{"sessions":[{"sessionId":"a1","title":"One"},{"sessionId":"a2","title":"Two"}]}"#.to_owned() }
                            else if !remote { r#"{"sessions":[{"sessionId":"l1","cwd":"/home/me","title":"Local one"}]}"#.to_owned() }
                            else if p.get("cursor").is_none() { r#"{"sessions":[{"sessionId":"c1","cwd":"/sandbox","title":"Fix the footer","updatedAt":"2026-10-05T10:00:00Z"},{"sessionId":"l1","cwd":"/home/me","title":"Local one"}],"nextCursor":"p2"}"#.to_owned() }
                            else { r#"{"sessions":[{"sessionId":"c2","cwd":"/sandbox","title":"  Add tests  "}]}"#.to_owned() })
                    }
                    Some("session/load") => {
                        replay(&o2, done);
                        Some(r#"{"configOptions":[]}"#.to_owned())
                    }
                    _ => None,
                };
                if let (Some(i), Some(r)) = (id, r) { say(&o2, &format!(r#"{{"jsonrpc":"2.0","id":{i},"result":{r}}}"#)); }
                // After the load has answered, the cloud goes on with the turn.
                if live && m.get("method").and_then(Json::as_str) == Some("session/load") {
                    let o3 = o2.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        update(&o3, r#"{"sessionUpdate":"tool_call","toolCallId":"live1","kind":"edit","title":"Edit File","status":"completed","locations":[{"path":"new.rs"}]}"#);
                        update(&o3, r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Finished the work."}}"#);
                        update(&o3, r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion"}}}"#);
                    });
                }
            }
        });
        let o4 = out.clone();
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { o4.lock().unwrap().take(); }), errors: Box::new(String::new) }))
    });
    (host, got)
}

fn attach(host: &AcpHost) -> (hover_agents::stream::KiroResult, Vec<KiroEvent>) {
    let dir = std::env::temp_dir().join(format!("hover-attach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let seen: Arc<Mutex<Vec<KiroEvent>>> = Default::default();
    let s2 = seen.clone();
    let r = host.runner()(RunArgs {
        folder: dir.to_string_lossy().into_owned(), prompt: ATTACH_PROMPT.into(), progress: Box::new(|_| {}), ct: Cancel::new(), resume: Some("c1".into()),
        events: Box::new(move |e| s2.lock().unwrap().push(e)), access: None, tag: None, cloud: Some(vec![]),
    });
    let ev = seen.lock().unwrap().clone();
    (r, ev)
}

fn step_ids(ev: &[KiroEvent]) -> Vec<String> { ev.iter().filter_map(|e| e.step.as_ref()).map(|s| s.id.clone()).collect() }

#[test]
fn a_turn_that_finished_while_away_gives_its_answer_and_steps() {
    let (host, got) = host(true, false);
    let (r, ev) = attach(&host);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Second answer, finished while away."));
    let ids = step_ids(&ev);
    assert!(ids.contains(&"cut1".to_owned()) && !ids.contains(&"old1".to_owned()), "only the cut-off turn's steps: {ids:?}");
    let load = got.lock().unwrap().iter().find(|m| m.get("method").and_then(Json::as_str) == Some("session/load")).cloned().unwrap();
    assert!(load.compact().contains(r#""sessionSource":"remote""#), "{}", load.compact());
    assert!(!got.lock().unwrap().iter().any(|m| m.get("method").and_then(Json::as_str) == Some("session/prompt")), "nothing is prompted");
    host.shutdown("test");
}

#[test]
fn a_turn_still_running_is_followed_to_its_end() {
    let (host, _) = host(false, true);
    let (r, ev) = attach(&host);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Finished the work."));
    let ids = step_ids(&ev);
    assert!(ids.contains(&"cut1".to_owned()) && ids.contains(&"live1".to_owned()) && !ids.contains(&"old1".to_owned()), "{ids:?}");
    host.shutdown("test");
}

#[test]
fn kiro_web_sessions_are_listed_page_by_page_without_the_local_ones() {
    let (host, got) = host(true, false);
    let list = host.cloud_sessions().unwrap();
    let ids: Vec<(&str, &str, bool)> = list.sessions.iter().map(|c| (c.id.as_str(), c.title.as_str(), c.updated.is_some())).collect();
    assert_eq!(ids, [("c1", "Fix the footer", true), ("c2", "Add tests", false)], "this computer's session is left out");
    assert!(list.note.is_empty());
    let first = got.lock().unwrap().iter().find(|m| m.get("method").and_then(Json::as_str) == Some("session/list")).cloned().unwrap();
    assert!(first.compact().contains(r#""sessionSource":"remote""#), "{}", first.compact());
    host.shutdown("test");
}

#[test]
fn a_kiro_web_conversation_is_read_back_turn_by_turn() {
    let (host, _) = host(true, false);
    let turns = host.cloud_transcript("c1", &std::env::temp_dir().to_string_lossy()).unwrap();
    let got: Vec<(&str, &str, Vec<&str>, bool)> = turns.iter().map(|t| (t.prompt.as_str(), t.text.as_str(), t.steps.iter().map(|s| s.id.as_str()).collect(), t.completed)).collect();
    assert_eq!(got, [("first", "First answer.", vec!["old1"], true), ("second one", "Second answer, finished while away.", vec!["cut1"], true)]);
    host.shutdown("test");
}

/// When Kiro gives the same sessions whichever source it is asked for, Hover can't tell which are Kiro
/// Web's: it shows none, and says so in words.
#[test]
fn when_kiro_cannot_be_told_apart_it_says_so() {
    let (host, _) = host_with(true, false, true);
    let list = host.cloud_sessions().unwrap();
    assert!(list.sessions.is_empty());
    assert!(list.note.contains("listed 2 for Kiro Web and 2 for this computer") && list.note.contains("can’t tell"), "{}", list.note);
    host.shutdown("test");
}
