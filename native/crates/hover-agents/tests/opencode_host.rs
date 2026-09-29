//! OpenCodeHostTests.cs: OpenCode through Hover's host, against a stand-in "opencode
//! serve" with the same routes, Basic auth and event stream the real one has (the C#
//! checked it against 1.18.31). Each test's server is its own, on a port of its own.

use hover_agents::ask::{AgentAsk, AgentQuestion, AskAnswer};
use hover_agents::cancel::Cancel;
use hover_agents::opencode::{OpenCodeHost, OpenCodeLink, Timeouts};
use hover_agents::session::{KiroSessions, RunArgs, RunTask};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::json::{self, Json};
use hover_core::model::{AgentApproval, AgentOptions, AgentTool, KiroState};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PASSWORD: &str = "pw-for-tests";
const PROVIDERS: &str = r#"{"providers":[{"id":"p","name":"Prov","models":{"a/b":{"id":"a/b","name":"A B","variants":{"low":{},"high":{}},"limit":{"context":1000}},"m":{"id":"m","name":"M","limit":{"context":1000}}}}],"default":{"p":"m"}}"#;
const AGENTS: &str = r#"[{"name":"build","mode":"primary","permission":[{"permission":"*","pattern":"*","action":"allow"},{"permission":"question","pattern":"*","action":"deny"},{"permission":"question","pattern":"*","action":"allow"},{"permission":"bash","pattern":"rm *","action":"deny"}]},
 {"name":"plan","mode":"primary","permission":[{"permission":"edit","pattern":"*","action":"deny"}]},
 {"name":"title","mode":"primary","hidden":true,"permission":[]},
 {"name":"explore","mode":"subagent","permission":[]}]"#;

type OnPrompt = Box<dyn Fn(&Fake, &str, &str) + Send + Sync>;
type OnReply = Box<dyn Fn(&Fake, &str, &str) + Send + Sync>;
type OnAbort = Box<dyn Fn(&Fake, &str) + Send + Sync>;

#[derive(Default)]
struct State {
    version: String,
    unauthorized: usize,
    connects: usize,
    prompt_hangs: bool,
    status: HashMap<String, String>,
    last_mid: Option<String>,
    last_sid: Option<String>,
    directories: Vec<String>,
    prompts: Vec<Json>,
    created: Vec<Json>,
    prompt_sessions: Vec<String>,
    patched: Vec<String>,
    aborts: Vec<String>,
    messages: Vec<String>,
    sessions: HashSet<String>,
    replies: Vec<(String, String, Option<Json>)>,
    history: String,
    streams: Vec<TcpStream>,
}

#[derive(Clone)]
struct Fake(Arc<Inner>);

