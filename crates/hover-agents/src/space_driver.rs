//! One agent's computer use in its project's Space: an MCP server on the agent's side
//! (stdio, through Hover's relay) and a client of the Space's own Cua Driver on the other
//! (cua-spacesd's `/mcp`, streamable HTTP, outside the agents' sandbox). Each agent gets a
//! driver session of its own there, so a cursor of its own, and works in the background on
//! its own app's windows: the agents of a project share its desktop without taking the
//! pointer or the keyboard from each other. The few calls that do take them (input to the
//! whole desktop, a window brought to the front for a moment) take turns, one agent at a
//! time per desktop.
//!
//! Memory and threads: one reader per agent connection, one short-lived thread per call
//! in flight (an agent makes one or two at a time), one HTTP connection per call (the
//! driver is on this Mac's own VM network), and the driver's tool list kept once for all.

use crate::http::{Client, HttpErr};
use crate::spaces::{self, DriverEndpoint};
use hover_core::json::{self, Json};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// What an agent may not do there: end or hide its own session (the user follows its
/// cursor), leave an app in front of the others' work, kill an app (it may be another
/// agent's), or change the driver itself.
pub const DENIED: &[&str] = &["kill_app", "bring_to_front", "replay_trajectory", "install_ffmpeg", "set_config", "escalate_session", "end_session", "set_agent_cursor_enabled"];

pub const INSTRUCTIONS: &str = "This is the project's own macOS desktop, a VM on the user's Mac, never the user's own screen. \
Other agents of the same project may be working on it at the same time; each has a cursor of its own. \
Work in the background on your own app's windows: snapshot a window (get_window_state) and act on its elements. \
Pass creates_new_application_instance to launch_app when another agent may be using the same app. \
Whole-desktop input and foreground delivery take turns with the other agents, so use them only when nothing else works.";

/// No line of MCP is this long; an agent that sends one is cut off (as the browser's bridge does).
const LINE_LIMIT: u64 = 4 * 1024 * 1024;

// The driver's tool list (the same for every desktop of an image), kept once for every
// agent so one that connects while its desktop still starts gets it at once.
static TOOLS: Mutex<Option<Arc<Json>>> = Mutex::new(None);
fn tools_file() -> std::path::PathBuf { hover_core::paths::support().join("spaces").join("driver-tools.json") }
// One at a time per desktop: calls that move the real pointer or front a window.
static TURNS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Default::default);

fn text(v: Option<&Json>) -> Option<&str> { v.and_then(Json::as_str) }

/// An object's value by name, made mutable: replaced if there, added if not.
fn put(o: &mut Json, name: &str, v: Json) {
    if let Json::Obj(p) = o {
        p.retain(|(k, _)| k != name);
        p.push((name.to_owned(), v));
    }
}

/// The agent's arguments with its own session, whatever it passed: an agent can't act in
/// (or end) another's.
pub fn arguments(given: Option<&Json>, label: &str) -> Json {
    let mut args = match given { Some(o @ Json::Obj(_)) => o.clone(), _ => Json::Obj(vec![]) };
    put(&mut args, "session", Json::str(label));
    args
}

/// Input to the whole desktop and foreground delivery: one agent of the project at a time.
pub fn exclusive(args: &Json) -> bool {
    text(args.get("delivery_mode")) == Some("foreground") || text(args.get("scope")) == Some("desktop")
        || args.get("target").is_some_and(|t| text(t.get("kind")) == Some("desktop"))
}

/// What the driver says when it won't take a session label (it ended an idle one).
pub fn refusal(reply: &Json) -> Option<&str> { text(reply.get("result")?.get("structuredContent")?.get("refusal")?.get("code")) }

/// The driver's tools as the agent sees them: none it may not use, and no session argument
/// (Hover gives each agent its own).
pub fn tools_for_agent(all: &Json) -> Json {
    let Json::Arr(items) = all else { return Json::Arr(vec![]) };
    Json::Arr(items.iter().filter_map(|t| {
        let name = text(t.get("name"))?;
        if DENIED.contains(&name) { return None; }
        let mut copy = t.clone();
        if let Json::Obj(props) = &mut copy {
            for (k, schema) in props.iter_mut() {
                if k != "inputSchema" { continue; }
                if let Json::Obj(sp) = schema {
                    for (sk, sv) in sp.iter_mut() {
                        match (sk.as_str(), sv) {
                            ("properties", Json::Obj(p)) => p.retain(|(n, _)| n != "session"),
                            ("required", Json::Arr(r)) => r.retain(|x| x.as_str() != Some("session")),
                            _ => {}
                        }
                    }
                }
            }
        }
        Some(copy)
    }).collect())
}

