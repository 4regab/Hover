//! fake-opencode: a stand-in "opencode serve" for Hover's deterministic runs, as
//! fake-agent is for the ACP tools. Copied as opencode(.exe) on a PATH of its own, it
//! answers `--version`, and `serve` listens on the address it is given (only
//! 127.0.0.1 is accepted, as Hover asks), prints "listening on http://127.0.0.1:PORT",
//! wants the password from OPENCODE_SERVER_PASSWORD as Basic auth on every request, and
//! serves the routes Hover uses with the event stream the real server sends (the same
//! shapes tests/opencode_host.rs checks the host against). What a turn does is written
//! into its prompt:
//!
//!   [seconds:N]   the turn's length (default 1)
//!   [bytes:N]     an answer of about N bytes (the rich fixture over and over), in deltas
//!   [ask:bash:CMD] or [ask:edit:FILE]   asks permission first (permission.asked); an
//!                 allowed edit writes FILE in the session's folder
//!   [question]    asks a question with two choices and room for one's own words
//!   [drop]        closes every event stream halfway (Hover reconnects and reads back)
//!   [lose]        takes the prompt but never answers the request (a lost response)
//!   [fail]        the turn ends in a session.error
//!
//! FAKE_OPENCODE_LOG=FILE appends one line per request (method, path, the folder it
//! named, and each prompt's message id), for a run's checks: a prompt sent twice shows
//! as its message id twice.

use hover_core::json::{self, Json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RICH: &str = "## What changed\n\nI moved the **token check** into `RefreshService`.\n\n| File | Lines |\n|---|---|\n| src/auth.rs | 42 |\n| src/refresh.rs | 17 |\n\n```rust\nfn refresh(t: &Token) -> Result<(), Error> {\n    if t.expired() { return Err(Error::Expired); }\n    Ok(())\n}\n```\n\n```mermaid\nflowchart LR\n  A[Request] --> B{Token ok?}\n  B -->|yes| C[Serve]\n  B -->|no| D[401]\n```\n\n";

const PROVIDERS: &str = r#"{"providers":[{"id":"fake","name":"Fake","models":{"fast":{"id":"fast","name":"Fast","variants":{"low":{},"high":{}},"limit":{"context":200000}},"slow":{"id":"slow","name":"Slow","limit":{"context":100000}}}}],"default":{"fake":"fast"}}"#;
const AGENTS: &str = r#"[{"name":"build","mode":"primary","permission":[{"permission":"*","pattern":"*","action":"allow"}]},{"name":"plan","mode":"primary","permission":[{"permission":"edit","pattern":"*","action":"deny"}]}]"#;

fn directive<'a>(prompt: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = vec![];
    let mut rest = prompt;
    while let Some(i) = rest.find('[') {
        let Some(j) = rest[i..].find(']') else { break };
        let inner = &rest[i + 1..i + j];
        if inner == name { out.push(""); } else if let Some(v) = inner.strip_prefix(name).and_then(|v| v.strip_prefix(':')) { out.push(v); }
        rest = &rest[i + j + 1..];
    }
    out
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() { if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; } }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn esc(s: &str) -> String { Json::str(s).compact() }

#[derive(Default)]
struct Session { folder: String, busy: bool, messages: Vec<String>, abort: Arc<AtomicBool> }

struct Server {
    auth: String,
    ids: AtomicU64,
    sessions: Mutex<HashMap<String, Session>>,
    streams: Mutex<Vec<TcpStream>>,
    /// Permission and question replies waited on, by their id.
    pending: Mutex<HashMap<String, Sender<(String, Json)>>>,
    log: Option<Mutex<std::fs::File>>,
}

impl Server {
    fn note(&self, line: &str) {
        if let Some(f) = &self.log { let _ = writeln!(f.lock().unwrap(), "{line}"); }
    }

    fn ev(&self, e: String) {
        let line = format!("data: {e}\n\n");
        let chunk = format!("{:x}\r\n{line}\r\n", line.len());
        self.streams.lock().unwrap().retain_mut(|w| w.write_all(chunk.as_bytes()).and_then(|_| w.flush()).is_ok());
    }

