//! Services/GitHubCli.cs, for Windows, Linux and macOS: the GitHub CLI (gh), which the
//! desk's pull request panels read through and Create pull request (desk.rs) runs. Is it
//! installed, is it signed in and as whom, a one-click install where Hover may install
//! it without asking for a password (winget on Windows; Homebrew on a Mac, else gh's own
//! release, pinned to its checksum, into ~/.local), and gh's own device-code sign-in: `gh auth login --web` prints a
//! one-time code, the user enters it at github.com/login/device, and git is then set to
//! use gh for GitHub (`gh auth setup-git`) so a push from Hover works.
//!
//! Where there is no such way (Linux) Hover does
//! not install anything: it says what to run (`install_hint`), and never uses sudo.
//! gh keeps its sign-in in the system's keychain; the agents never read it.
//!
//! Everything here blocks; call it from a thread of your own (`start` makes one).
//! `run` is the one way git and gh are started, here and in desk.rs: hidden, with no
//! prompts, a timeout and a cap on what is read.

use crate::cancel::Cancel;
use crate::proc::{hidden, on_path, strip_ansi, Group};
use fancy_regex::Regex;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEVICE_URL: &str = "https://github.com/login/device";
pub const INSTALL_URL: &str = "https://cli.github.com";

const NOT_INSTALLED: &str = "Install the GitHub CLI to see and open pull requests.";
const NOT_SIGNED_IN: &str = "Sign in to GitHub to see and open pull requests.";

static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());
static USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Logged in to \S+ (?:account|as) ([A-Za-z0-9-]+)").unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z0-9]{4}-[A-Z0-9]{4})\b").unwrap());

/// GitHubCli.ParseUser: the account in `gh auth status`'s answer ("Logged in to
/// github.com account X").
pub fn parse_user(text: &str) -> Option<String> {
    USER.captures(text).ok().flatten().map(|m| m[1].to_owned())
}

/// GitHubCli.ParseCode: the one-time code gh's device flow prints ("First copy your
/// one-time code: ABCD-1234").
pub fn parse_code(line: &str) -> Option<String> {
    if !line.to_lowercase().contains("code") { return None; }
    CODE.captures(line).ok().flatten().map(|m| m[1].to_owned())
}

/// A github.com address in a line of gh's, else None: only Hover's own default is
/// ever offered to open, never a host a line of output names.
fn parse_url(line: &str) -> Option<String> {
    let at = line.find("https://github.com/")?;
    let url: String = line[at..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>')).collect();
    Some(url)
}

// MARK: Running a program

/// What a program said: its exit code (-1 when it couldn't run or timed out, 0 when
/// its output was cut at the cap), stdout, stderr without colour codes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
    /// stdout went past the cap: the program was stopped and `out` is its start.
    pub capped: bool,
}

impl Ran {
    pub fn failed(why: impl Into<String>) -> Ran { Ran { code: -1, out: String::new(), err: why.into(), capped: false } }
    pub fn ok(&self) -> bool { self.code == 0 }
}

/// A .cmd or .bat (Windows): one that `hidden` would send through cmd.
fn is_shim(exe: &Path) -> bool {
    cfg!(windows) && exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// The program as a hidden child with all three pipes and no prompts: git's and gh's
/// (optional locks off, so a status never takes the index lock from an agent that is
/// working; no pager, no update notice), and none of the variables a git hook sets.
/// A .cmd shim is started by Rust itself, which refuses an argument cmd would read as
/// more than data; `hidden` would pass it through cmd.exe unchecked.
pub fn command(exe: &Path, args: &[&str], dir: Option<&Path>, env: &[(String, String)]) -> Command {
    let mut c = if is_shim(exe) {
        let mut c = Command::new(exe);
        c.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env("NO_COLOR", "1").env("TERM", "dumb");
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000 /* CREATE_NO_WINDOW */); }
        c
    } else {
        hidden(exe, args)
    };
    if let Some(d) = dir { c.current_dir(d); }
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR"] { c.env_remove(v); }
    for (k, v) in [("GIT_OPTIONAL_LOCKS", "0"), ("GIT_TERMINAL_PROMPT", "0"), ("GIT_PAGER", "cat"), ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"), ("GH_PAGER", "cat"), ("PAGER", "cat")] { c.env(k, v); }
    for (k, v) in env { c.env(k, v); }
    c
}