fn remember(tools: Option<&Json>) {
    let Some(t @ Json::Arr(a)) = tools else { return };
    if a.is_empty() { return; }
    *TOOLS.lock().unwrap() = Some(Arc::new(t.clone()));
    let f = tools_file();
    if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = std::fs::write(f, t.compact());
}

fn known_tools() -> Option<Arc<Json>> {
    let mut g = TOOLS.lock().unwrap();
    if g.is_none() {
        let read = std::fs::read_to_string(tools_file()).ok().and_then(|t| json::parse(&t).ok()).filter(|t| matches!(t, Json::Arr(a) if !a.is_empty()));
        *g = read.map(Arc::new);
    }
    g.clone()
}

fn ok(id: &Json, result: Json) -> Json { Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("result", result)]) }
fn error(id: &Json, message: &str, code: i64) -> Json {
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("error", Json::obj(vec![("code", Json::int(code)), ("message", Json::str(message))]))])
}
fn tool_error(id: &Json, message: &str) -> Json {
    ok(id, Json::obj(vec![("content", Json::Arr(vec![Json::obj(vec![("type", Json::str("text")), ("text", Json::str(message))])])), ("isError", Json::Bool(true))]))
}

/// Why a call to the driver didn't get an answer.
#[derive(Debug)]
enum Gone { Session(String), Net(String), Late }

/// Where this agent's driver session is: the desktop's endpoint and the MCP session id.
#[derive(Clone)]
struct Link { folder: String, end: DriverEndpoint, session: Option<String> }

pub struct Driver {
    folder: Box<dyn Fn() -> Option<String> + Send + Sync>,
    agent: String,
    label: Mutex<String>,
    init: Mutex<Option<Json>>,
    link: Mutex<Option<Link>>,
    // One connect at a time (a call and its retry, or two calls at once).
    connecting: Mutex<()>,
    why: Mutex<String>,
    ids: AtomicU64,
}

/// A session label of the agent's own: the driver ties one to the connection that made it
/// (until it expires), so each connection names a new one.
fn fresh(agent: &str) -> String {
    let mut b = [0u8; 2];
    let _ = getrandom::fill(&mut b);
    format!("{agent}-{:02x}{:02x}", b[0], b[1])
}

impl Driver {
    /// `folder` is asked at each call: OpenCode's one server serves whichever of its
    /// sessions is at work. `agent` names the agent ("hover-<tag>").
    pub fn new(folder: impl Fn() -> Option<String> + Send + Sync + 'static, agent: &str) -> Arc<Driver> {
        Arc::new(Driver { folder: Box::new(folder), agent: agent.to_owned(), label: Mutex::new(agent.to_owned()), init: Mutex::new(None), link: Mutex::new(None), connecting: Mutex::new(()), why: Mutex::new(String::new()), ids: AtomicU64::new(0) })
    }

