//! BrowserAndGitHubTests' browser half (tests/Hover.Tests/BrowserAndGitHubTests.cs),
//! ported: Hover's MCP server for agents answers initialize, tools/list and tools/call and
//! passes each call to the host's browser, here a fake. The MCP logic is OS-free and runs
//! on every OS; the socket and the perl relay are Unix's.

use hover_agents::browser::{self, Host, Limits, Reply};
use hover_core::json::{self, Json};
use hover_core::model::AgentTool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The host is the process's: one test at a time.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Fake { calls: Mutex<Vec<(String, String, Json)>>, closed: Mutex<Vec<String>>, slow: Mutex<Option<Duration>>, reply: Mutex<Option<Reply>> }

/// The host handed in: the orphan rule wants a type of this crate.
struct Shared(Arc<Fake>);

impl Host for Shared {
    fn has_session(&self, session: &str) -> bool { !self.0.closed.lock().unwrap().iter().any(|c| c == session) }

    fn call(&self, session: &str, op: &str, args: &Json) -> Reply {
        self.0.calls.lock().unwrap().push((session.into(), op.into(), args.clone()));
        let slow = *self.0.slow.lock().unwrap();
        if let Some(d) = slow { std::thread::sleep(d); }
        self.0.reply.lock().unwrap().clone().unwrap_or_else(|| Reply { ok: true, text: "Opened Demo — http://localhost:5173/".into(), image: Some("AAAA".into()), mime: None })
    }
}

