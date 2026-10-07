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
        // A custom agent is started from its own record (custom.rs), not found by name.
        AgentTool::Custom => None,
        AgentTool::Agy => agy_acp_exe(),
    }
}

/// Google's Antigravity ACP server, as T3 Code runs it (and as the ACP registry lists
/// it): not the agy CLI, which has no ACP mode, but its own release
/// (dl.google.com/agy-extensions, agy-acp-server-<version>-<os>-<arch>.zip). The zip holds
/// the server and the agent harness it runs, which must sit next to it. Looked for on
/// PATH (and ~/.local/bin), then where setup.rs unpacks it, AGY_ACP_DIR.
pub const AGY_ACP_VERSION: &str = "1.3.0";
const AGY_SERVER: &str = if cfg!(windows) { "agy_acp_server.exe" } else { "agy_acp_server.par" };
const AGY_HARNESS: &str = if cfg!(windows) { "localharness_external.exe" } else { "localharness_external" };

/// Where Hover's setup unpacks the server: outside Hover's own data folder, which the
/// sandbox keeps the tools from reading.
pub fn agy_acp_dir() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA").filter(|_| cfg!(windows)) {
        Some(l) => PathBuf::from(l).join("antigravity-acp"),
        None => crate::proc::home().join(".local/share/antigravity-acp"),
    }
}

fn agy_acp_exe() -> Option<PathBuf> {
    let unpacked = agy_acp_dir().join(AGY_SERVER);
    find(AGY_SERVER).or_else(|| find("agy_acp_server")).or_else(|| Some(unpacked).filter(|p| p.is_file()))
        .filter(|p| agy_harness(p).is_file())
}

/// The agent harness the server runs (ANTIGRAVITY_HARNESS_PATH), next to the server.
pub fn agy_harness(server: &std::path::Path) -> PathBuf {
    server.parent().map_or_else(|| PathBuf::from(AGY_HARNESS), |d| d.join(AGY_HARNESS))
}

/// What a tool's process gets in its environment besides Hover's own.
pub fn environment(t: AgentTool, exe: &std::path::Path) -> Vec<(String, String)> {
    match t {
        AgentTool::Agy => {
            // The server unpacks itself (about 1 GB) into its temp folder on every start,
            // and leaves its logs there: a folder of its own, emptied before each start
            // (one server per Hover), so a killed one leaves nothing behind for long.
            let temp = crate::sandbox::temp_root().join("hover-agy");
            let _ = std::fs::create_dir_all(&temp);
            if let Ok(d) = std::fs::read_dir(&temp) {
                for e in d.flatten() {
                    // The sandbox's relay is written here just before the start.
                    if e.file_name() == "relay.pl" { continue; }
                    let p = e.path();
                    let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                }
            }
            let t = temp.to_string_lossy().into_owned();
            let mut env = vec![
                ("ANTIGRAVITY_HARNESS_PATH".to_owned(), agy_harness(exe).to_string_lossy().into_owned()),
                ("PYTHONUNBUFFERED".to_owned(), "1".to_owned()),
            ];
            if cfg!(windows) { env.extend([("TEMP".to_owned(), t.clone()), ("TMP".to_owned(), t)]); } else { env.push(("TMPDIR".to_owned(), t)); }
            env
        }
        _ => vec![],
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
        AgentTool::Custom => &[],
        // On Linux the server wants its user id flag, empty (T3 Code starts it so).
        AgentTool::Agy if cfg!(target_os = "linux") => &["--uid="],
        AgentTool::Agy => &[],
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
        AgentTool::Custom => "Check the agent’s program in Settings.".into(),
        AgentTool::Agy => format!("Install Google’s Antigravity ACP server {AGY_ACP_VERSION}: unzip agy-acp-server-{AGY_ACP_VERSION}-<os>-<arch>.zip from dl.google.com/agy-extensions/releases into {}.",
            agy_acp_dir().display()),
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
        AgentTool::Custom => "Sign in with the agent’s own method in Settings.",
        // The server runs Google's sign-in itself when Hover starts it (acp.rs), or takes
        // GEMINI_API_KEY from the environment.
        AgentTool::Agy => "Sign in: start an Antigravity task and finish Google’s sign-in in the browser it opens, or set GEMINI_API_KEY.",
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
    /// What the status command printed, while signed in (Kiro's says who is signed in).
    said: HashMap<AgentTool, String>,
}

fn checks() -> &'static Mutex<Checks> {
    static C: OnceLock<Mutex<Checks>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The last check, if any, without running one.
pub fn known(t: AgentTool) -> Option<AgentReady> { checks().lock().unwrap().done.get(&t).map(|c| c.1.clone()) }

/// Records a check's answer without running one (the screenshots and tests, on a machine without the tool).
pub fn seed(t: AgentTool, ready: AgentReady) { checks().lock().unwrap().done.insert(t, (Instant::now(), ready)); }

/// What the last check's status command printed, if it said signed in; None otherwise.
pub fn said(t: AgentTool) -> Option<String> { checks().lock().unwrap().said.get(&t).cloned() }

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
    let (ready, said) = look(t);
    let mut c = checks().lock().unwrap();
    c.done.insert(t, (Instant::now(), ready.clone()));
    if ready.signed_in { c.said.insert(t, said); } else { c.said.remove(&t); }
    if let Some(w) = c.asking.remove(&t) { *w.0.lock().unwrap() = Some(ready.clone()); w.1.notify_all(); }
    ready
}