    /// Serves one agent connection until it ends; its session in the driver ends with it.
    pub fn run(self: &Arc<Driver>, mut from_agent: Box<dyn BufRead + Send>, to_agent: Box<dyn Write + Send>) {
        let out = Arc::new(Mutex::new(to_agent));
        let mut calls: Vec<std::thread::JoinHandle<()>> = vec![];
        let mut line = Vec::new();
        loop {
            line.clear();
            match (&mut from_agent).take(LINE_LIMIT + 1).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() as u64 > LINE_LIMIT => break,
                Ok(_) => {}
            }
            let Ok(m) = json::parse(String::from_utf8_lossy(&line).trim()) else { continue };
            let Some(method) = text(m.get("method")).map(str::to_owned) else { continue };
            let Some(id) = m.get("id").cloned().filter(|i| !i.is_null()) else {
                if method != "notifications/initialized" { self.notify(&m); }
                continue;
            };
            calls.retain(|h| !h.is_finished());
            // Each call on its own: a long one (a window's snapshot) holds up no other.
            let (me, out, params) = (self.clone(), out.clone(), m.get("params").cloned());
            if let Ok(h) = std::thread::Builder::new().name("space-call".into()).spawn(move || {
                let reply = me.answer(&method, &id, params.as_ref());
                let mut w = out.lock().unwrap();
                let _ = w.write_all(format!("{}\n", reply.compact()).as_bytes()).and_then(|_| w.flush());
            }) { calls.push(h); }
        }
        for h in calls { let _ = h.join(); }
        // Its cursor goes with the agent (its next connection starts a new one).
        if let Some(l) = self.link.lock().unwrap().clone() {
            let label = self.label.lock().unwrap().clone();
            let _ = self.post(&l, &self.request("tools/call", Json::obj(vec![("name", Json::str("end_session")), ("arguments", Json::obj(vec![("session", Json::str(label))]))])), Duration::from_secs(10));
        }
    }

    fn answer(&self, method: &str, id: &Json, p: Option<&Json>) -> Json {
        match method {
            // Answered here, at once: the desktop may still be starting, and the agent's tool
            // gives up on a server that doesn't answer its handshake in time.
            "initialize" => {
                *self.init.lock().unwrap() = p.cloned();
                let version = p.and_then(|p| text(p.get("protocolVersion"))).unwrap_or("2025-06-18");
                ok(id, Json::obj(vec![
                    ("protocolVersion", Json::str(version)),
                    ("capabilities", Json::obj(vec![("tools", Json::obj(vec![("listChanged", Json::Bool(false))]))])),
                    ("serverInfo", Json::obj(vec![("name", Json::str(spaces::SERVER_NAME)), ("title", Json::str("The project's desktop")), ("version", Json::str("1.0"))])),
                    ("instructions", Json::str(INSTRUCTIONS)),
                ]))
            }
            "ping" => ok(id, Json::Obj(vec![])),
            "tools/list" => {
                let all = match known_tools() {
                    Some(t) => t,
                    None => match self.call("tools/list", &|_| Json::Obj(vec![]), Duration::from_secs(120)) {
                        Ok(r) if r.get("error").is_none() => { remember(r.get("result").and_then(|r| r.get("tools"))); known_tools().unwrap_or_else(|| Arc::new(Json::Arr(vec![]))) }
                        Ok(r) => return relabel(r, id, false),
                        Err(e) => return error(id, &self.unreachable(e), -32000),
                    },
                };
                ok(id, Json::obj(vec![("tools", tools_for_agent(&all))]))
            }
            "tools/call" => self.tool_call(id, p),
            "resources/list" => ok(id, Json::obj(vec![("resources", Json::Arr(vec![]))])),
            "prompts/list" => ok(id, Json::obj(vec![("prompts", Json::Arr(vec![]))])),
            _ => error(id, &format!("Method not found: {method}"), -32601),
        }
    }

    fn tool_call(&self, id: &Json, p: Option<&Json>) -> Json {
        let name = p.and_then(|p| text(p.get("name"))).unwrap_or("").to_owned();
        if DENIED.contains(&name.as_str()) { return tool_error(id, &format!("{name} isn't available on the project's desktop: other agents work there too.")); }
        let given = p.and_then(|p| p.get("arguments")).cloned();
        let Some(folder) = (self.folder)() else { return tool_error(id, "There's no project desktop for this task.") };
        let _use = spaces::use_space(&folder);
        // Whole-desktop input and foreground delivery: one agent of the project at a time.
        let turn = exclusive(given.as_ref().unwrap_or(&Json::Null)).then(|| TURNS.lock().unwrap().entry(spaces::name_for(&folder)).or_default().clone());
        let _held = match &turn {
            Some(t) => {
                let until = Instant::now() + Duration::from_secs(120);
                loop {
                    match t.try_lock() {
                        Ok(g) => break Some(g),
                        Err(std::sync::TryLockError::Poisoned(g)) => break Some(g.into_inner()),
                        Err(std::sync::TryLockError::WouldBlock) if Instant::now() > until => {
                            return tool_error(id, "Another agent has been using the whole desktop for two minutes; try a background action on your own window instead.");
                        }
                        Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            }
            None => None,
        };
        // The label is read as the call goes: a reconnect on the way names a new one.
        let make = |label: &str| Json::obj(vec![("name", Json::str(&name)), ("arguments", arguments(given.as_ref(), label))]);
        let mut reply = self.call("tools/call", &make, Duration::from_secs(300));
        // The driver ends a session left idle a few minutes and won't take its label again:
        // the agent carries on under a new one.
        if reply.as_ref().is_ok_and(|r| refusal(r).is_some_and(|c| c.starts_with("session_"))) {
            self.renew();
            reply = self.call("tools/call", &make, Duration::from_secs(300));
        }
        match reply { Ok(r) => relabel(r, id, true), Err(e) => tool_error(id, &self.unreachable(e)) }
    }

    fn unreachable(&self, e: Gone) -> String {
        match e {
            Gone::Late => "The desktop's driver took too long.".into(),
            Gone::Session(m) | Gone::Net(m) => {
                let why = self.why.lock().unwrap().clone();
                let why = if why.is_empty() { m } else { why };
                format!("The project's desktop isn't on. {why}").trim_end().to_owned()
            }
        }
    }

    fn request(&self, method: &str, params: Json) -> Json {
        Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", Json::str(format!("hover-{}", self.ids.fetch_add(1, Ordering::SeqCst) + 1))), ("method", Json::str(method)), ("params", params)])
    }

    /// A call to the desktop's driver, connecting first (or again: the desktop may have been
    /// turned off while the agent was idle, or come back on a new address).
    fn call(&self, method: &str, params: &dyn Fn(&str) -> Json, timeout: Duration) -> Result<Json, Gone> {
        for attempt in 0..2 {
            let Some(l) = self.connect(attempt > 0) else { return Err(Gone::Net(String::new())) };
            let label = self.label.lock().unwrap().clone();
            match self.post(&l, &self.request(method, params(&label)), timeout) {
                Ok((replies, _)) => return replies.into_iter().rev().find(|r| r.get("id").is_some()).ok_or(Gone::Net("no answer".into())),
                Err(Gone::Late) => return Err(Gone::Late),
                Err(e) if attempt == 0 => { hover_core::log::line(&format!("space driver: reconnecting ({e:?})")); *self.link.lock().unwrap() = None; }
                Err(e) => return Err(e),
            }
        }
        Err(Gone::Net(String::new()))
    }

    fn connect(&self, again: bool) -> Option<Link> {
        let _one = self.connecting.lock().unwrap_or_else(|p| p.into_inner());
        let folder = match (self.folder)() { Some(f) => f, None => { *self.why.lock().unwrap() = "No project is at work.".into(); return None; } };
        if !again { if let Some(l) = self.link.lock().unwrap().clone().filter(|l| l.folder == folder) { return Some(l); } }
        // Bounded: a desktop still being made takes minutes, and the agent's call (or its
        // tool list, asked before its session begins) would hold it that long.
        if let Some(why) = spaces::ensure_within(&folder, again, spaces::TOOL_WAIT) { *self.why.lock().unwrap() = why; return None; }
        let Some(end) = spaces::driver(&folder, again) else { *self.why.lock().unwrap() = "Its driver couldn't be reached.".into(); return None; };
        let mut link = Link { folder, end, session: None };
        let init = self.init.lock().unwrap().clone().unwrap_or_else(|| Json::obj(vec![
            ("protocolVersion", Json::str("2025-06-18")), ("capabilities", Json::Obj(vec![])),
            ("clientInfo", Json::obj(vec![("name", Json::str("hover")), ("version", Json::str("1.0"))])),
        ]));
        // Its driver comes up a few seconds after the desktop says it has started.
        let until = Instant::now() + Duration::from_secs(60);
        let session = loop {
            match self.post(&link, &self.request("initialize", init.clone()), Duration::from_secs(30)) {
                Ok((replies, sid)) => {
                    if let Some(e) = replies.last().and_then(|r| r.get("error")) { *self.why.lock().unwrap() = text(e.get("message")).unwrap_or("").to_owned(); return None; }
                    break sid;
                }
                Err(Gone::Net(_)) if Instant::now() < until => std::thread::sleep(Duration::from_millis(1500)),
                Err(e) => { *self.why.lock().unwrap() = format!("{e:?}"); return None; }
            }
        };
        link.session = session;
        let _ = self.post(&link, &Json::obj(vec![("jsonrpc", Json::str("2.0")), ("method", Json::str("notifications/initialized"))]), Duration::from_secs(30));
        // Its own session, so a cursor of its own.
        let label = fresh(&self.agent);
        *self.label.lock().unwrap() = label.clone();
        let _ = self.post(&link, &self.request("tools/call", Json::obj(vec![("name", Json::str("start_session")), ("arguments", Json::obj(vec![("session", Json::str(label))]))])), Duration::from_secs(30));
        if known_tools().is_none() {
            if let Ok((r, _)) = self.post(&link, &self.request("tools/list", Json::Obj(vec![])), Duration::from_secs(60)) { remember(r.last().and_then(|r| r.get("result")).and_then(|r| r.get("tools"))); }
        }
        self.why.lock().unwrap().clear();
        *self.link.lock().unwrap() = Some(link.clone());
        Some(link)
    }

    /// A new session for the agent on the same connection.
    fn renew(&self) {
        let Some(l) = self.link.lock().unwrap().clone() else { return };
        let label = fresh(&self.agent);
        *self.label.lock().unwrap() = label.clone();
        if self.post(&l, &self.request("tools/call", Json::obj(vec![("name", Json::str("start_session")), ("arguments", Json::obj(vec![("session", Json::str(label))]))])), Duration::from_secs(30)).is_err() {
            *self.link.lock().unwrap() = None;
        }
    }

    fn notify(&self, m: &Json) {
        if let Some(l) = self.link.lock().unwrap().clone() { let _ = self.post(&l, m, Duration::from_secs(30)); }
    }

    /// Streamable HTTP: one POST per message; the answer is JSON or a short event stream,
    /// and the MCP session id comes back as a header.
    fn post(&self, l: &Link, body: &Json, timeout: Duration) -> Result<(Vec<Json>, Option<String>), Gone> {
        let mut c = Client::bare(&l.end.url).ok_or(Gone::Net("bad address".into()))?.with_header("Accept", "application/json, text/event-stream");
        for (k, v) in &l.end.headers { c = c.with_header(k, v); }
        if let Some(s) = &l.session { c = c.with_header("Mcp-Session-Id", s); }
        let res = c.open("POST", &l.end.path, Some(&body.compact()), Some(timeout), None).map_err(|e| match e { HttpErr::Timeout => Gone::Late, e => Gone::Net(format!("{e:?}")) })?;
        let status = res.status;
        let sid = res.header("Mcp-Session-Id").map(str::to_owned).or_else(|| l.session.clone());
        let event_stream = res.header("Content-Type").is_some_and(|t| t.starts_with("text/event-stream"));
        let is_init = text(body.get("method")) == Some("initialize");
        // The driver forgot the session (the desktop restarted), or the token changed: connect again.
        if l.session.is_some() && !is_init && matches!(status, 400 | 404) { return Err(Gone::Session(format!("session ended ({status})"))); }
        if matches!(status, 401 | 403) { return Err(Gone::Session("the desktop's token changed".into())); }
        if status == 202 { return Ok((vec![], sid)); }
        let (_, text) = res.text().map_err(|e| match e { HttpErr::Timeout => Gone::Late, e => Gone::Net(format!("{e:?}")) })?;
        if !(200..300).contains(&status) { return Err(Gone::Net(format!("the desktop's driver answered {status}"))); }
        let mut replies = vec![];
        if event_stream {
            for event in text.replace('\r', "").split("\n\n") {
                let data: Vec<&str> = event.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim_start).collect();
                if !data.is_empty() { if let Ok(v) = json::parse(&data.join("\n")) { replies.push(v); } }
            }
        } else if !text.trim().is_empty() {
            replies.push(json::parse(&text).map_err(|e| Gone::Net(e.0))?);
        }
        Ok((replies, sid))
    }
}

