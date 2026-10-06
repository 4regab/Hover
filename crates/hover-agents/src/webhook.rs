//! Webhooks that start saved tasks (sched.rs). A small HTTP listener, off until the user turns it on, bound to this computer only.
//!
//! - **Local by default.** It listens on 127.0.0.1. Reaching it from the internet (a tunnel, a reverse proxy, a forwarded port) is the
//!   user's own setup and is not done here; binding to any other address needs the explicit `allow_public` flag. Hover runs no relay.
//! - **Signed.** Each task has its own secret (in `secrets.dat`, sealed; shown once when made or rotated). A delivery is accepted only
//!   with a valid HMAC-SHA256 of its body: GitHub’s `X-Hub-Signature-256`, or Hover’s own `X-Hover-Signature-256` over
//!   `<timestamp>.<body>` with `X-Hover-Timestamp` (refused when more than 5 minutes off, so a captured call can’t be replayed later).
//! - **Once.** A delivery id (`X-GitHub-Delivery`, `X-Hover-Delivery`) seen before is refused (409). Ids are remembered after the
//!   signature is checked, so no one can use the list to lock out a real delivery, and are kept across restarts.
//! - **Bounded.** Headers 16 KB, body 1 MB, 16 connections at once, 100 log entries, 2000 remembered ids. Overflow is refused with a status, not queued without end.
//! - **Only what was chosen.** The prompt gets the task's own text and the fields the user named (paths into the JSON), each cut at 500
//!   characters. Nothing else of the delivery reaches an agent, and it grants no access the task doesn't have.
//!
//! Answers: 202 accepted or filtered out, 401 bad signature, 404 no such (or paused) task, 409 seen before, 410 too old, 413 too large,
//! 429 too many runs waiting, 400 unreadable.

