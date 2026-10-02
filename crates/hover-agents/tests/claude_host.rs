//! ClaudeHost against a stand-in Claude Code: the Agent SDK's stream-json and control
//! protocol over in-memory pipes, shaped as the real CLI (2.1.287) writes it (recorded
//! by hand from `claude --output-format stream-json --input-format stream-json
//! --permission-prompt-tool stdio`). What a turn does is written into its prompt:
//!
//!   [ask:TOOL:ARG]  calls TOOL (Bash with ARG as the command, Write with ARG as the file)
//!                   and asks Hover first, unless started with bypassPermissions
//!   [question]      AskUserQuestion with two choices
//!   [hang]          never ends until interrupted; [stubborn] ignores the interrupt too
//!   [crash]         says why on stderr and exits mid-turn
//!   [fail]          ends in an error result, as an API error does

use hover_agents::ask::AskAnswer;
use hover_agents::cancel::Cancel;
use hover_agents::claude::{ClaudeHost, Timeouts, MAX_LIVE};
use hover_agents::proc::Link;
use hover_agents::stream::KiroEvent;
use hover_core::json::{self, Json};
use hover_core::model::{AcpOption, AgentApproval, AgentOptions, KiroState};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Out = Arc<Mutex<Option<std::io::PipeWriter>>>;
/// The permission answers a turn waits on, by request id.
type Waiting = Arc<Mutex<Vec<(String, Sender<Json>)>>>;

#[derive(Default)]
struct State {
    /// Each start: its folder and arguments.
    starts: Vec<(String, Vec<String>)>,
    /// Every line Hover sent, in order.
    got: Vec<Json>,
    /// Conversations --resume can't find.
    missing: HashSet<String>,
    interrupts: usize,
    /// Each process's output, newest last (closed when killed).
    outs: Vec<Out>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

fn j(s: &str) -> Json { json::parse(s).unwrap() }
fn s<'a>(e: &'a Json, k: &str) -> Option<&'a str> { e.get(k).and_then(Json::as_str) }

fn say(out: &Out, m: &str) { if let Some(w) = out.lock().unwrap().as_mut() { let _ = writeln!(w, "{m}"); } }

fn directive<'a>(p: &'a str, name: &str) -> Option<&'a str> {
    let i = p.find(&format!("[{name}"))?;
    let rest = &p[i + name.len() + 1..];
    let end = rest.find(']')?;
    Some(rest[..end].trim_start_matches(':'))
}

const MODELS: &str = r#"[{"value":"default","displayName":"Default (recommended)","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"sonnet","displayName":"Sonnet","supportsEffort":true,"supportedEffortLevels":["low","high"]},{"value":"haiku","displayName":"Haiku"}]"#;

impl Fake {
    fn set(&self, f: impl FnOnce(&mut State)) { f(&mut self.0.lock().unwrap()) }
    fn starts(&self) -> Vec<(String, Vec<String>)> { self.0.lock().unwrap().starts.clone() }
    fn sent(&self, kind: &str) -> Vec<Json> { self.0.lock().unwrap().got.iter().filter(|m| s(m, "type") == Some(kind)).cloned().collect() }
    /// The answers Hover gave to can_use_tool, in order.
    fn answers(&self) -> Vec<Json> { self.sent("control_response").into_iter().filter_map(|m| m.get("response").and_then(|r| r.get("response")).cloned()).collect() }