/// The driver's answer under the agent's own id; an error reaching the desktop reads as
/// the tool failing, which the agent can act on.
fn relabel(mut reply: Json, id: &Json, as_tool: bool) -> Json {
    if as_tool {
        if let Some(e) = reply.get("error") {
            if matches!(e.get("code"), Some(Json::Num(n)) if n == "-32000") { return tool_error(id, text(e.get("message")).unwrap_or("The desktop didn't answer.")); }
        }
    }
    put(&mut reply, "id", id.clone());
    reply
}

/// Reads the driver's tool list from a desktop that is up (Setup's first one), so the
/// agents' first connection doesn't wait for one to start.
pub fn prefetch(end: &DriverEndpoint) {
    let d = Driver::new(|| None, "hover-setup");
    let link = Link { folder: String::new(), end: end.clone(), session: None };
    let init = Json::obj(vec![("protocolVersion", Json::str("2025-06-18")), ("capabilities", Json::Obj(vec![])), ("clientInfo", Json::obj(vec![("name", Json::str("hover")), ("version", Json::str("1.0"))]))]);
    let Ok((_, sid)) = d.post(&link, &d.request("initialize", init), Duration::from_secs(45)) else { return };
    let link = Link { session: sid, ..link };
    let _ = d.post(&link, &Json::obj(vec![("jsonrpc", Json::str("2.0")), ("method", Json::str("notifications/initialized"))]), Duration::from_secs(30));
    if let Ok((r, _)) = d.post(&link, &d.request("tools/list", Json::Obj(vec![])), Duration::from_secs(45)) { remember(r.last().and_then(|r| r.get("result")).and_then(|r| r.get("tools"))); }
}