struct Inner {
    port: u16,
    st: Mutex<State>,
    on_prompt: Mutex<Option<Arc<OnPrompt>>>,
    on_reply: Mutex<Option<Arc<OnReply>>>,
    on_abort: Mutex<Option<Arc<OnAbort>>>,
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Fake {
    fn new() -> Fake {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let f = Fake(Arc::new(Inner { port, st: Mutex::new(State { version: "1.18.31".into(), history: "[]".into(), ..Default::default() }),
            on_prompt: Mutex::new(None), on_reply: Mutex::new(None), on_abort: Mutex::new(None) }));
        let f2 = f.clone();
        std::thread::spawn(move || for s in l.incoming().flatten() { let f = f2.clone(); std::thread::spawn(move || f.answer(s)); });
        f
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> { self.0.st.lock().unwrap() }
    fn on_prompt(&self, f: impl Fn(&Fake, &str, &str) + Send + Sync + 'static) { *self.0.on_prompt.lock().unwrap() = Some(Arc::new(Box::new(f))); }
    fn on_reply(&self, f: impl Fn(&Fake, &str, &str) + Send + Sync + 'static) { *self.0.on_reply.lock().unwrap() = Some(Arc::new(Box::new(f))); }
    fn on_abort(&self, f: impl Fn(&Fake, &str) + Send + Sync + 'static) { *self.0.on_abort.lock().unwrap() = Some(Arc::new(Box::new(f))); }

    fn link(&self) -> OpenCodeLink {
        OpenCodeLink { url: format!("http://127.0.0.1:{}", self.0.port), password: PASSWORD.into(), kill: Box::new(|| {}), errors: Box::new(String::new), exited: None }
    }

    fn drop_streams(&self) { for s in self.st().streams.drain(..) { let _ = s.shutdown(std::net::Shutdown::Both); } }

    fn answer(&self, mut s: TcpStream) {
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 { return; }
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
        let (mut auth, mut length) = (String::new(), 0usize);
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).unwrap_or(0) == 0 { break; }
            let h = h.trim_end();
            if h.is_empty() { break; }
            if let Some((k, v)) = h.split_once(':') {
                if k.eq_ignore_ascii_case("authorization") { auth = v.trim().into(); }
                if k.eq_ignore_ascii_case("content-length") { length = v.trim().parse().unwrap_or(0); }
            }
        }
        let mut body = vec![0u8; length];
        r.read_exact(&mut body).unwrap();
        let body = (!body.is_empty()).then(|| json::parse(&String::from_utf8_lossy(&body)).unwrap());
        let send = |s: &mut TcpStream, status: u16, text: &str| {
            let _ = s.write_all(format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).as_bytes());
        };
        if auth != format!("Basic {}", hover_agents::http::base64(format!("opencode:{PASSWORD}").as_bytes())) {
            self.st().unauthorized += 1;
            return send(&mut s, 401, "{}");
        }
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        if let Some(d) = query.split('&').find_map(|q| q.strip_prefix("directory=")) { self.st().directories.push(unescape(d)); }
        let seg: Vec<String> = path.trim_matches('/').split('/').map(unescape).collect();
        let seg: Vec<&str> = seg.iter().map(String::as_str).collect();
        let reply_hook = |kind: &str, id: &str| { let h = self.0.on_reply.lock().unwrap().clone(); if let Some(h) = h { h(self, kind, id); } };
        match (method.as_str(), seg.as_slice()) {
            ("GET", ["global", "health"]) => { let v = self.st().version.clone(); send(&mut s, 200, &format!(r#"{{"healthy":true,"version":"{v}"}}"#)) }
            ("GET", ["config", "providers"]) => send(&mut s, 200, PROVIDERS),
            ("GET", ["config"]) => send(&mut s, 200, "{}"),
            ("GET", ["agent"]) => send(&mut s, 200, AGENTS),
            ("GET", ["permission"]) | ("GET", ["question"]) => send(&mut s, 200, "[]"),
            ("GET", ["event"]) => {
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                let mut st = self.st();
                st.connects += 1;
                // Chunked, as HttpListener sends it: one chunk per event.
                let mut w = Chunked(s.try_clone().unwrap());
                let _ = w.write_all(b"data: {\"type\":\"server.connected\",\"properties\":{}}\n\n");
                st.streams.push(s);
                drop(st);
                // Later events go through push() on the raw socket, so they are framed there.
                let _ = w;
            }
            ("POST", ["session"]) => {
                let mut st = self.st();
                st.created.push(body.unwrap());
                let sid = if st.sessions.contains("ses_2") && st.created.len() > 1 { "ses_2" } else { "ses_1" };
                st.sessions.insert(sid.into());
                drop(st);
                send(&mut s, 200, &format!(r#"{{"id":"{sid}","directory":"x"}}"#))
            }
            ("GET", ["session", "status"]) => {
                let st = self.st();
                let busy: Vec<String> = st.status.iter().filter(|(_, v)| *v != "idle").map(|(k, v)| format!(r#""{k}":{{"type":"{v}"}}"#)).collect();
                drop(st);
                send(&mut s, 200, &format!("{{{}}}", busy.join(",")))
            }
            ("GET", ["session", id]) => {
                if self.st().sessions.contains(*id) { send(&mut s, 200, &format!(r#"{{"id":"{id}"}}"#)) }
                else { send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"Session not found"}}"#) }
            }
            ("PATCH", ["session", id]) => { self.st().patched.push(id.to_string()); send(&mut s, 200, &format!(r#"{{"id":"{id}"}}"#)) }
            ("GET", ["session", _, "message", mid]) => {
                if self.st().messages.iter().any(|m| m == mid) { send(&mut s, 200, &format!(r#"{{"info":{{"id":"{mid}","role":"user"}},"parts":[]}}"#)) }
                else { send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"no"}}"#) }
            }
            ("GET", ["session", _, "message"]) => { let h = self.st().history.clone(); send(&mut s, 200, &h) }
            ("POST", ["session", id, "prompt_async"]) => {
                let body = body.unwrap();
                let mid = body.get("messageID").unwrap().as_str().unwrap().to_owned();
                let hangs = {
                    let mut st = self.st();
                    st.prompts.push(body);
                    st.prompt_sessions.push(id.to_string());
                    st.last_mid = Some(mid.clone());
                    st.last_sid = Some(id.to_string());
                    st.prompt_hangs
                };
                let h = self.0.on_prompt.lock().unwrap().clone();
                if let Some(h) = h { h(self, id, &mid); }
                if hangs { std::thread::sleep(Duration::from_millis(2500)); let _ = s.shutdown(std::net::Shutdown::Both); return; }
                send(&mut s, 204, "")
            }
            ("POST", ["session", id, "abort"]) => {
                self.st().aborts.push(id.to_string());
                send(&mut s, 200, "true");
                let h = self.0.on_abort.lock().unwrap().clone();
                if let Some(h) = h { h(self, id); }
            }
            ("POST", ["permission", id, "reply"]) => { self.st().replies.push(("permission".into(), id.to_string(), body)); send(&mut s, 200, "true"); reply_hook("permission", id) }
            ("POST", ["question", id, "reply"]) => { self.st().replies.push(("question".into(), id.to_string(), body)); send(&mut s, 200, "true"); reply_hook("question", id) }
            ("POST", ["question", id, "reject"]) => { self.st().replies.push(("reject".into(), id.to_string(), body)); send(&mut s, 200, "true"); reply_hook("reject", id) }
            _ => send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"no route"}}"#),
        }
    }
}

/// Writes each write as one HTTP chunk.
struct Chunked(TcpStream);
impl Write for Chunked {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.write_all(format!("{:x}\r\n", b.len()).as_bytes())?; self.0.write_all(b)?; self.0.write_all(b"\r\n")?; Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { self.0.flush() }
}

impl Fake {
    /// push() for chunked streams: every event is one chunk.
    fn ev(&self, e: String) {
        let line = format!("data: {e}\n\n");
        let chunk = format!("{:x}\r\n{line}\r\n", line.len());
        self.st().streams.retain_mut(|w| w.write_all(chunk.as_bytes()).and_then(|_| w.flush()).is_ok());
    }
}

fn status(f: &Fake, sid: &str, t: &str) { f.ev(format!(r#"{{"type":"session.status","properties":{{"sessionID":"{sid}","status":{{"type":"{t}"}}}}}}"#)); }
fn user(f: &Fake, sid: &str, mid: &str) { f.ev(format!(r#"{{"type":"message.updated","properties":{{"sessionID":"{sid}","info":{{"id":"{mid}","role":"user","sessionID":"{sid}"}}}}}}"#)); }

/// A turn as the real server sends it: the user message, busy, the assistant message, a
/// write tool, a text part made of a delta and then the whole part, and idle.
fn reply(f: &Fake, sid: &str, mid: &str, text: &str) {
    let am = format!("msg_zz{}", &mid[6..]);
    user(f, sid, mid);
    status(f, sid, "busy");
    f.ev(format!(r#"{{"type":"message.updated","properties":{{"sessionID":"{sid}","info":{{"id":"{am}","parentID":"{mid}","role":"assistant","sessionID":"{sid}","providerID":"p","modelID":"a/b","tokens":{{"input":400,"output":100,"cache":{{"read":0,"write":0}}}}}}}}}}"#));
    f.ev(format!(r#"{{"type":"message.part.updated","properties":{{"sessionID":"{sid}","part":{{"id":"prt_t1","messageID":"{am}","sessionID":"{sid}","type":"tool","tool":"write","callID":"call_1","state":{{"status":"completed","input":{{"filePath":"hello.txt","content":"hi\n"}},"title":"hello.txt"}}}}}}}}"#));
    f.ev(format!(r#"{{"type":"message.part.updated","properties":{{"sessionID":"{sid}","part":{{"id":"prt_x1","messageID":"{am}","sessionID":"{sid}","type":"text","text":""}}}}}}"#));
    let head: String = text.chars().take(5).collect();
    f.ev(format!(r#"{{"type":"message.part.delta","properties":{{"sessionID":"{sid}","messageID":"{am}","partID":"prt_x1","field":"text","delta":"{head}"}}}}"#));
    f.ev(format!(r#"{{"type":"message.part.updated","properties":{{"sessionID":"{sid}","part":{{"id":"prt_x1","messageID":"{am}","sessionID":"{sid}","type":"text","text":"{text}"}}}}}}"#));
    status(f, sid, "idle");
}

fn quick() -> Timeouts { Timeouts { send: Duration::from_secs(1), stop_grace: Duration::from_secs(2), quiet: Duration::from_secs(2), ..Default::default() } }

fn host(f: &Fake, o: AgentOptions) -> OpenCodeHost {
    let f = f.clone();
    OpenCodeHost::with_connect(move || o.clone(), move || Some(f.link()), quick())
}

fn dir() -> String {
    let d = std::env::temp_dir().join(format!("hover oc ü {}", &hover_core::guid_n()[..6]));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

type Seen = Arc<Mutex<Vec<KiroEvent>>>;

/// Run with a limit: a run that hangs fails the test instead of the suite.
fn run(h: &OpenCodeHost, folder: &str, ct: &Cancel, resume: Option<&str>) -> (KiroResult, Seen) {
    let seen: Seen = Default::default();
    let s2 = seen.clone();
    let (h, folder, ct, resume) = (h.clone(), folder.to_owned(), ct.clone(), resume.map(str::to_owned));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { let _ = tx.send(h.run(&folder, "Say hello", None, &ct, resume.as_deref(), Some(Box::new(move |e| s2.lock().unwrap().push(e))), None)); });
    (rx.recv_timeout(Duration::from_secs(20)).expect("the run hung"), seen)
}

fn asking(f: impl Fn(AgentAsk) -> AskAnswer + Send + Sync + 'static) -> hover_agents::acp::Asking {
    Arc::new(move |_sid: &str, a: AgentAsk, _ct: &Cancel, reply: Box<dyn FnOnce(AskAnswer) + Send>| reply(f(a)))
}

#[test]
fn a_turn_streams_text_once_steps_and_the_session_id_and_sends_the_model_as_named() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| reply(f, sid, mid, "Hello world"));
    let (r, seen) = run(&host(&f, AgentOptions { model: Some("p/a/b".into()), effort: Some("high".into()), ..Default::default() }), &d, &Cancel::new(), None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Hello world"), "the delta and the whole part say it once");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.iter().find_map(|e| e.session_id.clone()).as_deref(), Some("ses_1"));
    let step = seen.iter().rev().find_map(|e| e.step.clone()).unwrap();
    assert_eq!((step.kind.as_str(), step.added), ("edit", 1));
    assert!((seen.iter().rev().find_map(|e| e.context).unwrap() - 50.0).abs() < 0.1, "500 of a 1000-token window");
    let st = f.st();
    let prompt = &st.prompts[0];
    let m = prompt.get("model").unwrap();
    assert_eq!((m.get("providerID").unwrap().as_str(), m.get("modelID").unwrap().as_str()), (Some("p"), Some("a/b")), "a model id with a slash is kept whole");
    assert_eq!(prompt.get("variant").unwrap().as_str(), Some("high"));
    assert!(prompt.get("messageID").unwrap().as_str().unwrap().starts_with("msg_"));
    assert!(st.directories.iter().all(|x| x == &d) && !st.directories.is_empty(), "every call names the folder");
    assert_eq!(st.unauthorized, 0);
}

#[test]
fn a_variant_the_model_hasnt_got_and_a_model_it_doesnt_offer_are_never_sent() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| reply(f, sid, mid, "Hello world"));
    let (r, _) = run(&host(&f, AgentOptions { model: Some("p/m".into()), effort: Some("high".into()), ..Default::default() }), &d, &Cancel::new(), None);
    assert_eq!(r.state, KiroState::Completed);
    assert!(f.st().prompts[0].get("variant").is_none());
    let (gone, _) = run(&host(&f, AgentOptions { model: Some("p/gone".into()), ..Default::default() }), &d, &Cancel::new(), None);
    assert_eq!(gone.state, KiroState::Failed);
    assert!(gone.text.contains("p/gone"));
    assert_eq!(f.st().prompts.len(), 1, "nothing sent with a model that isn't there");
}

#[test]
fn an_idle_from_before_the_prompt_doesnt_end_the_turn() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| {
        // Left over from an earlier turn: no user message or busy for this one yet.
        status(f, sid, "idle");
        status(f, "ses_other", "idle");
        let (f, sid, mid) = (f.clone(), sid.to_owned(), mid.to_owned());
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(300)); reply(&f, &sid, &mid, "Real answer"); });
    });
    let (r, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Real answer"));
}

fn bash_ask(f: &Fake, id: &str, sid: &str) {
    f.ev(format!(r#"{{"type":"permission.asked","properties":{{"id":"{id}","sessionID":"{sid}","permission":"bash","patterns":["git status"],"metadata":{{"command":"git status"}},"always":["git status *"]}}}}"#));
}

#[test]
fn a_denied_command_is_rejected_and_trust_answers_the_same_again_itself() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| { user(f, sid, mid); bash_ask(f, "per_1", sid); });
    f.on_reply(|f, _, id| {
        if id == "per_1" { bash_ask(f, "per_2", "ses_1"); }
        if id == "per_2" { let (s, m) = { let st = f.st(); (st.last_sid.clone().unwrap(), st.last_mid.clone().unwrap()) }; reply(f, &s, &m, "Hello world"); }
    });
    let h = host(&f, AgentOptions { approval: AgentApproval::Always, ..Default::default() });
    let asked = Arc::new(Mutex::new(0));
    let a2 = asked.clone();
    h.set_asking(asking(move |a| { *a2.lock().unwrap() += 1; assert_eq!(a.command.as_deref(), Some("git status")); AskAnswer::Trust }));
    let (r, _) = run(&h, &d, &Cancel::new(), None);
    assert_eq!(r.state, KiroState::Completed);
    assert_eq!(*asked.lock().unwrap(), 1, "the second is Hover's own yes");
    let replies: Vec<String> = f.st().replies.iter().filter(|x| x.0 == "permission").map(|x| x.2.as_ref().unwrap().get("reply").unwrap().as_str().unwrap().to_owned()).collect();
    assert_eq!(replies, ["once", "once"], "trust is Hover's; OpenCode's lasting always is never sent");

    f.st().replies.clear();
    f.on_reply(|f, _, _| { let (s, m) = { let st = f.st(); (st.last_sid.clone().unwrap(), st.last_mid.clone().unwrap()) }; reply(f, &s, &m, "Hello world"); });
    f.st().sessions.insert("ses_2".into());
    f.on_prompt(|f, sid, mid| { user(f, sid, mid); bash_ask(f, "per_3", sid); });
    let h2 = host(&f, AgentOptions { approval: AgentApproval::Always, ..Default::default() });
    h2.set_asking(asking(|_| AskAnswer::Deny));
    run(&h2, &d, &Cancel::new(), None);
    let st = f.st();
    assert_eq!(st.replies.len(), 1);
    assert_eq!(st.replies[0].2.as_ref().unwrap().get("reply").unwrap().as_str(), Some("reject"));
}

#[test]
fn read_only_is_the_servers_rule_and_the_agents_own_denies_come_last() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| {
        user(f, sid, mid);
        f.ev(format!(r#"{{"type":"permission.asked","properties":{{"id":"per_9","sessionID":"{sid}","permission":"edit","patterns":["x.txt"],"metadata":{{}},"always":[]}}}}"#));
    });
    f.on_reply(|f, _, _| { let (s, m) = { let st = f.st(); (st.last_sid.clone().unwrap(), st.last_mid.clone().unwrap()) }; reply(f, &s, &m, ""); });
    let h = host(&f, AgentOptions { read_only: true, ..Default::default() });
    h.set_asking(asking(|_| panic!("read only never asks")));
    let (r, _) = run(&h, &d, &Cancel::new(), None);
    let st = f.st();
    let rules: Vec<(String, String, String)> = st.created[0].get("permission").unwrap().items().unwrap().iter()
        .map(|x| (x.get("permission").unwrap().as_str().unwrap().into(), x.get("pattern").unwrap().as_str().unwrap().into(), x.get("action").unwrap().as_str().unwrap().into())).collect();
    let t = |a: &str, b: &str, c: &str| (a.to_owned(), b.to_owned(), c.to_owned());
    assert_eq!(rules[0], t("*", "*", "ask"), "unknown tools ask, and read only turns every ask down");
    assert!(rules.contains(&t("external_directory", "*", "ask")));
    assert!(rules.iter().filter(|x| matches!(x.0.as_str(), "bash" | "edit" | "task" | "*")).all(|x| x.2 != "allow"));
    assert_eq!(rules.last().unwrap(), &t("bash", "rm *", "deny"), "the agent's deny is the last word");
    assert_eq!(st.replies[0].2.as_ref().unwrap().get("reply").unwrap().as_str(), Some("reject"));
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("read only"), "{}", r.text);
}

fn indent_question(f: &Fake, id: &str, sid: &str) {
    f.ev(format!(r#"{{"type":"question.asked","properties":{{"id":"{id}","sessionID":"{sid}","questions":[{{"question":"Tabs or spaces?","header":"Indent","options":[{{"label":"Tabs","description":""}},{{"label":"Spaces","description":""}}]}}]}}}}"#));
}

#[test]
fn a_question_gets_the_users_labels_and_a_skipped_one_is_rejected() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| { user(f, sid, mid); indent_question(f, "que_1", sid); });
    f.on_reply(|f, _, _| { let (s, m) = { let st = f.st(); (st.last_sid.clone().unwrap(), st.last_mid.clone().unwrap()) }; reply(f, &s, &m, "Hello world"); });
    let h = host(&f, AgentOptions::default());
    let seen: Arc<Mutex<Option<AgentAsk>>> = Default::default();
    let s2 = seen.clone();
    h.set_questioning(Arc::new(move |_, a, _, reply| { *s2.lock().unwrap() = Some(a); reply(Some(vec![vec!["Tabs".into()]])) }));
    h.set_asking(asking(|_| panic!("a question isn't an approval")));
    let (r, _) = run(&h, &d, &Cancel::new(), None);
    assert_eq!(r.state, KiroState::Completed);
    let q = seen.lock().unwrap().clone().unwrap();
    assert_eq!(q.questions.unwrap()[0].options.iter().map(|o| o.0.as_str()).collect::<Vec<_>>(), ["Tabs", "Spaces"]);
    {
        let st = f.st();
        assert_eq!(st.replies.len(), 1);
        assert_eq!(st.replies[0].0, "question");
        assert_eq!(st.replies[0].2.as_ref().unwrap().get("answers").unwrap().compact(), r#"[["Tabs"]]"#);
    }
    f.st().replies.clear();
    f.st().sessions.insert("ses_2".into());
    let h2 = host(&f, AgentOptions::default());
    h2.set_questioning(Arc::new(|_, _, _, reply| reply(None)));
    run(&h2, &d, &Cancel::new(), None);
    let st = f.st();
    assert_eq!(st.replies.len(), 1);
    assert_eq!(st.replies[0].0, "reject");
}

#[test]
fn stop_while_a_question_waits_withdraws_it_and_aborts_only_that_session() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| {
        user(f, sid, mid);
        status(f, sid, "busy");
        f.ev(format!(r#"{{"type":"question.asked","properties":{{"id":"que_2","sessionID":"{sid}","questions":[{{"question":"?","header":"H","options":[{{"label":"A","description":""}}]}}]}}}}"#));
    });
    f.on_abort(|f, sid| {
        f.ev(format!(r#"{{"type":"session.error","properties":{{"sessionID":"{sid}","error":{{"name":"MessageAbortedError","data":{{"message":"aborted"}}}}}}}}"#));
        status(f, sid, "idle");
    });
    let ct = Cancel::new();
    let h = host(&f, AgentOptions::default());
    let c2 = ct.clone();
    // Held until withdrawn; the run is stopped 200 ms after it is asked.
    h.set_questioning(Arc::new(move |_, _, q: &Cancel, reply| {
        let c3 = c2.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(200)); c3.cancel(); });
        let reply = Mutex::new(Some(reply));
        std::mem::forget(q.on_cancel(move || { if let Some(r) = reply.lock().unwrap().take() { r(None) } }));
    }));
    let (r, _) = run(&h, &d, &ct, None);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.state, KiroState::Cancelled);
    let st = f.st();
    assert_eq!(st.aborts, ["ses_1"]);
    assert_eq!(st.replies.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["reject"], "the withdrawn question is told so");
}

#[test]
fn a_lost_prompt_answer_is_looked_up_not_sent_twice() {
    let (f, d) = (Fake::new(), dir());
    // The server takes the prompt and runs it, but its answer never comes back.
    f.st().prompt_hangs = true;
    f.on_prompt(|f, sid, mid| { f.st().messages.push(mid.into()); reply(f, sid, mid, "Did it"); });
    let (r, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Did it"));
    assert_eq!(f.st().prompts.len(), 1);
    // Not taken at all: a clear failure, still sent once.
    f.st().prompts.clear();
    f.on_prompt(|_, _, _| {});
    let (lost, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), None);
    assert_eq!(lost.state, KiroState::Failed);
    assert!(lost.text.contains("wasn’t sent again"), "{}", lost.text);
    assert_eq!(f.st().prompts.len(), 1);
}

#[test]
fn a_dropped_event_stream_reconnects_and_reads_the_finished_turn_back() {
    let (f, d) = (Fake::new(), dir());
    f.on_prompt(|f, sid, mid| {
        user(f, sid, mid);
        status(f, sid, "busy");
        // The stream drops; the turn finishes while nobody listens.
        let mut st = f.st();
        st.messages.push(mid.into());
        st.history = format!(r#"[{{"info":{{"id":"{mid}","role":"user","sessionID":"{sid}"}},"parts":[]}},{{"info":{{"id":"msg_zzz","parentID":"{mid}","role":"assistant","sessionID":"{sid}"}},"parts":[{{"id":"prt_1","messageID":"msg_zzz","sessionID":"{sid}","type":"text","text":"Finished while away"}}]}}]"#);
        st.status.insert(sid.into(), "idle".into());
        drop(st);
        f.drop_streams();
    });
    let (r, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), None);
    assert_eq!((r.state, r.text.as_str()), (KiroState::Completed, "Finished while away"));
    assert!(f.st().connects >= 2);
    assert_eq!(f.st().prompts.len(), 1);
}

#[test]
fn a_conversation_opencode_lost_fails_and_isnt_quietly_replaced() {
    let (f, d) = (Fake::new(), dir());
    let (r, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), Some("ses_gone"));
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("no longer has this conversation"));
    assert!(f.st().created.is_empty(), "no new conversation in its place");
    assert!(f.st().prompts.is_empty());
}

#[test]
fn a_reply_resumes_the_same_session_and_sets_its_rules_again() {
    let (f, d) = (Fake::new(), dir());
    f.st().sessions.insert("ses_old".into());
    f.on_prompt(|f, sid, mid| reply(f, sid, mid, "Hello world"));
    let (r, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), Some("ses_old"));
    assert_eq!(r.state, KiroState::Completed);
    let st = f.st();
    assert!(st.created.is_empty());
    assert_eq!(st.patched, ["ses_old"]);
    assert_eq!(st.prompt_sessions, ["ses_old"]);
}

#[test]
fn a_server_too_old_or_one_that_doesnt_start_is_a_readable_failure() {
    let (f, d) = (Fake::new(), dir());
    f.st().version = "1.2.0".into();
    let (old, _) = run(&host(&f, AgentOptions::default()), &d, &Cancel::new(), None);
    assert_eq!(old.state, KiroState::Failed);
    assert!(old.text.contains("too old"), "{}", old.text);
    let none = OpenCodeHost::with_connect(AgentOptions::default, || None, quick());
    let (r, _) = run(&none, &d, &Cancel::new(), None);
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("isn’t installed"));
}

/// OpenCode_never_falls_through_to_cursors_program_or_arguments.
#[test]
fn opencode_never_falls_through_to_cursors_program_or_arguments() {
    use hover_agents::agents;
    assert_eq!(agents::arguments(AgentTool::OpenCode), ["serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false"]);
    assert!(agents::install_hint(AgentTool::OpenCode).contains("OpenCode"));
    assert!(!agents::sign_in_hint(AgentTool::OpenCode).contains("cursor"));
    assert!(!agents::exe(AgentTool::OpenCode).is_some_and(|p| p.to_string_lossy().contains("cursor-agent")));
    assert_eq!(AgentTool::parse(Some("opencode")), Some(AgentTool::OpenCode));
    assert_eq!(AgentTool::parse(Some("cursor")), Some(AgentTool::Cursor), "old ids read as before");
}

fn q(id: &str) -> AgentAsk {
    AgentAsk { id: id.into(), kind: "question".into(), title: "Indent".into(), command: None, path: None, preview: None, added: 0, removed: 0,
        reason: "Tabs or spaces?".into(), danger: false,
        questions: Some(vec![AgentQuestion { header: "Indent".into(), question: "Tabs or spaces?".into(), options: vec![("Tabs".into(), String::new()), ("Spaces".into(), String::new())], multiple: false, custom: true }]) }
}

/// A_session_holds_a_question_until_its_answered_and_a_stop_skips_it.
#[test]
fn a_session_holds_a_question_until_its_answered_and_a_stop_skips_it() {
    let d = dir();
    let ks: Arc<Mutex<Option<KiroSessions>>> = Default::default();
    let (tx, rx) = std::sync::mpsc::channel::<std::sync::mpsc::Receiver<Option<Vec<Vec<String>>>>>();
    let tx = Mutex::new(tx);
    let k2 = ks.clone();
    let runner: RunTask = Arc::new(move |a: RunArgs| {
        (a.events)(KiroEvent { session_id: Some("ses_q".into()), ..Default::default() });
        let (atx, arx) = std::sync::mpsc::channel();
        let (qtx, qrx) = std::sync::mpsc::channel();
        let s = k2.lock().unwrap().clone().unwrap();
        s.ask_question(AgentTool::OpenCode, "ses_q", q("que_1"), &a.ct, Box::new(move |x| { let _ = atx.send(x.clone()); let _ = qtx.send(x); }));
        tx.lock().unwrap().send(qrx).unwrap();
        let got = arx.recv().unwrap();
        KiroResult::new(KiroState::Completed, got.map_or("skipped".into(), |g| g[0][0].clone()))
    });
    let sessions = KiroSessions::new(move |_| runner.clone(), None);
    *ks.lock().unwrap() = Some(sessions.clone());
    let s = sessions.start(AgentTool::OpenCode, &d, "go", vec![]).unwrap();
    let pending = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(sessions.get(s.id).unwrap().asking().unwrap().is_question());
    assert!(!sessions.answer_question(s.id, "que_1", vec![vec![]]), "an empty answer isn't one");
    assert!(!sessions.answer_question(s.id, "que_other", vec![vec!["Tabs".into()]]), "a stale id is turned down");
    assert!(sessions.answer_question(s.id, "que_1", vec![vec!["Tabs".into()]]));
    assert!(!sessions.answer_question(s.id, "que_1", vec![vec!["Tabs".into()]]), "answered once only");
    assert_eq!(pending.recv_timeout(Duration::from_secs(5)).unwrap().unwrap()[0][0], "Tabs");

    for _ in 0..100 { if !sessions.get(s.id).unwrap().busy() { break; } std::thread::sleep(Duration::from_millis(20)); }
    assert!(sessions.reply(s.id, "again", vec![]));
    let pending = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    sessions.stop(s.id);
    assert_eq!(pending.recv_timeout(Duration::from_secs(5)).unwrap(), None, "a stop skips the question, it doesn't answer it");
    assert!(!sessions.get(s.id).unwrap().waiting());
}

/// History_keeps_opencodes_session_and_old_tools_read_as_before.
#[test]
fn history_keeps_opencodes_session_and_old_tools_read_as_before() {
    use hover_core::history::{AgentHistory, SavedSession};
    let d = std::path::PathBuf::from(dir()).join("history");
    let c = Arc::new(hover_core::crypto::Crypto::with_key([7u8; 32]));
    let saved = |key: &str, tool, acp: &str| SavedSession { key: key.into(), tool, folder: "/x".into(), title: "Task".into(), acp_id: Some(acp.into()), context: None,
        turns: vec![], updated: hover_core::time::Stamp::now(), access: None };
    let h = AgentHistory::new(d.clone(), c.clone());
    h.save(&saved("abc123", AgentTool::OpenCode, "ses_keep"));
    h.save(&saved("def456", AgentTool::Cursor, "acp-1"));
    h.flush();
    let again = AgentHistory::new(d, c);
    let oc = again.load("abc123").unwrap();
    assert_eq!((oc.tool, oc.acp_id.as_deref()), (AgentTool::OpenCode, Some("ses_keep")), "the conversation it resumes is OpenCode's own id");
    assert_eq!(again.load("def456").unwrap().tool, AgentTool::Cursor);
    let mut tools: Vec<AgentTool> = again.entries().iter().map(|e| e.tool).collect();
    tools.sort_by_key(|t| *t as usize);
    assert_eq!(tools, [AgentTool::Cursor, AgentTool::OpenCode]);
}
