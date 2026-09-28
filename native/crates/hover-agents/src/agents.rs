//! Services/Agents.cs: where each tool is, how it starts, and whether it is installed
//! and signed in.

use crate::proc::{hidden, on_path, strip_ansi, Group};
use hover_core::model::AgentTool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Whether a tool can take a task now; the hint says what to do when it can't.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentReady { pub installed: bool, pub signed_in: bool, pub hint: String }

impl AgentReady {
    pub fn ok(&self) -> bool { self.installed && self.signed_in }
}

/// On Linux a tool installed for the user lands in ~/.local/bin, which a session
/// started from the desktop doesn't always have on PATH (the Linux counterpart of
/// the Cursor shim C# looks for in %LOCALAPPDATA%).
fn user_bin(name: &str) -> Option<PathBuf> {
    if cfg!(windows) { return None; }
    Some(crate::proc::home().join(".local/bin").join(name)).filter(|p| p.is_file())
}

fn find(name: &str) -> Option<PathBuf> { on_path(name).or_else(|| user_bin(name)) }

/// The program that speaks ACP for the tool, or none when it isn't installed.
pub fn exe(t: AgentTool) -> Option<PathBuf> {
    match t {
        AgentTool::Kiro => find("kiro-cli"),
        AgentTool::Codex => find("codex-acp"),
        // Cursor's Windows installer puts it here and adds the folder to PATH, but a
        // Hover started before the install has the old PATH. Its "agent" alias is not
        // used: other tools (Grok) install an "agent" too.
        AgentTool::Cursor => cursor_shim().filter(|p| p.is_file()).or_else(|| find("cursor-agent")),
    }
}

fn cursor_shim() -> Option<PathBuf> {
    if !cfg!(windows) { return None; }
    std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("cursor-agent").join("cursor-agent.cmd"))
}

pub fn arguments(t: AgentTool) -> &'static [&'static str] {
    match t {
        // v3 is the engine with ACP sessions that load; "cli" keeps the sign-in inside
        // kiro-cli rather than asking Hover for tokens.
        AgentTool::Kiro => &["acp", "--agent-engine", "v3", "--auth-method", "cli"],
        AgentTool::Codex => &[],
        AgentTool::Cursor => &["acp"],
    }
}

/// What it takes to install the tool, for the greyed-out choice.
pub fn install_hint(t: AgentTool) -> &'static str {
    match t {
        AgentTool::Kiro => "Install kiro-cli from kiro.dev/cli.",
        AgentTool::Codex => "Install Codex and its ACP adapter: npm i -g @openai/codex @agentclientprotocol/codex-acp",
        AgentTool::Cursor if cfg!(windows) => "Install the Cursor CLI: irm 'https://cursor.com/install?win32=true' | iex",
        AgentTool::Cursor => "Install the Cursor CLI: curl https://cursor.com/install -fsS | bash",
    }
}

pub fn sign_in_hint(t: AgentTool) -> &'static str {
    match t {
        AgentTool::Kiro => "Sign in: run “kiro-cli login” in a terminal.",
        AgentTool::Codex => "Sign in: run “codex login” in a terminal.",
        AgentTool::Cursor => "Sign in: run “cursor-agent login” in a terminal.",
    }
}

/// Read only holds for Kiro (writes wait for an approval Hover refuses) and Cursor
/// (Ask mode). Codex's read-only mode leans on a sandbox it doesn't have on Windows,
/// so there it wrote files anyway; on Linux it has one (Landlock), so it is offered.
pub fn read_only_works(t: AgentTool) -> bool { t != AgentTool::Codex || !cfg!(windows) }

/// A check under way, and its answer once there is one.
type Asking = Arc<(Mutex<Option<AgentReady>>, Condvar)>;

#[derive(Default)]
struct Checks {
    done: HashMap<AgentTool, (Instant, AgentReady)>,
    asking: HashMap<AgentTool, Asking>,
}

fn checks() -> &'static Mutex<Checks> {
    static C: OnceLock<Mutex<Checks>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The last check, if any, without running one.
pub fn known(t: AgentTool) -> Option<AgentReady> { checks().lock().unwrap().done.get(&t).map(|c| c.1.clone()) }

/// Installed and signed in, by the tool's own status command. Kept five minutes; a
/// check already under way is shared rather than run twice. Blocks; call it off the
/// UI thread.
pub fn check(t: AgentTool, fresh: bool) -> AgentReady {
    let wait = {
        let mut c = checks().lock().unwrap();
        if !fresh {
            if let Some((at, r)) = c.done.get(&t) { if at.elapsed() < Duration::from_secs(300) { return r.clone(); } }
        }
        match c.asking.get(&t) {
            Some(w) => Some(w.clone()),
            None => { c.asking.insert(t, Arc::new((Mutex::new(None), Condvar::new()))); None }
        }
    };
    if let Some(w) = wait {
        let (m, cv) = &*w;
        let g = cv.wait_while(m.lock().unwrap(), |r| r.is_none()).unwrap();
        return g.clone().unwrap();
    }
    let ready = look(t);
    let mut c = checks().lock().unwrap();
    c.done.insert(t, (Instant::now(), ready.clone()));
    if let Some(w) = c.asking.remove(&t) { *w.0.lock().unwrap() = Some(ready.clone()); w.1.notify_all(); }
    ready
}

fn look(t: AgentTool) -> AgentReady {
    let Some(exe) = exe(t) else { return AgentReady { installed: false, signed_in: false, hint: install_hint(t).into() } };
    let (cmd, args): (Option<PathBuf>, &[&str]) = match t {
        AgentTool::Kiro => (Some(exe), &["whoami"]),
        AgentTool::Codex => (find("codex"), &["login", "status"]),
        AgentTool::Cursor => (Some(exe), &["status"]),
    };
    // The adapter can carry its own Codex; without the CLI there is nothing to ask.
    let Some(cmd) = cmd else { return AgentReady { installed: true, signed_in: true, hint: String::new() } };
    let (code, text) = ask(&cmd, args);
    let lower = text.to_lowercase();
    let signed_in = code == 0 && !lower.contains("not logged in") && !lower.contains("not signed in") && !lower.contains("logged out");
    AgentReady { installed: true, signed_in, hint: if signed_in { String::new() } else { sign_in_hint(t).into() } }
}

/// Runs a status command with its input closed: its code and its output, or -1
/// after 20 s (the tree killed).
pub fn ask(exe: &std::path::Path, args: &[&str]) -> (i32, String) {
    let g = match Group::spawn(hidden(exe, args)) { Ok(g) => g, Err(e) => return (-1, e.to_string()) };
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    let read = |p: Option<Box<dyn std::io::Read + Send>>| std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(mut p) = p { let _ = p.read_to_end(&mut s); }
        String::from_utf8_lossy(&s).into_owned()
    });
    let o = read(stdout.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let e = read(stderr.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    match g.wait_timeout(Duration::from_secs(20)) {
        None => { g.kill(); (-1, String::new()) }
        Some(code) => (code, strip_ansi(&format!("{}\n{}", o.join().unwrap_or_default(), e.join().unwrap_or_default()))),
    }
}
