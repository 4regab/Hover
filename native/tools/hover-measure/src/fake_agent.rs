//! fake-agent: a stand-in ACP tool for Hover's deterministic runs (the role FakeAcp
//! played for 2.x). Copied or linked as kiro-cli, codex-acp, codex or cursor-agent on
//! a PATH of its own, it answers their status commands as signed in and serves ACP on
//! stdio: sessions (new, load, prompt, cancel), tool calls, usage, thinking, and a
//! streamed answer. What a turn does is written into its prompt, so a scripted run
//! types it through the real UI:
//!
//!   [seconds:N]  the turn's length (default FAKEACP_SECONDS, else 1)
//!   [rate:N]     updates a second (default FAKEACP_RATE, else 20)
//!   [bytes:N]    an answer of about N bytes (the rich fixture over and over), streamed
//!   [answer:FILE] the answer from a file (default FAKEACP_ANSWER, else a short line)
//!   [ask:KIND] or [ask:KIND:TARGET]  asks permission first (edit, execute, delete,
//!                move, fetch); an allowed edit writes TARGET in the folder, an allowed
//!                delete removes it; the answer says what was allowed or denied
//!   [crash]      exits halfway; [hang] ignores session/cancel; [garbage] sends broken
//!                and split lines; [stderr:N] writes N KB to stderr
//!   [question]   not an ACP feature; ignored (OpenCode's questions have their own fake)

