//! Services/AgentSetup.cs: one click from "not installed" to "ready". Installs what a tool
//! is missing with its maker's own installer, then opens its own sign-in. Nothing here
//! signs in for the user or touches a tool's credentials: sign-in is the tool's command in
//! a Terminal window (each one's login is interactive in its own way: a license choice, a
//! pasted code, a browser), and Hover only watches the tool's status command until it says
//! yes. macOS only for now (the Windows and Linux installers differ): `supported()` is
//! false elsewhere, with a note for the button.
//!
//! The installers are run only when the user asks, with no stdin, one at a time.

use crate::agents;
use crate::cancel::Cancel;
use crate::proc::{hidden, home, on_path, strip_ansi, Group};
use hover_core::model::AgentTool;
use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What Settings shows beside the setup button where it can't run.
pub const UNSUPPORTED: &str = "One-click setup is available on macOS.";

pub fn supported() -> bool { cfg!(target_os = "macos") }

/// Why the button is disabled here, or none.
pub fn note() -> Option<&'static str> { (!supported()).then_some(UNSUPPORTED) }

/// What a setup is doing: "installing" or "signing-in" (computer use's: "granting"),
/// the installer's last line, and why it stopped if it failed.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Progress { pub step: Option<String>, pub line: String, pub error: Option<String> }

/// One install step: what it is called and the bash command that does it.
#[derive(Clone, Debug, PartialEq)]
pub struct Step { pub title: String, pub command: String }

impl Step {
    fn new(title: &str, command: &str) -> Step { Step { title: title.into(), command: command.into() } }
}

/// How a step ended, short of succeeding.
#[derive(Debug)]
pub enum StepError { Cancelled, Failed(String) }

// MARK: What to run

/// npm packages go to ~/.local (bins in ~/.local/bin, already on Hover's PATH), so a
/// Homebrew or system Node never needs sudo, and the prefix is pinned to the npm that
/// installs them (as T3 Code pins its npm updates).
pub fn npm(packages: &[&str]) -> String { format!("npm install --global --no-fund --no-audit --prefix \"$HOME/.local\" {}", packages.join(" ")) }

/// What the sandbox still needs installed, when it is wanted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SandboxNeeds { pub srt_missing: bool, pub rg_missing: bool }

/// What a tool still needs, in order; empty when everything is there.
pub fn plan(t: AgentTool) -> Vec<Step> {
    let has = |n: &str| on_path(n).is_some();
    let sandbox = crate::sandbox::wanted().then(|| SandboxNeeds {
        srt_missing: crate::sandbox::exe().is_none(),
        rg_missing: !has("rg") && !Path::new("/opt/homebrew/bin/rg").is_file() && !Path::new("/usr/local/bin/rg").is_file(),
    });
    plan_with(t, &has, sandbox)
}

/// plan, with what is on PATH (`has`) and the sandbox's needs given.
pub fn plan_with(t: AgentTool, has: &dyn Fn(&str) -> bool, sandbox: Option<SandboxNeeds>) -> Vec<Step> {
    let mut steps: Vec<Step> = vec![];
    let mut packages: Vec<&str> = vec![];
    match t {
        AgentTool::Codex => {
            if !has("codex") { packages.push("@openai/codex"); }
            if !has("codex-acp") { packages.push("@agentclientprotocol/codex-acp"); }
        }
        AgentTool::Kiro => if !has("kiro-cli") { steps.push(Step::new("Installing Kiro CLI", "curl -fsSL https://cli.kiro.dev/install | bash")); },
        AgentTool::Cursor => if !has("cursor-agent") { steps.push(Step::new("Installing the Cursor CLI", "curl -fsS https://cursor.com/install | bash")); },
        AgentTool::OpenCode => if !has("opencode") { steps.push(Step::new("Installing OpenCode", "curl -fsSL https://opencode.ai/install | bash")); },
        // Not in 2.x's macOS build.
        AgentTool::Claude => if !has("claude") { steps.push(Step::new("Installing Claude Code", "curl -fsSL https://claude.ai/install.sh | bash")); },
    }
    let node = Step::new("Installing Node.js", "brew install node");
    if !packages.is_empty() {
        // The adapters are Node programs; Homebrew's Node when there is no Node yet.
        if !has("npm") && has("brew") { steps.push(node.clone()); }
        let title = if packages.len() > 1 { "Installing Codex and its ACP adapter" } else { "Installing Codex's ACP adapter" };
        steps.push(Step { title: title.into(), command: npm(&packages) });
    }
    // Every tool runs in the sandbox (sandbox.rs): srt, a Node program at the version
    // Hover was checked against, and ripgrep, which srt needs on a Mac.
    if let Some(need) = sandbox {
        if need.srt_missing {
            if !has("npm") && has("brew") && !steps.contains(&node) { steps.push(node.clone()); }
            steps.push(Step { title: "Installing the agent sandbox (srt)".into(), command: npm(&[&format!("{}@{}", crate::sandbox::PACKAGE, crate::sandbox::VERSION)]) });
        }
        if need.rg_missing && has("brew") { steps.push(Step::new("Installing ripgrep for the sandbox", "brew install ripgrep")); }
    }
    steps
}