/// DeskInfo.Run: a program run hidden in a folder, with no prompts, stdout read up to
/// `max` bytes, and stopped (with what it started) after `timeout`. `stdin` is written
/// to it and closed; without, it is closed at once. Long text (a pull request's
/// description, a commit message) goes this way, never on a command line.
pub fn run(exe: &Path, dir: Option<&Path>, timeout: Duration, max: usize, args: &[&str], stdin: Option<&[u8]>, env: &[(String, String)]) -> Ran {
    let group = match Group::spawn(command(exe, args, dir, env)) {
        Ok(g) => Arc::new(g),
        Err(e) => return Ran::failed(e.to_string()),
    };
    let (sin, sout, serr) = group.take_pipes();
    if let (Some(mut w), Some(bytes)) = (sin, stdin) {
        let bytes = bytes.to_vec();
        // Off this thread: a pipe holds only so much, and the program may not read it yet.
        std::thread::spawn(move || { let _ = w.write_all(&bytes); });
    }
    let capped = Arc::new(AtomicBool::new(false));
    let out = pump(sout, max, Some((capped.clone(), group.clone())));
    let err = pump(serr, 64 * 1024, None);
    let Some(code) = group.wait_timeout(timeout) else {
        group.kill();
        return Ran::failed("Timed out.");
    };
    // The pipes end with the program, unless something it started holds them: then what
    // is there is what there is, and the group's end (below) closes them.
    for p in [&out, &err] { let _ = p.done.recv_timeout(Duration::from_secs(2)); }
    let capped = capped.load(Ordering::SeqCst);
    let text = |p: &Pump| String::from_utf8_lossy(&p.buf.lock().unwrap()).into_owned();
    Ran { code: if capped { 0 } else { code }, out: text(&out), err: strip_ansi(&text(&err)), capped }
}

struct Pump { buf: Arc<Mutex<Vec<u8>>>, done: mpsc::Receiver<()> }

/// Reads a pipe to its end on a thread of its own. With a `cap` to stop at: what is
/// past it ends the program (its output is more than anyone reads); without, it is
/// read on and dropped, so the program never blocks on a full pipe.
fn pump(pipe: Option<impl Read + Send + 'static>, cap: usize, stop: Option<(Arc<AtomicBool>, Arc<Group>)>) -> Pump {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    match pipe {
        None => { let _ = tx.send(()); }
        Some(mut r) => {
            let b = buf.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 16384];
                loop {
                    let n = match r.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    };
                    let mut g = b.lock().unwrap();
                    let room = cap.saturating_sub(g.len());
                    g.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        drop(g);
                        if let Some((capped, group)) = &stop {
                            capped.store(true, Ordering::SeqCst);
                            group.kill();
                            break;
                        }
                    }
                }
                let _ = tx.send(());
            });
        }
    }
    Pump { buf, done: rx }
}

enum Ended { Exit(i32), Cancelled, TimedOut, NoStart(String) }

/// Runs a program and hands each line it prints (either pipe; a line ends at \n or \r,
/// as a progress bar redraws itself) to `on_line`, which answers true to have an Enter
/// sent to it. Ends with the program, the cancel, or the timeout (which end it and what
/// it started).
fn stream(exe: &Path, args: &[&str], env: &[(String, String)], timeout: Duration, cancel: &Cancel, keep_stdin: bool, mut on_line: impl FnMut(&str) -> bool) -> Ended {
    let group = match Group::spawn(command(exe, args, None, env)) {
        Ok(g) => Arc::new(g),
        Err(e) => return Ended::NoStart(e.to_string()),
    };
    let (sin, sout, serr) = group.take_pipes();
    let mut sin = if keep_stdin { sin } else { None };
    let (tx, rx) = mpsc::channel::<String>();
    if let Some(r) = sout { lines(r, tx.clone()); }
    if let Some(r) = serr { lines(r, tx.clone()); }
    drop(tx);
    let start = Instant::now();
    let mut open = true;
    let mut handle = |line: String, sin: &mut Option<std::process::ChildStdin>| {
        if on_line(&line) {
            if let Some(w) = sin.as_mut() { let _ = w.write_all(b"\n").and_then(|_| w.flush()); }
        }
    };
    loop {
        if open {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(l) => handle(l, &mut sin),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => open = false,
            }
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
        if cancel.is_cancelled() { group.kill(); return Ended::Cancelled; }
        if start.elapsed() >= timeout { group.kill(); return Ended::TimedOut; }
        if let Some(code) = group.wait_timeout(Duration::ZERO) {
            // What it printed last is what the user needs if it failed.
            while let Ok(l) = rx.recv_timeout(Duration::from_millis(300)) { handle(l, &mut sin); }
            return Ended::Exit(code);
        }
    }
}

