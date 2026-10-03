//! Kiro's auto compact: with the setting on and the context past its percent, a
//! `/compact` turn goes before the next prompt. The session logic against a stubbed
//! runner, then the real prompt over ACP pipes, then (with FAKEACP=<fake-agent>) the
//! stand-in agent as a process.

use hover_agents::acp::AcpHost;
use hover_agents::proc::{launch_grouped, Link};
use hover_agents::session::{compact_title, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::json::{self, Json};
use hover_core::model::{AgentOptions, AgentTool, KiroState};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-compact-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn wait_for(secs: u64, f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(secs) { std::thread::sleep(Duration::from_millis(10)); } }

/// Every turn ended, `n` of them.
fn settle(k: &KiroSessions, id: i32, n: usize) {
    wait_for(5, || { let s = k.get(id).unwrap(); !s.busy() && s.turns.len() == n && s.turns.iter().all(|t| t.result.is_some()) });
    assert_eq!(k.get(id).unwrap().turns.len(), n);
}

/// What /compact does in the stand-in runner.
#[derive(Clone, Copy)]
enum Compact { Says(&'static str), Fails, Blocks }

#[derive(Clone)]
struct Script {
    /// The context each of the tool's own turns reports when it ends, by turn (none: nothing reported).
    report: Vec<Option<f64>>,
    compact: Compact,
    /// The first own turn waits for this.
    hold: Option<Arc<AtomicBool>>,
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

impl Script {
    fn new(report: &[Option<f64>]) -> Script { Script { report: report.to_vec(), compact: Compact::Says("Compacted the conversation."), hold: None } }

    fn make(&self) -> (impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, Seen) {
        let (script, seen): (Script, Seen) = (self.clone(), Default::default());
        let (s2, own) = (seen.clone(), Arc::new(AtomicUsize::new(0)));
        let make = move |_| -> RunTask {
            let (script, seen, own) = (script.clone(), s2.clone(), own.clone());
            Arc::new(move |a: RunArgs| {
                seen.lock().unwrap().push((a.prompt.clone(), a.access.clone()));
                (a.events)(KiroEvent { session_id: Some("s1".into()), ..Default::default() });
                if a.prompt == "/compact" {
                    return match script.compact {
                        Compact::Says(t) => {
                            (a.events)(KiroEvent { context: Some(12.0), ..Default::default() });
                            KiroResult::new(KiroState::Completed, t)
                        }
                        Compact::Fails => KiroResult::new(KiroState::Failed, "Kiro couldn't compact."),
                        Compact::Blocks => {
                            while !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); }
                            KiroResult::new(KiroState::Cancelled, "")
                        }
                    };
                }
                let n = own.fetch_add(1, Ordering::SeqCst);
                if n == 0 { if let Some(h) = &script.hold { while !h.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(5)); } } }
                if let Some(Some(u)) = script.report.get(n) { (a.events)(KiroEvent { context: Some(*u), ..Default::default() }); }
                KiroResult::new(KiroState::Completed, "ok")
            })
        };
        (make, seen)
    }
}

fn prompts(seen: &Seen) -> Vec<String> { seen.lock().unwrap().iter().map(|p| p.0.clone()).collect() }

fn on_at(k: &KiroSessions, at: Option<u8>) { k.set_auto_compact(move || at); }

#[test]
fn off_by_default_nothing_is_compacted_however_full() {
    let f = folder("off");
    let (make, seen) = Script::new(&[Some(99.0), Some(99.0)]).make();
    let k = KiroSessions::new(make, None);
    on_at(&k, None);
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(prompts(&seen), ["first", "second"]);
    assert!(k.get(s.id).unwrap().turns[1].steps.is_empty());
}

