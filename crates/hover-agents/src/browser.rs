//! Services/BrowserTool.cs: Hover's built-in browser, handed to every session as an MCP
//! server, as T3 Code hands its agents their preview tools: the agent opens pages, reads
//! them, clicks, types and takes screenshots in a browser the host owns (a WKWebView per
//! session on a Mac), and the user watches the same page in the desk's Browser panel.
//!
//! The agent's tool starts the MCP server itself, inside its sandbox: a small relay
//! (perl, as Cua's guard) that joins its stdio to one Unix socket Hover listens on,
//! sending the session's token first. Hover answers MCP here (initialize, tools/list,
//! tools/call) and passes each call to the host (the `Host` trait), which drives the
//! session's browser and answers with text or a screenshot. The browser has no cookies of
//! the user's (its own, non-persistent store), and opens http(s) pages only. Each call is
//! an MCP tool call under the session's access, so Ask first asks about it and Read only
//! turns it down.
//!
//! Only a host that has a browser (the Mac app) sets one (`set_host`), and only a Mac
//! lists the server for an agent (`supported`): elsewhere `servers` is empty, with a note
//! for the switch. The MCP logic is OS-free and tested everywhere; the socket is Unix's.
//!
//! Unlike the C# relay, the token comes to the relay in its environment (the MCP
//! server's env), not on its command line.
//!
//! The same socket and relay reach other MCP servers Hover runs itself for a session
//! (`bridge`, under a token of its own): a project's Cua Space is one (spaces.rs).

use crate::agents::toggles;
use crate::computer_use::McpServer;
use hover_core::json::Json;
use hover_core::model::AgentTool;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const SERVER_NAME: &str = "hover-browser";
/// What Settings shows beside the switch where there is no browser to hand out.
pub const UNSUPPORTED: &str = "Agent browser needs macOS.";

/// Only the Mac app has a browser to drive.
pub fn supported() -> bool { cfg!(target_os = "macos") }

/// Why the switch is disabled here, or none.
pub fn note() -> Option<&'static str> { (!supported()).then_some(UNSUPPORTED) }

/// What the host's browser answered: whether it worked, text to say, and perhaps a
/// screenshot (base64 of the image, and its type; JPEG when none is said).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply { pub ok: bool, pub text: String, pub image: Option<String>, pub mime: Option<String> }

impl Reply {
    pub fn text(ok: bool, text: &str) -> Reply { Reply { ok, text: text.into(), ..Default::default() } }
}

/// The browser the host drives. A session is named by its key (the tag the agent's relay
/// was made for); OpenCode has one server for all its sessions, so its calls come with
/// the tag "opencode" and the host picks the session at work.
pub trait Host: Send + Sync {
    /// Whether the session has a browser now; a call for one that hasn't is answered
    /// here ("isn't open for this session right now").
    fn has_session(&self, _session: &str) -> bool { true }

    /// One tool call: `op` is its name without "browser_" (open, snapshot, click, type,
    /// press, scroll, screenshot, evaluate, wait, console, back, reload), `args` its
    /// arguments. Blocks until the browser has done it, on a thread of its own per call,
    /// so a slow page doesn't hold up a ping.
    fn call(&self, session: &str, op: &str, args: &Json) -> Reply;
}

static HOST: RwLock<Option<Arc<dyn Host>>> = RwLock::new(None);

/// The host can drive a browser (the Mac app says so by handing one in).
pub fn set_host(host: Box<dyn Host>) { *HOST.write().unwrap() = Some(Arc::from(host)); }

/// The host's browser is gone (Hover quits).
pub fn clear_host() { *HOST.write().unwrap() = None; }

fn host() -> Option<Arc<dyn Host>> { HOST.read().unwrap().clone() }

/// A browser can be handed to agents: a Mac, with the host's browser set.
pub fn available() -> bool { supported() && host().is_some() }

// MARK: MCP

pub const INSTRUCTIONS: &str =
    "Hover's built-in browser. Use it whenever you need to see a web page: the local dev server you started, a site \
you are building or testing, or documentation. The user watches the same browser in Hover, so prefer it over curl \
for pages and over computer use for anything in a browser. Open a page with browser_open, read it with \
browser_snapshot (interactive elements get [ref] numbers), act with browser_click, browser_type and browser_press \
using those refs, and check the result with browser_screenshot and browser_console. Take a new snapshot after the \
page changes: refs belong to the last snapshot. It has no cookies or sign-ins of the user's.";