use crate::sched::Scheduler;
use hover_core::json::{self, Json};
use hover_core::secrets::Secrets;
use hover_core::store::Sealed;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HEADERS_MAX: usize = 16 * 1024;
const BODY_MAX: usize = 1 << 20;
const SEEN_MAX: usize = 2000;
const LOG_MAX: usize = 100;
const CONNECTIONS: usize = 16;
/// How far a Hover-signed delivery's timestamp may be from now.
pub const SKEW_SECS: i64 = 300;

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 { k[..32].copy_from_slice(&Sha256::digest(key)); } else { k[..key.len()].copy_from_slice(key); }
    let inner = { let mut h = Sha256::new(); h.update(k.map(|b| b ^ 0x36)); h.update(msg); h.finalize() };
    let mut h = Sha256::new();
    h.update(k.map(|b| b ^ 0x5c));
    h.update(inner);
    h.finalize().into()
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// Equal without stopping at the first difference.
fn same(a: &str, b: &str) -> bool { a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { Accepted, Filtered, BadSignature, Unknown, Duplicate, Expired, TooLarge, Busy, Invalid }

impl Outcome {
    const NAMES: [&'static str; 9] = ["accepted", "filtered", "bad signature", "unknown task", "duplicate", "expired", "too large", "queue full", "unreadable"];
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    pub fn status(self) -> (u16, &'static str) {
        match self { Outcome::Accepted | Outcome::Filtered => (202, "Accepted"), Outcome::BadSignature => (401, "Unauthorized"), Outcome::Unknown => (404, "Not Found"), Outcome::Duplicate => (409, "Conflict"),
            Outcome::Expired => (410, "Gone"), Outcome::TooLarge => (413, "Payload Too Large"), Outcome::Busy => (429, "Too Many Requests"), Outcome::Invalid => (400, "Bad Request") }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry { pub at: i64, pub task: String, pub delivery: String, pub event: String, pub outcome: Outcome }

pub struct Request { pub path: String, pub headers: Vec<(String, String)>, pub body: Vec<u8> }

impl Request {
    fn header(&self, n: &str) -> Option<&str> { self.headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str()) }
}

#[derive(Default)]
struct St { seen: VecDeque<String>, log: VecDeque<Entry> }

pub struct Hooks {
    sched: Arc<Scheduler>,
    secrets: Arc<Secrets>,
    doc: Option<Sealed>,
    st: Mutex<St>,
    open: AtomicUsize,
    listening: Mutex<Option<(SocketAddr, Arc<std::sync::atomic::AtomicBool>)>>,
}

fn secret_key(task: &str) -> String { format!("webhook.{task}") }

/// A JSON value picked out by a path like `/pull_request/title` or `/commits/0/message`, as short text.
pub fn pick(v: &Json, path: &str) -> Option<String> {
    let mut cur = v;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        cur = match cur { Json::Arr(a) => a.get(seg.parse::<usize>().ok()?)?, Json::Obj(_) => cur.get(seg)?, _ => return None };
    }
    let text = match cur { Json::Str(s) => s.clone(), Json::Num(n) => n.clone(), Json::Bool(b) => b.to_string(), Json::Null => return None, other => other.compact() };
    Some(crate::handoff::clip(&text, 500))
}

impl Hooks {
    pub fn new(sched: Arc<Scheduler>, secrets: Arc<Secrets>, doc: Option<Sealed>) -> Arc<Hooks> {
        let mut st = St::default();
        if let Some(Json::Arr(a)) = doc.as_ref().and_then(Sealed::read).and_then(|v| v.get("Seen").cloned()) { st.seen = a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(); }
        Arc::new(Hooks { sched, secrets, doc, st: Mutex::new(st), open: AtomicUsize::new(0), listening: Mutex::new(None) })
    }

    fn save(&self, st: &St) {
        if let Some(d) = &self.doc { let _ = d.write(&Json::obj(vec![("Seen", Json::Arr(st.seen.iter().map(Json::str).collect()))])); }
    }

    /// Makes (or replaces) the task's secret. The old one stops working at once. Returned once; only its existence is kept readable.
    pub fn rotate(&self, task: &str) -> Result<String, String> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|e| e.to_string())?;
        let secret = hex(&b);
        self.secrets.set(&secret_key(task), Some(&secret)).map_err(|m| format!("The secret couldn’t be stored: {m}"))?;
        Ok(secret)
    }

    pub fn has_secret(&self, task: &str) -> bool { self.secrets.has(&secret_key(task)) }
    pub fn forget(&self, task: &str) { let _ = self.secrets.set(&secret_key(task), None); }
    pub fn log(&self) -> Vec<Entry> { self.st.lock().unwrap().log.iter().cloned().collect() }
    pub fn addr(&self) -> Option<SocketAddr> { self.listening.lock().unwrap().as_ref().map(|l| l.0) }

    fn note(&self, task: &str, delivery: &str, event: &str, o: Outcome) -> Outcome {
        let mut g = self.st.lock().unwrap();
        g.log.push_back(Entry { at: crate::wake::now_ms(), task: task.into(), delivery: delivery.into(), event: event.into(), outcome: o });
        while g.log.len() > LOG_MAX { g.log.pop_front(); }
        o
    }

    /// One delivery, checked and handed to the scheduler. The outcome says what the sender is told.
    pub fn handle(&self, req: &Request) -> Outcome {
        let Some(task_id) = req.path.strip_prefix("/hook/").map(|t| t.trim_end_matches('/')).filter(|t| !t.is_empty() && !t.contains('/')) else { return Outcome::Unknown };
        let Some(task) = self.sched.get(task_id).filter(|t| t.enabled && t.hook.as_ref().is_some_and(|h| h.enabled)) else { return Outcome::Unknown };
        let hook = task.hook.clone().unwrap();
        let github = req.header("x-hub-signature-256").is_some();
        let event = req.header("x-github-event").or_else(|| req.header("x-hover-event")).unwrap_or("").to_owned();
        let delivery = req.header("x-github-delivery").or_else(|| req.header("x-hover-delivery")).unwrap_or("").to_owned();
        let note = |o: Outcome| self.note(task_id, &delivery, &event, o);
        if req.body.len() > BODY_MAX { return note(Outcome::TooLarge); }
        let Some(secret) = self.secrets.get(&secret_key(task_id)) else { return note(Outcome::BadSignature) };
        let (sent, signed): (String, Vec<u8>) = if github {
            (req.header("x-hub-signature-256").unwrap_or("").to_owned(), req.body.clone())
        } else {
            let ts = req.header("x-hover-timestamp").unwrap_or("");
            (req.header("x-hover-signature-256").unwrap_or("").to_owned(), [ts.as_bytes(), b".", &req.body].concat())
        };
        let want = format!("sha256={}", hex(&hmac_sha256(secret.as_bytes(), &signed)));
        if !same(sent.trim(), &want) { return note(Outcome::BadSignature); }
        if !github {
            let ts: i64 = req.header("x-hover-timestamp").and_then(|t| t.parse().ok()).unwrap_or(0);
            if (crate::wake::now_ms() / 1000 - ts).abs() > SKEW_SECS { return note(Outcome::Expired); }
        }
        if delivery.is_empty() || delivery.len() > 200 { return note(Outcome::Invalid); }
        {
            let mut g = self.st.lock().unwrap();
            let key = format!("{task_id}:{delivery}");
            if g.seen.contains(&key) { drop(g); return note(Outcome::Duplicate); }
            g.seen.push_back(key);
            while g.seen.len() > SEEN_MAX { g.seen.pop_front(); }
            self.save(&g);
        }
        if !hook.events.is_empty() && !hook.events.iter().any(|e| *e == event) { return note(Outcome::Filtered); }
        let body = match json::parse(&String::from_utf8_lossy(&req.body)) { Ok(v) => v, Err(_) if hook.fields.is_empty() => Json::Null, Err(_) => return note(Outcome::Invalid) };
        let lines: Vec<String> = hook.fields.iter().filter_map(|f| pick(&body, f).map(|v| format!("- {f}: {v}"))).collect();
        let extra = if lines.is_empty() { String::new() } else { format!("The webhook that started this run sent (only the fields chosen for this task):\n{}", lines.join("\n")) };
        match self.sched.trigger(task_id, &delivery, &extra) { Ok(()) => note(Outcome::Accepted), Err(e) if e.contains("Too many") => note(Outcome::Busy), Err(_) => note(Outcome::Unknown) }
    }

    /// Starts listening on `addr`. Only this computer's own address, unless `allow_public` says otherwise. Returns the address.
    pub fn listen(self: &Arc<Self>, addr: &str, allow_public: bool) -> Result<SocketAddr, String> {
        let sa: SocketAddr = addr.parse().map_err(|_| "That isn’t an address and port, like 127.0.0.1:47653.".to_owned())?;
        if !sa.ip().is_loopback() && !allow_public { return Err("Hover listens on this computer only. To take calls from elsewhere, you set up the way in (a tunnel or a proxy) and choose to allow it here.".into()); }
        self.stop();
        let l = TcpListener::bind(sa).map_err(|e| format!("Couldn’t listen on {sa}: {e}"))?;
        let bound = l.local_addr().map_err(|e| e.to_string())?;
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *self.listening.lock().unwrap() = Some((bound, stop.clone()));
        let me = Arc::downgrade(self);
        std::thread::Builder::new().name("webhook".into()).spawn(move || {
            for conn in l.incoming() {
                if stop.load(Ordering::SeqCst) { return; }
                let Ok(c) = conn else { continue };
                let Some(h) = me.upgrade() else { return };
                if h.open.fetch_add(1, Ordering::SeqCst) >= CONNECTIONS { h.open.fetch_sub(1, Ordering::SeqCst); respond(&c, 503, "Service Unavailable", "busy"); continue; }
                let _ = std::thread::Builder::new().name("webhook-conn".into()).spawn(move || { serve(&h, c); h.open.fetch_sub(1, Ordering::SeqCst); });
            }
        }).map_err(|e| e.to_string())?;
        hover_core::log::line(&format!("webhook: listening on {bound}"));
        Ok(bound)
    }

    pub fn stop(&self) {
        if let Some((addr, stop)) = self.listening.lock().unwrap().take() { stop.store(true, Ordering::SeqCst); let _ = TcpStream::connect(addr); }
    }
}