    fn connect(&self, folder: &str, args: &[String]) -> std::io::Result<Option<Link>> {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        let out: Out = Arc::new(Mutex::new(Some(agent_writes)));
        let errors = Arc::new(Mutex::new(String::new()));
        { let mut g = self.0.lock().unwrap(); g.starts.push((folder.into(), args.to_vec())); g.outs.push(out.clone()); }
        let (me, o2, e2, a2) = (self.clone(), out.clone(), errors.clone(), args.to_vec());
        std::thread::spawn(move || me.serve(agent_reads, o2, e2, a2));
        let (o3, e3) = (out.clone(), errors.clone());
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { o3.lock().unwrap().take(); }),
            errors: Box::new(move || e3.lock().unwrap().clone()) }))
    }

    fn serve(&self, r: std::io::PipeReader, out: Out, errors: Arc<Mutex<String>>, args: Vec<String>) {
        let bypass = args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "bypassPermissions");
        let resume = args.iter().find_map(|a| a.strip_prefix("--resume=")).map(str::to_owned);
        let n = self.0.lock().unwrap().starts.len();
        let sid = resume.clone().unwrap_or_else(|| format!("sid-{n}"));
        let answers: Waiting = Default::default();
        let interrupt: Arc<Mutex<Option<Sender<()>>>> = Default::default();
        for line in BufReader::new(r).lines() {
            let Ok(line) = line else { return };
            let m = j(&line);
            self.0.lock().unwrap().got.push(m.clone());
            match s(&m, "type") {
                Some("control_request") => {
                    let id = s(&m, "request_id").unwrap().to_owned();
                    match m.get("request").and_then(|r| s(r, "subtype")) {
                        Some("initialize") => {
                            if resume.as_ref().is_some_and(|r| self.0.lock().unwrap().missing.contains(r)) {
                                let why = format!("No conversation found with session ID: {}", resume.as_ref().unwrap());
                                *errors.lock().unwrap() = format!("{why}\n");
                                say(&out, &format!(r#"{{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["{why}"],"num_turns":0}}"#));
                                out.lock().unwrap().take();
                                return;
                            }
                            say(&out, &format!(r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}","response":{{"models":{MODELS},"current_permission_mode":"default"}}}}}}"#));
                        }
                        Some("interrupt") => {
                            self.0.lock().unwrap().interrupts += 1;
                            say(&out, &format!(r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}","response":{{"still_queued":[]}}}}}}"#));
                            if let Some(tx) = interrupt.lock().unwrap().take() { let _ = tx.send(()); }
                        }
                        _ => say(&out, &format!(r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{id}","error":"unknown"}}}}"#)),
                    }
                }
                Some("control_response") => {
                    let r = m.get("response").unwrap();
                    let id = s(r, "request_id").unwrap();
                    let tx = { let mut a = answers.lock().unwrap(); a.iter().position(|x| x.0 == id).map(|i| a.remove(i).1) };
                    if let Some(tx) = tx { let _ = tx.send(r.get("response").cloned().unwrap_or(Json::Null)); }
                }
                Some("user") => {
                    let text = m.get("message").and_then(|x| x.get("content")).and_then(|c| match c { Json::Arr(a) => a.first().cloned(), _ => None })
                        .and_then(|b| s(&b, "text").map(str::to_owned)).unwrap_or_default();
                    let (out, sid, answers, interrupt, errors) = (out.clone(), sid.clone(), answers.clone(), interrupt.clone(), errors.clone());
                    std::thread::spawn(move || turn(&out, &sid, &text, bypass, &answers, &interrupt, &errors));
                }
                _ => {}
            }
        }
    }
}

fn turn(out: &Out, sid: &str, text: &str, bypass: bool, answers: &Mutex<Vec<(String, Sender<Json>)>>, interrupt: &Mutex<Option<Sender<()>>>, errors: &Mutex<String>) {
    let ev = |e: &str| say(out, &format!(r#"{{"type":"stream_event","event":{e},"session_id":"{sid}","parent_tool_use_id":null}}"#));
    let result = |r: &str| say(out, &format!(r#"{{"type":"result","subtype":"success","is_error":false,"result":{},"session_id":"{sid}","num_turns":1,"modelUsage":{{"claude-x":{{"contextWindow":200000}}}}}}"#, Json::str(r).compact()));
    say(out, &format!(r#"{{"type":"system","subtype":"init","cwd":"/p","session_id":"{sid}","permissionMode":"default"}}"#));
    let (itx, irx) = channel();
    *interrupt.lock().unwrap() = Some(itx);
    let interrupted = || say(out, &format!(r#"{{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["[ede_diagnostic] result_type=user"],"session_id":"{sid}","num_turns":1}}"#));
    if text.contains("[hang]") || text.contains("[stubborn]") {
        ev(r#"{"type":"message_start","message":{"id":"m0"}}"#);
        ev(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Halfway there"}}"#);
        if irx.recv().is_ok() && text.contains("[hang]") { interrupted(); }
        return;
    }
    if text.contains("[crash]") {
        *errors.lock().unwrap() = "boom: the stand-in fell over\n".into();
        out.lock().unwrap().take();
        return;
    }
    if text.contains("[fail]") {
        say(out, &format!(r#"{{"type":"result","subtype":"success","is_error":true,"result":"API Error: 400 bad request","session_id":"{sid}","num_turns":1}}"#));
        return;
    }
    let ask = |tool: &str, input: &str, tid: &str| -> Json {
        let id = format!("req-{tid}");
        let (tx, rx) = channel();
        answers.lock().unwrap().push((id.clone(), tx));
        say(out, &format!(r#"{{"type":"control_request","request_id":"{id}","request":{{"subtype":"can_use_tool","tool_name":"{tool}","input":{input},"tool_use_id":"{tid}"}}}}"#));
        rx.recv_timeout(Duration::from_secs(10)).unwrap_or(Json::Null)
    };
    let mut said = String::from("Done.");
    if let Some(a) = directive(text, "ask") {
        let (tool, arg) = a.split_once(':').unwrap_or((a, ""));
        let input = if tool == "Write" { format!(r#"{{"file_path":{},"content":"x\n"}}"#, Json::str(arg).compact()) } else { format!(r#"{{"command":{},"description":"Run it"}}"#, Json::str(arg).compact()) };
        say(out, &format!(r#"{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"tool_use","id":"t1","name":"{tool}","input":{input}}}],"usage":{{"input_tokens":30000,"output_tokens":10}}}},"parent_tool_use_id":null,"session_id":"{sid}"}}"#));
        let r = if bypass { j(r#"{"behavior":"allow"}"#) } else { ask(tool, &input, "t1") };
        let allowed = s(&r, "behavior") == Some("allow");
        let content = if allowed { "ok".to_owned() } else { s(&r, "message").unwrap_or("denied").to_owned() };
        let extra = if tool == "Bash" && allowed { r#","tool_use_result":{"stdout":"ran it","stderr":"","interrupted":false}"# } else { "" };
        say(out, &format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":{},"is_error":{}}}]}},"parent_tool_use_id":null,"session_id":"{sid}"{extra}}}"#, Json::str(&content).compact(), !allowed));
        said = if allowed { format!("Done. {tool} went through.") } else { format!("Done. {tool} was refused: {content}") };
    }
    if text.contains("[question]") {
        let input = r#"{"questions":[{"question":"Tabs or spaces?","header":"Indent","multiSelect":false,"options":[{"label":"Tabs","description":"t"},{"label":"Spaces","description":"s"}]}]}"#;
        say(out, &format!(r#"{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{input}}}]}},"parent_tool_use_id":null,"session_id":"{sid}"}}"#));
        let r = ask("AskUserQuestion", input, "q1");
        said = match r.get("updatedInput").and_then(|u| u.get("answers")) { Some(a) => format!("Done. answered {}", a.compact()), None => format!("Done. skipped: {}", s(&r, "message").unwrap_or("")) };
    }
    // A subagent's own words never reach the answer.
    say(out, &format!(r#"{{"type":"assistant","message":{{"id":"sub","content":[{{"type":"text","text":"inside the subagent"}}]}},"parent_tool_use_id":"t9","session_id":"{sid}"}}"#));
    ev(r#"{"type":"message_start","message":{"id":"m2"}}"#);
    ev(&format!(r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":{}}}}}"#, Json::str(&said).compact()));
    say(out, &format!(r#"{{"type":"assistant","message":{{"id":"m2","content":[{{"type":"text","text":{}}}],"usage":{{"input_tokens":40000,"cache_read_input_tokens":10000,"output_tokens":0}}}},"parent_tool_use_id":null,"session_id":"{sid}"}}"#, Json::str(&said).compact()));
    result(&said);
}

fn dir(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-claude-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn short() -> Timeouts { Timeouts { start: Duration::from_secs(5), stop_grace: Duration::from_millis(600) } }

fn make(o: AgentOptions) -> (ClaudeHost, Fake) {
    let fake = Fake::default();
    let f = fake.clone();
    let opts = Arc::new(Mutex::new(o));
    let o2 = opts.clone();
    (ClaudeHost::with_connect(move || o2.lock().unwrap().clone(), move |folder: &str, args: &[String]| f.connect(folder, args), short()), fake)
}

fn has(args: &[String], a: &str, b: &str) -> bool { args.windows(2).any(|w| w[0] == a && w[1] == b) }

type Events = Arc<Mutex<Vec<KiroEvent>>>;

fn run(h: &ClaudeHost, folder: &str, prompt: &str, resume: Option<&str>, access: Option<&str>) -> (hover_agents::stream::KiroResult, Events) {
    let ev: Events = Default::default();
    let e2 = ev.clone();
    let r = h.run(folder, prompt, None, &Cancel::new(), resume, Some(Box::new(move |e| e2.lock().unwrap().push(e))), access);
    (r, ev)
}

fn sid_of(ev: &Events) -> Option<String> { ev.lock().unwrap().iter().find_map(|e| e.session_id.clone()) }

fn answering(h: &ClaudeHost, answer: AskAnswer) -> Arc<Mutex<Vec<String>>> {
    let asked: Arc<Mutex<Vec<String>>> = Default::default();
    let a2 = asked.clone();
    h.set_asking(Arc::new(move |sid, ask, _, reply| { a2.lock().unwrap().push(format!("{sid} {} {} {:?} {:?}", ask.kind, ask.title, ask.command, ask.path)); reply(answer) }));
    asked
}

#[test]
fn a_turn_starts_it_in_the_folder_in_sdk_mode_and_reads_the_answer() {
    let d = dir("turn");
    let (h, fake) = make(AgentOptions::default());
    let offers: Arc<Mutex<Vec<AcpOption>>> = Default::default();
    let o2 = offers.clone();
    h.on_options_seen(move |_, o| *o2.lock().unwrap() = o.to_vec());
    let (r, ev) = run(&h, &d, "hello", None, None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Done."));
    let starts = fake.starts();
    assert_eq!(starts.len(), 1);
    let (folder, args) = &starts[0];
    assert_eq!(folder, &d, "started in the session's folder");
    for (a, b) in [("--output-format", "stream-json"), ("--input-format", "stream-json"), ("--permission-prompt-tool", "stdio"), ("--permission-mode", "bypassPermissions")] {
        assert!(has(args, a, b), "{a} {b} in {args:?}");
    }
    assert!(args.contains(&"--allow-dangerously-skip-permissions".into()) && args.contains(&"--include-partial-messages".into()));
    assert!(!args.iter().any(|a| a.contains("hello")), "the prompt never goes on the command line");
    // Initialize first, then the prompt as a user message.
    let init = fake.sent("control_request");
    assert_eq!(init[0].get("request").and_then(|r| s(r, "subtype")), Some("initialize"));
    let user = fake.sent("user");
    assert_eq!(user[0].get("message").unwrap().get("content").unwrap().compact(), r#"[{"type":"text","text":"hello"}]"#);
    assert_eq!(sid_of(&ev).as_deref(), Some("sid-1"));
    // The context: the last answer's 50k tokens of a 200k window.
    assert!(ev.lock().unwrap().iter().any(|e| e.context == Some(25.0)), "{:?}", ev.lock().unwrap());
    // Its models, each with the efforts it takes.
    let o = offers.lock().unwrap().clone();
    assert_eq!(o[0].id, "model");
    assert_eq!(o[0].choices.iter().map(|c| (c.value.as_str(), c.levels.as_ref().map(Vec::len))).collect::<Vec<_>>(), vec![("default", Some(5)), ("sonnet", Some(2)), ("haiku", Some(0))]);
    h.shutdown("test");
}

#[test]
fn a_reply_carries_on_in_the_same_process_until_its_settings_change() {
    let d = dir("reply");
    let (h, fake) = make(AgentOptions::default());
    let (_, ev) = run(&h, &d, "first", None, None);
    let sid = sid_of(&ev).unwrap();
    let (r, _) = run(&h, &d, "second", Some(&sid), None);
    assert_eq!(r.state, KiroState::Completed);
    assert_eq!(fake.starts().len(), 1, "the same process");
    assert_eq!(fake.sent("control_request").iter().filter(|m| m.get("request").and_then(|r| s(r, "subtype")) == Some("initialize")).count(), 1);
    // Ask first for this one: started again, on the same conversation.
    let (r, _) = run(&h, &d, "third", Some(&sid), Some("risky"));
    assert_eq!(r.state, KiroState::Completed);
    let starts = fake.starts();
    assert_eq!(starts.len(), 2);
    assert!(has(&starts[1].1, "--permission-mode", "default") && starts[1].1.contains(&format!("--resume={sid}")));
    assert_eq!(h.live(), 1, "the old one went");
    h.shutdown("test");
}

#[test]
fn after_a_shutdown_a_reply_resumes_and_a_lost_conversation_starts_anew() {
    let d = dir("resume");
    let (h, fake) = make(AgentOptions::default());
    let (_, ev) = run(&h, &d, "first", None, None);
    let sid = sid_of(&ev).unwrap();
    h.shutdown("idle");
    assert!(!h.alive());
    let (r, _) = run(&h, &d, "again", Some(&sid), None);
    assert_eq!(r.state, KiroState::Completed);
    assert!(fake.starts()[1].1.contains(&format!("--resume={sid}")));
    h.shutdown("idle");
    fake.set(|s| { s.missing.insert(sid.clone()); });
    let (r, ev) = run(&h, &d, "once more", Some(&sid), None);
    assert_eq!(r.state, KiroState::Completed, "{}", r.text);
    assert!(r.text.ends_with("no longer had the earlier conversation, so this reply started a new one.*"), "{}", r.text);
    let starts = fake.starts();
    assert!(!starts[3].1.iter().any(|a| a.starts_with("--resume")), "a new conversation");
    assert_eq!(sid_of(&ev).as_deref(), Some("sid-4"));
    h.shutdown("test");
}

#[test]
fn ask_first_asks_for_commands_and_leaves_edits_in_the_folder_alone() {
    let d = dir("ask");
    let (h, fake) = make(AgentOptions { approval: AgentApproval::Risky, ..Default::default() });
    let asked = answering(&h, AskAnswer::Allow);
    let inside = std::path::Path::new(&d).join("a.txt").to_string_lossy().into_owned();
    let (r, _) = run(&h, &d, &format!("[ask:Write:{inside}]"), None, None);
    assert_eq!(r.text, "Done. Write went through.");
    assert!(asked.lock().unwrap().is_empty(), "an edit in the folder goes ahead");
    let (r, ev) = run(&h, &d, "[ask:Bash:npm install left-pad]", None, None);
    assert_eq!(r.text, "Done. Bash went through.");
    let sid = sid_of(&ev).unwrap();
    assert_eq!(asked.lock().unwrap().clone(), vec![format!("{sid} execute Run a command Some(\"npm install left-pad\") None")]);
    // Allowed with the input it asked about; the command's output is in its step.
    assert_eq!(fake.answers().last().unwrap().compact(), r#"{"behavior":"allow","updatedInput":{"command":"npm install left-pad","description":"Run it"}}"#);
    let steps: Vec<_> = ev.lock().unwrap().iter().filter_map(|e| e.step.clone()).collect();
    let done = steps.iter().rev().find(|x| x.kind == "execute").unwrap();
    assert_eq!((done.status.as_str(), done.output.as_deref(), done.target.as_deref()), ("completed", Some("ran it"), Some("npm install left-pad")));
    h.shutdown("test");
}

#[test]
fn a_denied_command_is_said_and_trust_lasts_the_conversation() {
    let d = dir("deny");
    let (h, fake) = make(AgentOptions { approval: AgentApproval::Always, ..Default::default() });
    let asked = answering(&h, AskAnswer::Deny);
    let (r, ev) = run(&h, &d, "[ask:Bash:rm -rf build]", None, None);
    assert_eq!(r.text, "Done. Bash was refused: The user declined this.");
    assert_eq!(fake.answers()[0].compact(), r#"{"behavior":"deny","message":"The user declined this."}"#);
    let sid = sid_of(&ev).unwrap();
    // Trusted once, the same command goes ahead from then on without asking.
    let asked2 = answering(&h, AskAnswer::Trust);
    run(&h, &d, "[ask:Bash:cargo test]", Some(&sid), None);
    run(&h, &d, "[ask:Bash:cargo test]", Some(&sid), None);
    assert_eq!((asked.lock().unwrap().len(), asked2.lock().unwrap().len()), (1, 1));
    assert_eq!(fake.answers().iter().filter(|a| s(a, "behavior") == Some("allow")).count(), 2);
    h.shutdown("test");
}

#[test]
fn read_only_switches_off_its_edit_tools_and_refuses_the_rest() {
    let d = dir("ro");
    let (h, fake) = make(AgentOptions { read_only: true, ..Default::default() });
    answering(&h, AskAnswer::Allow);
    let (r, _) = run(&h, &d, "[ask:mcp__db__drop:x]", None, None);
    let args = &fake.starts()[0].1;
    assert!(has(args, "--disallowedTools", "Edit,MultiEdit,Write,NotebookEdit,Bash,PowerShell") && has(args, "--permission-mode", "default"));
    assert_eq!(r.state, KiroState::Completed);
    assert!(r.text.starts_with("Done. mcp__db__drop was refused: Hover has Claude Code set to read only"), "{}", r.text);
    assert!(r.text.ends_with("so the changes or commands it tried were refused.*"));
    // Voice's routing turn: no tools at all.
    let (_, _) = run(&h, &d, "route this", None, Some("none"));
    assert!(has(&fake.starts()[1].1, "--tools", ""));
    h.shutdown("test");
}

#[test]
fn a_question_goes_to_the_user_and_comes_back_by_its_own_text() {
    let d = dir("question");
    let (h, fake) = make(AgentOptions::default());
    answering(&h, AskAnswer::Deny);
    let got: Arc<Mutex<Vec<String>>> = Default::default();
    let g2 = got.clone();
    h.set_questioning(Arc::new(move |_, ask, _, reply| {
        let q = ask.questions.clone().unwrap();
        g2.lock().unwrap().push(format!("{} / {} / {}", ask.title, q[0].question, q[0].options.iter().map(|o| o.0.clone()).collect::<Vec<_>>().join(",")));
        reply(Some(vec![vec!["Tabs".into()]]))
    }));
    let (r, _) = run(&h, &d, "[question]", None, None);
    assert_eq!(got.lock().unwrap().clone(), vec!["Indent / Tabs or spaces? / Tabs,Spaces".to_string()]);
    assert_eq!(r.text, r#"Done. answered {"Tabs or spaces?":"Tabs"}"#);
    let a = fake.answers()[0].clone();
    assert!(a.get("updatedInput").unwrap().get("questions").is_some(), "the questions go back with the answers");
    // Skipped: denied, and the agent hears so.
    h.set_questioning(Arc::new(|_, _, _, reply| reply(None)));
    let (r, _) = run(&h, &d, "[question]", None, None);
    assert_eq!(r.text, "Done. skipped: The user skipped the question.");
    h.shutdown("test");
}

#[test]
fn stop_interrupts_the_turn_and_one_that_wont_stop_is_ended() {
    let d = dir("stop");
    let (h, fake) = make(AgentOptions::default());
    let ct = Cancel::new();
    let (h2, d2, c2) = (h.clone(), d.clone(), ct.clone());
    let t = std::thread::spawn(move || h2.run(&d2, "[hang]", None, &c2, None, None, None));
    std::thread::sleep(Duration::from_millis(300));
    ct.cancel();
    let r = t.join().unwrap();
    assert_eq!((r.state, r.text.as_str()), (KiroState::Cancelled, "Halfway there"));
    assert_eq!(fake.0.lock().unwrap().interrupts, 1);
    assert!(h.alive(), "an interrupted turn keeps its process for the next reply");
    // It doesn't stop: after the grace period its process is ended, and it reads as stopped.
    let ct = Cancel::new();
    let (h2, d2, c2) = (h.clone(), d.clone(), ct.clone());
    let started = Instant::now();
    let t = std::thread::spawn(move || h2.run(&d2, "[stubborn]", None, &c2, None, None, None));
    std::thread::sleep(Duration::from_millis(300));
    ct.cancel();
    let r = t.join().unwrap();
    assert_eq!(r.state, KiroState::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(fake.0.lock().unwrap().outs.last().unwrap().lock().unwrap().is_none(), "its process was ended");
    h.shutdown("test");
}

#[test]
fn a_crash_and_an_error_fail_with_what_it_said() {
    let d = dir("crash");
    let (h, _) = make(AgentOptions::default());
    let (r, _) = run(&h, &d, "[crash]", None, None);
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("stopped unexpectedly") && r.text.contains("boom: the stand-in fell over"), "{}", r.text);
    assert!(!h.alive());
    let (r, _) = run(&h, &d, "[fail]", None, None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Failed, "API Error: 400 bad request"));
    let none = ClaudeHost::with_connect(AgentOptions::default, |_: &str, _: &[String]| Ok(None), short());
    let r = none.run(&d, "hi", None, &Cancel::new(), None, None, None);
    assert!(r.state == KiroState::Failed && r.text.starts_with("Claude Code isn’t installed. Install Claude Code"), "{}", r.text);
    h.shutdown("test");
}

#[test]
fn only_a_few_conversations_keep_a_process() {
    let d = dir("cap");
    let (h, fake) = make(AgentOptions::default());
    let mut sids = vec![];
    for i in 0..MAX_LIVE + 1 { let (_, ev) = run(&h, &d, &format!("task {i}"), None, None); sids.push(sid_of(&ev).unwrap()); }
    assert_eq!(h.live(), MAX_LIVE);
    assert!(fake.0.lock().unwrap().outs[0].lock().unwrap().is_none(), "the least recently used one went");
    // A reply to it starts it again on its conversation.
    let (r, _) = run(&h, &d, "back", Some(&sids[0]), None);
    assert_eq!(r.state, KiroState::Completed);
    assert!(fake.starts().last().unwrap().1.contains(&format!("--resume={}", sids[0])));
    h.shutdown("test");
}

#[test]
fn the_model_and_an_effort_it_takes_go_on_the_command_line() {
    let d = dir("model");
    let (h, fake) = make(AgentOptions { model: Some("sonnet".into()), effort: Some("high".into()), ..Default::default() });
    run(&h, &d, "hi", None, None);
    let a = &fake.starts()[0].1;
    assert!(has(a, "--model", "sonnet") && has(a, "--effort", "high"), "{a:?}");
    h.shutdown("test");
    // Haiku takes no effort: none is sent, now that the models are known.
    let (h2, fake2) = make(AgentOptions { model: Some("haiku".into()), effort: Some("high".into()), ..Default::default() });
    run(&h2, &d, "hi", None, None);
    let (_, ev) = run(&h2, &d, "hi again", None, None);
    assert!(sid_of(&ev).is_some());
    let a = &fake2.starts()[1].1;
    assert!(has(a, "--model", "haiku") && !a.iter().any(|x| x == "--effort"), "{a:?}");
    h2.shutdown("test");
}