fn prop(kind: &str, description: &str) -> Json { Json::obj(vec![("type", Json::str(kind)), ("description", Json::str(description))]) }

fn tool(name: &str, description: &str, properties: Vec<(&str, Json)>, required: &[&str]) -> Json {
    Json::obj(vec![
        ("name", Json::str(name)), ("description", Json::str(description)),
        ("inputSchema", Json::obj(vec![
            ("type", Json::str("object")), ("properties", Json::obj(properties)),
            ("required", Json::Arr(required.iter().map(|r| Json::str(*r)).collect())), ("additionalProperties", Json::Bool(false)),
        ])),
    ])
}

/// The tools the server lists (tools/list).
pub fn tools() -> Json {
    let target = || prop("integer", "The element's [ref] number from the last browser_snapshot.");
    let selector = || prop("string", "A CSS selector, when there is no ref.");
    let text = || prop("string", "Visible text or label of the element, when there is no ref or selector.");
    let plain = |kind: &str| Json::obj(vec![("type", Json::str(kind))]);
    Json::Arr(vec![
        tool("browser_open", "Open a URL in Hover's browser (http or https; \"localhost:3000\" works) and wait for it to load. Returns the title, the final URL and the HTTP status.",
            vec![("url", prop("string", "The address to open."))], &["url"]),
        tool("browser_snapshot", "Read the open page as text: its title, URL, headings, text and every interactive element with a [ref] number to act on.",
            vec![("max_chars", prop("integer", "Longest answer (default 12000)."))], &[]),
        tool("browser_click", "Click an element on the page, then wait for any navigation it starts.",
            vec![("ref", target()), ("selector", selector()), ("text", text())], &[]),
        tool("browser_type", "Type into a text field (replacing what is there unless append is true), optionally submitting its form.",
            vec![("ref", target()), ("selector", selector()), ("label", text()), ("text", prop("string", "What to type.")), ("append", plain("boolean")),
                ("submit", prop("boolean", "Press Enter / submit the form afterwards."))], &["text"]),
        tool("browser_press", "Press a key in the focused element: Enter, Escape, Tab, ArrowDown, ArrowUp, Backspace, or a character.",
            vec![("key", plain("string"))], &["key"]),
        tool("browser_scroll", "Scroll the page by a number of pixels, to its top or bottom, or to an element.",
            vec![("ref", target()), ("dy", prop("integer", "Pixels down (negative: up). Default 600.")),
                ("to", Json::obj(vec![("type", Json::str("string")), ("enum", Json::Arr(vec![Json::str("top"), Json::str("bottom")]))]))], &[]),
        tool("browser_screenshot", "A screenshot of what the page shows now, as an image.", vec![], &[]),
        tool("browser_evaluate", "Run JavaScript in the page and return its result as JSON. The script is a function body: use return.",
            vec![("script", plain("string"))], &["script"]),
        tool("browser_wait", "Wait until some text or an element is on the page, up to a timeout.",
            vec![("text", plain("string")), ("selector", selector()), ("timeout_ms", prop("integer", "Default 5000, at most 20000."))], &[]),
        tool("browser_console", "The page's console messages and errors since it loaded (newest last).",
            vec![("clear", prop("boolean", "Empty the log afterwards."))], &[]),
        tool("browser_back", "Go back to the previous page.", vec![], &[]),
        tool("browser_reload", "Reload the page and wait for it to load.", vec![], &[]),
    ])
}

/// How long a call waits for the host; waiting for the page takes less.
#[derive(Clone, Copy, Debug)]
pub struct Limits { pub call: Duration, pub wait: Duration }

impl Default for Limits {
    fn default() -> Self { Limits { call: Duration::from_secs(90), wait: Duration::from_secs(40) } }
}

fn ok(id: &Json, result: Json) -> Json { Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("result", result)]) }
fn fail(id: &Json, code: i64, message: &str) -> Json {
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("error", Json::obj(vec![("code", Json::int(code)), ("message", Json::str(message))]))])
}
fn said(text: &str, error: bool) -> Json {
    Json::obj(vec![("content", Json::Arr(vec![Json::obj(vec![("type", Json::str("text")), ("text", Json::str(text))])])), ("isError", Json::Bool(error))])
}

