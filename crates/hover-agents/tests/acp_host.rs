//! AcpHostTests (tests/Hover.Tests/KiroRunnerTests.cs), ported: AcpHost against the
//! same stand-in agent, speaking ACP over in-memory pipes. The stand-in is the C#
//! test's Fake, line for line: what it answers and what it records.

use hover_agents::acp::AcpHost;
use hover_agents::ask::{AgentAsk, AskAnswer};
use hover_agents::cancel::Cancel;
use hover_agents::proc::Link;
use hover_agents::stream::{KiroEvent, KiroPhase};
use hover_core::json::{self, Json};
use hover_core::model::{AcpOption, AgentApproval, AgentOptions, AgentTool, KiroState};
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
    /// What the permission request asks for: its kind, and its raw input.
    ask_kind: Option<String>,
    ask_input: Option<String>,
    asked: usize,
    /// Offer the access options the real tools do: Kiro's autopilot and a mode with
    /// each tool's values.
    offer: bool,
    hanging: Option<i64>,
    model: String,
    /// MCP servers it reports as failed (_kiro/mcp/status) as the prompt starts.
    mcp_failed: Vec<String>,
    /// It says it takes pictures in a prompt (promptCapabilities.image).
    images: bool,
    out: Option<Arc<Mutex<Option<std::io::PipeWriter>>>>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);

fn j(s: &str) -> Json { json::parse(s).unwrap() }

fn models() -> &'static str { r#"[{"value":"m1","name":"Model one"},{"value":"m2","name":"Model two"}]"# }