/// A pipe as lines, sent as they arrive. A prompt waits without its newline (gh's
/// "Press Enter to open..."), so one that says so is sent as it stands.
fn lines(mut r: impl Read + Send + 'static, tx: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = vec![];
        let mut chunk = [0u8; 4096];
        let send = |pending: &mut Vec<u8>| {
            let s = String::from_utf8_lossy(pending).trim().to_owned();
            pending.clear();
            s.is_empty() || tx.send(s).is_ok()
        };
        'read: loop {
            let n = match r.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            for &b in &chunk[..n] {
                if b == b'\n' || b == b'\r' {
                    if !send(&mut pending) { break 'read; }
                } else {
                    pending.push(b);
                }
            }
            if (pending.len() > 64 * 1024 || String::from_utf8_lossy(&pending).contains("Press Enter")) && !send(&mut pending) { break; }
        }
        send(&mut pending);
    });
}

fn clip(line: &str, limit: usize) -> String {
    if line.chars().count() > limit { format!("{}…", line.chars().take(limit - 1).collect::<String>()) } else { line.to_owned() }
}

// MARK: The CLI

/// What Hover knows: installed, signed in and as whom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub installed: bool,
    pub signed_in: bool,
    pub user: Option<String>,
    pub version: Option<String>,
    /// What to tell the user when it isn't ready; empty when it is.
    pub hint: String,
}

/// What a setup is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step { Installing, SigningIn }

impl Step {
    /// The word desk.js and the C# used: "installing" or "signing-in".
    pub fn name(self) -> &'static str { match self { Step::Installing => "installing", Step::SigningIn => "signing-in" } }
}

/// A setup's state: what it is doing, its newest line, the one-time code while sign-in
/// waits (and the address to enter it at), and why it stopped if it failed. All empty
/// when no setup runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub step: Option<Step>,
    pub line: String,
    pub code: Option<String>,
    /// Where the code is entered: always on github.com, DEVICE_URL unless gh named another page there.
    pub url: Option<String>,
    pub error: Option<String>,
}

impl Progress {
    fn at(step: Step, line: impl Into<String>) -> Progress { Progress { step: Some(step), line: line.into(), ..Default::default() } }
    fn failed(why: impl Into<String>) -> Progress { Progress { error: Some(why.into()), ..Default::default() } }
}

/// How gh gets installed from Hover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallPlan {
    /// Windows: `winget install GitHub.cli`.
    Winget(PathBuf),
    /// macOS: `brew install gh`.
    Homebrew(PathBuf),
    /// macOS without Homebrew: gh's own release, checked against its pinned sum, into
    /// ~/.local (setup::release_script).
    Release(crate::setup::Release),
    /// Hover doesn't install it here; this is what to tell the user.
    Manual(String),
}

/// The line Linux's users are told to run, from /etc/os-release's text. Hover never
/// runs it: it would need sudo.
pub fn linux_install_line(os_release: &str) -> String {
    let field = |k: &str| os_release.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix('='))).unwrap_or("").trim_matches('"').to_lowercase();
    let ids = format!("{} {}", field("ID"), field("ID_LIKE"));
    let has = |names: &[&str]| ids.split_whitespace().any(|i| names.contains(&i));
    if has(&["debian", "ubuntu", "linuxmint", "pop", "raspbian"]) { "sudo apt install gh" }
    else if has(&["fedora", "rhel", "centos", "rocky", "almalinux"]) { "sudo dnf install gh" }
    else if has(&["arch", "manjaro", "endeavouros"]) { "sudo pacman -S github-cli" }
    else if has(&["suse", "opensuse", "opensuse-leap", "opensuse-tumbleweed", "sles"]) { "sudo zypper install gh" }
    else if has(&["alpine"]) { "sudo apk add github-cli" }
    else { "" }.to_owned()
}

