//! fake-anthropic: a stand-in Anthropic Messages API, so the real Claude Code can run
//! Hover's deterministic tasks with no account and no network. Started as
//! `fake-anthropic [PORT]` (0 or none: one the system picks), it listens on 127.0.0.1
//! only and prints "listening on http://127.0.0.1:PORT"; Claude Code is pointed at it with
//! ANTHROPIC_BASE_URL=http://127.0.0.1:PORT and any ANTHROPIC_API_KEY. It answers
//! POST /v1/messages (streamed or not) as the API does, and what a turn does is written
//! into its prompt:
//!
//!   [seconds:N]     the answer takes about N seconds, in deltas (default: at once)
//!   [write:NAME]    writes NAME in the working folder (the Write tool), then says how it went
//!   [run:CMD]       runs CMD (PowerShell on Windows, Bash elsewhere), then says how it went
//!   [question]      asks "Tabs or spaces?" (AskUserQuestion), then says what the answer was
//!   [think]         thinks out loud first (a thinking block)
//!   [fail]          the request fails (400 invalid_request_error)
//!
//! A request without tools (Claude Code's own titles and summaries) gets a short text.
//! FAKE_ANTHROPIC_LOG=FILE appends one line per request: whether it streamed, how many
//! tools it carried and the start of the prompt it answered.

use hover_core::json::{self, Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static IDS: AtomicU64 = AtomicU64::new(0);
static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn note(line: &str) {
    if let Some(f) = LOG.lock().unwrap().as_mut() { let _ = writeln!(f, "{line}"); }
}

fn st(s: &str) -> Json { Json::str(s) }
fn s<'a>(e: &'a Json, k: &str) -> Option<&'a str> { e.get(k).and_then(Json::as_str) }
fn arr(e: Option<&Json>) -> &[Json] { match e { Some(Json::Arr(a)) => a, _ => &[] } }

fn directive<'a>(prompt: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = prompt;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        let inner = &rest[i + 1..i + j];
        if inner == name { return Some(""); }
        if let Some(v) = inner.strip_prefix(name).and_then(|v| v.strip_prefix(':')) { return Some(v); }
        rest = &rest[i + j + 1..];
    }
    None
}

/// A message's text blocks (or its text, when content is a string).
fn texts(m: &Json) -> Vec<String> {
    match m.get("content") {
        Some(Json::Str(t)) => vec![t.clone()],
        Some(Json::Arr(parts)) => parts.iter().filter(|p| s(p, "type") == Some("text")).filter_map(|p| s(p, "text")).map(str::to_owned).collect(),
        _ => vec![],
    }
}

/// The user's own prompt: the newest user text that isn't one of Claude Code's reminders.
fn prompt_of(messages: &[Json]) -> String {
    for m in messages.iter().rev().filter(|m| s(m, "role") == Some("user")) {
        let own: Vec<String> = texts(m).into_iter().filter(|t| !t.trim_start().starts_with('<')).collect();
        if !own.is_empty() { return own.join("\n"); }
    }
    String::new()
}

/// The working folder, as Claude Code tells the model in its environment section
/// (in the system prompt or a system message, wherever this version puts it).
fn folder_of(req: &Json) -> String {
    fn find(v: &Json) -> Option<String> {
        const KEY: &str = "Primary working directory: ";
        match v {
            Json::Str(t) => t.find(KEY).map(|i| t[i + KEY.len()..].lines().next().unwrap_or("").trim().to_owned()),
            Json::Arr(a) => a.iter().find_map(find),
            Json::Obj(p) => p.iter().find_map(|(_, x)| find(x)),
            _ => None,
        }
    }
    find(req).unwrap_or_default()
}

/// What the model says or does next: blocks of text, thinking or a tool call.
enum Block { Think(String), Text(String), Tool(String, Json) }