fn access() -> String {
    let c = |v: &str| format!(r#"{{"value":"{v}","name":"{v}"}}"#);
    let modes: Vec<String> = ["vibe", "read-only", "workspace-write", "agent", "agent-full-access", "ask", "plan", "yolo", "default"].iter().map(|v| c(v)).collect();
    format!(r#"[{{"id":"autopilot","currentValue":"unset","options":[{},{}]}},{{"id":"mode","category":"mode","currentValue":"x","options":[{}]}}]"#, c("on"), c("off"), modes.join(","))
}

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
                // The answer to a permission request: allowed, it finishes the turn.
                let outcome = m.get("result").and_then(|r| r.get("outcome")).cloned().unwrap_or(Json::Null);
                let ans = outcome.get("optionId").or_else(|| outcome.get("outcome")).and_then(Json::as_str).map(str::to_owned);
                let h = { let mut g = self.0.lock().unwrap(); g.permission_answer = ans.clone(); g.hanging.take() };
                if let Some(h) = h {
                    let stop = if matches!(ans.as_deref(), Some("yes" | "always")) { "end_turn" } else { "cancelled" };
                    Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":{h},"result":{{"stopReason":"{stop}"}}}}"#));
                }
                continue;
            };
            self.0.lock().unwrap().got.push((method.clone(), p.clone()));
            let id = m.get("id").and_then(|i| i.i64().ok());
            let result: Option<String> = match method.as_str() {
                "initialize" => Some(if self.0.lock().unwrap().images { r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true}},"authMethods":[{"id":"oauth-personal","name":"Log in with Google"},{"id":"gemini-api-key","name":"Gemini API key"}]}"# }
                    else { r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[{"id":"oauth-personal","name":"Log in with Google"},{"id":"gemini-api-key","name":"Gemini API key"}]}"# }.into()),
                "authenticate" => Some("{}".into()),
                "session/new" => Some(if self.0.lock().unwrap().offer { format!(r#"{{"sessionId":"s1","configOptions":{}}}"#, access()) }
                    else { format!(r#"{{"sessionId":"s1","configOptions":[{{"id":"model","category":"model","currentValue":"m1","options":{}}}]}}"#, models()) }),
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
                    let failed = self.0.lock().unwrap().mcp_failed.clone();
                    if !failed.is_empty() {
                        // Every server, the working one too, and the same report twice.
                        let mut servers: Vec<String> = failed.iter().map(|n| format!(r#"{{"name":"{n}","status":"failed"}}"#)).collect();
                        servers.push(r#"{"name":"fine","status":"running"}"#.into());
                        let m = format!(r#"{{"jsonrpc":"2.0","method":"_kiro/mcp/status","params":{{"sessionId":"{sid}","servers":[{}]}}}}"#, servers.join(","));
                        Self::say(&out, &m);
                        Self::say(&out, &m);
                    }
                    if ask {
                        let (kind, input) = {
                            let mut g = self.0.lock().unwrap();
                            g.hanging = id;
                            g.asked += 1;
                            (g.ask_kind.clone().unwrap_or_else(|| "edit".into()), g.ask_input.clone().unwrap_or_else(|| "{}".into()))
                        };
                        Self::say(&out, &format!(r#"{{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{{"sessionId":"{sid}","toolCall":{{"toolCallId":"e","kind":"{kind}","title":"Write","rawInput":{input}}},"options":[{{"optionId":"yes","name":"Accept","kind":"allow_once"}},{{"optionId":"always","name":"Always","kind":"allow_always"}},{{"optionId":"no","name":"Reject","kind":"reject_once"}}]}}}}"#));
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

fn make(o: AgentOptions) -> (AcpHost, Fake) { make_for(o, AgentTool::Kiro) }

fn make_for(o: AgentOptions, tool: AgentTool) -> (AcpHost, Fake) {
    let fake = Fake::default();
    let f = fake.clone();
    (AcpHost::with_connect(tool, move || o.clone(), move || f.connect()), fake)
}

fn asks(a: AgentApproval) -> AgentOptions { AgentOptions { approval: a, ..Default::default() } }

/// host.Asking that answers at once.
fn answering(host: &AcpHost, answer: AskAnswer, seen: Option<Arc<Mutex<Vec<(String, AgentAsk)>>>>) {
    host.set_asking(Arc::new(move |sid, ask, _, reply| { if let Some(s) = &seen { s.lock().unwrap().push((sid.to_owned(), ask)); } reply(answer); }));
}

fn answer_of(f: &Fake) -> Option<String> { f.0.lock().unwrap().permission_answer.clone() }

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
fn an_mcp_server_that_does_not_start_is_said_and_the_turn_goes_on() {
    let d = dir("mcp");
    // On, as a 2.x settings.json may have it: ignored now.
    let (host, fake) = make(AgentOptions { require_mcp: true, ..Default::default() });
    fake.set(|g| g.mcp_failed = vec!["playwriter".into()]);
    let (_, events, _, e) = recorders();
    let r = host.run(&d, "go", None, &Cancel::new(), None, Some(e));
    assert_eq!(r.state, KiroState::Completed);
    assert_eq!(r.text, "Renamed it.\n\nMCP server `playwriter` didn’t start, so its tools weren’t available.");
    assert!(!fake.methods().contains(&"session/cancel".to_string()), "the turn isn't stopped for it");
    let mcp: Vec<(String, String, String)> = events.lock().unwrap().iter().filter_map(|e| e.step.as_ref()).filter(|s| s.id.starts_with("hover-mcp-"))
        .map(|s| (s.kind.clone(), s.title.clone(), s.status.clone())).collect();
    assert_eq!(mcp, [("other".to_string(), "Started MCP server playwriter".to_string(), "failed".to_string())], "one step, though it was reported twice");
    // Each turn says what was reported during it.
    fake.set(|g| g.mcp_failed = vec![]);
    let again = host.run(&d, "and again", None, &Cancel::new(), Some("s1"), None);
    assert_eq!(again.text, "Renamed it.");
    fake.set(|g| g.mcp_failed = vec!["a".into(), "b".into()]);
    let both = host.run(&d, "both", None, &Cancel::new(), Some("s1"), None);
    assert!(both.text.ends_with("MCP servers `a`, `b` didn’t start, so their tools weren’t available."), "{}", both.text);
    host.shutdown("test");
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

#[test]
fn autopilot_allows_without_asking() {
    let d = dir("autopilot");
    let (host, fake) = make(AgentOptions::default());
    fake.set(|g| g.ask_to_edit = true);
    let seen: Arc<Mutex<Vec<(String, AgentAsk)>>> = Default::default();
    answering(&host, AskAnswer::Deny, Some(seen.clone()));
    let r = host.run(&d, "change it", None, &Cancel::new(), None, None);
    assert_eq!(answer_of(&fake).as_deref(), Some("yes"));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(r.state, KiroState::Completed);
    host.shutdown("test");
}

#[test]
fn asking_waits_for_the_user_and_trust_holds_for_the_session() {
    let d = dir("trust");
    let (host, fake) = make(asks(AgentApproval::Always));
    fake.set(|g| g.ask_to_edit = true);
    let seen: Arc<Mutex<Vec<(String, AgentAsk)>>> = Default::default();
    let held: Arc<Mutex<Option<Box<dyn FnOnce(AskAnswer) + Send>>>> = Default::default();
    let (s2, h2) = (seen.clone(), held.clone());
    host.set_asking(Arc::new(move |sid, ask, _, reply| { s2.lock().unwrap().push((sid.to_owned(), ask)); *h2.lock().unwrap() = Some(reply); }));
    let (h, d2) = (host.clone(), d.clone());
    let run = std::thread::spawn(move || h.run(&d2, "change it", None, &Cancel::new(), None, None));
    wait_for(|| held.lock().unwrap().is_some());
    std::thread::sleep(Duration::from_millis(100));
    assert!(!run.is_finished(), "the turn waits for the answer");
    (held.lock().unwrap().take().unwrap())(AskAnswer::Trust);
    let r = run.join().unwrap();
    let trusted = answer_of(&fake);
    let again = host.run(&d, "and again", None, &Cancel::new(), Some("s1"), None);
    let seen = seen.lock().unwrap();
    assert_eq!((seen[0].0.as_str(), seen[0].1.kind.as_str()), ("s1", "edit"));
    assert_eq!(r.state, KiroState::Completed);
    assert_eq!(trusted.as_deref(), Some("yes"), "Trust is Hover's: Kiro's own allow-always can change a Kiro setting");
    assert_eq!(again.state, KiroState::Completed);
    assert_eq!(fake.0.lock().unwrap().asked, 2);
    assert_eq!(seen.len(), 1, "the trusted call went ahead without asking again");
    host.shutdown("test");
}

#[test]
fn only_codexs_allow_always_is_picked_its_lasts_the_session_only() {
    for (tool, want) in [(AgentTool::Codex, "always"), (AgentTool::Cursor, "yes")] {
        let d = dir(&format!("always-{}", tool.id()));
        let (host, fake) = make_for(asks(AgentApproval::Always), tool);
        fake.set(|g| g.ask_to_edit = true);
        answering(&host, AskAnswer::Trust, None);
        host.run(&d, "change it", None, &Cancel::new(), None, None);
        assert_eq!(answer_of(&fake).as_deref(), Some(want), "{tool:?}");
        host.shutdown("test");
    }
}

#[test]
fn each_tool_is_put_where_it_asks() {
    let sets = |tool: AgentTool, o: AgentOptions| -> Vec<String> {
        let d = dir(&format!("sets-{}", tool.id()));
        let (host, fake) = make_for(o, tool);
        fake.set(|g| g.offer = true);
        host.run(&d, "go", None, &Cancel::new(), None, None);
        host.shutdown("test");
        let g = fake.0.lock().unwrap();
        g.got.iter().filter(|g| g.0 == "session/set_config_option")
            .map(|g| format!("{}={}", g.1.get("configId").unwrap().as_str().unwrap(), g.1.get("value").unwrap().as_str().unwrap())).collect()
    };
    let (risky, always) = (asks(AgentApproval::Risky), asks(AgentApproval::Always));
    assert!(sets(AgentTool::Kiro, risky.clone()).contains(&"autopilot=off".into()), "asking needs Kiro out of its autopilot");
    assert!(sets(AgentTool::Kiro, AgentOptions::default()).contains(&"autopilot=on".into()));
    assert!(sets(AgentTool::Codex, AgentOptions::default()).contains(&"mode=agent-full-access".into()));
    assert!(sets(AgentTool::Codex, risky.clone()).contains(&"mode=workspace-write".into()), "not agent: Codex's own reviewer would answer for the user");
    assert!(sets(AgentTool::Codex, always).contains(&"mode=read-only".into()));
    assert!(sets(AgentTool::Cursor, risky.clone()).contains(&"mode=agent".into()));
    // Antigravity (T3 Code's mapping): yolo never asks; default sends edits and commands to Hover.
    assert!(sets(AgentTool::Agy, AgentOptions::default()).contains(&"mode=yolo".into()));
    assert!(sets(AgentTool::Agy, risky).contains(&"mode=default".into()));
    assert!(sets(AgentTool::Agy, AgentOptions { read_only: true, ..Default::default() }).contains(&"mode=default".into()), "read only: Hover refuses what it asks");
}

#[test]
fn antigravity_signs_in_before_its_first_session_and_the_others_dont() {
    for (tool, signs_in) in [(AgentTool::Agy, true), (AgentTool::Cursor, false)] {
        let d = dir(&format!("auth-{}", tool.id()));
        let (host, fake) = make_for(AgentOptions::default(), tool);
        let r = host.run(&d, "go", None, &Cancel::new(), None, None);
        host.shutdown("test");
        assert_eq!(r.state, KiroState::Completed, "{tool:?}: {}", r.text);
        let m = fake.methods();
        assert_eq!(m.iter().any(|x| x == "authenticate"), signs_in, "{tool:?}: {m:?}");
        if signs_in {
            let g = fake.0.lock().unwrap();
            let auth = g.got.iter().find(|g| g.0 == "authenticate").unwrap();
            let want = if std::env::var_os("GEMINI_API_KEY").is_some_and(|k| !k.is_empty()) { "gemini-api-key" } else { "oauth-personal" };
            // An API key in the environment, else Google's sign-in.
            assert_eq!(auth.1.get("methodId").and_then(Json::as_str), Some(want));
            assert!(m.iter().position(|x| x == "authenticate") < m.iter().position(|x| x == "session/new"), "{m:?}");
        }
    }
}

#[test]
fn a_denied_call_is_rejected() {
    let d = dir("deny");
    let (host, fake) = make(asks(AgentApproval::Always));
    fake.set(|g| g.ask_to_edit = true);
    answering(&host, AskAnswer::Deny, None);
    let r = host.run(&d, "change it", None, &Cancel::new(), None, None);
    assert_eq!(answer_of(&fake).as_deref(), Some("no"));
    assert_ne!(r.state, KiroState::Completed);
    host.shutdown("test");
}

#[test]
fn risky_lets_edits_in_the_folder_go_and_asks_about_commands() {
    let d = dir("risky");
    let (host, fake) = make(asks(AgentApproval::Risky));
    fake.set(|g| g.ask_to_edit = true);
    let seen: Arc<Mutex<Vec<(String, AgentAsk)>>> = Default::default();
    answering(&host, AskAnswer::Allow, Some(seen.clone()));
    host.run(&d, "edit", None, &Cancel::new(), None, None);
    let edit = seen.lock().unwrap().len();
    fake.set(|g| { g.ask_kind = Some("execute".into()); g.ask_input = Some(r#"{"command":["bash","-lc","npm install three@0.171.0"]}"#.into()); });
    let r = host.run(&d, "install", None, &Cancel::new(), Some("s1"), None);
    let seen = seen.lock().unwrap();
    let last = &seen.last().unwrap().1;
    assert_eq!(edit, 0, "an edit inside the folder isn't asked about");
    assert_eq!(last.command.as_deref(), Some("npm install three@0.171.0"));
    assert!(last.reason.contains("network"), "{}", last.reason);
    assert!(!last.danger);
    assert_eq!(r.state, KiroState::Completed);
    host.shutdown("test");
}

#[test]
fn stopping_withdraws_the_question() {
    let d = dir("withdraw");
    let (host, fake) = make(asks(AgentApproval::Always));
    fake.set(|g| g.ask_to_edit = true);
    let held: Arc<Mutex<Vec<Box<dyn FnOnce(AskAnswer) + Send>>>> = Default::default();
    let h2 = held.clone();
    host.set_asking(Arc::new(move |_, _, _, reply| h2.lock().unwrap().push(reply)));
    let ct = Cancel::new();
    let (h, c2) = (host.clone(), ct.clone());
    let run = std::thread::spawn(move || h.run(&d, "change it", None, &c2, None, None));
    wait_for(|| !held.lock().unwrap().is_empty());
    ct.cancel();
    let r = run.join().unwrap();
    wait_for(|| answer_of(&fake).is_some());
    assert_eq!(r.state, KiroState::Cancelled);
    assert_eq!(answer_of(&fake).as_deref(), Some("cancelled"));
    host.shutdown("test");
}

/// A pasted picture goes to Kiro as an image block (its contents, which a Kiro Web sandbox can see),
/// not as a path; an agent that takes no pictures still gets the path, and a missing file stays a path.
#[test]
fn pictures_go_to_kiro_as_image_blocks_when_it_takes_them() {
    let d = dir("pictures");
    let pic = std::path::Path::new(&d).join("shot.png");
    std::fs::write(&pic, b"\x89PNG fake").unwrap();
    let gone = std::path::Path::new(&d).join("gone.png");
    let prompt = format!("What is this?\n\n{a}{}\n{a}{}", pic.display(), gone.display(), a = hover_agents::acp::ATTACHED);
    let sent = |images: bool| {
        let (host, fake) = make(AgentOptions::default());
        fake.set(|g| g.images = images);
        host.run(&d, &prompt, None, &Cancel::new(), None, None);
        host.shutdown("test");
        let got = fake.0.lock().unwrap().got.clone();
        got.into_iter().find(|g| g.0 == "session/prompt").unwrap().1.get("prompt").unwrap().clone()
    };
    let with = sent(true);
    let blocks = with.items().unwrap();
    assert_eq!(blocks.len(), 2, "{}", with.compact());
    let text = blocks[0].get("text").and_then(Json::as_str).unwrap();
    assert!(text.starts_with("What is this?") && !text.contains("shot.png") && text.contains("gone.png"), "{text}");
    assert_eq!((blocks[1].get("type").and_then(Json::as_str), blocks[1].get("mimeType").and_then(Json::as_str)), (Some("image"), Some("image/png")));
    assert_eq!(hover_core::images::from_base64(blocks[1].get("data").and_then(Json::as_str).unwrap()).unwrap(), b"\x89PNG fake");
    let without = sent(false);
    assert_eq!(without.items().unwrap().len(), 1);
    assert!(without.items().unwrap()[0].get("text").and_then(Json::as_str).unwrap().contains("shot.png"));
}