#[test]
fn past_the_percent_a_compact_goes_once_before_the_reply() {
    let f = folder("on");
    let (make, seen) = Script::new(&[Some(85.0)]).make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert_eq!(prompts(&seen), ["first"], "nothing is compacted before the first prompt");
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert!(k.reply(s.id, "third", vec![]));
    settle(&k, s.id, 3);
    // The second turn reported nothing new, so the third goes as it is.
    assert_eq!(prompts(&seen), ["first", "/compact", "second", "third"]);
    let s = k.get(s.id).unwrap();
    let step = &s.turns[1].steps[0];
    assert_eq!((step.title.as_str(), step.status.as_str(), step.kind.as_str()), ("Compacted the conversation (it was 85% full)", "completed", "other"));
    assert!(step.ms.is_some());
    assert!(s.turns[0].steps.is_empty() && s.turns[2].steps.is_empty());
    assert_eq!(s.turns[1].result.as_ref().unwrap().text, "ok", "the reply's answer, not the compaction's");
    assert_eq!(s.context, Some(12.0), "what the compaction reported is shown");
}

#[test]
fn a_new_report_after_each_reply_compacts_again_and_the_percent_itself_counts() {
    let f = folder("again");
    let (make, seen) = Script::new(&[Some(80.0), Some(90.0), Some(79.9)]).make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "a", vec![]).unwrap();
    settle(&k, s.id, 1);
    for (i, t) in ["b", "c", "d"].iter().enumerate() {
        assert!(k.reply(s.id, t, vec![]));
        settle(&k, s.id, i + 2);
    }
    // 80 is at the percent (compacts), 90 compacts, 79.9 is under it.
    assert_eq!(prompts(&seen), ["a", "/compact", "b", "/compact", "c", "d"]);
}

#[test]
fn under_the_percent_nothing_is_compacted() {
    let f = folder("under");
    let (make, seen) = Script::new(&[Some(50.0)]).make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(prompts(&seen), ["first", "second"]);
}

#[test]
fn only_kiro_is_compacted() {
    let f = folder("others");
    for tool in [AgentTool::Codex, AgentTool::Cursor, AgentTool::OpenCode, AgentTool::Claude] {
        let (make, seen) = Script::new(&[Some(97.0)]).make();
        let k = KiroSessions::new(make, None);
        on_at(&k, Some(50));
        let s = k.start(tool, &f, "first", vec![]).unwrap();
        settle(&k, s.id, 1);
        assert!(k.reply(s.id, "second", vec![]));
        settle(&k, s.id, 2);
        assert_eq!(prompts(&seen), ["first", "second"], "{}", tool.id());
    }
}