fn plan(messages: &[Json], req: &Json) -> Vec<Block> {
    let prompt = prompt_of(messages);
    let last = messages.iter().rev().find(|m| s(m, "role") == Some("user"));
    // After a tool ran (or was refused): the tool call before it, and how it went.
    let result = last.and_then(|m| arr(m.get("content")).iter().find(|p| s(p, "type") == Some("tool_result")).cloned());
    if let Some(r) = result {
        let call = messages.iter().rev().filter(|m| s(m, "role") == Some("assistant"))
            .flat_map(|m| arr(m.get("content")).iter().filter(|p| s(p, "type") == Some("tool_use")).cloned().collect::<Vec<_>>()).next();
        let name = call.as_ref().and_then(|c| s(c, "name")).unwrap_or("tool").to_owned();
        let content = match r.get("content") {
            Some(Json::Str(t)) => t.clone(),
            Some(Json::Arr(parts)) => parts.iter().filter_map(|p| s(p, "text")).collect::<Vec<_>>().join(" "),
            _ => String::new(),
        };
        let refused = r.get("is_error") == Some(&Json::Bool(true));
        let first = content.lines().next().unwrap_or("").trim().to_owned();
        return vec![Block::Text(if refused { format!("Done. {name} was refused: {first}") } else if name == "AskUserQuestion" { format!("Done. answered: {first}") } else { format!("Done. {name} went through.") })];
    }
    let mut out = vec![];
    if directive(&prompt, "think").is_some() { out.push(Block::Think("Let me look at what is asked before I answer.".into())); }
    let folder = folder_of(req);
    let sep = if cfg!(windows) { "\\" } else { "/" };
    if let Some(name) = directive(&prompt, "write") {
        out.push(Block::Tool("Write".into(), Json::obj(vec![("file_path", st(&format!("{folder}{sep}{name}"))), ("content", st("written by fake-anthropic\n"))])));
    } else if let Some(cmd) = directive(&prompt, "run") {
        let tool = if cfg!(windows) { "PowerShell" } else { "Bash" };
        out.push(Block::Tool(tool.into(), Json::obj(vec![("command", st(cmd)), ("description", st(&format!("Run {cmd}")))])));
    } else if directive(&prompt, "question").is_some() {
        let q = Json::obj(vec![("question", st("Tabs or spaces?")), ("header", st("Indent")), ("multiSelect", Json::Bool(false)),
            ("options", Json::Arr(vec![Json::obj(vec![("label", st("Tabs")), ("description", st("One tab per level"))]),
                Json::obj(vec![("label", st("Spaces")), ("description", st("Four spaces per level"))])]))]);
        out.push(Block::Tool("AskUserQuestion".into(), Json::obj(vec![("questions", Json::Arr(vec![q]))])));
    } else {
        out.push(Block::Text("Done. Answered by fake-anthropic.".into()));
    }
    out
}