/// The tool's own sign-in, run in Terminal.
pub fn sign_in_command(t: AgentTool) -> &'static str {
    match t {
        AgentTool::Codex => "codex login",
        AgentTool::Kiro => "kiro-cli login",
        AgentTool::Cursor => "cursor-agent login",
        AgentTool::OpenCode => "opencode auth login",
        AgentTool::Claude => "claude auth login",
    }
}

fn quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

/// The .command file Terminal opens. It carries Hover's PATH (from the login shell) so
/// Terminal finds the tool just installed even before a new shell would.
pub fn sign_in_script(t: AgentTool, path: &str) -> String {
    format!("#!/bin/bash\nexport PATH={}\nclear\nprintf '\\n  Hover · Sign in to {}\\n\\n'\n{}\nprintf '\\n  Done. You can close this window; Hover picks it up by itself.\\n\\n'\n",
        quote(path), t.name(), sign_in_command(t))
}

// MARK: Running a step

/// Runs a step with no stdin, showing its newest line (`line`); fails with its last lines.
/// A running step is ended when `ct` is cancelled or the time is up (its whole tree).
pub(crate) fn stream_step(exe: &Path, args: &[&str], env: &[(&str, &str)], timeout: Duration, failure: &str, ct: &Cancel, line: &dyn Fn(String)) -> Result<(), StepError> {
    let mut cmd = hidden(exe, args);
    cmd.current_dir(home());
    // Installers that draw progress bars or colours fall back to plain lines.
    cmd.env("CI", "1");
    for (k, v) in env { cmd.env(k, v); }
    let g = Group::spawn(cmd).map_err(|e| StepError::Failed(format!("{failure}: {e}")))?;
    let (stdin, stdout, stderr) = g.take_pipes();
    drop(stdin);
    let (tx, rx) = mpsc::channel::<String>();
    fn pump(p: Option<impl std::io::Read + Send + 'static>, tx: mpsc::Sender<String>) {
        if let Some(p) = p {
            std::thread::spawn(move || {
                let mut r = std::io::BufReader::new(p);
                let mut buf = Vec::new();
                while let Ok(n) = r.read_until(b'\n', &mut buf) {
                    if n == 0 { break; }
                    // A carriage return redraws a progress line: each piece is a line.
                    for piece in String::from_utf8_lossy(&buf).split(['\n', '\r']) { let _ = tx.send(piece.to_owned()); }
                    buf.clear();
                }
            });
        }
    }
    pump(stdout, tx.clone());
    pump(stderr, tx);
    let tail: std::cell::RefCell<VecDeque<String>> = Default::default();
    let take = |raw: String| {
        let l = strip_ansi(&raw).trim().to_owned();
        if l.is_empty() { return; }
        {
            let mut t = tail.borrow_mut();
            t.push_back(l.clone());
            while t.len() > 6 { t.pop_front(); }
        }
        line(if l.chars().count() > 120 { format!("{}…", l.chars().take(119).collect::<String>()) } else { l });
    };
    let began = Instant::now();
    let code = loop {
        while let Ok(l) = rx.try_recv() { take(l); }
        if ct.is_cancelled() { g.kill(); return Err(StepError::Cancelled); }
        if let Some(c) = g.wait_timeout(Duration::from_millis(50)) { break c; }
        if began.elapsed() >= timeout {
            g.kill();
            return Err(StepError::Failed(format!("{failure}: it took longer than {} minutes and was stopped.", timeout.as_secs() / 60)));
        }
    };
    // What it printed last, still on its way through the pipes.
    let until = Instant::now() + Duration::from_millis(500);
    while let Ok(l) = rx.recv_timeout(Duration::from_millis(100)) { take(l); if Instant::now() > until { break; } }
    if code != 0 {
        let tail = tail.borrow();
        let last: Vec<&str> = tail.iter().rev().take(2).rev().map(String::as_str).collect();
        return Err(StepError::Failed(format!("{failure}: {}", if last.is_empty() { format!("exit code {code}") } else { last.join(" · ") })));
    }
    Ok(())
}

