//! ComputerUseTests (tests/Hover.Tests/ComputerUseTests.cs), ported: which servers a
//! session gets, how they are written for ACP, OpenCode and Claude Code, how cua-driver's
//! permission report is read, and the servers reaching a real host's sessions. Nothing of
//! Cua is run: cua-driver is an empty stand-in file on a PATH of its own.

use hover_agents::acp::AcpHost;
use hover_agents::agents::{self, Toggles};
use hover_agents::cancel::Cancel;
use hover_agents::claude::{ClaudeHost, Timeouts};
use hover_agents::computer_use::{self, McpServer, Status};
use hover_agents::proc::Link;
use hover_core::json::{self, Json};
use hover_core::model::{AgentOptions, AgentTool, KiroState};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The settings and PATH are the process's: one test at a time.
static LOCK: Mutex<()> = Mutex::new(());
static ON: AtomicBool = AtomicBool::new(false);

struct Env { _g: MutexGuard<'static, ()>, dir: std::path::PathBuf, path: Option<std::ffi::OsString> }

impl Env {
    /// A PATH of its own (empty but for what a test puts there), computer use off.
    fn new(name: &str) -> Env {
        let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("hover-cua-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::var_os("PATH");
        std::env::set_var("PATH", &dir);
        ON.store(false, Ordering::SeqCst);
        agents::set_toggles(|| Toggles { computer_use: ON.load(Ordering::SeqCst), ..Toggles::default() });
        Env { _g: g, dir, path }
    }

    fn driver(&self) -> String {
        let exe = self.dir.join(if cfg!(windows) { "cua-driver.exe" } else { "cua-driver" });
        std::fs::write(&exe, "").unwrap();
        exe.to_string_lossy().into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        match &self.path { Some(p) => std::env::set_var("PATH", p), None => std::env::remove_var("PATH") }
        agents::set_toggles(Toggles::default);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn cua(exe: &str) -> McpServer { McpServer::new("cua-driver", exe, &["mcp"]) }

#[test]
fn off_by_default_and_off_means_no_servers() {
    let e = Env::new("off");
    e.driver();
    assert!(computer_use::servers().is_empty());
}

#[test]
fn on_hands_out_cua_drivers_mcp_command_from_path() {
    let e = Env::new("on");
    let exe = e.driver();
    ON.store(true, Ordering::SeqCst);
    let servers = computer_use::servers();
    if !computer_use::supported() {
        // The setting and a cua-driver on PATH change nothing where Cua isn't offered.
        assert!(servers.is_empty());
        return;
    }
    assert_eq!(servers.len(), 1);
    let s = &servers[0];
    assert_eq!(s.name, "cua-driver");
    if !std::path::Path::new("/usr/bin/perl").is_file() {
        assert_eq!(s.command, exe);
        assert_eq!(s.args, vec!["mcp"]);
        return;
    }
    // Behind the guard, written to Hover's own folder; still no approval bypass.
    let guard = computer_use::guard_dir().join("guard.pl");
    assert_eq!(s.command, "/usr/bin/perl");
    assert_eq!(s.args, vec![guard.to_string_lossy().into_owned(), exe, "mcp".to_owned()]);
    assert_eq!(std::fs::read_to_string(&guard).unwrap(), computer_use::GUARD);
}

#[test]
fn never_cuas_approval_bypass() {
    let plain = computer_use::server_for("/x/cua-driver", None);
    let guarded = computer_use::server_for("/x/cua-driver", Some("/d/guard.pl"));
    assert_eq!(plain.args, vec!["mcp"]);
    assert_eq!((guarded.command.as_str(), guarded.args.as_slice()), ("/usr/bin/perl", &["/d/guard.pl".to_owned(), "/x/cua-driver".to_owned(), "mcp".to_owned()][..]));
    for s in [plain, guarded] {
        assert!(s.args.iter().all(|a| !a.starts_with('-')), "no flags at all: {:?}", s.args);
        assert!(s.env.is_empty());
    }
}

#[test]
fn not_installed_is_no_server_even_when_on() {
    // CuaDriver.app in /Applications is found wherever PATH and HOME point: it is installed.
    if cfg!(target_os = "macos") && std::path::Path::new("/Applications/CuaDriver.app/Contents/MacOS/cua-driver").is_file() {
        eprintln!("skipped: Cua Driver is installed on this Mac"); return;
    }
    let _e = Env::new("none");
    ON.store(true, Ordering::SeqCst);
    assert!(computer_use::servers().is_empty());
}

#[test]
fn the_acp_entry_is_a_stdio_server_with_an_env_list() {
    assert_eq!(computer_use::acp(&[cua("/x/cua-driver")]).compact(), r#"[{"name":"cua-driver","command":"/x/cua-driver","args":["mcp"],"env":[]}]"#);
    let mut b = McpServer::new("hover-browser", "/usr/bin/perl", &["/r.pl", "/s.sock"]);
    b.env.push(("HOVER_BROWSER_TOKEN".into(), "t0k".into()));
    assert_eq!(computer_use::acp(&[b]).compact(),
        r#"[{"name":"hover-browser","command":"/usr/bin/perl","args":["/r.pl","/s.sock"],"env":[{"name":"HOVER_BROWSER_TOKEN","value":"t0k"}]}]"#);
    assert_eq!(computer_use::acp(&[]).compact(), "[]");
}

#[test]
fn opencodes_inline_config_adds_a_local_server_and_keeps_what_was_there() {
    let servers = [cua("/x/cua-driver")];
    assert_eq!(computer_use::opencode_config(&[], Some(r#"{"a":1}"#)).as_deref(), Some(r#"{"a":1}"#), "nothing to add: left alone");
    assert_eq!(computer_use::opencode_config(&[], None), None);
    let text = computer_use::opencode_config(&servers, Some(r#"{"model":"p/m","mcp":{"mine":{"type":"remote","url":"https://x"}}}"#)).unwrap();
    let v = json::parse(&text).unwrap();
    let mcp = v.get("mcp").unwrap();
    let c = mcp.get("cua-driver").unwrap();
    assert_eq!(v.get("model").and_then(Json::as_str), Some("p/m"));
    assert!(mcp.get("mine").is_some(), "the user's own server stays");
    assert_eq!(c.get("type").and_then(Json::as_str), Some("local"));
    assert_eq!(c.get("command").unwrap().items().unwrap().iter().filter_map(Json::as_str).collect::<Vec<_>>(), vec!["/x/cua-driver", "mcp"]);
    assert_eq!(c.get("enabled"), Some(&Json::Bool(true)));
    assert_eq!(c.get("timeout"), Some(&Json::int(30000)));
    assert!(c.get("environment").is_none());
    // A config that isn't JSON is replaced rather than breaking the start.
    assert!(computer_use::opencode_config(&servers, Some("not json")).unwrap().contains("\"cua-driver\""));
    // An environment goes as OpenCode names it.
    let mut b = McpServer::new("hover-browser", "/usr/bin/perl", &["/r.pl"]);
    b.env.push(("HOVER_BROWSER_TOKEN".into(), "t0k".into()));
    let v = json::parse(&computer_use::opencode_config(&[b], None).unwrap()).unwrap();
    assert_eq!(v.get("mcp").unwrap().get("hover-browser").unwrap().get("environment").unwrap().get("HOVER_BROWSER_TOKEN").and_then(Json::as_str), Some("t0k"));
}

#[test]
fn claude_codes_mcp_config_names_each_server_and_its_env() {
    assert_eq!(computer_use::claude_config(&[]), None);
    let mut b = McpServer::new("hover-browser", "/usr/bin/perl", &["/r.pl", "/s.sock"]);
    b.env.push(("HOVER_BROWSER_TOKEN".into(), "t0k".into()));
    let v = json::parse(&computer_use::claude_config(&[cua("/x/cua-driver"), b]).unwrap()).unwrap();
    let all = v.get("mcpServers").unwrap();
    let c = all.get("cua-driver").unwrap();
    assert_eq!(c.get("type").and_then(Json::as_str), Some("stdio"));
    assert_eq!(c.get("command").and_then(Json::as_str), Some("/x/cua-driver"));
    assert_eq!(c.get("args").unwrap().items().unwrap().len(), 1);
    assert_eq!(all.get("hover-browser").unwrap().get("env").unwrap().get("HOVER_BROWSER_TOKEN").and_then(Json::as_str), Some("t0k"));
}

#[test]
fn a_change_in_servers_changes_the_signature() {
    let a = computer_use::signature(&[]);
    let b = computer_use::signature(&[cua("/x")]);
    assert_ne!(a, b);
    assert_eq!(b, computer_use::signature(&[cua("/x")]));
    let mut with_env = cua("/x");
    with_env.env.push(("K".into(), "v".into()));
    assert_ne!(b, computer_use::signature(&[with_env]));
}

#[test]
fn permission_reports_are_read_only_when_cua_driver_vouches_for_them() {
    // As cua-driver 0.31 prints them: no booleans at all when it can't say.
    assert_eq!(computer_use::parse_permissions("{\n  \"daemon_running\": true,\n  \"reason\": \"…\",\n  \"status\": \"unknown\"\n}"), None);
    assert_eq!(computer_use::parse_permissions(r#"{"accessibility":true,"screen_recording":true,"source":{"attribution":"driver-daemon"}}"#), Some((true, true)));
    assert_eq!(computer_use::parse_permissions("note: proxying\n{\"accessibility\":true,\"screen_recording\":false}"), Some((true, false)));
    assert_eq!(computer_use::parse_permissions(r#"{"accessibility":false}"#), Some((false, false)));
    assert_eq!(computer_use::parse_permissions("garbage"), None);
}

#[test]
fn ready_needs_it_installed_and_at_least_accessibility() {
    let s = |installed, permissions| Status { installed, version: "1".into(), permissions, hint: String::new() };
    assert!(s(true, "granted").ready() && s(true, "partial").ready());
    assert!(!s(true, "missing").ready() && !s(true, "unknown").ready());
    assert!(!s(false, "granted").ready());
}

#[test]
fn where_it_runs_and_what_is_said() {
    assert_eq!(computer_use::supported(), cfg!(target_os = "macos"));
    assert_eq!(computer_use::UNSUPPORTED, "Computer use needs macOS.");
    assert_eq!(computer_use::can_grant(), cfg!(target_os = "macos"));
    assert!(computer_use::install_hint().starts_with("Install Cua Driver: "));
}

/// Where Cua isn't offered a cua-driver on PATH is neither asked nor installed over: the
/// status says so, and Install reports the note. (On a Mac this would run the real thing.)
#[test]
fn where_it_is_not_offered_it_is_never_run_or_installed() {
    if computer_use::supported() { return; }
    let e = Env::new("unsupported");
    e.driver();
    ON.store(true, Ordering::SeqCst);
    let s = computer_use::check(true);
    assert_eq!((s.installed, s.permissions, s.hint.as_str()), (false, "unknown", computer_use::UNSUPPORTED));
    assert!(!s.ready() && s.version.is_empty());
    computer_use::install();
    assert_eq!(computer_use::setup().error.as_deref(), Some(computer_use::UNSUPPORTED));
    assert!(!computer_use::busy());
}

/// The guard against a stand-in cua-driver that echoes what reaches it: foreground becomes
/// background, input with no app or on the desktop and the tools that take over the user's
/// screen are answered by the guard and never reach the driver, and the tool list and
/// initialize answer are fixed on the way back.
#[cfg(unix)]
#[test]
fn the_guard_keeps_computer_use_in_the_background() {
    use std::process::{Command, Stdio};
    let have = |p: &str| std::path::Path::new(p).is_file();
    let python = ["/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3"].into_iter().find(|p| have(p));
    let (true, Some(python)) = (have("/usr/bin/perl"), python) else { eprintln!("needs perl and python3"); return };
    let dir = std::env::temp_dir().join(format!("hover-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let guard = dir.join("guard.pl");
    std::fs::write(&guard, computer_use::GUARD).unwrap();
    const FAKE: &str = r#"
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get('method') == 'initialize':
        r = {'protocolVersion': '2025-06-18', 'instructions': 'Cua.'}
    elif m.get('method') == 'tools/list':
        r = {'tools': [{'name': 'click', 'inputSchema': {'properties': {'pid': {}, 'delivery_mode': {'enum': ['background', 'foreground']}, 'scope': {'enum': ['window', 'desktop']}}}},
                       {'name': 'bring_to_front', 'inputSchema': {'properties': {}}}, {'name': 'get_window_state', 'inputSchema': {'properties': {}}}]}
    else:
        r = {'got': m['params']}
    print(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}), flush=True)
"#;
    let mut p = Command::new("/usr/bin/perl").arg(&guard).arg(python).args(["-c", FAKE]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = p.stdin.take().unwrap();
    let mut out = BufReader::new(p.stdout.take().unwrap());
    let mut ask = |line: &str| -> Json {
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut answer = String::new();
        out.read_line(&mut answer).unwrap();
        json::parse(&answer).unwrap().get("result").unwrap().clone()
    };
    let init = ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let tools = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let fg = ask(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"click","arguments":{"pid":7,"window_id":1,"x":5,"y":5,"delivery_mode":"foreground"}}}"#);
    let front = ask(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"bring_to_front","arguments":{"pid":7}}}"#);
    let nowhere = ask(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"type_text","arguments":{"text":"hi"}}}"#);
    let desk = ask(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"click","arguments":{"scope":"desktop","x":5,"y":5}}}"#);
    let pointer = ask(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"move_cursor","arguments":{"scope":"desktop","x":5,"y":5}}}"#);
    let look = ask(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"get_desktop_state","arguments":{"scope":"desktop"}}}"#);
    drop(stdin);
    let status = p.wait().unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let listed = tools.get("tools").unwrap().items().unwrap();
    let names: Vec<&str> = listed.iter().map(|t| t.get("name").and_then(Json::as_str).unwrap()).collect();
    let click = listed[0].get("inputSchema").unwrap().get("properties").unwrap();
    let instructions = init.get("instructions").and_then(Json::as_str).unwrap();
    assert!(instructions.starts_with("Cua.") && instructions.contains("background"));
    assert_eq!(names, vec!["click", "get_window_state"], "what is off isn't offered");
    assert!(click.get("delivery_mode").is_none());
    assert_eq!(click.get("scope").unwrap().get("enum").unwrap().items().unwrap(), &[Json::str("window")]);
    assert_eq!(fg.get("got").unwrap().get("arguments").unwrap().get("delivery_mode").and_then(Json::as_str), Some("background"));
    for r in [&front, &nowhere, &desk, &pointer] {
        assert_eq!(r.get("isError"), Some(&Json::Bool(true)), "answered by the guard");
        assert!(r.get("got").is_none(), "never reached the driver");
    }
    assert_eq!(look.get("got").unwrap().get("name").and_then(Json::as_str), Some("get_desktop_state"), "looking is fine");
    assert!(status.success());
}

// MARK: The servers reaching a real host's sessions

#[derive(Default)]
struct Fake { got: Mutex<Vec<(String, Json)>>, starts: AtomicUsize }

impl Fake {
    fn params(&self, method: &str) -> Vec<Json> { self.got.lock().unwrap().iter().filter(|g| g.0 == method).map(|g| g.1.clone()).collect() }

    /// An ACP agent that can load sessions, answering every call plainly.
    fn connect(self: &Arc<Self>) -> std::io::Result<Option<Link>> {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        self.starts.fetch_add(1, Ordering::SeqCst);
        let out = Arc::new(Mutex::new(Some(agent_writes)));
        let (me, o2) = (self.clone(), out.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(agent_reads).lines() {
                let Ok(line) = line else { return };
                let m = json::parse(&line).unwrap();
                let (Some(method), Some(id)) = (m.get("method").and_then(Json::as_str), m.get("id")) else { continue };
                let params = m.get("params").cloned().unwrap_or(Json::Null);
                me.got.lock().unwrap().push((method.to_owned(), params));
                let n = me.got.lock().unwrap().iter().filter(|g| g.0 == "session/new").count();
                let result = match method {
                    "initialize" => r#"{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}"#.to_owned(),
                    "session/new" => format!(r#"{{"sessionId":"s{n}"}}"#),
                    "session/prompt" => r#"{"stopReason":"end_turn"}"#.to_owned(),
                    _ => "{}".to_owned(),
                };
                if let Some(w) = o2.lock().unwrap().as_mut() { let _ = writeln!(w, r#"{{"jsonrpc":"2.0","id":{},"result":{result}}}"#, id.compact()); }
            }
        });
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { out.lock().unwrap().take(); }), errors: Box::new(String::new) }))
    }
}

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-cua-folder-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn servers_of(p: &Json) -> String { p.get("mcpServers").unwrap().compact() }

#[test]
fn an_acp_session_gets_the_servers_and_a_reply_after_a_change_loads_it_in_a_fresh_process() {
    let _e = Env::new("acp");
    let fake = Arc::new(Fake::default());
    let f = fake.clone();
    let h = AcpHost::with_connect(AgentTool::Codex, AgentOptions::default, move || f.connect());
    let dir = folder("acp");
    let run = |resume: Option<&str>| h.run_as(&dir, "hi", None, &Cancel::new(), resume, None, None);
    let one = r#"[{"name":"cua-driver","command":"/x/cua-driver","args":["mcp"],"env":[]}]"#;

    // Off (the default): the session is made with none, as before.
    assert_eq!(run(None).state, KiroState::Completed);
    assert_eq!(servers_of(&fake.params("session/new")[0]), "[]");

    // On: a new session carries Cua Driver's server.
    h.set_mcp(|_| vec![cua("/x/cua-driver")]);
    assert_eq!(run(None).state, KiroState::Completed);
    assert_eq!(servers_of(&fake.params("session/new")[1]), one);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 1, "the same process");

    // A reply to the first (made with none) now needs the servers: a fresh process loads it.
    assert_eq!(run(Some("s1")).state, KiroState::Completed);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 2, "started again for the changed servers");
    let load = fake.params("session/load");
    assert_eq!((load.len(), servers_of(&load[0])), (1, one.to_owned()));

    // The same servers again: it carries on as it is.
    assert_eq!(run(Some("s1")).state, KiroState::Completed);
    assert_eq!(fake.starts.load(Ordering::SeqCst), 2);
    assert_eq!(fake.params("session/load").len(), 1);
    h.shutdown("test");
}

#[test]
fn claude_code_is_started_with_an_mcp_config_file_only_when_there_are_servers() {
    let _e = Env::new("claude");
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let s2 = seen.clone();
    // A process that never answers: the start gives up, and the arguments are what counts.
    let connect = move |_: &str, args: &[String]| -> std::io::Result<Option<Link>> {
        s2.lock().unwrap().push(args.to_vec());
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (mut agent_reads, hover_writes) = std::io::pipe()?;
        std::thread::spawn(move || { let _ = std::io::copy(&mut agent_reads, &mut std::io::sink()); });
        let held = Mutex::new(Some(agent_writes));
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { held.lock().unwrap().take(); }), errors: Box::new(String::new) }))
    };
    let h = ClaudeHost::with_connect(AgentOptions::default, connect, Timeouts { start: Duration::from_millis(300), stop_grace: Duration::from_secs(1) });
    let dir = folder("claude");
    let run = |tag: &str| h.run_tagged(&dir, "hi", None, &Cancel::new(), None, None, None, Some(tag));
    assert_eq!(run("k1").state, KiroState::Failed);
    assert!(!seen.lock().unwrap()[0].iter().any(|a| a == "--mcp-config"), "none off: the arguments as they were");

    let mut browser = McpServer::new("hover-browser", "/usr/bin/perl", &["/r.pl", "/s.sock"]);
    browser.env.push(("HOVER_BROWSER_TOKEN".into(), "t0k".into()));
    let servers = vec![cua("/x/cua-driver"), browser];
    let want = computer_use::claude_config(&servers).unwrap();
    h.set_mcp(move |_| servers.clone());
    assert_eq!(run("k2").state, KiroState::Failed);
    let args = seen.lock().unwrap()[1].clone();
    let at = args.iter().position(|a| a == "--mcp-config").expect("--mcp-config is passed");
    assert_eq!(std::fs::read_to_string(&args[at + 1]).unwrap(), want);
    assert!(!args.iter().any(|a| a.contains("t0k")), "the token is not on the command line");
    assert!(!args.iter().any(|a| a == "--strict-mcp-config"), "the user's own servers stay");
    h.shutdown("test");
}
