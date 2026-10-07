//! The reply queue: waiting messages can be edited, moved, taken back and sent ahead; each operation reaches the message
//! it names, a stop holds the rest, and the queue, its chips and its order come back after a restart (held).

use hover_agents::session::{KiroSessions, Msg, QueueError, RunArgs, RunTask, SendNow};
use hover_agents::stream::KiroResult;
use hover_core::crypto::Crypto;
use hover_core::ext::Chip;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn folder(name: &str) -> String {
    let d: PathBuf = std::env::temp_dir().join(format!("hover-queue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

/// How the scripted agent treats a stop.
#[derive(Clone, Copy, PartialEq)]
enum Stop { Confirms, Never }

struct Agent { seen: Arc<Mutex<Vec<String>>>, go: Arc<Mutex<bool>>, stop: Arc<Mutex<Stop>> }

/// Runs each prompt until `go` is set; a stop ends it (confirmed) or, for `Never`, leaves it unconfirmed.
fn agent() -> (Agent, KiroSessions) {
    let a = Agent { seen: Default::default(), go: Default::default(), stop: Arc::new(Mutex::new(Stop::Confirms)) };
    let (seen, go, stop) = (a.seen.clone(), a.go.clone(), a.stop.clone());
    let k = KiroSessions::new(move |_| -> RunTask {
        let (seen, go, stop) = (seen.clone(), go.clone(), stop.clone());
        Arc::new(move |r: RunArgs| {
            seen.lock().unwrap().push(r.prompt.clone());
            (r.events)(hover_agents::stream::KiroEvent { session_id: Some("conv-1".into()), ..Default::default() });
            while !*go.lock().unwrap() && !r.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); }
            if r.ct.is_cancelled() {
                return if *stop.lock().unwrap() == Stop::Confirms { KiroResult::new(KiroState::Cancelled, "Stopped.") } else { KiroResult { unconfirmed: true, ..KiroResult::new(KiroState::Cancelled, "Stop not confirmed.") } };
            }
            KiroResult::new(KiroState::Completed, "done")
        })
    }, None);
    (a, k)
}

impl Agent {
    fn prompts(&self) -> Vec<String> { self.seen.lock().unwrap().iter().map(|p| p.lines().next().unwrap_or("").to_owned()).collect() }
    /// Lets the turns that run finish.
    fn open(&self) { *self.go.lock().unwrap() = true; }
}

fn uids(k: &KiroSessions, id: i32) -> Vec<(String, String)> { k.get(id).unwrap().turns.iter().filter(|t| t.queued).map(|t| (t.uid.clone(), t.prompt.clone())).collect() }

#[test]
fn waiting_messages_are_edited_moved_and_taken_back_by_name_and_only_in_their_own_session() {
    let (a, k) = agent();
    let f = folder("ops");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    let other = k.start(AgentTool::Codex, &f, "elsewhere", vec![]).unwrap();
    wait_for("both to run", || a.prompts().len() == 2);
    for t in ["two", "three", "four"] { assert!(k.reply(s.id, t, vec![])); }
    assert!(k.reply(other.id, "other two", vec![]));
    let q = uids(&k, s.id);
    assert_eq!(q.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["two", "three", "four"]);
    // Edit one; the others and the other session are untouched.
    let chip = Chip { kind: "file".into(), label: "a.rs".into(), source: "a.rs".into(), live: true, ..Default::default() };
    k.edit_queued(s.id, &q[1].0, Msg { text: "three, edited".into(), chips: vec![chip.clone()], images: vec!["/tmp/p.png".into()], switch_to: None }).unwrap();
    let now = k.get(s.id).unwrap();
    assert_eq!(now.turns.iter().filter(|t| t.queued).map(|t| t.prompt.as_str()).collect::<Vec<_>>(), ["two", "three, edited", "four"]);
    assert_eq!((now.turns[2].chips.clone(), now.turns[2].images.clone()), (vec![chip], vec!["/tmp/p.png".to_owned()]));
    assert_eq!(uids(&k, other.id)[0].1, "other two");
    // The id of a message in another session reaches nothing there.
    assert_eq!(k.edit_queued(other.id, &q[0].0, Msg::text("x")), Err(QueueError::Gone));
    assert_eq!(k.remove_queued(other.id, &q[2].0), Err(QueueError::Gone));
    // Move the last to the front, then take the middle one back.
    k.move_queued(s.id, &q[2].0, 0).unwrap();
    assert_eq!(uids(&k, s.id).iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["four", "two", "three, edited"]);
    let back = k.remove_queued(s.id, &q[0].0).unwrap();
    assert_eq!(back.text, "two");
    assert_eq!(k.remove_queued(s.id, &q[0].0), Err(QueueError::Gone), "a second click finds it gone");
    assert_eq!(uids(&k, s.id).iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["four", "three, edited"]);
    // An empty message is not a message.
    assert!(matches!(k.edit_queued(s.id, &q[1].0, Msg::text("  ")), Err(QueueError::Invalid(_))));
    a.open();
    wait_for("all to finish", || k.running() == 0);
    assert_eq!(a.prompts().iter().filter(|p| p.starts_with("four") || p.starts_with("three")).count(), 2);
}

#[test]
fn a_message_that_starts_while_it_is_being_edited_comes_back_with_its_edited_text() {
    let (a, k) = agent();
    let f = folder("race");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    wait_for("the run", || a.prompts().len() == 1);
    k.reply(s.id, "second", vec![]);
    let uid = uids(&k, s.id)[0].0.clone();
    a.open();
    wait_for("the second to start", || a.prompts().len() == 2);
    let err = k.edit_queued(s.id, &uid, Msg { text: "second, but better".into(), ..Default::default() }).unwrap_err();
    match err { QueueError::Started(m) => assert_eq!(m.text, "second, but better", "the edited text is handed back for the composer"), e => panic!("{e:?}") }
    assert_eq!(a.prompts()[1], "second", "the message that started is the one that was queued");
}

#[test]
fn send_now_stops_through_the_tool_and_sends_once_and_an_unconfirmed_stop_sends_nothing() {
    let (a, k) = agent();
    let f = folder("steer");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    wait_for("the run", || a.prompts().len() == 1);
    k.reply(s.id, "later", vec![]);
    k.reply(s.id, "urgent", vec![]);
    let urgent = uids(&k, s.id)[1].0.clone();
    assert_eq!(k.send_now(s.id, &urgent), Ok(SendNow::Steering));
    assert_eq!(k.send_now(s.id, &urgent).err().map(|e| matches!(e, QueueError::Started(_) | QueueError::Gone)), None, "a second click is harmless");
    wait_for("the urgent one to start", || a.prompts().len() == 2);
    assert_eq!(a.prompts()[1], "urgent", "ahead of the message that waited longer");
    assert_eq!(a.prompts().iter().filter(|p| *p == "urgent").count(), 1, "sent once");
    assert_eq!(uids(&k, s.id).iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["later"]);
    assert_eq!(k.get(s.id).unwrap().turns[0].result.as_ref().unwrap().state, KiroState::Cancelled, "the first attempt keeps its place in the history");
    // A tool that never confirms the stop: nothing more goes, so no second writer starts.
    *a.stop.lock().unwrap() = Stop::Never;
    let later = uids(&k, s.id)[0].0.clone();
    assert_eq!(k.send_now(s.id, &later), Ok(SendNow::Steering));
    wait_for("the stop to end the run", || !k.get(s.id).unwrap().busy());
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(a.prompts().len(), 2, "nothing was sent after an unconfirmed stop");
    let now = k.get(s.id).unwrap();
    assert!(now.held && uids(&k, s.id).len() == 1, "the message waits, held");
    // The user resuming it is the explicit step.
    *a.stop.lock().unwrap() = Stop::Confirms;
    a.open();
    assert!(k.resume_queue(s.id));
    wait_for("the held message to go", || a.prompts().len() == 3);
}

#[test]
fn stop_holds_what_waits_and_only_the_user_lets_it_go() {
    let (a, k) = agent();
    let f = folder("hold");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    wait_for("the run", || a.prompts().len() == 1);
    k.reply(s.id, "second", vec![]);
    k.reply(s.id, "third", vec![]);
    k.stop(s.id);
    wait_for("the stop", || !k.get(s.id).unwrap().busy());
    std::thread::sleep(Duration::from_millis(100));
    assert!(k.get(s.id).unwrap().held);
    assert_eq!(a.prompts().len(), 1);
    // A message the user sends now is their own action: the oldest waiting one goes first, in order.
    a.open();
    assert!(k.reply(s.id, "fourth", vec![]));
    wait_for("all three to go, in order", || a.prompts().len() == 4);
    assert_eq!(a.prompts(), ["first", "second", "third", "fourth"]);
}

#[test]
fn the_queue_its_order_and_its_chips_come_back_after_a_restart_held() {
    let root = folder("restart");
    let crypto = Arc::new(Crypto::with_key([5; 32]));
    let hist = root.clone() + "/history";
    // One session with a run that waits and two waiting messages, saved as it goes.
    let gate = Arc::new(Mutex::new(false));
    let g2 = gate.clone();
    let make = move |_| -> RunTask { let g2 = g2.clone(); Arc::new(move |r: RunArgs| { while !*g2.lock().unwrap() && !r.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); } KiroResult::new(KiroState::Completed, "ok") }) };
    let k = KiroSessions::new(make, Some(Arc::new(AgentHistory::new(hist.clone().into(), crypto.clone()))));
    let s = k.start(AgentTool::Kiro, &root, "first", vec![]).unwrap();
    let chip = Chip { kind: "quote".into(), label: "a quote".into(), source: "k:0".into(), text: Some("quoted".into()), ..Default::default() };
    k.reply_msg(s.id, Msg { text: "second".into(), chips: vec![chip.clone()], ..Default::default() });
    k.reply_msg(s.id, Msg { text: "third".into(), images: vec!["/tmp/x.png".into()], switch_to: Some("codex".into()), ..Default::default() });
    let before = uids(&k, s.id);
    let key = k.get(s.id).unwrap().key;
    k.history().unwrap().flush();
    // A new start of Hover: the same waiting messages, in order, with their chips, pictures and ids, and held.
    let again = KiroSessions::new(|_| -> RunTask { Arc::new(|_| KiroResult::new(KiroState::Completed, "after restart")) }, Some(Arc::new(AgentHistory::new(hist.into(), crypto))));
    let woke = again.wake(&key).unwrap();
    assert!(woke.held && !woke.busy());
    let q: Vec<_> = woke.turns.iter().filter(|t| t.queued).collect();
    assert_eq!(q.iter().map(|t| (t.uid.clone(), t.prompt.clone())).collect::<Vec<_>>(), before);
    assert_eq!((q[0].chips.clone(), q[1].images.clone(), q[1].switch_to.clone()), (vec![chip], vec!["/tmp/x.png".to_owned()], Some("codex".to_owned())));
    assert_eq!(woke.turns[0].result.as_ref().unwrap().state, KiroState::Cancelled, "the run that was going reads as stopped");
    std::thread::sleep(Duration::from_millis(150));
    assert!(!again.get(woke.id).unwrap().busy(), "saved messages alone start nothing");
    // The user resumes: both go, in order.
    assert!(again.resume_queue(woke.id));
    wait_for("the queue to run", || again.get(woke.id).is_some_and(|s| !s.busy() && s.turns.iter().all(|t| t.result.is_some())));
    *gate.lock().unwrap() = true;
}

