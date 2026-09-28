//! AcpHostTests (tests/Hover.Tests/KiroRunnerTests.cs), ported: AcpHost against the
//! same stand-in agent, speaking ACP over in-memory pipes. The stand-in is the C#
//! test's Fake, line for line: what it answers and what it records.

use hover_agents::acp::AcpHost;
use hover_agents::cancel::Cancel;
use hover_agents::proc::Link;
use hover_agents::stream::{KiroEvent, KiroPhase};
use hover_core::json::{self, Json};
use hover_core::model::{AcpOption, AgentOptions, AgentTool, KiroState};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct FakeState {
    got: Vec<(String, Json)>,
    starts: usize,
    hang_prompt: bool,
    ask_to_edit: bool,
    permission_answer: Option<String>,
    hanging: Option<i64>,
    model: String,
    out: Option<Arc<Mutex<Option<std::io::PipeWriter>>>>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);

fn j(s: &str) -> Json { json::parse(s).unwrap() }

fn models() -> &'static str { r#"[{"value":"m1","name":"Model one"},{"value":"m2","name":"Model two"}]"# }

impl Fake {
    fn methods(&self) -> Vec<String> { self.0.lock().unwrap().got.iter().map(|g| g.0.clone()).collect() }
    fn starts(&self) -> usize { self.0.lock().unwrap().starts }
    fn set(&self, f: impl FnOnce(&mut FakeState)) { f(&mut self.0.lock().unwrap()) }
    /// The agent dies: its output closes, as when the process exits.
    fn crash(&self) { if let Some(o) = self.0.lock().unwrap().out.clone() { o.lock().unwrap().take(); } }