#[test]
fn queued_replies_keep_their_order_and_one_compact_goes_before_the_first() {
    let f = folder("queue");
    let hold = Arc::new(AtomicBool::new(false));
    let (make, seen) = Script { hold: Some(hold.clone()), ..Script::new(&[Some(90.0)]) }.make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "a", vec![]).unwrap();
    wait_for(5, || !seen.lock().unwrap().is_empty());
    assert!(k.reply(s.id, "b", vec![]) && k.reply(s.id, "c", vec![]));
    hold.store(true, Ordering::SeqCst);
    settle(&k, s.id, 3);
    assert_eq!(prompts(&seen), ["a", "/compact", "b", "c"]);
    let s = k.get(s.id).unwrap();
    assert_eq!(s.turns.iter().map(|t| t.prompt.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!((s.turns[1].steps.len(), s.turns[2].steps.len()), (1, 0), "the step sits with the reply it came before");
}

#[test]
fn stopping_during_the_compaction_stops_the_reply_and_what_waited_behind_it() {
    let f = folder("stop");
    let (make, seen) = Script { compact: Compact::Blocks, ..Script::new(&[Some(90.0)]) }.make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    wait_for(5, || prompts(&seen).contains(&"/compact".to_owned()));
    assert!(k.reply(s.id, "third", vec![]));
    k.stop(s.id);
    settle(&k, s.id, 3);
    assert_eq!(prompts(&seen), ["first", "/compact"], "the reply was never sent");
    let s = k.get(s.id).unwrap();
    assert_eq!(s.turns[1].result.as_ref().unwrap().state, KiroState::Cancelled);
    assert_eq!(s.turns[2].result.as_ref().unwrap().text, "Not sent: the run before it was stopped.");
    let step = &s.turns[1].steps[0];
    assert_eq!((step.title.as_str(), step.status.as_str()), ("Stopped while compacting the conversation (90% full)", "failed"));
}

#[test]
fn nothing_to_compact_is_said_quietly_and_the_reply_goes_on() {
    let f = folder("nothing");
    let (make, seen) = Script { compact: Compact::Says("Nothing to compact yet. The conversation is still short."), ..Script::new(&[Some(85.0)]) }.make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(prompts(&seen), ["first", "/compact", "second"]);
    let s = k.get(s.id).unwrap();
    let step = &s.turns[1].steps[0];
    assert_eq!((step.title.as_str(), step.status.as_str()), ("Nothing to compact yet (the context is 85% full)", "completed"));
    assert_eq!(s.turns[1].result.as_ref().unwrap().state, KiroState::Completed);
}

#[test]
fn a_compaction_that_fails_is_said_and_the_reply_still_goes() {
    let f = folder("fails");
    let (make, seen) = Script { compact: Compact::Fails, ..Script::new(&[Some(85.0)]) }.make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(prompts(&seen), ["first", "/compact", "second"]);
    let s = k.get(s.id).unwrap();
    assert_eq!((s.turns[1].steps[0].title.as_str(), s.turns[1].steps[0].status.as_str()), ("Couldn't compact the conversation (it was 85% full)", "failed"));
    assert_eq!((s.turns[1].result.as_ref().unwrap().state, s.turns[1].result.as_ref().unwrap().text.as_str()), (KiroState::Completed, "ok"));
}

#[test]
fn a_read_only_session_still_compacts_with_its_own_access() {
    let f = folder("readonly");
    let (make, seen) = Script::new(&[Some(85.0)]).make();
    let k = KiroSessions::new(make, None);
    on_at(&k, Some(80));
    let s = k.start_as(AgentTool::Kiro, &f, "first", vec![], Some("read-only")).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    let got = seen.lock().unwrap().clone();
    assert_eq!(got.iter().map(|p| (p.0.as_str(), p.1.as_deref())).collect::<Vec<_>>(),
        [("first", Some("read-only")), ("/compact", Some("read-only")), ("second", Some("read-only"))]);
}

#[test]
fn the_steps_title_says_what_happened() {
    assert_eq!(compact_title(83.4, None, ""), "Compacting the conversation (83% full)");
    assert_eq!(compact_title(83.4, Some(KiroState::Completed), "Compacted."), "Compacted the conversation (it was 83% full)");
    assert_eq!(compact_title(83.4, Some(KiroState::Completed), "NOTHING TO COMPACT yet"), "Nothing to compact yet (the context is 83% full)");
    assert_eq!(compact_title(83.4, Some(KiroState::Failed), ""), "Couldn't compact the conversation (it was 83% full)");
    assert_eq!(compact_title(83.4, Some(KiroState::Cancelled), ""), "Stopped while compacting the conversation (83% full)");
}

/// Kiro over ACP pipes: it reports its context after a turn as session_info_update; its
/// compaction is the `_kiro/session/compact` request (a `/compact` prompt is only chat to
/// Kiro's model). What arrives is recorded: `prompt: TEXT` or `compact`. With `hang`, the
/// compaction never answers.
fn kiro_pipe(seen: Arc<Mutex<Vec<String>>>, hang: bool) -> impl Fn() -> std::io::Result<Option<Link>> + Send + Sync + 'static {
    move || {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        let out = Arc::new(Mutex::new(Some(agent_writes)));
        let (seen, o2) = (seen.clone(), out.clone());
        std::thread::spawn(move || {
            let say = |m: &str| { if let Some(w) = o2.lock().unwrap().as_mut() { let _ = writeln!(w, "{m}"); } };
            let upd = |u: &str| format!(r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{u}}}}}"#);
            for line in BufReader::new(agent_reads).lines() {
                let Ok(line) = line else { return };
                let m = json::parse(&line).unwrap();
                let (Some(method), Some(id)) = (m.get("method").and_then(Json::as_str), m.get("id").and_then(|i| i.i64().ok())) else { continue };
                let result = match method {
                    "initialize" => r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}"#.to_owned(),
                    "session/new" => r#"{"sessionId":"s1","configOptions":[]}"#.to_owned(),
                    "_kiro/session/compact" => {
                        seen.lock().unwrap().push("compact".into());
                        if hang { continue; }
                        say(&upd(r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"summarization":{"status":"success","summary":{"conversationSummary":"short","truncated":false}}}}}"#));
                        r#"{"success":true}"#.to_owned()
                    }
                    "session/prompt" => {
                        let text = m.get("params").and_then(|p| p.get("prompt")).and_then(|p| p.items().ok().map(|i| i[0].clone())).and_then(|b| b.get("text").and_then(Json::as_str).map(str::to_owned)).unwrap();
                        seen.lock().unwrap().push(format!("prompt: {text}"));
                        say(&upd(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}"#));
                        say(&upd(r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"contextUsage":{"usagePercentage":85.0}}}}"#));
                        r#"{"stopReason":"end_turn"}"#.to_owned()
                    }
                    _ => "{}".to_owned(),
                };
                say(&format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#));
            }
        });
        let o3 = out.clone();
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { o3.lock().unwrap().take(); }), errors: Box::new(String::new) }))
    }
}