/// Where gh's installers put it that a desktop app's PATH may not list (a Mac app
/// started from the Dock has only /usr/bin:/bin:/usr/sbin:/sbin).
pub fn known_places() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(p) = std::env::var_os(var) { v.push(PathBuf::from(p).join("GitHub CLI").join("gh.exe")); }
        }
        if let Some(p) = std::env::var_os("LOCALAPPDATA") { v.push(PathBuf::from(p).join("Programs").join("GitHub CLI").join("gh.exe")); }
    }
    #[cfg(target_os = "macos")]
    {
        v.extend(["/opt/homebrew/bin/gh", "/usr/local/bin/gh"].map(PathBuf::from));
        v.push(crate::proc::home().join(".local").join("bin").join("gh"));
        v.push(PathBuf::from("/usr/bin/gh"));
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        v.extend(["/usr/bin/gh", "/usr/local/bin/gh"].map(PathBuf::from));
        v.push(crate::proc::home().join(".local").join("bin").join("gh"));
        v.extend(["/home/linuxbrew/.linuxbrew/bin/gh", "/snap/bin/gh"].map(PathBuf::from));
    }
    v
}

/// gh, from PATH or where its installers put it.
pub fn find_exe() -> Option<PathBuf> {
    on_path("gh").or_else(|| known_places().into_iter().find(|p| p.is_file()))
}

/// The one gh of the app.
pub fn shared() -> Arc<GitHubCli> {
    static ONE: OnceLock<Arc<GitHubCli>> = OnceLock::new();
    ONE.get_or_init(|| Arc::new(GitHubCli::new())).clone()
}

struct State {
    progress: Progress,
    known: Option<(Instant, Status)>,
    running: Option<Cancel>,
}

pub struct GitHubCli {
    /// A gh to use whatever PATH says (tests use a stand-in).
    fixed: Option<PathBuf>,
    /// More variables for every program started (tests point git away from the user's config).
    env: Vec<(String, String)>,
    /// An install plan to use whatever the system has (tests use a stand-in for winget).
    plan: Option<InstallPlan>,
    state: Mutex<State>,
    changed: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

type Done<T> = Result<T, Stop>;

/// Why a setup step ended early.
enum Stop { Cancelled, Failed(String) }

impl Stop {
    fn failed(why: impl Into<String>) -> Stop { Stop::Failed(why.into()) }
}

impl Default for GitHubCli {
    fn default() -> Self { Self::new() }
}

impl GitHubCli {
    pub fn new() -> GitHubCli {
        GitHubCli { fixed: None, env: vec![], plan: None, state: Mutex::new(State { progress: Progress::default(), known: None, running: None }), changed: Mutex::new(vec![]) }
    }

    /// Uses this gh, and this extra environment for every program it starts.
    pub fn with(mut self, gh: Option<PathBuf>, env: Vec<(String, String)>) -> GitHubCli {
        self.fixed = gh;
        self.env = env;
        self
    }

    /// Installs with this instead of what the system has.
    pub fn with_install(mut self, plan: InstallPlan) -> GitHubCli {
        self.plan = Some(plan);
        self
    }

    /// The extra environment for programs started (Desk passes it on to git).
    pub fn env(&self) -> &[(String, String)] { &self.env }

    /// GitHubCli.Exe.
    pub fn exe(&self) -> Option<PathBuf> {
        match &self.fixed {
            Some(p) => p.is_file().then(|| p.clone()),
            None => find_exe(),
        }
    }