    fn status(&self, sid: &str, t: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get_mut(sid) { s.busy = t != "idle"; }
        self.ev(format!(r#"{{"type":"session.status","properties":{{"sessionID":"{sid}","status":{{"type":"{t}"}}}}}}"#));
    }

    fn next(&self, p: &str) -> String { format!("{p}_{:06}", self.ids.fetch_add(1, Ordering::SeqCst) + 1) }

    /// Waits for the reply to a permission or question, or the session's stop.
    fn wait(&self, id: &str, abort: &AtomicBool) -> Option<(String, Json)> {
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id.to_owned(), tx);
        loop {
            if let Ok(r) = rx.recv_timeout(Duration::from_millis(100)) { return Some(r); }
            if abort.load(Ordering::SeqCst) { self.pending.lock().unwrap().remove(id); return None; }
        }
    }

    fn turn(self: Arc<Self>, sid: String, mid: String, text: String) {
        let (folder, abort) = {
            let mut g = self.sessions.lock().unwrap();
            let s = g.entry(sid.clone()).or_default();
            s.messages.push(mid.clone());
            s.abort.store(false, Ordering::SeqCst);
            (s.folder.clone(), s.abort.clone())
        };
        let secs: f64 = directive(&text, "seconds").first().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        self.ev(format!(r#"{{"type":"message.updated","properties":{{"sessionID":"{sid}","info":{{"id":"{mid}","role":"user","sessionID":"{sid}"}}}}}}"#));
        self.status(&sid, "busy");
        let am = self.next("msg_zz");
        self.ev(format!(r#"{{"type":"message.updated","properties":{{"sessionID":"{sid}","info":{{"id":"{am}","parentID":"{mid}","role":"assistant","sessionID":"{sid}","providerID":"fake","modelID":"fast","tokens":{{"input":4000,"output":1000,"cache":{{"read":0,"write":0}}}}}}}}}}"#));
        let mut said = vec![];
        for a in directive(&text, "ask") {
            let (kind, target) = a.split_once(':').unwrap_or((a, "notes.txt"));
            let pid = self.next("per");
            let meta = if kind == "bash" { format!(r#"{{"command":{}}}"#, esc(target)) } else { "{}".into() };
            self.ev(format!(r#"{{"type":"permission.asked","properties":{{"id":"{pid}","sessionID":"{sid}","permission":"{kind}","patterns":[{}],"metadata":{meta},"always":[]}}}}"#, esc(target)));
            let got = self.wait(&pid, &abort);
            let how = got.as_ref().and_then(|(_, b)| b.get("reply").and_then(Json::as_str).map(str::to_owned)).unwrap_or_else(|| "reject".into());
            self.ev(format!(r#"{{"type":"permission.replied","properties":{{"sessionID":"{sid}","requestID":"{pid}","reply":"{how}"}}}}"#));
            let ok = how == "once" || how == "always";
            if ok && kind == "edit" { let _ = std::fs::write(std::path::Path::new(&folder).join(target), "written by fake-opencode\n"); }
            said.push(format!("{} {kind} {target}", if ok { "allowed" } else { "denied" }));
        }
        if !directive(&text, "question").is_empty() && !abort.load(Ordering::SeqCst) {
            let qid = self.next("que");
            self.ev(format!(r#"{{"type":"question.asked","properties":{{"id":"{qid}","sessionID":"{sid}","questions":[{{"question":"Tabs or spaces?","header":"Indent","options":[{{"label":"Tabs","description":"One tab a level"}},{{"label":"Spaces","description":"Four spaces"}}],"custom":true}}]}}}}"#));
            match self.wait(&qid, &abort) {
                Some((kind, body)) if kind == "reply" => {
                    let picked = body.get("answers").map(|a| a.compact()).unwrap_or_default();
                    self.ev(format!(r#"{{"type":"question.replied","properties":{{"sessionID":"{sid}","requestID":"{qid}"}}}}"#));
                    said.push(format!("answered {picked}"));
                }
                _ => {
                    self.ev(format!(r#"{{"type":"question.rejected","properties":{{"sessionID":"{sid}","requestID":"{qid}"}}}}"#));
                    said.push("the question was skipped".into());
                }
            }
        }
        let answer = if let Some(n) = directive(&text, "bytes").first().and_then(|v| v.parse::<usize>().ok()) {
            let mut a = String::with_capacity(n + RICH.len());
            let mut i = 0;
            while a.len() < n { i += 1; a.push_str(&format!("### Part {i}\n\n{RICH}")); }
            a
        } else { "Done. Nothing needed changing.".into() };
        let answer = if said.is_empty() { answer } else { format!("{}\n\n{answer}", said.join("\n\n")) };
        let part = self.next("prt");
        self.ev(format!(r#"{{"type":"message.part.updated","properties":{{"sessionID":"{sid}","part":{{"id":"{part}","messageID":"{am}","sessionID":"{sid}","type":"text","text":""}}}}}}"#));
        let chars: Vec<char> = answer.chars().collect();
        let chunks: Vec<String> = chars.chunks(512).map(|c| c.iter().collect()).collect();
        let pause = Duration::from_secs_f64((secs / chunks.len().max(1) as f64).clamp(0.002, 0.5));
        let (drop_at, fail) = (!directive(&text, "drop").is_empty(), !directive(&text, "fail").is_empty());
        let mut sent = String::new();
        for (k, c) in chunks.iter().enumerate() {
            if abort.load(Ordering::SeqCst) { break; }
            if drop_at && k == chunks.len() / 2 { for s in self.streams.lock().unwrap().drain(..) { let _ = s.shutdown(std::net::Shutdown::Both); } }
            sent.push_str(c);
            self.ev(format!(r#"{{"type":"message.part.delta","properties":{{"sessionID":"{sid}","messageID":"{am}","partID":"{part}","field":"text","delta":{}}}}}"#, esc(c)));
            std::thread::sleep(pause);
        }
        self.ev(format!(r#"{{"type":"message.part.updated","properties":{{"sessionID":"{sid}","part":{{"id":"{part}","messageID":"{am}","sessionID":"{sid}","type":"text","text":{}}}}}}}"#, esc(&sent)));
        if fail { self.ev(format!(r#"{{"type":"session.error","properties":{{"sessionID":"{sid}","error":{{"name":"ProviderError","data":{{"message":"the fake provider failed"}}}}}}}}"#)); }
        self.status(&sid, "idle");
    }

    fn answer(self: Arc<Self>, mut s: TcpStream) {
        let mut r = BufReader::new(match s.try_clone() { Ok(c) => c, Err(_) => return });
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
        if r.read_exact(&mut body).is_err() { return; }
        let body = json::parse(&String::from_utf8_lossy(&body)).unwrap_or(Json::Null);
        let send = |s: &mut TcpStream, status: u16, text: &str| {
            let _ = s.write_all(format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).as_bytes());
        };
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let folder = query.split('&').find_map(|q| q.strip_prefix("directory=")).map(unescape);
        if auth != self.auth {
            self.note(&format!("401 {method} {path}"));
            return send(&mut s, 401, "{}");
        }
        let seg: Vec<String> = path.trim_matches('/').split('/').map(unescape).collect();
        let seg: Vec<&str> = seg.iter().map(String::as_str).collect();
        let mid = body.get("messageID").and_then(Json::as_str).unwrap_or("");
        self.note(&format!("{method} {path} dir={} {mid}", folder.as_deref().unwrap_or("-")));
        match (method.as_str(), seg.as_slice()) {
            ("GET", ["global", "health"]) => send(&mut s, 200, r#"{"healthy":true,"version":"1.18.31"}"#),
            ("GET", ["config", "providers"]) => send(&mut s, 200, PROVIDERS),
            ("GET", ["config"]) => send(&mut s, 200, "{}"),
            ("GET", ["agent"]) => send(&mut s, 200, AGENTS),
            ("GET", ["permission"]) | ("GET", ["question"]) => send(&mut s, 200, "[]"),
            ("GET", ["event"]) => {
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                let line = "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n";
                let _ = s.write_all(format!("{:x}\r\n{line}\r\n", line.len()).as_bytes());
                self.streams.lock().unwrap().push(s);
            }
            ("POST", ["session"]) => {
                let sid = self.next("ses");
                self.sessions.lock().unwrap().insert(sid.clone(), Session { folder: folder.unwrap_or_default(), ..Default::default() });
                send(&mut s, 200, &format!(r#"{{"id":"{sid}","directory":"x"}}"#))
            }
            ("GET", ["session", "status"]) => {
                let busy: Vec<String> = self.sessions.lock().unwrap().iter().filter(|(_, v)| v.busy).map(|(k, _)| format!(r#""{k}":{{"type":"busy"}}"#)).collect();
                send(&mut s, 200, &format!("{{{}}}", busy.join(",")))
            }
            ("GET", ["session", id]) => {
                if self.sessions.lock().unwrap().contains_key(*id) { send(&mut s, 200, &format!(r#"{{"id":"{id}"}}"#)) }
                else { send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"Session not found"}}"#) }
            }
            ("PATCH", ["session", id]) => send(&mut s, 200, &format!(r#"{{"id":"{id}"}}"#)),
            ("GET", ["session", id, "message", m]) => {
                let known = self.sessions.lock().unwrap().get(*id).is_some_and(|x| x.messages.iter().any(|y| y == m));
                if known { send(&mut s, 200, &format!(r#"{{"info":{{"id":"{m}","role":"user"}},"parts":[]}}"#)) }
                else { send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"no such message"}}"#) }
            }
            ("GET", ["session", _, "message"]) => send(&mut s, 200, "[]"),
            ("POST", ["session", id, "prompt_async"]) => {
                let text = match body.get("parts") { Some(Json::Arr(p)) => p.iter().filter_map(|x| x.get("text").and_then(Json::as_str)).collect::<Vec<_>>().join(" "), _ => String::new() };
                if let Some(f) = folder { self.sessions.lock().unwrap().entry(id.to_string()).or_default().folder = f; }
                let lose = !directive(&text, "lose").is_empty();
                let me = self.clone();
                let (sid, mid) = (id.to_string(), mid.to_owned());
                std::thread::spawn(move || me.turn(sid, mid, text));
                if lose { let _ = s.shutdown(std::net::Shutdown::Both); return; }
                send(&mut s, 204, "")
            }
            ("POST", ["session", id, "abort"]) => {
                if let Some(x) = self.sessions.lock().unwrap().get(*id) { x.abort.store(true, Ordering::SeqCst); }
                send(&mut s, 200, "true")
            }
            ("POST", ["permission", id, "reply"]) | ("POST", ["question", id, "reply"]) | ("POST", ["question", id, "reject"]) => {
                let kind = seg[2].to_owned();
                let tx = self.pending.lock().unwrap().remove(*id);
                send(&mut s, 200, "true");
                if let Some(tx) = tx { let _ = tx.send((kind, body)); }
            }
            _ => send(&mut s, 404, r#"{"name":"NotFoundError","data":{"message":"no route"}}"#),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") { println!("1.18.31"); return; }
    if args.first().map(String::as_str) != Some("serve") { eprintln!("fake-opencode: only `serve` and `--version`"); std::process::exit(2); }
    let flag = |name: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{name}="))).map(str::to_owned);
    let host = flag("hostname").unwrap_or_else(|| "127.0.0.1".into());
    // Hover must never ask for more than the loopback address.
    if host != "127.0.0.1" { eprintln!("fake-opencode: refusing to listen on {host}"); std::process::exit(3); }
    let Ok(password) = std::env::var("OPENCODE_SERVER_PASSWORD") else { eprintln!("fake-opencode: no OPENCODE_SERVER_PASSWORD"); std::process::exit(4) };
    let port = flag("port").unwrap_or_else(|| "0".into());
    let l = TcpListener::bind(format!("127.0.0.1:{port}")).expect("the port");
    let log = std::env::var_os("FAKE_OPENCODE_LOG").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()).map(Mutex::new);
    let srv = Arc::new(Server { auth: format!("Basic {}", hover_core_base64(format!("opencode:{password}").as_bytes())), ids: AtomicU64::new(0),
        sessions: Default::default(), streams: Default::default(), pending: Default::default(), log });
    srv.note(&format!("serve {}", args.join(" ")));
    println!("opencode server listening on http://{}", l.local_addr().unwrap());
    let _ = std::io::stdout().flush();
    let quit = Arc::new(AtomicBool::new(false));
    for s in l.incoming().flatten() {
        if quit.load(Ordering::SeqCst) { break; }
        let me = srv.clone();
        std::thread::spawn(move || me.answer(s));
    }
}

/// Standard base64 (RFC 4648), as Basic auth is written.
fn hover_core_base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 { if i <= c.len() { out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char); } else { out.push('='); } }
    }
    out
}