    fn connect(&self) -> std::io::Result<Option<Link>> {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        let out = Arc::new(Mutex::new(Some(agent_writes)));
        { let mut g = self.0.lock().unwrap(); g.starts += 1; g.out = Some(out.clone()); if g.model.is_empty() { g.model = "m1".into(); } }
        let me = self.clone();
        let o2 = out.clone();
        std::thread::spawn(move || me.serve(agent_reads, o2));
        let o3 = out.clone();
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { o3.lock().unwrap().take(); }),
            errors: Box::new(String::new) }))
    }

    fn say(out: &Mutex<Option<std::io::PipeWriter>>, m: &str) {
        if let Some(w) = out.lock().unwrap().as_mut() { let _ = writeln!(w, "{m}"); }
    }

    fn update(out: &Mutex<Option<std::io::PipeWriter>>, sid: &str, u: &str) {
        Self::say(out, &format!(r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{sid}","update":{u}}}}}"#));
    }

    fn serve(&self, r: std::io::PipeReader, out: Arc<Mutex<Option<std::io::PipeWriter>>>) {
        for line in BufReader::new(r).lines() {
            let Ok(line) = line else { return };
            let m = j(&line);
            let method = m.get("method").and_then(Json::as_str).map(str::to_owned);
            let p = m.get("params").cloned().unwrap_or(Json::Null);
            let Some(method) = method else {
                // The answer to a permission request.
                let ans = m.get("result").and_then(|r| r.get("outcome")).and_then(|o| o.get("optionId")).and_then(Json::as_str).map(str::to_owned);
                let h = { let mut g = self.0.lock().unwrap(); g.permission_answer = ans; g.hanging };
                if let Some(h) = h { Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":{h},"result":{{"stopReason":"cancelled"}}}}"#)); }
                continue;
            };
            self.0.lock().unwrap().got.push((method.clone(), p.clone()));
            let id = m.get("id").and_then(|i| i.i64().ok());
            let result: Option<String> = match method.as_str() {
                "initialize" => Some(r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}"#.into()),
                "session/new" => Some(format!(r#"{{"sessionId":"s1","configOptions":[{{"id":"model","category":"model","currentValue":"m1","options":{}}}]}}"#, models())),
                "session/load" => {
                    // It replays the conversation before it answers.
                    Self::update(&out, "s1", r#"{"sessionUpdate":"tool_call","toolCallId":"old","kind":"read","title":"Read","status":"completed"}"#);
                    Some(format!(r#"{{"configOptions":[{{"id":"model","category":"model","currentValue":"m1","options":{}}}]}}"#, models()))
                }
                "session/set_config_option" => {
                    let mut g = self.0.lock().unwrap();
                    if p.get("configId").and_then(Json::as_str) == Some("model") { g.model = p.get("value").and_then(Json::as_str).unwrap().into(); }
                    Some(format!(r#"{{"configOptions":[{{"id":"model","category":"model","currentValue":"{}","options":{}}},{{"id":"effortLevel","category":"thought_level","currentValue":"medium","options":[{{"value":"medium","name":"Medium"}},{{"value":"high","name":"High"}}]}}]}}"#, g.model, models()))
                }
                "session/cancel" => {
                    if let Some(h) = self.0.lock().unwrap().hanging { Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":{h},"result":{{"stopReason":"cancelled"}}}}"#)); }
                    None
                }
                "session/prompt" => {
                    let sid = p.get("sessionId").and_then(Json::as_str).unwrap().to_owned();
                    let (ask, hang) = { let g = self.0.lock().unwrap(); (g.ask_to_edit, g.hang_prompt) };
                    if ask {
                        self.0.lock().unwrap().hanging = id;
                        Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{{"sessionId":"{sid}","toolCall":{{"toolCallId":"e","kind":"edit","title":"Write"}},"options":[{{"optionId":"yes","name":"Accept","kind":"allow_once"}},{{"optionId":"no","name":"Reject","kind":"reject_once"}}]}}}}"#));
                        continue;
                    }
                    Self::update(&out, &sid, r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Let me look."}}"#);
                    Self::update(&out, &sid, r#"{"sessionUpdate":"tool_call","toolCallId":"t1","kind":"read","title":"Read","status":"in_progress","locations":[{"path":"a.cs"}]}"#);
                    Self::update(&out, &sid, r#"{"sessionUpdate":"tool_call","toolCallId":"t2","kind":"edit","title":"Edit","status":"completed"}"#);
                    Self::update(&out, &sid, r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Renamed it."}}"#);
                    if hang { self.0.lock().unwrap().hanging = id; continue; }
                    Some(r#"{"stopReason":"end_turn"}"#.into())
                }
                _ => None,
            };
            if let (Some(i), Some(r)) = (id, result) { Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":{i},"result":{r}}}"#)); }
        }
    }
}

fn dir(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-acp-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn make(o: AgentOptions) -> (AcpHost, Fake) {
    let fake = Fake::default();
    let f = fake.clone();
    (AcpHost::with_connect(AgentTool::Kiro, move || o.clone(), move || f.connect()), fake)
}

fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); } }

type Seen<T> = Arc<Mutex<Vec<T>>>;

type Recorders = (Seen<KiroPhase>, Seen<KiroEvent>, Box<dyn Fn(KiroPhase) + Send + Sync>, Box<dyn Fn(KiroEvent) + Send + Sync>);

fn recorders() -> Recorders {
    let (p, e): (Seen<KiroPhase>, Seen<KiroEvent>) = Default::default();
    let (p2, e2) = (p.clone(), e.clone());
    (p, e, Box::new(move |x| p2.lock().unwrap().push(x)), Box::new(move |x| e2.lock().unwrap().push(x)))
}

#[test]
fn a_turn_goes_over_the_pipe_and_a_reply_carries_on_in_the_same_session() {
    let d = dir("turn");
    let (host, fake) = make(AgentOptions::default());
    let (phases, events, p, e) = recorders();
    let prompt = "Fix the \"failing\" tests & don't touch %PATH% ünïcode";
    let r = host.run(&d, prompt, Some(p), &Cancel::new(), None, Some(e));
    let again = host.run(&d, "and the docs", None, &Cancel::new(), Some("s1"), None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Renamed it."), "the answer is the text after the last tool call");
    let got = fake.0.lock().unwrap().got.clone();
    assert_eq!(got.iter().find(|g| g.0 == "session/new").unwrap().1.get("cwd").unwrap().as_str(), Some(d.as_str()));
    let sent = got.iter().find(|g| g.0 == "session/prompt").unwrap().1.clone();
    assert_eq!(sent.get("prompt").unwrap().items().unwrap()[0].get("text").unwrap().as_str(), Some(prompt));
    // The bytes as System.Text.Json writes the anonymous object (default encoder).
    assert_eq!(sent.compact(), r#"{"sessionId":"s1","prompt":[{"type":"text","text":"Fix the \u0022failing\u0022 tests \u0026 don\u0027t touch %PATH% \u00FCn\u00EFcode"}]}"#);
    assert_eq!(again.state, KiroState::Completed);
    assert_eq!(fake.methods().iter().filter(|m| *m == "session/new").count(), 1, "the reply used the same session");
    assert_eq!(fake.starts(), 1, "one process for both");
    assert_eq!(*phases.lock().unwrap(), [KiroPhase::Starting, KiroPhase::Writing, KiroPhase::Reading, KiroPhase::Editing, KiroPhase::Writing]);
    let ev = events.lock().unwrap();
    assert!(ev.iter().any(|e| e.session_id.as_deref() == Some("s1")));
    assert_eq!(ev.iter().find_map(|e| e.step.as_ref()).unwrap().target.as_deref(), Some("a.cs"));
    host.shutdown("test");
}

#[test]
fn after_a_shutdown_a_reply_loads_the_conversation_and_ignores_its_replay() {
    let d = dir("load");
    let (host, fake) = make(AgentOptions::default());
    host.run(&d, "first", None, &Cancel::new(), None, None);
    host.shutdown("idle");
    let (_, events, _, e) = recorders();
    let r = host.run(&d, "second", None, &Cancel::new(), Some("s1"), Some(e));
    assert_eq!(r.state, KiroState::Completed);
    assert_eq!(fake.starts(), 2);
    assert!(fake.methods().contains(&"session/load".to_string()));
    assert!(!events.lock().unwrap().iter().filter_map(|e| e.step.as_ref()).any(|s| s.id == "old"));
    host.shutdown("test");
}

#[test]
fn a_tool_that_dies_fails_its_run_and_the_next_run_starts_it_again() {
    let d = dir("dies");
    let (host, fake) = make(AgentOptions::default());
    fake.set(|g| g.hang_prompt = true);
    let (h2, d2) = (host.clone(), d.clone());
    let run = std::thread::spawn(move || h2.run(&d2, "long task", None, &Cancel::new(), None, None));
    wait_for(|| fake.methods().contains(&"session/prompt".to_string()));
    fake.crash();
    let r = run.join().unwrap();
    fake.set(|g| g.hang_prompt = false);
    let next = host.run(&d, "again", None, &Cancel::new(), None, None);
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.starts_with("Kiro stopped unexpectedly"), "{}", r.text);
    assert_eq!(next.state, KiroState::Completed);
    assert_eq!(fake.starts(), 2);
    assert!(host.alive());
    host.shutdown("test");
    assert!(!host.alive());
}

#[test]
fn stopping_cancels_the_turn() {
    let d = dir("stop");
    let (host, fake) = make(AgentOptions::default());
    fake.set(|g| g.hang_prompt = true);
    let ct = Cancel::new();
    let (h2, c2) = (host.clone(), ct.clone());
    let run = std::thread::spawn(move || h2.run(&d, "long task", None, &c2, None, None));
    wait_for(|| fake.methods().contains(&"session/prompt".to_string()));
    ct.cancel();
    let r = run.join().unwrap();
    assert_eq!(r.state, KiroState::Cancelled);
    wait_for(|| fake.methods().contains(&"session/cancel".to_string()));
    assert!(fake.methods().contains(&"session/cancel".to_string()));
    host.shutdown("test");
}

#[test]
fn read_only_refuses_a_write_and_says_why() {
    let d = dir("ro");
    let (host, fake) = make(AgentOptions { read_only: true, ..Default::default() });
    fake.set(|g| g.ask_to_edit = true);
    let r = host.run(&d, "change it", None, &Cancel::new(), None, None);
    assert_eq!(fake.0.lock().unwrap().permission_answer.as_deref(), Some("no"));
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("read only"), "{}", r.text);
    host.shutdown("test");
}

#[test]
fn the_model_is_set_and_then_the_effort_it_offers() {
    let d = dir("model");
    let (host, fake) = make(AgentOptions { model: Some("m2".into()), effort: Some("high".into()), ..Default::default() });
    let seen: Arc<Mutex<Option<Vec<AcpOption>>>> = Default::default();
    let s2 = seen.clone();
    host.on_options_seen(move |_, o| *s2.lock().unwrap() = Some(o.to_vec()));
    host.run(&d, "go", None, &Cancel::new(), None, None);
    let sets: Vec<String> = fake.0.lock().unwrap().got.iter().filter(|g| g.0 == "session/set_config_option")
        .map(|g| format!("{}={}", g.1.get("configId").unwrap().as_str().unwrap(), g.1.get("value").unwrap().as_str().unwrap())).collect();
    assert_eq!(&sets[..2], ["model=m2", "effortLevel=high"]);
    assert_eq!(seen.lock().unwrap().as_ref().unwrap().iter().find(|o| o.id == "model").unwrap().current.as_deref(), Some("m2"));
    host.shutdown("test");
}

#[test]
fn a_missing_folder_or_tool_never_starts_anything() {
    let d = dir("missing");
    let (host, fake) = make(AgentOptions::default());
    let gone = host.run(&format!("{d}/gone"), "hi", None, &Cancel::new(), None, None);
    let none = AcpHost::with_connect(AgentTool::Codex, AgentOptions::default, || Ok(None)).run(&d, "hi", None, &Cancel::new(), None, None);
    assert!(gone.text.contains("Choose another"));
    assert_eq!(fake.starts(), 0);
    assert_eq!(none.state, KiroState::Failed);
    assert!(none.text.starts_with("Codex isn’t installed"), "{}", none.text);
}

/// Not in the C# tests; from AcpHost.Handle: a request Hover doesn't serve is -32601,
/// with the id it came with; a permission for a turn that isn't known is cancelled.
#[test]
fn requests_hover_does_not_serve_are_refused() {
    let (hover_reads, mut agent_writes) = std::io::pipe().unwrap();
    let (agent_reads, hover_writes) = std::io::pipe().unwrap();
    let host = {
        let cell = Mutex::new(Some((hover_reads, hover_writes)));
        AcpHost::with_connect(AgentTool::Cursor, AgentOptions::default, move || {
            let (r, w) = cell.lock().unwrap().take().unwrap();
            Ok(Some(Link { to_agent: Box::new(w), from_agent: Box::new(r), kill: Box::new(|| {}), errors: Box::new(String::new) }))
        })
    };
    let d = dir("refuse");
    let h2 = host.clone();
    let run = std::thread::spawn(move || h2.run(&d, "x", None, &Cancel::new(), None, None));
    let mut lines = BufReader::new(agent_reads).lines();
    let init = lines.next().unwrap().unwrap();
    assert_eq!(init, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"hover","version":"1"}}}"#);
    writeln!(agent_writes, r#"{{"jsonrpc":"2.0","id":"fs-1","method":"fs/read_text_file","params":{{"path":"/etc/passwd"}}}}"#).unwrap();
    assert_eq!(lines.next().unwrap().unwrap(), r#"{"jsonrpc":"2.0","id":"fs-1","error":{"code":-32601,"message":"Not supported by Hover."}}"#);
    writeln!(agent_writes, r#"{{"jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{{"sessionId":"nobody","options":[{{"optionId":"y","kind":"allow_once"}}]}}}}"#).unwrap();
    assert_eq!(lines.next().unwrap().unwrap(), r#"{"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}}"#);
    writeln!(agent_writes, r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32000,"message":"Authentication required"}}}}"#).unwrap();
    let r = run.join().unwrap();
    assert_eq!(r.text, "Cursor needs you to sign in. Sign in: run “cursor-agent login” in a terminal.");
}
