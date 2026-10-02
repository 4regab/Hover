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

/// A command on PATH, else in the user's own bin folder (a desktop session's PATH
/// often lacks ~/.local/bin). The quota read looks for kiro-cli this way too.
pub fn find(name: &str) -> Option<PathBuf> { on_path(name).or_else(|| user_bin(name)) }


/// The oldest OpenCode whose server API Hover was checked against (T3 Code's floor).
pub const OPENCODE_MIN_VERSION: &str = "1.14.19";

/// What the macOS build's settings say about the agent integrations, read whenever a
/// tool starts or a session is made: computer use (off until switched on), the sandbox
/// and Hover's agent browser (on until switched off), and the folder new tasks start in
/// (Settings.KiroFolder; the sandbox opens it for the next start).
#[derive(Clone, Debug, PartialEq)]
pub struct Toggles { pub computer_use: bool, pub sandbox: bool, pub agent_browser: bool, pub folder: Option<String> }

impl Default for Toggles {
    /// Settings' own defaults, for a host that never gave any.
    fn default() -> Self { Toggles { computer_use: false, sandbox: true, agent_browser: true, folder: None } }
}

type TogglesFn = Box<dyn Fn() -> Toggles + Send + Sync>;

fn toggles_fn() -> &'static Mutex<Option<std::sync::Arc<TogglesFn>>> {
    static T: OnceLock<Mutex<Option<std::sync::Arc<TogglesFn>>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

/// Where the toggles come from: the app hands in a reader of its settings (hover-core's
/// `Settings::computer_use`, `sandbox`, `agent_browser` and `kiro_folder`).
pub fn set_toggles(f: impl Fn() -> Toggles + Send + Sync + 'static) { *toggles_fn().lock().unwrap() = Some(std::sync::Arc::new(Box::new(f))); }

/// The toggles now: the app's, or Settings' defaults when none were given.
pub fn toggles() -> Toggles {
    let f = toggles_fn().lock().unwrap().clone();
    f.map_or_else(Toggles::default, |f| f())
}

/// The program that runs the tool, or none when it isn't installed.
pub fn exe(t: AgentTool) -> Option<PathBuf> {
    match t {
        AgentTool::Kiro => find("kiro-cli"),
        AgentTool::Codex => find("codex-acp"),
        // Cursor's Windows installer puts it here and adds the folder to PATH, but a
        // Hover started before the install has the old PATH. Its "agent" alias is not
        // used: other tools (Grok) install an "agent" too.
        AgentTool::Cursor => cursor_shim().filter(|p| p.is_file()).or_else(|| find("cursor-agent")),
        AgentTool::OpenCode => opencode_exe(),
        AgentTool::Claude => claude_exe(),
    }
}

/// Claude Code's native installer puts it in ~/.local/bin on Windows too, and says the
/// folder may not be on PATH; npm's global install is a claude.cmd shim on PATH, and
/// the older local install ~/.claude/local/claude.
fn claude_exe() -> Option<PathBuf> {
    let home = crate::proc::home();
    let native = home.join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" });
    find("claude").or_else(|| Some(native).filter(|p| p.is_file()))
        .or_else(|| Some(home.join(".claude").join("local").join(if cfg!(windows) { "claude.exe" } else { "claude" })).filter(|p| p.is_file()))
}

/// OpenCode's own exe. npm installs a .cmd shim that runs it on Windows; going to the
/// exe it points at saves a cmd.exe per server and keeps the server Hover's direct
/// child. On Linux its installer puts it in ~/.opencode/bin, which a desktop
/// session's PATH often lacks (as ~/.local/bin).
fn opencode_exe() -> Option<PathBuf> {
    let found = find("opencode").or_else(|| {
        if cfg!(windows) { return None; }
        Some(crate::proc::home().join(".opencode/bin/opencode")).filter(|p| p.is_file())
    })?;
    if !found.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) { return Some(found); }
    let exe = found.parent()?.join("node_modules").join("opencode-ai").join("bin").join("opencode.exe");
    Some(if exe.is_file() { exe } else { found })
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
        // Local only: this address, a port the system picks, and never announced on
        // the network (mDNS), whatever the user's opencode config says.
        AgentTool::OpenCode => &["serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false"],
        // The Agent SDK's own way of running it (T3 Code's): streamed JSON both ways and
        // its control protocol on stdio, so every permission comes to Hover. The rest
        // (access, model, effort, resume) is added per conversation (claude::launch_args).
        AgentTool::Claude => &["--output-format", "stream-json", "--verbose", "--input-format", "stream-json", "--permission-prompt-tool", "stdio", "--include-partial-messages"],
    }
}