fn respond(mut c: &TcpStream, code: u16, reason: &str, body: &str) {
    let b = format!("{{\"status\":\"{body}\"}}");
    let _ = write!(c, "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
}

/// Reads one request from the socket, within the limits, and answers it.
fn serve(h: &Hooks, mut c: TcpStream) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { break i; }
        if buf.len() > HEADERS_MAX { return respond(&c, 431, "Request Header Fields Too Large", "headers too large"); }
        match c.read(&mut chunk) { Ok(0) | Err(_) => return, Ok(n) => buf.extend_from_slice(&chunk[..n]) }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut p = first.split(' ');
    let (method, path) = (p.next().unwrap_or(""), p.next().unwrap_or(""));
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_owned()))).collect();
    if method != "POST" { return respond(&c, 405, "Method Not Allowed", "post only"); }
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if len > BODY_MAX { h.note("", "", "", Outcome::TooLarge); return respond(&c, 413, "Payload Too Large", "too large"); }
    let mut body = buf[end + 4..].to_vec();
    while body.len() < len {
        match c.read(&mut chunk) { Ok(0) | Err(_) => return respond(&c, 400, "Bad Request", "body cut short"), Ok(n) => body.extend_from_slice(&chunk[..n]) }
    }
    body.truncate(len);
    let o = h.handle(&Request { path: path.split('?').next().unwrap_or("").to_owned(), headers, body });
    let (code, reason) = o.status();
    respond(&c, code, reason, o.name());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_is_the_one_github_documents() {
        // GitHub’s own example (docs: validating webhook deliveries).
        let sig = hex(&hmac_sha256(b"It's a Secret to Everybody", b"Hello, World!"));
        assert_eq!(sig, "757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17");
        // A key longer than a block is hashed first (RFC 4231 test case 6).
        assert_eq!(hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")), "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    #[test]
    fn fields_are_picked_by_path_and_cut() {
        let v = json::parse(r#"{"pull_request":{"title":"Fix login","number":12,"draft":false,"labels":["a","b"],"body":null},"commits":[{"message":"first"}]}"#).unwrap();
        assert_eq!(pick(&v, "/pull_request/title").as_deref(), Some("Fix login"));
        assert_eq!(pick(&v, "/pull_request/number").as_deref(), Some("12"));
        assert_eq!(pick(&v, "/pull_request/draft").as_deref(), Some("false"));
        assert_eq!(pick(&v, "/commits/0/message").as_deref(), Some("first"));
        assert_eq!(pick(&v, "/pull_request/body"), None);
        assert_eq!(pick(&v, "/nope/x"), None);
        assert!(pick(&v, "/pull_request/labels").unwrap().contains("\"a\""));
        let long = json::parse(&format!(r#"{{"t":"{}"}}"#, "x".repeat(900))).unwrap();
        assert!(pick(&long, "/t").unwrap().contains("[cut: 400 more characters]"));
    }
}