/// One message from the agent, for the session `session` (the tag its relay was made
/// for): the reply to send, or none for a notification. Blocks for a tools/call until the
/// host has answered.
pub fn answer(session: &str, m: &Json) -> Option<Json> { answer_within(session, m, &Limits::default()) }

/// answer, with the waits given.
pub fn answer_within(session: &str, m: &Json, limits: &Limits) -> Option<Json> {
    if !matches!(m, Json::Obj(_)) { return None; }
    let id = m.get("id")?;
    let method = m.get("method").and_then(Json::as_str);
    let params = m.get("params");
    match method {
        Some("initialize") => {
            let version = params.and_then(|p| p.get("protocolVersion")).and_then(Json::as_str).unwrap_or("2025-06-18");
            Some(ok(id, Json::obj(vec![
                ("protocolVersion", Json::str(version)),
                ("capabilities", Json::obj(vec![("tools", Json::obj(vec![("listChanged", Json::Bool(false))]))])),
                ("serverInfo", Json::obj(vec![("name", Json::str(SERVER_NAME)), ("title", Json::str("Hover browser")), ("version", Json::str("1.0"))])),
                ("instructions", Json::str(INSTRUCTIONS)),
            ])))
        }
        Some("ping") => Some(ok(id, Json::obj(vec![]))),
        Some("tools/list") => Some(ok(id, Json::obj(vec![("tools", tools())]))),
        Some("tools/call") => {
            let name = params.and_then(|p| p.get("name")).and_then(Json::as_str).unwrap_or("");
            let args = match params.and_then(|p| p.get("arguments")) { Some(a @ Json::Obj(_)) => a.clone(), _ => Json::obj(vec![]) };
            if !is_tool(name) { return Some(fail(id, -32602, &format!("Unknown tool {name}."))); }
            Some(ok(id, call(session, name, &args, limits)))
        }
        other => Some(fail(id, -32601, &format!("Method {} isn't supported.", other.unwrap_or("")))),
    }
}

fn is_tool(name: &str) -> bool {
    matches!(tools(), Json::Arr(all) if all.iter().any(|t| t.get("name").and_then(Json::as_str) == Some(name)))
}

/// A tool call, driven by the host in the session's browser.
fn call(session: &str, name: &str, args: &Json, limits: &Limits) -> Json {
    let Some(host) = host() else { return said("Hover's browser isn't available here.", true) };
    if !host.has_session(session) { return said("Hover's browser isn't open for this session right now.", true); }
    let op = name.strip_prefix("browser_").unwrap_or(name).to_owned();
    let (tx, rx) = mpsc::channel();
    let (s, a, o) = (session.to_owned(), args.clone(), op.clone());
    std::thread::Builder::new().name("browser-call".into()).spawn(move || { let _ = tx.send(host.call(&s, &o, &a)); }).expect("a thread for the browser call");
    match rx.recv_timeout(if op == "wait" { limits.wait } else { limits.call }) {
        Ok(r) => result(&r),
        Err(mpsc::RecvTimeoutError::Timeout) => said("The browser didn’t answer in time.", true),
        Err(mpsc::RecvTimeoutError::Disconnected) => said("The browser stopped before it answered.", true),
    }
}

/// The host's answer as MCP content: its text, and a screenshot when it took one.
pub fn result(r: &Reply) -> Json {
    let mut content = vec![];
    if !r.text.is_empty() || r.image.is_none() {
        let text = if !r.text.is_empty() { r.text.as_str() } else if r.ok { "Done." } else { "That didn’t work." };
        content.push(Json::obj(vec![("type", Json::str("text")), ("text", Json::str(text))]));
    }
    if let Some(data) = &r.image {
        content.push(Json::obj(vec![("type", Json::str("image")), ("data", Json::str(data.as_str())), ("mimeType", Json::str(r.mime.as_deref().unwrap_or("image/jpeg")))]));
    }
    Json::obj(vec![("content", Json::Arr(content)), ("isError", Json::Bool(!r.ok))])
}

// MARK: Steps

const OPS: [&str; 12] = ["open", "snapshot", "click", "type", "press", "scroll", "screenshot", "evaluate", "wait", "console", "back", "reload"];