    /// Raised off the caller's thread when the status or a setup's progress changes.
    pub fn on_changed(&self, f: impl Fn() + Send + Sync + 'static) { self.changed.lock().unwrap().push(Arc::new(f)); }

    fn raise(&self) {
        let all: Vec<_> = self.changed.lock().unwrap().clone();
        for f in all { f(); }
    }

    fn report(&self, p: Progress) {
        self.state.lock().unwrap().progress = p;
        self.raise();
    }

    /// What a setup is doing now (empty when none is).
    pub fn setup(&self) -> Progress { self.state.lock().unwrap().progress.clone() }
    pub fn busy(&self) -> bool { self.state.lock().unwrap().running.is_some() }
    /// The last status read, however old.
    pub fn known(&self) -> Option<Status> { self.state.lock().unwrap().known.as_ref().map(|k| k.1.clone()) }

    /// GitHubCli.Check: installed and signed in, from gh itself. Kept a minute unless `fresh`.
    pub fn check(&self, fresh: bool) -> Status {
        if !fresh {
            if let Some((at, s)) = &self.state.lock().unwrap().known {
                if at.elapsed() < Duration::from_secs(60) { return s.clone(); }
            }
        }
        let s = match self.exe() {
            None => Status { installed: false, signed_in: false, user: None, version: None, hint: NOT_INSTALLED.into() },
            Some(gh) => {
                let v = run(&gh, None, Duration::from_secs(15), 64 * 1024, &["--version"], None, &self.env);
                let version = if v.ok() { VERSION.find(&v.out).ok().flatten().map(|m| m.as_str().to_owned()) } else { None };
                let a = run(&gh, None, Duration::from_secs(20), 64 * 1024, &["auth", "status", "--hostname", "github.com"], None, &self.env);
                if a.ok() {
                    // gh prints this on stdout, older versions on stderr.
                    Status { installed: true, signed_in: true, user: parse_user(&a.out).or_else(|| parse_user(&a.err)), version, hint: String::new() }
                } else {
                    Status { installed: true, signed_in: false, user: None, version, hint: NOT_SIGNED_IN.into() }
                }
            }
        };
        self.state.lock().unwrap().known = Some((Instant::now(), s.clone()));
        self.raise();
        s
    }

    /// Ends a setup under way (its sign-in waiting for the code, or its install).
    pub fn cancel(&self) {
        let c = self.state.lock().unwrap().running.clone();
        if let Some(c) = c { c.cancel(); }
    }

    // MARK: Install

    /// How gh would be installed here: with winget (Windows) or Homebrew (a Mac) if the
    /// system has it, else by hand.
    pub fn install_plan(&self) -> InstallPlan {
        if let Some(p) = &self.plan { return p.clone(); }
        if cfg!(windows) {
            return match on_path("winget") {
                Some(w) => InstallPlan::Winget(w),
                None => InstallPlan::Manual(format!("Install the GitHub CLI from {}. (winget isn’t available.)", INSTALL_URL.trim_start_matches("https://"))),
            };
        }
        if cfg!(target_os = "macos") {
            let brew = on_path("brew").or_else(|| ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].iter().map(PathBuf::from).find(|p| p.is_file()));
            return match brew {
                Some(b) => InstallPlan::Homebrew(b),
                None => InstallPlan::Release(crate::setup::GH),
            };
        }
        let line = std::fs::read_to_string("/etc/os-release").map(|t| linux_install_line(&t)).unwrap_or_default();
        let site = INSTALL_URL.trim_start_matches("https://");
        InstallPlan::Manual(if line.is_empty() { format!("Install the GitHub CLI with your package manager, or from {site}.") } else { format!("Install the GitHub CLI by running `{line}` in a terminal, or see {site}.") })
    }

    /// Whether one click can install gh here.
    pub fn can_install(&self) -> bool { !matches!(self.install_plan(), InstallPlan::Manual(_)) }

    /// What to tell the user when one click can't install it; None when it can.
    pub fn install_hint(&self) -> Option<String> {
        match self.install_plan() { InstallPlan::Manual(h) => Some(h), _ => None }
    }