/// What reaches the agent and what the history keeps for a message with chips; and who may read a referenced conversation.
#[test]
fn chips_reach_the_agent_as_text_stay_in_the_history_and_a_conversation_reference_lets_it_read_only() {
    use hover_agents::context;
    use hover_agents::orch::{Env, Orch, Provider, SystemEnv};
    let root = folder("chips");
    let crypto = Arc::new(Crypto::with_key([6; 32]));
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let s2 = seen.clone();
    let k = KiroSessions::new(move |_| -> RunTask { let s2 = s2.clone(); Arc::new(move |r: RunArgs| { s2.lock().unwrap().push(r.prompt.clone()); KiroResult::new(KiroState::Completed, "The earlier talk said: use retries.") }) },
        Some(Arc::new(AgentHistory::new(format!("{root}/history").into(), crypto.clone()))));
    std::fs::write(format!("{root}/a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    // An earlier conversation to refer to.
    let earlier = k.start(AgentTool::Codex, &root, "How should we handle flaky calls?", vec![]).unwrap();
    wait_for("the earlier one", || k.get(earlier.id).is_some_and(|s| !s.busy()));
    // A new task is sent a snapshot of lines, a live file and a reference to that conversation.
    let chips = vec![context::lines(&root, "a.rs", 2, 2).unwrap(), context::file_live(&root, "a.rs").unwrap(), context::thread(&earlier.key, "Flaky calls")];
    let s = k.start_bound(AgentTool::Kiro, &root, "Use what we decided", vec![], None, None, Default::default()).unwrap();
    wait_for("that one", || k.get(s.id).is_some_and(|x| !x.busy()));
    assert!(k.reply_msg(s.id, Msg { text: "Now apply it to this".into(), chips: chips.clone(), ..Default::default() }));
    wait_for("the reply", || k.get(s.id).is_some_and(|x| !x.busy() && x.turns.len() == 2 && x.turns[1].result.is_some()));
    let sent = seen.lock().unwrap().last().unwrap().clone();
    assert!(sent.starts_with("Now apply it to this\n\n[Attached by Hover]") && sent.contains("fn b() {}") && sent.contains("Conversation “Flaky calls”"), "{sent}");
    assert!(!sent.contains("How should we handle flaky calls?"), "the other conversation is a reference, not a copy");
    // The history keeps the chips with the sent message.
    k.history().unwrap().flush();
    assert_eq!(k.history().unwrap().load(&s.key).unwrap().turns[1].ext.chips, chips);
    // The agent that was sent the reference can read that conversation, in pages; another can't.
    struct E;
    impl Env for E {
        fn providers(&self) -> Vec<Provider> { vec![] }
        fn access_of(&self, _: &hover_agents::session::KiroSession) -> String { "full".into() }
        fn limits(&self) -> hover_core::model::DelegationLimits { Default::default() }
    }
    let _ = SystemEnv::new;
    let o = Orch::new(k.clone(), Arc::new(E), None);
    assert!(o.can_read(&s.key, &earlier.key));
    assert!(!o.can_read(&earlier.key, &s.key), "a reference is one way");
    assert!(!o.can_read("someone-else", &earlier.key));
}