use hover_core::json::{self, Json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RICH: &str = "## What changed\n\nI moved the **token check** into `RefreshService`.\n\n| File | Lines |\n|---|---|\n| src/auth.rs | 42 |\n| src/refresh.rs | 17 |\n\n```rust\nfn refresh(t: &Token) -> Result<(), Error> {\n    if t.expired() { return Err(Error::Expired); }\n    Ok(())\n}\n```\n\n```mermaid\nflowchart LR\n  A[Request] --> B{Token ok?}\n  B -->|yes| C[Serve]\n  B -->|no| D[401]\n```\n\n- one\n- two\n\n";

struct Out(Mutex<std::io::Stdout>);
impl Out {
    fn line(&self, j: &Json) { let mut o = self.0.lock().unwrap(); let _ = writeln!(o, "{}", j.compact()); let _ = o.flush(); }
    fn raw(&self, s: &str) { let mut o = self.0.lock().unwrap(); let _ = o.write_all(s.as_bytes()); let _ = o.flush(); }
}

fn st(s: &str) -> Json { Json::str(s) }
fn obj(p: Vec<(&str, Json)>) -> Json { Json::obj(p) }

struct Agent {
    out: Out,
    ids: AtomicU64,
    sessions: Mutex<HashMap<String, String>>,
    cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pending: Mutex<HashMap<String, Sender<Json>>>,
}

fn update(sid: &str, u: Json) -> Json {
    obj(vec![("jsonrpc", st("2.0")), ("method", st("session/update")), ("params", obj(vec![("sessionId", st(sid)), ("update", u)]))])
}

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

impl Agent {
    fn reply(&self, id: &Json, result: Json) { self.out.line(&obj(vec![("jsonrpc", st("2.0")), ("id", id.clone()), ("result", result)])); }
    fn error(&self, id: &Json, msg: &str) { self.out.line(&obj(vec![("jsonrpc", st("2.0")), ("id", id.clone()), ("error", obj(vec![("code", Json::int(-32000)), ("message", st(msg))]))])); }

    /// session/request_permission, waited on; true when an allow option came back.
    fn ask(&self, sid: &str, kind: &str, target: &str, cwd: &str, n: usize) -> bool {
        let id = format!("perm-{}", self.ids.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        let full = if std::path::Path::new(target).is_absolute() { target.to_owned() } else { format!("{cwd}{}{target}", std::path::MAIN_SEPARATOR) };
        let mut call = vec![("toolCallId", st(&format!("ask{n}"))), ("kind", st(kind)), ("title", st(&format!("{kind} {target}")))];
        match kind {
            "execute" => call.push(("rawInput", obj(vec![("command", st(target))]))),
            "fetch" => call.push(("rawInput", obj(vec![("url", st(target))]))),
            _ => {
                call.push(("locations", Json::Arr(vec![obj(vec![("path", st(&full))])])));
                if kind == "edit" { call.push(("content", Json::Arr(vec![obj(vec![("type", st("diff")), ("path", st(&full)), ("oldText", Json::Null), ("newText", st("written by fake-agent\n"))])]))); }
            }
        }
        let options = Json::Arr(vec![
            obj(vec![("optionId", st("allow")), ("kind", st("allow_once")), ("name", st("Allow"))]),
            obj(vec![("optionId", st("always")), ("kind", st("allow_always")), ("name", st("Always allow"))]),
            obj(vec![("optionId", st("reject")), ("kind", st("reject_once")), ("name", st("Reject"))]),
        ]);
        self.out.line(&obj(vec![("jsonrpc", st("2.0")), ("id", st(&id)), ("method", st("session/request_permission")),
            ("params", obj(vec![("sessionId", st(sid)), ("toolCall", obj(call)), ("options", options)]))]));
        let got = rx.recv().unwrap_or(Json::Null);
        let outcome = got.get("outcome");
        let picked = outcome.and_then(|o| o.get("optionId")).and_then(Json::as_str).unwrap_or("");
        let ok = picked == "allow" || picked == "always";
        if ok {
            match kind {
                "edit" => { let _ = std::fs::write(&full, "written by fake-agent\n"); }
                "delete" => { let _ = std::fs::remove_file(&full); }
                _ => {}
            }
        }
        ok
    }

    fn prompt(self: &Arc<Self>, id: Json, sid: String, text: String) {
        let cwd = self.sessions.lock().unwrap().get(&sid).cloned().unwrap_or_default();
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel.lock().unwrap().insert(sid.clone(), flag.clone());
        let num = |name: &str, env: &str, d: f64| directive(&text, name).first().and_then(|v| v.parse().ok()).or_else(|| std::env::var(env).ok().and_then(|v| v.parse().ok())).unwrap_or(d);
        let secs = num("seconds", "FAKEACP_SECONDS", 1.0);
        let rate = num("rate", "FAKEACP_RATE", 20.0).max(1.0);
        let hang = !directive(&text, "hang").is_empty();
        let crash = !directive(&text, "crash").is_empty();
        let garbage = !directive(&text, "garbage").is_empty();
        if let Some(kb) = directive(&text, "stderr").first().and_then(|v| v.parse::<usize>().ok()) {
            let line = "fake-agent: a noisy line on stderr, as some tools print while they work\n";
            let mut e = std::io::stderr().lock();
            for _ in 0..(kb * 1024 / line.len()).max(1) { let _ = e.write_all(line.as_bytes()); }
        }
        let mut said: Vec<String> = vec![];
        for (n, a) in directive(&text, "ask").iter().enumerate() {
            let (kind, target) = a.split_once(':').unwrap_or((a, "notes.txt"));
            let ok = self.ask(&sid, kind, target, &cwd, n);
            said.push(format!("{} {kind} {target}", if ok { "allowed" } else { "denied" }));
            if flag.load(Ordering::SeqCst) && !hang { break; }
        }
        let ticks = (secs * rate).round().max(1.0) as usize;
        let mut cancelled = false;
        for k in 0..ticks {
            if flag.load(Ordering::SeqCst) && !hang { cancelled = true; break; }
            if crash && k == ticks / 2 { std::process::exit(3); }
            let u = match k % 10 {
                0 => obj(vec![("sessionUpdate", st("tool_call")), ("toolCallId", st(&format!("t{k}"))), ("kind", st(if k % 20 == 0 { "read" } else { "search" })), ("title", st(&format!("Step {}", k / 10))),
                    ("status", st("in_progress")), ("locations", Json::Arr(vec![obj(vec![("path", st(&format!("src/file{}.rs", k / 10)))])]))]),
                5 => obj(vec![("sessionUpdate", st("tool_call_update")), ("toolCallId", st(&format!("t{}", k - 5))), ("status", st("completed"))]),
                7 => obj(vec![("sessionUpdate", st("usage_update")), ("used", Json::int(1000 + k as i64 * 40)), ("size", Json::int(200000))]),
                _ => obj(vec![("sessionUpdate", st("agent_thought_chunk")), ("content", obj(vec![("type", st("text")), ("text", st("thinking "))]))]),
            };
            let line = update(&sid, u).compact();
            if garbage && k % 7 == 3 {
                self.out.raw("{\"jsonrpc\":\"2.0\",\"method\":\"session/upd\n");
                self.out.raw("not json at all\n");
                let (a, b) = line.split_at(line.len() / 2);
                self.out.raw(a);
                std::thread::sleep(Duration::from_millis(30));
                self.out.raw(&format!("{b}\n"));
            } else {
                self.out.raw(&format!("{line}\n"));
            }
            std::thread::sleep(Duration::from_secs_f64(1.0 / rate));
        }
        if !cancelled {
            let answer = if let Some(n) = directive(&text, "bytes").first().and_then(|v| v.parse::<usize>().ok()) {
                let mut a = String::with_capacity(n + RICH.len());
                let mut i = 0;
                while a.len() < n { i += 1; a.push_str(&format!("### Part {i}\n\n{RICH}")); }
                a
            } else if let Some(f) = directive(&text, "answer").first().map(|s| s.to_string()).or_else(|| std::env::var("FAKEACP_ANSWER").ok()) {
                std::fs::read_to_string(f).unwrap_or_else(|e| format!("couldn't read the answer: {e}"))
            } else { "Done. Nothing needed changing.".into() };
            let answer = if said.is_empty() { answer } else { format!("{}\n\n{answer}", said.join("\n\n")) };
            // Streamed in chunks of about 2 KB, as a model writes.
            let chars: Vec<char> = answer.chars().collect();
            for c in chars.chunks(2048) {
                let t: String = c.iter().collect();
                self.out.line(&update(&sid, obj(vec![("sessionUpdate", st("agent_message_chunk")), ("content", obj(vec![("type", st("text")), ("text", st(&t))]))])));
                if flag.load(Ordering::SeqCst) && !hang { cancelled = true; break; }
                if chars.len() > 4096 { std::thread::sleep(Duration::from_millis(5)); }
            }
        }
        self.cancel.lock().unwrap().remove(&sid);
        self.reply(&id, obj(vec![("stopReason", st(if cancelled { "cancelled" } else { "end_turn" }))]));
    }

    fn handle(self: &Arc<Self>, line: &str) {
        let Ok(m) = json::parse(line) else { return };
        let id = m.get("id").cloned();
        let method = m.get("method").and_then(Json::as_str).map(str::to_owned);
        let p = m.get("params").cloned().unwrap_or(Json::Null);
        let sid = || p.get("sessionId").and_then(Json::as_str).unwrap_or("").to_owned();
        match (method.as_deref(), id) {
            (None, Some(id)) => {
                let key = id.as_str().map(str::to_owned).unwrap_or_else(|| id.compact());
                if let Some(tx) = self.pending.lock().unwrap().remove(&key) { let _ = tx.send(m.get("result").cloned().unwrap_or(Json::Null)); }
            }
            (Some("initialize"), Some(id)) => self.reply(&id, obj(vec![("protocolVersion", Json::int(1)), ("agentCapabilities", obj(vec![("loadSession", Json::Bool(true))]))])),
            (Some("session/new"), Some(id)) => {
                let s = format!("fake-{}-{}", std::process::id(), self.ids.fetch_add(1, Ordering::SeqCst) + 1);
                self.sessions.lock().unwrap().insert(s.clone(), p.get("cwd").and_then(Json::as_str).unwrap_or("").to_owned());
                self.reply(&id, obj(vec![("sessionId", st(&s)), ("configOptions", Json::Arr(vec![]))]));
            }
            (Some("session/load"), Some(id)) => {
                self.sessions.lock().unwrap().insert(sid(), p.get("cwd").and_then(Json::as_str).unwrap_or("").to_owned());
                self.reply(&id, obj(vec![("configOptions", Json::Arr(vec![]))]));
            }
            (Some("session/prompt"), Some(id)) => {
                let s = sid();
                if !self.sessions.lock().unwrap().contains_key(&s) { return self.error(&id, "Session not found"); }
                let text = match p.get("prompt") { Some(Json::Arr(parts)) => parts.iter().filter_map(|x| x.get("text").and_then(Json::as_str)).collect::<Vec<_>>().join(" "), _ => String::new() };
                let me = self.clone();
                std::thread::spawn(move || me.prompt(id, s, text));
            }
            (Some("session/cancel"), _) => { if let Some(f) = self.cancel.lock().unwrap().get(&sid()) { f.store(true, Ordering::SeqCst); } }
            (Some("session/set_config_option"), Some(id)) => self.reply(&id, obj(vec![("configOptions", Json::Arr(vec![]))])),
            (Some(_), Some(id)) => self.error(&id, "Method not found"),
            _ => {}
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") { println!("fake-agent 1.0.0"); return; }
    if args.iter().any(|a| matches!(a.as_str(), "whoami" | "status" | "login")) { println!("Logged in as fake@example.com"); return; }
    let a = Arc::new(Agent { out: Out(Mutex::new(std::io::stdout())), ids: AtomicU64::new(0), sessions: Default::default(), cancel: Default::default(), pending: Default::default() });
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        a.handle(&line);
    }
}