/// The browser tool a step called ("open", "click"…), or none when it isn't one: titles
/// name it as "hover-browser/browser_open", "mcp__hover-browser__browser_click" or plain.
pub fn op_of(title: Option<&str>) -> Option<&'static str> {
    let t = title?.to_lowercase();
    for (at, _) in t.match_indices("browser_") {
        if t[..at].chars().next_back().is_some_and(|c| c.is_ascii_lowercase()) { continue; }
        let rest = &t[at + "browser_".len()..];
        for op in OPS {
            if let Some(after) = rest.strip_prefix(op) {
                if after.chars().next().is_none_or(|c| !(c.is_ascii_lowercase() || c == '_')) { return Some(op); }
            }
        }
    }
    None
}

// MARK: The relay and the socket

#[cfg(unix)]
const PERL: &str = "/usr/bin/perl";

/// Where the relay is written: Hover's own folder, readable but not writable to the
/// sandboxed tool, so it can't change what it runs.
pub fn dir() -> PathBuf { hover_core::paths::support().join("browser") }

/// Where the socket is: a short path (Unix sockets take 104 bytes) in srt's temp folder,
/// the place sandboxed tools may connect to sockets, in a folder only the user can open.
pub fn socket_path() -> PathBuf {
    if let Some(s) = std::env::var_os("HOVER_BROWSER_SOCKET").filter(|s| !s.is_empty()) { return PathBuf::from(s); }
    let who = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let mut user: String = who.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).collect();
    user.truncate(16);
    crate::sandbox::temp_root().join(format!("hover-browser-{user}")).join("b.sock")
}

/// The tool's MCP command joins its stdio to the socket, after its token line (from the
/// environment, HOVER_BROWSER_TOKEN; the second argument serves a caller that has none).
pub const RELAY: &str = r###"#!/usr/bin/perl
# Hover's built-in browser for agents: joins this MCP server's stdio to Hover's socket.
# See hover-agents/src/browser.rs in Hover's source.
use strict; use warnings;
use IO::Socket::UNIX;
use IO::Select;
use POSIX qw(EAGAIN EINTR);
die "usage: relay.pl socket\n" unless @ARGV >= 1;
my $token = $ENV{HOVER_BROWSER_TOKEN} // $ARGV[1] // die "Hover's browser token is missing\n";
my $s = IO::Socket::UNIX->new(Type => SOCK_STREAM(), Peer => $ARGV[0]) or die "Hover's browser isn't reachable ($ARGV[0]): $!\n";
$SIG{PIPE} = 'IGNORE';
sub put {
    my ($fh, $b) = @_;
    while (length $b) {
        my $n = syswrite($fh, $b);
        if (!defined $n) { return 0 if $! != EAGAIN && $! != EINTR; IO::Select->new($fh)->can_write(1); next; }
        substr($b, 0, $n) = '';
    }
    return 1;
}
put($s, "HELLO $token\n") or exit 1;
my $sel = IO::Select->new(\*STDIN, $s);
my $parent = getppid();
while ($sel->count) {
    exit 0 if getppid() != $parent;
    for my $fh ($sel->can_read(1)) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        exit 0 if $n == 0;
        put($fh == $s ? \*STDOUT : $s, $chunk) or exit 1;
    }
}
"###;

