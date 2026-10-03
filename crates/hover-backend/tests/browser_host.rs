//! The agent's browser calls, through hover-agents' MCP answering and this backend's host:
//! `{type:"browser", call, id, op, args}` out, `{type:"browserResult", call, ok, text,
//! image, mime}` back (BrowserTool.ToHost and Complete). One test, since the host is
//! the process's.

use hover_agents::browser;
use hover_agents::session::{KiroSessions, RunArgs};
use hover_agents::stream::KiroResult;
use hover_backend::browser_host::{BrowserHost, Handle};
use hover_backend::wire::Out;
use hover_core::json::{self, Json};
use hover_core::model::{AgentTool, KiroState};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

impl Sink {
    fn lines(&self) -> Vec<Json> {
        String::from_utf8_lossy(&self.0.lock().unwrap()).lines().map(|l| json::parse(l).unwrap()).collect()
    }

    /// The next `browser` message that is not one of `seen`.
    fn next_call(&self, seen: usize) -> Json {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let all = self.lines();
            if let Some(m) = all.into_iter().filter(|m| m.get("type").and_then(Json::as_str) == Some("browser")).nth(seen) { return m; }
            assert!(Instant::now() < end, "the host was never asked");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn call(name: &str, args: Json) -> Json {
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", Json::int(1)), ("method", Json::str("tools/call")),
        ("params", Json::obj(vec![("name", Json::str(name)), ("arguments", args)]))])
}

fn texts(answer: &Json) -> Vec<String> {
    let content = answer.get("result").unwrap().get("content").unwrap().items().unwrap();
    content.iter().map(|c| format!("{}:{}", c.get("type").unwrap().as_str().unwrap(), c.get("text").or(c.get("data")).unwrap().as_str().unwrap())).collect()
}

#[test]
fn an_agents_browser_call_goes_to_the_host_and_back() {
    let sink = Sink::default();
    let out = Arc::new(Out::new(sink.clone()));
    let folder = std::env::temp_dir();
    // Sessions that work until stopped.
    let sessions = KiroSessions::new(|_| Arc::new(|a: RunArgs| {
        while !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); }
        KiroResult::new(KiroState::Cancelled, "")
    }), None);
    let s = sessions.start_as(AgentTool::Codex, &folder.to_string_lossy(), "Look at the page", vec![], None).unwrap();
    let oc = sessions.start_as(AgentTool::OpenCode, &folder.to_string_lossy(), "And this one", vec![], None).unwrap();
    let host = BrowserHost::new(out, sessions.clone());
    browser::set_host(Box::new(Handle(host.clone())));

    // An open call: the host is told the office's session id and the arguments as they came.
    let key = s.key.clone();
    let asking = std::thread::spawn(move || browser::answer(&key, &call("browser_open", Json::obj(vec![("url", Json::str("http://localhost:3000"))]))).unwrap());
    let m = sink.next_call(0);
    assert_eq!(m.compact(), format!(r#"{{"type":"browser","call":1,"id":{},"op":"open","args":{{"url":"http://localhost:3000"}}}}"#, s.id));
    host.complete(&json::parse(r#"{"type":"browserResult","call":1,"ok":true,"text":"Opened http://localhost:3000/ (200)"}"#).unwrap());
    let answer = asking.join().unwrap();
    assert_eq!(texts(&answer), ["text:Opened http://localhost:3000/ (200)"]);
    assert_eq!(answer.get("result").unwrap().get("isError"), Some(&Json::Bool(false)));

    // A screenshot comes back as an image; a failure as an error.
    let key = s.key.clone();
    let shot = std::thread::spawn(move || browser::answer(&key, &call("browser_screenshot", Json::obj(vec![]))).unwrap());
    let m = sink.next_call(1);
    assert_eq!((m.get("call").unwrap().compact(), m.get("op").unwrap().as_str()), ("2".to_owned(), Some("screenshot")));
    host.complete(&json::parse(r#"{"type":"browserResult","call":2,"ok":true,"text":"","image":"AAAA","mime":"image/png"}"#).unwrap());
    let answer = shot.join().unwrap();
    assert_eq!(texts(&answer), ["image:AAAA"]);
    assert_eq!(answer.get("result").unwrap().get("content").unwrap().items().unwrap()[0].get("mimeType").unwrap().as_str(), Some("image/png"));

    let key = s.key.clone();
    let failed = std::thread::spawn(move || browser::answer(&key, &call("browser_click", Json::obj(vec![("ref", Json::int(3))]))).unwrap());
    sink.next_call(2);
    host.complete(&json::parse(r#"{"type":"browserResult","call":3,"ok":false,"text":"No element [3]."}"#).unwrap());
    let answer = failed.join().unwrap();
    assert_eq!(texts(&answer), ["text:No element [3]."]);
    assert_eq!(answer.get("result").unwrap().get("isError"), Some(&Json::Bool(true)));

    // OpenCode has one server for all its sessions: its tag names the one at work.
    let asking = std::thread::spawn(move || browser::answer("opencode", &call("browser_back", Json::obj(vec![]))).unwrap());
    let m = sink.next_call(3);
    assert_eq!(m.get("id").unwrap().compact(), oc.id.to_string());
    host.complete(&json::parse(r#"{"type":"browserResult","call":4,"ok":true,"text":"Went back."}"#).unwrap());
    assert_eq!(texts(&asking.join().unwrap()), ["text:Went back."]);

    // No session by that tag: the agent is told, and the host is not asked.
    let answer = browser::answer("no-such-session", &call("browser_reload", Json::obj(vec![]))).unwrap();
    assert_eq!(texts(&answer), ["text:Hover's browser isn't open for this session right now."]);
    assert_eq!(sink.lines().iter().filter(|m| m.get("type").and_then(Json::as_str) == Some("browser")).count(), 4);

    // An answer for a call nobody waits on is ignored; a host going away fails what waits.
    host.complete(&json::parse(r#"{"type":"browserResult","call":99,"ok":true}"#).unwrap());
    let key = s.key.clone();
    let closing = std::thread::spawn(move || browser::answer(&key, &call("browser_console", Json::obj(vec![]))).unwrap());
    sink.next_call(4);
    host.stop();
    assert_eq!(texts(&closing.join().unwrap()), ["text:Hover is closing."]);

    browser::clear_host();
    let answer = browser::answer(&s.key, &call("browser_open", Json::obj(vec![("url", Json::str("http://x"))]))).unwrap();
    assert_eq!(texts(&answer), ["text:Hover's browser isn't available here."]);
    sessions.stop_all();
}