#[test]
fn over_acp_the_compaction_is_kiros_own_request_not_a_prompt() {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, kiro_pipe(seen.clone(), false));
    let h = host.clone();
    let k = KiroSessions::new(move |_| h.runner(), None);
    on_at(&k, Some(80));
    let f = folder("acp");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(*seen.lock().unwrap(), ["prompt: first", "compact", "prompt: second"], "no /compact prompt: Kiro's model would only talk about it");
    let s = k.get(s.id).unwrap();
    assert_eq!(s.turns[1].steps[0].title, "Compacted the conversation (it was 85% full)");
    assert_eq!(s.turns[1].result.as_ref().unwrap().text, "Done.");
    host.shutdown("test");
}

/// Compaction is a model call that can take a while: a stop ends it, and the reply
/// behind it is not sent.
#[test]
fn over_acp_a_stop_during_the_compaction_ends_it_and_sends_no_reply() {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, kiro_pipe(seen.clone(), true));
    let h = host.clone();
    let k = KiroSessions::new(move |_| h.runner(), None);
    on_at(&k, Some(80));
    let f = folder("acp-stop");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "second", vec![]));
    wait_for(5, || seen.lock().unwrap().iter().any(|x| x == "compact"));
    k.stop(s.id);
    settle(&k, s.id, 2);
    assert_eq!(*seen.lock().unwrap(), ["prompt: first", "compact"]);
    assert_eq!(k.get(s.id).unwrap().turns[1].result.as_ref().unwrap().state, KiroState::Cancelled);
    host.shutdown("test");
}

/// A reply of exactly `/compact` compacts for real too (the chat's way of asking).
#[test]
fn over_acp_a_reply_of_slash_compact_compacts() {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, kiro_pipe(seen.clone(), false));
    let h = host.clone();
    let k = KiroSessions::new(move |_| h.runner(), None);
    on_at(&k, None);
    let f = folder("acp-typed");
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    settle(&k, s.id, 1);
    assert!(k.reply(s.id, "/compact", vec![]));
    settle(&k, s.id, 2);
    assert_eq!(*seen.lock().unwrap(), ["prompt: first", "compact"]);
    assert_eq!(k.get(s.id).unwrap().turns[1].result.as_ref().unwrap().state, KiroState::Completed);
    host.shutdown("test");
}
/// With FAKEACP=<path to fake-agent>: the stand-in process reports 85 % after a turn and
/// takes `/compact` as Kiro does (down to 20 %).
#[test]
fn the_stand_in_agent_as_a_process() {
    let Some(exe) = std::env::var_os("FAKEACP").map(std::path::PathBuf::from).filter(|p| p.is_file()) else { return };
    let f = folder("proc");
    let log = std::path::Path::new(&f).join("sent.log");
    let env: Vec<(String, String)> = [("FAKEACP_CONTEXT", "85"), ("FAKEACP_COMPACT_TO", "20"), ("FAKEACP_SECONDS", "0.2"), ("FAKEACP_LOG", log.to_str().unwrap())]
        .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let groups: Arc<Mutex<Vec<Arc<hover_agents::proc::Group>>>> = Default::default();
    let g2 = groups.clone();
    let connect = move || { let (link, group) = launch_grouped(&exe, &["acp"], &env)?; g2.lock().unwrap().push(group); Ok(Some(link)) };
    let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, connect);
    let h = host.clone();
    let k = KiroSessions::new(move |_| h.runner(), None);
    on_at(&k, Some(80));
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    wait_for(20, || { let s = k.get(s.id).unwrap(); !s.busy() && s.turns.iter().all(|t| t.result.is_some()) });
    assert!(k.reply(s.id, "second", vec![]));
    wait_for(20, || { let s = k.get(s.id).unwrap(); !s.busy() && s.turns.len() == 2 && s.turns.iter().all(|t| t.result.is_some()) });
    let s = k.get(s.id).unwrap();
    assert_eq!(s.turns[1].steps[0].title, "Compacted the conversation (it was 85% full)");
    assert_eq!(s.turns[1].result.as_ref().unwrap().state, KiroState::Completed);
    host.shutdown("test");
    for g in groups.lock().unwrap().iter() { g.kill(); }
    let sent = std::fs::read_to_string(&log).unwrap();
    let at = |needle: &str| sent.find(needle).unwrap_or_else(|| panic!("{needle} in {sent}"));
    assert!(at("\"text\":\"first") < at("\"method\":\"_kiro/session/compact\"") && at("\"method\":\"_kiro/session/compact\"") < at("\"text\":\"second"), "{sent}");
    assert_eq!(sent.matches("_kiro/session/compact").count(), 1);
    assert_eq!(sent.matches("\"text\":\"/compact\"").count(), 0, "never as a prompt");
}