fn respond(mut w: &TcpStream, code: u16, kind: &str, body: &str) {
    let reason = match code { 200 => "OK", 400 => "Bad Request", 404 => "Not Found", _ => "Error" };
    let _ = write!(w, "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = w.flush();
}

fn messages(w: &TcpStream, body: &str) {
    let Ok(req @ Json::Obj(_)) = json::parse(body) else { return respond(w, 400, "application/json", r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad json"}}"#) };
    let msgs = arr(req.get("messages"));
    let tools = arr(req.get("tools")).len();
    let stream = req.get("stream") == Some(&Json::Bool(true));
    let model = s(&req, "model").unwrap_or("claude-fake").to_owned();
    let prompt = prompt_of(msgs);
    note(&format!("messages stream={stream} tools={tools} prompt={}", prompt.chars().take(80).collect::<String>().replace('\n', " ")));
    if tools > 0 && directive(&prompt, "fail").is_some() {
        return respond(w, 400, "application/json", r#"{"type":"error","error":{"type":"invalid_request_error","message":"fake-anthropic: this turn fails"}}"#);
    }
    let blocks = if tools == 0 { vec![Block::Text("Fake task".into())] } else { plan(msgs, &req) };
    let secs: f64 = directive(&prompt, "seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let id = format!("msg_fake{:04}", IDS.fetch_add(1, Ordering::SeqCst) + 1);
    let stop = if blocks.iter().any(|b| matches!(b, Block::Tool(..))) { "tool_use" } else { "end_turn" };
    let usage = Json::obj(vec![("input_tokens", Json::int(20000)), ("output_tokens", Json::int(50)), ("cache_creation_input_tokens", Json::int(0)), ("cache_read_input_tokens", Json::int(0))]);
    let content: Vec<Json> = blocks.iter().map(|b| match b {
        Block::Think(t) => Json::obj(vec![("type", st("thinking")), ("thinking", st(t)), ("signature", st("fake"))]),
        Block::Text(t) => Json::obj(vec![("type", st("text")), ("text", st(t))]),
        Block::Tool(name, input) => Json::obj(vec![("type", st("tool_use")), ("id", st(&format!("toolu_{id}"))), ("name", st(name)), ("input", input.clone())]),
    }).collect();
    if !stream {
        let m = Json::obj(vec![("id", st(&id)), ("type", st("message")), ("role", st("assistant")), ("model", st(&model)), ("content", Json::Arr(content)),
            ("stop_reason", st(stop)), ("stop_sequence", Json::Null), ("usage", usage)]);
        return respond(w, 200, "application/json", &m.compact());
    }
    let mut w = w;
    if write!(w, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").is_err() { return; }
    let ev = |w: &mut &TcpStream, kind: &str, data: Json| -> bool { write!(w, "event: {kind}\ndata: {}\n\n", data.compact()).and_then(|_| w.flush()).is_ok() };
    let start = Json::obj(vec![("type", st("message_start")), ("message", Json::obj(vec![("id", st(&id)), ("type", st("message")), ("role", st("assistant")),
        ("model", st(&model)), ("content", Json::Arr(vec![])), ("stop_reason", Json::Null), ("stop_sequence", Json::Null), ("usage", usage)]))]);
    if !ev(&mut w, "message_start", start) { return; }
    for (i, b) in blocks.iter().enumerate() {
        let at = ("index", Json::int(i as i64));
        let (first, deltas): (Json, Vec<Json>) = match b {
            Block::Think(t) => (Json::obj(vec![("type", st("thinking")), ("thinking", st(""))]),
                vec![Json::obj(vec![("type", st("thinking_delta")), ("thinking", st(t))]), Json::obj(vec![("type", st("signature_delta")), ("signature", st("fake"))])]),
            Block::Text(t) => {
                // Said in pieces over the turn's length, so a stop can land halfway.
                let words: Vec<&str> = t.split_inclusive(' ').collect();
                let n = if secs > 0.0 { (secs * 5.0).ceil() as usize } else { 1 };
                let mut parts = vec![];
                for k in 0..n {
                    let piece: String = words.iter().skip(k * words.len() / n).take((k + 1) * words.len() / n - k * words.len() / n).copied().collect();
                    parts.push(Json::obj(vec![("type", st("text_delta")), ("text", st(&piece))]));
                }
                (Json::obj(vec![("type", st("text")), ("text", st(""))]), parts)
            }
            Block::Tool(name, input) => (Json::obj(vec![("type", st("tool_use")), ("id", st(&format!("toolu_{id}"))), ("name", st(name)), ("input", Json::obj(vec![]))]),
                vec![Json::obj(vec![("type", st("input_json_delta")), ("partial_json", st(&input.compact()))])]),
        };
        if !ev(&mut w, "content_block_start", Json::obj(vec![("type", st("content_block_start")), at.clone(), ("content_block", first)])) { return; }
        let pause = if secs > 0.0 && matches!(b, Block::Text(_)) { Duration::from_secs_f64(secs / deltas.len().max(1) as f64) } else { Duration::ZERO };
        for d in deltas {
            if !pause.is_zero() { std::thread::sleep(pause); }
            if !ev(&mut w, "content_block_delta", Json::obj(vec![("type", st("content_block_delta")), at.clone(), ("delta", d)])) { return; }
        }
        if !ev(&mut w, "content_block_stop", Json::obj(vec![("type", st("content_block_stop")), at])) { return; }
    }
    ev(&mut w, "message_delta", Json::obj(vec![("type", st("message_delta")), ("delta", Json::obj(vec![("stop_reason", st(stop)), ("stop_sequence", Json::Null)])),
        ("usage", Json::obj(vec![("output_tokens", Json::int(50))]))]));
    ev(&mut w, "message_stop", Json::obj(vec![("type", st("message_stop"))]));
}

fn answer(c: TcpStream) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(30)));
    let mut r = BufReader::new(match c.try_clone() { Ok(x) => x, Err(_) => return });
    let mut line = String::new();
    if r.read_line(&mut line).is_err() { return; }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).is_err() || h.trim().is_empty() { break; }
        if let Some((k, v)) = h.split_once(':') { if k.trim().eq_ignore_ascii_case("content-length") { length = v.trim().parse().unwrap_or(0); } }
    }
    let mut body = vec![0u8; length];
    if r.read_exact(&mut body).is_err() { return; }
    let body = String::from_utf8_lossy(&body).into_owned();
    let route = path.split('?').next().unwrap_or("");
    match (method.as_str(), route) {
        ("POST", "/v1/messages") => messages(&c, &body),
        ("POST", "/v1/messages/count_tokens") => respond(&c, 200, "application/json", r#"{"input_tokens":20000}"#),
        ("HEAD" | "GET", "/api/hello") => respond(&c, 200, "application/json", "{}"),
        _ => { note(&format!("unknown {method} {route}")); respond(&c, 404, "application/json", r#"{"type":"error","error":{"type":"not_found_error","message":"not here"}}"#) }
    }
}

fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "0".into());
    if let Some(p) = std::env::var_os("FAKE_ANTHROPIC_LOG") { *LOG.lock().unwrap() = std::fs::OpenOptions::new().create(true).append(true).open(p).ok(); }
    let l = TcpListener::bind(format!("127.0.0.1:{port}")).expect("the port");
    println!("listening on http://{}", l.local_addr().unwrap());
    let _ = std::io::stdout().flush();
    for c in l.incoming().flatten() { std::thread::spawn(move || answer(c)); }
}