    fn install(&self, cancel: &Cancel) -> Done<()> {
        let script;
        let (exe, args): (PathBuf, Vec<&str>) = match self.install_plan() {
            InstallPlan::Manual(hint) => return Err(Stop::Failed(hint)),
            InstallPlan::Winget(w) => (w, vec!["install", "--id", "GitHub.cli", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"]),
            InstallPlan::Homebrew(b) => (b, vec!["install", "gh"]),
            InstallPlan::Release(r) => { script = crate::setup::release_script(&r); (PathBuf::from("/bin/bash"), vec!["-c", &script]) }
        };
        let mut env = self.env.clone();
        env.push(("HOMEBREW_NO_AUTO_UPDATE".into(), "1".into()));
        env.push(("NONINTERACTIVE".into(), "1".into()));
        let mut tail = String::new();
        let ended = stream(&exe, &args, &env, Duration::from_secs(10 * 60), cancel, false, |l| {
            tail = strip_ansi(l).trim().to_owned();
            if !tail.is_empty() { self.report(Progress::at(Step::Installing, clip(&tail, 140))); }
            false
        });
        match ended {
            Ended::Exit(0) => Ok(()),
            Ended::Exit(code) => Err(Stop::Failed(format!("Couldn’t install the GitHub CLI: {}", if tail.is_empty() { format!("exit code {code}") } else { tail }))),
            Ended::Cancelled => Err(Stop::Cancelled),
            Ended::TimedOut => Err(Stop::failed("The install took too long and was stopped.")),
            Ended::NoStart(e) => Err(Stop::Failed(format!("Couldn’t install the GitHub CLI: {e}"))),
        }
    }

    // MARK: Sign in

    /// gh's device flow: it prints a one-time code, the user enters it at
    /// github.com/login/device, and gh waits until they have.
    fn sign_in(&self, cancel: &Cancel) -> Done<()> {
        let gh = self.exe().ok_or_else(|| Stop::failed("gh isn’t installed."))?;
        self.report(Progress::at(Step::SigningIn, "Asking GitHub for a sign-in code…"));
        let mut code: Option<String> = None;
        let mut url = DEVICE_URL.to_owned();
        let ended = stream(&gh, &["auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web"], &self.env, Duration::from_secs(15 * 60), cancel, true, |raw| {
            let l = strip_ansi(raw).trim().to_owned();
            if l.is_empty() { return false; }
            if code.is_none() { code = parse_code(&l); }
            if let Some(u) = parse_url(&l) { url = u; }
            let line = if code.is_none() { clip(&l, 140) } else { format!("Enter the code at {}, then come back.", url.trim_start_matches("https://")) };
            self.report(Progress { step: Some(Step::SigningIn), line, code: code.clone(), url: code.is_some().then(|| url.clone()), error: None });
            // "Press Enter to open github.com in your browser..."
            l.to_lowercase().contains("press enter")
        });
        match ended {
            Ended::Exit(0) => {}
            Ended::Exit(_) | Ended::NoStart(_) => return Err(Stop::failed("GitHub sign-in didn’t finish.")),
            Ended::Cancelled => return Err(Stop::Cancelled),
            Ended::TimedOut => return Err(Stop::failed("GitHub sign-in timed out.")),
        }
        self.report(Progress::at(Step::SigningIn, "Setting up git to use your GitHub sign-in…"));
        self.setup_git(&gh);
        Ok(())
    }

    /// `gh auth setup-git`: git asks gh for GitHub's credentials, so a push from Hover works.
    pub fn setup_git(&self, gh: &Path) -> bool {
        run(gh, None, Duration::from_secs(30), 64 * 1024, &["auth", "setup-git", "--hostname", "github.com"], None, &self.env).ok()
    }

    // MARK: One click

    /// Starts `run_setup` on a thread of its own; false when a setup already runs.
    pub fn start(self: &Arc<Self>) -> bool {
        if self.busy() { return false; }
        let me = self.clone();
        std::thread::Builder::new().name("github-setup".into()).spawn(move || me.run_setup()).is_ok()
    }

    /// GitHubCli.Run: one click. Installs gh if it is missing, then signs in if it isn't.
    /// Blocks until done, failed or cancelled; `setup()` says how it went (its `error`).
    pub fn run_setup(&self) {
        let cancel = {
            let mut s = self.state.lock().unwrap();
            if s.running.is_some() { return; }
            let c = Cancel::new();
            s.running = Some(c.clone());
            c
        };
        // However this ends (a panic too), the next click can start.
        struct Free<'a>(&'a GitHubCli);
        impl Drop for Free<'_> {
            fn drop(&mut self) { self.0.state.lock().unwrap().running = None; }
        }
        let free = Free(self);
        let r = (|| -> Done<()> {
            let mut s = self.check(true);
            if !s.installed {
                self.report(Progress::at(Step::Installing, "Installing the GitHub CLI…"));
                self.install(&cancel)?;
                s = self.check(true);
                if !s.installed { return Err(Stop::failed("The install finished, but gh still isn’t found.")); }
            }
            if !s.signed_in {
                self.sign_in(&cancel)?;
                s = self.check(true);
                if !s.signed_in { return Err(Stop::failed("GitHub sign-in didn’t finish. Try again.")); }
            }
            Ok(())
        })();
        self.report(match r {
            Ok(()) | Err(Stop::Cancelled) => Progress::default(),
            Err(Stop::Failed(why)) => Progress::failed(why),
        });
        // Free before the last read of the status, which can take a while.
        drop(free);
        self.check(true);
    }
}