#[derive(Default)]
#[cfg_attr(not(unix), allow(dead_code))]
struct Registry {
    by_tag: std::collections::HashMap<String, String>,
    by_token: std::collections::HashMap<String, String>,
    /// Other MCP servers Hover runs itself (see `bridge`): a token to the name it was made
    /// for and the code that serves it, and the name back to its token.
    bridges: std::collections::HashMap<String, (String, Bridged)>,
    bridge_tokens: std::collections::HashMap<String, String>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn token_bytes() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system has no randomness");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The token the relay of a session's tool sends first; the same for the same tag.
pub fn register(tag: &str) -> String {
    let mut g = REGISTRY.lock().unwrap();
    let r = g.get_or_insert_with(Registry::default);
    if let Some(t) = r.by_tag.get(tag) { return t.clone(); }
    let token = token_bytes();
    r.by_tag.insert(tag.into(), token.clone());
    r.by_token.insert(token.clone(), tag.into());
    token
}

/// What serves a bridged MCP server once its relay has said hello: the name it was made
/// for, what the agent writes (its lines) and where to write back. Runs on a thread of
/// its own and returns when the agent's side is done.
pub type Bridged = Arc<dyn Fn(&str, Box<dyn std::io::BufRead + Send>, Box<dyn std::io::Write + Send>) + Send + Sync>;

/// Another MCP server Hover runs itself for a session (a Cua Space's `cua mcp`), reached by
/// the sandboxed tool over the same relay and socket as the browser's, under a token of its
/// own that stands for `name`. None where there is no socket or no perl (not a Unix).
#[cfg(not(unix))]
pub fn bridge(_name: &str, _server: &str, _run: Bridged) -> Vec<McpServer> { vec![] }

/// The bridge registered under a token, if there is one.
#[cfg(unix)]
fn bridge_of(token: &str) -> Option<(String, Bridged)> { REGISTRY.lock().unwrap().as_ref().and_then(|r| r.bridges.get(token).cloned()) }

#[cfg(unix)]
pub fn bridge(name: &str, server: &str, run: Bridged) -> Vec<McpServer> {
    if !std::path::Path::new(PERL).is_file() { return vec![]; }
    match unix::start() {
        Ok(relay) => {
            let token = {
                let mut g = REGISTRY.lock().unwrap();
                let r = g.get_or_insert_with(Registry::default);
                let token = r.bridge_tokens.entry(name.into()).or_insert_with(token_bytes).clone();
                r.bridges.insert(token.clone(), (name.into(), run));
                token
            };
            let mut s = McpServer::new(server, PERL, &[&relay.to_string_lossy(), &socket_path().to_string_lossy()]);
            s.env.push(("HOVER_BROWSER_TOKEN".into(), token));
            vec![s]
        }
        Err(e) => { hover_core::log::line(&format!("bridge: couldn't listen - {e}")); vec![] }
    }
}


/// The tag a token was made for.
#[cfg(unix)]
fn tag_of(token: &str) -> Option<String> { REGISTRY.lock().unwrap().as_ref().and_then(|r| r.by_token.get(token).cloned()) }

/// The browser's MCP server for a session's tool, or none (no host browser, switched off
/// in Settings, not a Unix, or no perl). The tag names the session (its key); OpenCode's
/// one server for all its sessions passes "opencode".
pub fn servers(_tool: AgentTool, tag: Option<&str>) -> Vec<McpServer> {
    let Some(tag) = tag.filter(|t| !t.is_empty()) else { return vec![] };
    if !available() || !toggles().agent_browser { return vec![]; }
    served(tag)
}

#[cfg(unix)]
fn served(tag: &str) -> Vec<McpServer> {
    if !std::path::Path::new(PERL).is_file() { return vec![]; }
    match unix::start() {
        Ok(relay) => {
            let mut s = McpServer::new(SERVER_NAME, PERL, &[&relay.to_string_lossy(), &socket_path().to_string_lossy()]);
            s.env.push(("HOVER_BROWSER_TOKEN".into(), register(tag)));
            vec![s]
        }
        Err(e) => { hover_core::log::line(&format!("browser: couldn't listen - {e}")); vec![] }
    }
}

#[cfg(not(unix))]
fn served(_tag: &str) -> Vec<McpServer> { vec![] }

/// Starts listening (if not already) and writes the relay; where the relay is.
#[cfg(unix)]
pub fn listen() -> std::io::Result<PathBuf> { unix::start() }

/// Stops listening (Hover quits).
pub fn stop() {
    #[cfg(unix)]
    unix::stop();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Listening { path: PathBuf, stopping: Arc<AtomicBool> }

    static LISTENING: Mutex<Option<Listening>> = Mutex::new(None);

    /// Longest line taken from the agent.
    const LIMIT: u64 = 4 * 1024 * 1024;

    /// Listening at socket_path(), and the relay written: its path.
    pub fn start() -> std::io::Result<PathBuf> {
        let mut g = LISTENING.lock().unwrap();
        let relay = write_relay()?;
        if g.as_ref().is_some_and(|l| l.path == socket_path()) { return Ok(relay); }
        let path = socket_path();
        let dir = path.parent().ok_or_else(|| std::io::Error::other("the socket has no folder"))?;
        // Only the folder Hover makes for it is locked down; one named by
        // HOVER_BROWSER_SOCKET (a test's) is somebody's own, maybe /tmp.
        if std::env::var_os("HOVER_BROWSER_SOCKET").is_some_and(|s| !s.is_empty()) { std::fs::create_dir_all(dir)?; } else { crate::sandbox::private_dir(dir)?; }
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let stopping = Arc::new(AtomicBool::new(false));
        let flag = stopping.clone();
        std::thread::Builder::new().name("browser-accept".into()).spawn(move || {
            for client in listener.incoming() {
                if flag.load(Ordering::SeqCst) { return; }
                let Ok(client) = client else { continue };
                let _ = std::thread::Builder::new().name("browser-serve".into()).spawn(move || serve(client));
            }
        })?;
        hover_core::log::line(&format!("browser: listening at {}", path.display()));
        *g = Some(Listening { path, stopping });
        Ok(relay)
    }

    pub fn stop() {
        let Some(l) = LISTENING.lock().unwrap().take() else { return };
        l.stopping.store(true, Ordering::SeqCst);
        // The accept is waiting: a connection of our own wakes it to see the flag.
        let _ = UnixStream::connect(&l.path);
        let _ = std::fs::remove_file(&l.path);
    }

    fn write_relay() -> std::io::Result<PathBuf> {
        let dir = dir();
        std::fs::create_dir_all(&dir)?;
        let relay = dir.join("relay.pl");
        if std::fs::read_to_string(&relay).ok().as_deref() != Some(RELAY) { std::fs::write(&relay, RELAY)?; }
        Ok(relay)
    }

    /// One line, or none at its end; a line over the limit is read to its end and skipped
    /// (an empty one is returned).
    fn line(r: &mut impl BufRead) -> std::io::Result<Option<String>> {
        let mut buf = Vec::new();
        let n = r.by_ref().take(LIMIT).read_until(b'\n', &mut buf)?;
        if n == 0 { return Ok(None); }
        if buf.last() != Some(&b'\n') && n as u64 == LIMIT {
            let mut rest = Vec::new();
            loop {
                rest.clear();
                let k = r.by_ref().take(LIMIT).read_until(b'\n', &mut rest)?;
                if k == 0 || rest.last() == Some(&b'\n') { break; }
            }
            return Ok(Some(String::new()));
        }
        Ok(Some(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_owned()))
    }

    fn serve(client: UnixStream) {
        let _ = client.set_read_timeout(Some(Duration::from_secs(10)));
        let Ok(write) = client.try_clone() else { return };
        let mut reader = BufReader::new(client);
        let Ok(Some(hello)) = line(&mut reader) else { return };
        let Some(token) = hello.strip_prefix("HELLO ").map(str::trim) else { return };
        // Another server Hover runs for the session: the rest of the connection is its.
        if let Some((name, run)) = bridge_of(token) {
            let _ = reader.get_ref().set_read_timeout(None);
            run(&name, Box::new(reader), Box::new(write));
            return;
        }
        let Some(tag) = tag_of(token) else { return };
        let _ = reader.get_ref().set_read_timeout(None);
        let write = Arc::new(Mutex::new(write));
        while let Ok(Some(text)) = line(&mut reader) {
            if text.is_empty() { continue; }
            let Ok(m) = hover_core::json::parse(&text) else { continue };
            // Calls run side by side: a slow page doesn't hold up a ping.
            let (tag, write) = (tag.clone(), write.clone());
            let _ = std::thread::Builder::new().name("browser-answer".into()).spawn(move || {
                if let Some(reply) = answer(&tag, &m) {
                    let mut bytes = reply.compact().into_bytes();
                    bytes.push(b'\n');
                    let mut w = write.lock().unwrap();
                    let _ = w.write_all(&bytes).and_then(|_| w.flush());
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relay_is_plain_lf_text() {
        assert!(!RELAY.contains('\r') && RELAY.starts_with("#!/usr/bin/perl\n"));
    }

    #[test]
    fn steps_name_the_browser_tool_whatever_the_agent_calls_it() {
        assert_eq!(op_of(Some("hover-browser/browser_open")), Some("open"));
        assert_eq!(op_of(Some("mcp__hover-browser__browser_click")), Some("click"));
        assert_eq!(op_of(Some("browser_screenshot")), Some("screenshot"));
        assert_eq!(op_of(Some("Browser_Reload now")), Some("reload"));
        assert_eq!(op_of(Some("Ran npm test")), None);
        assert_eq!(op_of(Some("browser_prepare")), None);
        assert_eq!(op_of(Some("mybrowser_open")), None);
        assert_eq!(op_of(Some("browser_opener")), None);
        assert_eq!(op_of(None), None);
    }
}