/// The answer, and what the status command printed ("" when none ran).
fn look(t: AgentTool) -> (AgentReady, String) {
    let Some(exe) = exe(t) else { return (AgentReady { installed: false, signed_in: false, hint: install_hint(t) }, String::new()) };
    if t == AgentTool::Custom { return (AgentReady { installed: false, signed_in: false, hint: install_hint(t) }, String::new()); }
    if t == AgentTool::OpenCode {
        // Having no sign-in doesn't mean it can't run: API keys in the environment and
        // local models count too. The version is all that is checked here; a provider
        // that can't answer shows up as the task's own error.
        let (vc, vt) = ask(&exe, &["--version"]);
        let version = vt.trim().split('\n').next_back().unwrap_or("").trim().to_owned();
        let bad = |hint: String| AgentReady { installed: true, signed_in: false, hint };
        return (match (vc, parse_version(version.split('-').next().unwrap_or(""))) {
            (0, Some(have)) if have < parse_version(OPENCODE_MIN_VERSION).unwrap() => bad(format!("OpenCode {version} is too old for Hover. {}", install_hint(t))),
            (0, Some(_)) => AgentReady { installed: true, signed_in: true, hint: String::new() },
            _ => bad(format!("Couldn’t read OpenCode’s version. {}", install_hint(t))),
        }, String::new());
    }
    if t == AgentTool::Agy {
        // Never started to ask: it unpacks about 1 GB per start (T3 Code doesn't either).
        // It has no status command; a missing sign-in shows as the task's own error.
        return (AgentReady { installed: true, signed_in: true, hint: String::new() }, String::new());
    }
    let (cmd, args): (Option<PathBuf>, &[&str]) = match t {
        AgentTool::Kiro => (Some(exe), &["whoami"]),
        AgentTool::Codex => (find("codex"), &["login", "status"]),
        AgentTool::Cursor => (Some(exe), &["status"]),
        // Exit 0 and {"loggedIn": true} when signed in, 1 when not.
        AgentTool::Claude => (Some(exe), &["auth", "status"]),
        AgentTool::OpenCode | AgentTool::Custom | AgentTool::Agy => unreachable!(),
    };
    // The adapter can carry its own Codex; without the CLI there is nothing to ask.
    let Some(cmd) = cmd else { return (AgentReady { installed: true, signed_in: true, hint: String::new() }, String::new()) };
    let (code, text) = ask(&cmd, args);
    let lower = text.to_lowercase();
    let signed_in = code == 0 && !lower.contains("not logged in") && !lower.contains("not signed in") && !lower.contains("logged out");
    (AgentReady { installed: true, signed_in, hint: if signed_in { String::new() } else { sign_in_hint(t).into() } }, text)
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