struct With { _g: MutexGuard<'static, ()>, fake: Arc<Fake> }

impl With {
    fn new() -> With {
        let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fake = Arc::new(Fake::default());
        browser::set_host(Box::new(Shared(fake.clone())));
        With { _g: g, fake }
    }
}

impl Drop for With {
    fn drop(&mut self) { browser::clear_host(); }
}

fn m(s: &str) -> Json { json::parse(s).unwrap() }

fn call(tag: &str, name: &str, args: &str) -> Json {
    let msg = m(&format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#));
    browser::answer(tag, &msg).unwrap()
}

fn result_of(reply: &Json) -> &Json { reply.get("result").unwrap() }

fn text_of(reply: &Json) -> String { result_of(reply).get("content").unwrap().items().unwrap()[0].get("text").and_then(Json::as_str).unwrap().to_owned() }

#[test]
fn initialize_says_who_it_is_and_how_to_use_it() {
    let r = browser::answer("key-1", &m(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#)).unwrap();
    let res = result_of(&r);
    assert_eq!(r.get("id"), Some(&Json::int(1)));
    assert_eq!(res.get("serverInfo").unwrap().get("name").and_then(Json::as_str), Some("hover-browser"));
    assert_eq!(res.get("protocolVersion").and_then(Json::as_str), Some("2025-06-18"));
    assert!(res.get("instructions").and_then(Json::as_str).unwrap().contains("browser_snapshot"));
    // The client's version is echoed; none asked for gets the default.
    let r = browser::answer("k", &m(r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#)).unwrap();
    assert_eq!((result_of(&r).get("protocolVersion").and_then(Json::as_str), r.get("id").and_then(Json::as_str)), (Some("2024-11-05"), Some("a")));
    let r = browser::answer("k", &m(r#"{"jsonrpc":"2.0","id":2,"method":"initialize"}"#)).unwrap();
    assert_eq!(result_of(&r).get("protocolVersion").and_then(Json::as_str), Some("2025-06-18"));
}

#[test]
fn notifications_are_not_answered_and_the_rest_is_plain_json_rpc() {
    assert_eq!(browser::answer("k", &m(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)), None);
    assert_eq!(browser::answer("k", &m("[1]")), None);
    let pong = browser::answer("k", &m(r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#)).unwrap();
    assert_eq!(result_of(&pong), &m("{}"));
    let no = browser::answer("k", &m(r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#)).unwrap();
    assert_eq!((no.get("error").unwrap().get("code"), no.get("error").unwrap().get("message").and_then(Json::as_str)),
        (Some(&Json::int(-32601)), Some("Method resources/list isn't supported.")));
}

#[test]
fn the_tool_list_has_every_browser_tool_with_its_schema() {
    let list = browser::answer("k", &m(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)).unwrap();
    let tools = result_of(&list).get("tools").unwrap().items().unwrap().to_vec();
    let names: Vec<&str> = tools.iter().map(|t| t.get("name").and_then(Json::as_str).unwrap()).collect();
    assert_eq!(names, ["browser_open", "browser_snapshot", "browser_click", "browser_type", "browser_press", "browser_scroll",
        "browser_screenshot", "browser_evaluate", "browser_wait", "browser_console", "browser_back", "browser_reload"]);
    let click = tools[2].get("inputSchema").unwrap();
    assert!(click.get("properties").unwrap().get("ref").is_some(), "the ref property is called ref");
    assert_eq!(click.get("additionalProperties"), Some(&Json::Bool(false)));
    let typed = tools[3].get("inputSchema").unwrap().get("required").unwrap().items().unwrap();
    assert_eq!(typed, &[Json::str("text")]);
    assert_eq!(tools[6].get("inputSchema").unwrap().get("properties"), Some(&m("{}")));
    assert_eq!(tools[5].get("inputSchema").unwrap().get("properties").unwrap().get("to").unwrap().get("enum"), Some(&m(r#"["top","bottom"]"#)));
}

#[test]
fn a_call_goes_to_the_host_for_the_session_the_token_names() {
    let w = With::new();
    let r = call("key-1", "browser_open", r#"{"url":"localhost:5173"}"#);
    let res = result_of(&r);
    let content = res.get("content").unwrap().items().unwrap();
    assert_eq!(res.get("isError"), Some(&Json::Bool(false)));
    assert!(text_of(&r).contains("Opened Demo"));
    assert_eq!((content[1].get("type").and_then(Json::as_str), content[1].get("data").and_then(Json::as_str), content[1].get("mimeType").and_then(Json::as_str)),
        (Some("image"), Some("AAAA"), Some("image/jpeg")));
    let calls = w.fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].0.as_str(), calls[0].1.as_str()), ("key-1", "open"));
    assert_eq!(calls[0].2.get("url").and_then(Json::as_str), Some("localhost:5173"));
}

#[test]
fn an_unknown_tool_is_an_error_and_never_reaches_the_host() {
    let w = With::new();
    let r = call("key-1", "rm_rf", "{}");
    assert_eq!(r.get("error").unwrap().get("code"), Some(&Json::int(-32602)));
    assert_eq!(r.get("error").unwrap().get("message").and_then(Json::as_str), Some("Unknown tool rm_rf."));
    assert!(w.fake.calls.lock().unwrap().is_empty());
}

#[test]
fn arguments_that_are_not_an_object_are_an_empty_one() {
    let w = With::new();
    let msg = m(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"browser_back","arguments":[1]}}"#);
    browser::answer("k", &msg).unwrap();
    assert_eq!(w.fake.calls.lock().unwrap()[0].2, m("{}"));
}

#[test]
fn what_the_host_could_not_do_is_an_error_result() {
    let w = With::new();
    *w.fake.reply.lock().unwrap() = Some(Reply { ok: false, text: String::new(), image: None, mime: None });
    let r = call("k", "browser_click", r#"{"ref":3}"#);
    assert_eq!((result_of(&r).get("isError"), text_of(&r).as_str()), (Some(&Json::Bool(true)), "That didn’t work."));
    *w.fake.reply.lock().unwrap() = Some(Reply { ok: true, text: String::new(), image: None, mime: None });
    assert_eq!(text_of(&call("k", "browser_back", "{}")), "Done.");
    // A screenshot alone is just the image.
    *w.fake.reply.lock().unwrap() = Some(Reply { ok: true, text: String::new(), image: Some("QUJD".into()), mime: Some("image/png".into()) });
    let shot = call("k", "browser_screenshot", "{}");
    let content = result_of(&shot).get("content").unwrap().items().unwrap();
    assert_eq!((content.len(), content[0].get("type").and_then(Json::as_str), content[0].get("mimeType").and_then(Json::as_str)), (1, Some("image"), Some("image/png")));
}

#[test]
fn a_session_without_a_browser_and_a_host_without_one_are_said() {
    let w = With::new();
    w.fake.closed.lock().unwrap().push("gone".into());
    assert_eq!(text_of(&call("gone", "browser_snapshot", "{}")), "Hover's browser isn't open for this session right now.");
    assert_eq!(result_of(&call("gone", "browser_snapshot", "{}")).get("isError"), Some(&Json::Bool(true)));
    assert!(w.fake.calls.lock().unwrap().is_empty());
    browser::clear_host();
    assert_eq!(text_of(&call("k", "browser_snapshot", "{}")), "Hover's browser isn't available here.");
}

#[test]
fn a_slow_page_times_out_and_waiting_has_its_own_limit() {
    let w = With::new();
    *w.fake.slow.lock().unwrap() = Some(Duration::from_millis(400));
    let msg = |name: &str| m(&format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"{name}","arguments":{{}}}}}}"#));
    // The call limit is short, the wait limit long: a wait outlasts what a snapshot can't.
    let limits = Limits { call: Duration::from_millis(80), wait: Duration::from_secs(5) };
    let slow = browser::answer_within("k", &msg("browser_snapshot"), &limits).unwrap();
    assert_eq!((text_of(&slow).as_str(), result_of(&slow).get("isError")), ("The browser didn’t answer in time.", Some(&Json::Bool(true))));
    let waited = browser::answer_within("k", &msg("browser_wait"), &limits).unwrap();
    assert_eq!(result_of(&waited).get("isError"), Some(&Json::Bool(false)));
}

#[test]
fn calls_run_side_by_side() {
    let w = With::new();
    *w.fake.slow.lock().unwrap() = Some(Duration::from_millis(300));
    let t = std::time::Instant::now();
    let hs: Vec<_> = (0..4).map(|i| std::thread::spawn(move || call(&format!("k{i}"), "browser_snapshot", "{}"))).collect();
    for h in hs { assert_eq!(result_of(&h.join().unwrap()).get("isError"), Some(&Json::Bool(false))); }
    assert!(t.elapsed() < Duration::from_millis(1100), "not one after another");
    assert_eq!(w.fake.calls.lock().unwrap().len(), 4);
}

#[test]
fn browser_steps_are_named_whatever_the_agent_calls_them() {
    assert_eq!(browser::op_of(Some("hover-browser/browser_open")), Some("open"));
    assert_eq!(browser::op_of(Some("mcp__hover-browser__browser_click")), Some("click"));
    assert_eq!(browser::op_of(Some("browser_screenshot")), Some("screenshot"));
    assert_eq!(browser::op_of(Some("Ran npm test")), None);
    assert_eq!(browser::op_of(Some("browser_prepare")), None);
}

#[test]
fn only_a_mac_hands_the_browser_out() {
    let _w = With::new();
    assert_eq!(browser::supported(), cfg!(target_os = "macos"));
    assert_eq!(browser::note(), if cfg!(target_os = "macos") { None } else { Some("Agent browser needs macOS.") });
    assert_eq!(browser::UNSUPPORTED, "Agent browser needs macOS.");
    assert_eq!(browser::available(), cfg!(target_os = "macos"), "a host is set here");
    // Without a tag there is no server to name; where there is no browser there is none at all.
    assert!(browser::servers(AgentTool::Codex, None).is_empty());
    assert!(browser::servers(AgentTool::Codex, Some("")).is_empty());
    if !cfg!(target_os = "macos") { assert!(browser::servers(AgentTool::Codex, Some("key-1")).is_empty()); }
    browser::clear_host();
    assert!(!browser::available() && browser::servers(AgentTool::Codex, Some("key-1")).is_empty(), "no host browser, no server");
}

#[test]
fn a_session_keeps_its_token() {
    let a = browser::register("key-a");
    assert_eq!(a.len(), 32);
    assert_eq!(a, browser::register("key-a"));
    assert_ne!(a, browser::register("key-b"));
    assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[cfg(unix)]
mod socket {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};

    /// Listening at a path of the test's own (Unix sockets take short ones).
    fn listening(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::path::PathBuf::from("/tmp").join(format!("hb-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("b.sock");
        std::env::set_var("HOVER_BROWSER_SOCKET", &sock);
        let relay = browser::listen().unwrap();
        (sock, relay)
    }

    fn cleanup(dir: &std::path::Path) {
        browser::stop();
        std::env::remove_var("HOVER_BROWSER_SOCKET");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_socket_is_the_users_only_and_checks_the_token() {
        let w = With::new();
        let (sock, relay) = listening("tok");
        let token = browser::register("key-1");
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&relay).unwrap(), browser::RELAY);

        // A wrong token gets nothing: the connection is closed.
        let mut bad = UnixStream::connect(&sock).unwrap();
        writeln!(bad, "HELLO nope").unwrap();
        let mut answer = String::new();
        assert_eq!(BufReader::new(bad.try_clone().unwrap()).read_line(&mut answer).unwrap(), 0);

        // The right one speaks MCP, and its calls reach the host under its tag.
        let mut good = UnixStream::connect(&sock).unwrap();
        writeln!(good, "HELLO {token}").unwrap();
        writeln!(good, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).unwrap();
        writeln!(good, "not json").unwrap();
        writeln!(good, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
        writeln!(good, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"browser_open","arguments":{{"url":"localhost:5173"}}}}}}"#).unwrap();
        let mut r = BufReader::new(good.try_clone().unwrap());
        let mut lines = vec![];
        for _ in 0..2 {
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            lines.push(json::parse(&l).unwrap());
        }
        lines.sort_by_key(|l| l.get("id").and_then(|i| i.i64().ok()));
        assert_eq!(lines[0].get("result").unwrap().get("serverInfo").unwrap().get("name").and_then(Json::as_str), Some("hover-browser"));
        assert!(text_of(&lines[1]).contains("Opened Demo"));
        assert_eq!(w.fake.calls.lock().unwrap()[0].0, "key-1");
        cleanup(sock.parent().unwrap());
    }

    #[test]
    fn the_agents_relay_joins_its_stdio_to_the_socket() {
        if !std::path::Path::new("/usr/bin/perl").is_file() { eprintln!("needs perl"); return; }
        let w = With::new();
        let (sock, relay) = listening("relay");
        let token = browser::register("key-2");
        let mut p = Command::new("/usr/bin/perl").arg(&relay).arg(&sock).env("HOVER_BROWSER_TOKEN", &token)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut stdin = p.stdin.take().unwrap();
        let mut out = BufReader::new(p.stdout.take().unwrap());
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#).unwrap();
        let mut l = String::new();
        out.read_line(&mut l).unwrap();
        let names = json::parse(&l).unwrap().get("result").unwrap().get("tools").unwrap().items().unwrap().len();
        assert_eq!(names, 12);
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"browser_screenshot","arguments":{{}}}}}}"#).unwrap();
        l.clear();
        out.read_line(&mut l).unwrap();
        assert!(l.contains("\"type\":\"image\""));
        assert_eq!(w.fake.calls.lock().unwrap()[0].0, "key-2");
        // Its stdin closing ends it.
        drop(stdin);
        assert!(p.wait().unwrap().success());
        cleanup(sock.parent().unwrap());
    }
}