/// What it takes to install the tool, for the greyed-out choice.
pub fn install_hint(t: AgentTool) -> String {
    match t {
        AgentTool::Kiro => "Install kiro-cli from kiro.dev/cli.".into(),
        AgentTool::Codex => "Install Codex and its ACP adapter: npm i -g @openai/codex @agentclientprotocol/codex-acp".into(),
        AgentTool::Cursor if cfg!(windows) => "Install the Cursor CLI: irm 'https://cursor.com/install?win32=true' | iex".into(),
        AgentTool::Cursor => "Install the Cursor CLI: curl https://cursor.com/install -fsS | bash".into(),
        AgentTool::OpenCode => format!("Install OpenCode {OPENCODE_MIN_VERSION} or newer from opencode.ai."),
        AgentTool::Claude if cfg!(windows) => "Install Claude Code: irm https://claude.ai/install.ps1 | iex".into(),
        AgentTool::Claude => "Install Claude Code: curl -fsSL https://claude.ai/install.sh | bash".into(),
    }
}

pub fn sign_in_hint(t: AgentTool) -> &'static str {
    match t {
        AgentTool::Kiro => "Sign in: run “kiro-cli login” in a terminal.",
        AgentTool::Codex => "Sign in: run “codex login” in a terminal.",
        AgentTool::Cursor => "Sign in: run “cursor-agent login” in a terminal.",
        // OpenCode keeps its own providers: API keys, cloud sign-ins, local models.
        AgentTool::OpenCode => "Add a model provider: run “opencode auth login”, or set one up in your opencode config.",
        // An API key in its environment, Bedrock or Vertex count as signed in too (its
        // auth status says so).
        AgentTool::Claude => "Sign in: run “claude auth login” in a terminal.",
    }
}

/// Read only holds for Kiro (writes wait for an approval Hover refuses), Cursor
/// (Ask mode) and OpenCode (session rules its server enforces). Codex's read-only mode leans on a sandbox it doesn't have on Windows,
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
    let Some(exe) = exe(t) else { return AgentReady { installed: false, signed_in: false, hint: install_hint(t) } };
    if t == AgentTool::OpenCode {
        // Having no sign-in doesn't mean it can't run: API keys in the environment and
        // local models count too. The version is all that is checked here; a provider
        // that can't answer shows up as the task's own error.
        let (vc, vt) = ask(&exe, &["--version"]);
        let version = vt.trim().split('\n').next_back().unwrap_or("").trim().to_owned();
        let bad = |hint: String| AgentReady { installed: true, signed_in: false, hint };
        return match (vc, parse_version(version.split('-').next().unwrap_or(""))) {
            (0, Some(have)) if have < parse_version(OPENCODE_MIN_VERSION).unwrap() => bad(format!("OpenCode {version} is too old for Hover. {}", install_hint(t))),
            (0, Some(_)) => AgentReady { installed: true, signed_in: true, hint: String::new() },
            _ => bad(format!("Couldn’t read OpenCode’s version. {}", install_hint(t))),
        };
    }
    let (cmd, args): (Option<PathBuf>, &[&str]) = match t {
        AgentTool::Kiro => (Some(exe), &["whoami"]),
        AgentTool::Codex => (find("codex"), &["login", "status"]),
        AgentTool::Cursor => (Some(exe), &["status"]),
        // Exit 0 and {"loggedIn": true} when signed in, 1 when not.
        AgentTool::Claude => (Some(exe), &["auth", "status"]),
        AgentTool::OpenCode => unreachable!(),
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

/// System.Version.TryParse: two to four whole numbers between dots, none negative;
/// the parts left out compare as lower (-1), as Version does.
pub fn parse_version(s: &str) -> Option<[i64; 4]> {
    let parts: Vec<&str> = s.trim().split('.').collect();
    if !(2..=4).contains(&parts.len()) { return None; }
    let mut v = [-1i64; 4];
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) { return None; }
        v[i] = p.parse().ok()?;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_dotnet_does() {
        assert!(parse_version("1.18.31").unwrap() > parse_version(OPENCODE_MIN_VERSION).unwrap());
        assert!(parse_version("1.14.2").unwrap() < parse_version("1.14.19").unwrap());
        assert!(parse_version("1.14").unwrap() < parse_version("1.14.0").unwrap());
        assert_eq!(parse_version("1"), None);
        assert_eq!(parse_version("v1.2.3"), None);
        assert_eq!(parse_version("1.2.3.4.5"), None);
    }
}