// MARK: The state

static STATE: Mutex<Option<HashMap<AgentTool, Progress>>> = Mutex::new(None);
static RUNNING: Mutex<Option<HashMap<AgentTool, Cancel>>> = Mutex::new(None);
// npm and the vendors' installers write shared folders (~/.local/bin, the npm prefix);
// one at a time.
static GATE: Mutex<()> = Mutex::new(());
type Listener = Arc<dyn Fn(AgentTool) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Called, off the caller's thread, whenever a tool's progress changes.
pub fn on_change(f: impl Fn(AgentTool) + Send + Sync + 'static) { LISTENERS.lock().unwrap().push(Arc::new(f)); }

pub fn of(t: AgentTool) -> Progress { STATE.lock().unwrap().as_ref().and_then(|m| m.get(&t).cloned()).unwrap_or_default() }

fn set(t: AgentTool, p: Progress) {
    STATE.lock().unwrap().get_or_insert_with(HashMap::new).insert(t, p);
    let all: Vec<Listener> = LISTENERS.lock().unwrap().clone();
    for f in all { f(t); }
}

pub fn busy(t: AgentTool) -> bool { RUNNING.lock().unwrap().as_ref().is_some_and(|m| m.contains_key(&t)) }

pub fn cancel(t: AgentTool) { if let Some(c) = RUNNING.lock().unwrap().as_ref().and_then(|m| m.get(&t)) { c.cancel(); } }

/// Opens a script for the user (Terminal, by default).
pub type OpenFile<'a> = &'a dyn Fn(&Path) -> std::io::Result<()>;

/// Installs what is missing, then opens sign-in if the tool still isn't signed in. One
/// run per tool; a second click while one runs does nothing. `open_file` opens a script
/// (Terminal does by default). Blocks until it is done: run it off the UI thread.
pub fn run(t: AgentTool, open_file: Option<OpenFile>) {
    if !supported() { set(t, Progress { step: None, line: String::new(), error: Some(UNSUPPORTED.into()) }); return; }
    let ct = Cancel::new();
    {
        let mut r = RUNNING.lock().unwrap();
        let m = r.get_or_insert_with(HashMap::new);
        if m.contains_key(&t) { return; }
        m.insert(t, ct.clone());
    }
    match work(t, open_file, &ct) {
        Ok(()) => set(t, Progress::default()),
        Err(StepError::Cancelled) => set(t, Progress::default()),
        Err(StepError::Failed(m)) => set(t, Progress { step: None, line: String::new(), error: Some(m) }),
    }
    if let Some(m) = RUNNING.lock().unwrap().as_mut() { m.remove(&t); }
}

fn progress(step: &str, line: impl Into<String>) -> Progress { Progress { step: Some(step.into()), line: line.into(), error: None } }

fn work(t: AgentTool, open_file: Option<OpenFile>, ct: &Cancel) -> Result<(), StepError> {
    let fail = |m: String| StepError::Failed(m);
    let steps = plan(t);
    if !steps.is_empty() {
        if steps.iter().any(|s| s.command.starts_with("npm ")) && on_path("npm").is_none() && !steps.iter().any(|s| s.command.starts_with("brew ")) {
            return Err(fail("Node.js is needed for this tool's ACP adapter. Install Node.js from nodejs.org, then try again.".into()));
        }
        set(t, progress("installing", "Waiting for another install to finish…"));
        let _gate = loop {
            if ct.is_cancelled() { return Err(StepError::Cancelled); }
            match GATE.try_lock() {
                Ok(g) => break g,
                Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        };
        for s in &steps {
            set(t, progress("installing", format!("{}…", s.title)));
            let command = format!("set -o pipefail; {}", s.command);
            stream_step(Path::new("/bin/bash"), &["-c", &command], &[("HOMEBREW_NO_AUTO_UPDATE", "1")], Duration::from_secs(600),
                &s.title.replace("Installing", "Couldn’t install"), ct, &|l| set(t, progress("installing", l)))?;
        }
    }
    let ready = agents::check(t, true);
    if !ready.installed { return Err(fail(format!("The installer finished, but {} still isn't found. {}", t.name(), agents::install_hint(t)))); }
    if !ready.signed_in { sign_in(t, open_file, ct) } else { Ok(()) }
}

/// Opens the tool's own login in Terminal and waits (up to ten minutes) for its status
/// command to say signed in.
fn sign_in(t: AgentTool, open_file: Option<OpenFile>, ct: &Cancel) -> Result<(), StepError> {
    let dir = hover_core::paths::support().join("setup");
    let io = |e: std::io::Error| StepError::Failed(format!("Setup stopped: {e}"));
    std::fs::create_dir_all(&dir).map_err(io)?;
    let script = dir.join(format!("sign-in-{}.command", t.id()));
    std::fs::write(&script, sign_in_script(t, &std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()))).map_err(io)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(io)?; }
    set(t, progress("signing-in", "Finish signing in in the Terminal window and your browser…"));
    match open_file {
        Some(open) => open(&script).map_err(io)?,
        None => { let _ = agents::ask(Path::new("/usr/bin/open"), &["-a", "Terminal", &script.to_string_lossy()]); }
    }
    let until = Instant::now() + Duration::from_secs(600);
    while Instant::now() < until {
        for _ in 0..15 {
            if ct.is_cancelled() { return Err(StepError::Cancelled); }
            std::thread::sleep(Duration::from_millis(200));
        }
        if agents::check(t, true).signed_in { return Ok(()); }
    }
    Err(StepError::Failed(format!("Not signed in yet. Click Sign in to try again, or run “{}” in a terminal.", sign_in_command(t))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have(names: &'static [&'static str]) -> impl Fn(&str) -> bool { move |n| names.contains(&n) }
    fn cmds(v: &[Step]) -> Vec<&str> { v.iter().map(|s| s.command.as_str()).collect() }

    #[test]
    fn codex_needs_only_its_adapter_when_the_cli_is_there() {
        let p = plan_with(AgentTool::Codex, &have(&["codex", "npm"]), None);
        assert_eq!(p.len(), 1);
        assert!(p[0].command.contains("@agentclientprotocol/codex-acp") && !p[0].command.contains("@openai/codex "));
        // Into ~/.local, so no sudo and the bin lands on Hover's PATH.
        assert!(p[0].command.contains("--prefix \"$HOME/.local\""));
    }

    #[test]
    fn node_comes_from_homebrew_when_there_is_no_npm() {
        let p = plan_with(AgentTool::Codex, &have(&["brew"]), None);
        assert_eq!(cmds(&p), vec!["brew install node", p[1].command.as_str()]);
        assert!(p[1].command.contains("@openai/codex") && p[1].command.contains("@agentclientprotocol/codex-acp"));
    }

    /// A shell command that prints `script`'s lines and ends as it says, on either OS
    /// (PowerShell reads the same `echo`, `;`, `exit` and `sleep`).
    fn shell(script: &str) -> (&'static str, Vec<String>) {
        if cfg!(windows) { ("powershell.exe", vec!["-NoProfile".into(), "-Command".into(), script.into()]) } else { ("/bin/sh", vec!["-c".into(), script.into()]) }
    }

    fn run_step(script: &str, timeout: Duration, ct: &Cancel) -> (Result<(), StepError>, Vec<String>) {
        let (exe, args) = shell(script);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let lines = Mutex::new(vec![]);
        let r = stream_step(Path::new(exe), &args, &[], timeout, "Couldn’t do it", ct, &|l| lines.lock().unwrap().push(l));
        (r, lines.into_inner().unwrap())
    }

    #[test]
    fn a_step_shows_its_lines_and_fails_with_its_last_two() {
        let (r, lines) = run_step("echo one; echo two; echo three; exit 3", Duration::from_secs(20), &Cancel::new());
        assert_eq!(lines, vec!["one", "two", "three"]);
        match r { Err(StepError::Failed(m)) => assert_eq!(m, "Couldn’t do it: two · three"), other => panic!("{other:?}") }
        let (r, lines) = run_step("echo fine", Duration::from_secs(20), &Cancel::new());
        assert!(r.is_ok() && lines == vec!["fine"]);
        // No output: the exit code says it.
        let (r, _) = run_step("exit 7", Duration::from_secs(20), &Cancel::new());
        match r { Err(StepError::Failed(m)) => assert_eq!(m, "Couldn’t do it: exit code 7"), other => panic!("{other:?}") }
    }

    #[test]
    fn a_step_that_takes_too_long_or_is_cancelled_is_ended() {
        let slow = "sleep 30";
        let t = Instant::now();
        let (r, _) = run_step(slow, Duration::from_millis(300), &Cancel::new());
        assert!(matches!(r, Err(StepError::Failed(m)) if m.starts_with("Couldn’t do it: it took longer than")));
        let ct = Cancel::new();
        let c2 = ct.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(200)); c2.cancel(); });
        let (r, _) = run_step(slow, Duration::from_secs(20), &ct);
        assert!(matches!(r, Err(StepError::Cancelled)));
        assert!(t.elapsed() < Duration::from_secs(15), "both were ended, not waited out");
    }
}