/// The real kiro-cli (costs a little credit): `cargo test -p hover-agents --test compact -- --ignored --nocapture real_kiro`.
/// The threshold is 1 %, so the second reply is preceded by a /compact.
#[test]
#[ignore]
fn real_kiro_cli_compacts_before_the_second_reply() {
    let f = folder("real");
    let host = AcpHost::new(AgentTool::Kiro, AgentOptions::default);
    let h = host.clone();
    let k = KiroSessions::new(move |_| h.runner(), None);
    on_at(&k, Some(1));
    let s = k.start(AgentTool::Kiro, &f, "Reply with just the word: ok", vec![]).unwrap();
    wait_for(120, || { let s = k.get(s.id).unwrap(); !s.busy() && s.turns.iter().all(|t| t.result.is_some()) });
    let one = k.get(s.id).unwrap();
    println!("turn 1: {:?} {:?} context {:?}", one.turns[0].result, one.turns[0].steps.len(), one.context);
    assert!(k.reply(s.id, "Reply with just the word: done", vec![]));
    wait_for(180, || { let s = k.get(s.id).unwrap(); !s.busy() && s.turns.len() == 2 && s.turns.iter().all(|t| t.result.is_some()) });
    let two = k.get(s.id).unwrap();
    for x in &two.turns[1].steps { println!("step: {} | {} | {:?}", x.title, x.status, x.output); }
    println!("turn 2: {:?} context {:?}", two.turns[1].result, two.context);
    k.stop(s.id);
    host.shutdown("test");
    assert_eq!(two.turns[1].steps[0].status, "completed");
    assert!(two.turns[1].steps[0].title.starts_with("Compacted") || two.turns[1].steps[0].title.starts_with("Nothing to compact"));
    assert_eq!(two.turns[1].result.as_ref().unwrap().state, KiroState::Completed);
    println!("log tail:\n{}", std::fs::read_to_string(hover_core::paths::log()).unwrap_or_default().lines().rev().take(8).collect::<Vec<_>>().join("\n"));
    // Kiro's own log of the run it just had: the compaction, in its words.
    let logs = std::path::PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap()).join(".kiro").join("logs");
    let newest = std::fs::read_dir(&logs).unwrap().flatten().max_by_key(|d| d.metadata().and_then(|m| m.modified()).ok());
    if let Some(d) = newest {
        let text = std::fs::read_to_string(d.path().join("kiro.log")).unwrap_or_default();
        for l in text.lines().filter(|l| { let l = l.to_lowercase(); l.contains("compact") || l.contains("summariz") }).take(12) { println!("kiro.log: {}", &l[..l.len().min(300)]); }
    }
}