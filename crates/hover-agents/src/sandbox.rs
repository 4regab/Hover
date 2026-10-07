//! Services/Sandbox.cs: every agent tool runs inside Anthropic's sandbox-runtime (srt,
//! Apache-2.0, github.com/anthropics/sandbox-runtime), the sandbox Claude Code runs
//! commands in: sandbox-exec with a generated profile on a Mac, bubblewrap on Linux. It
//! works in the background without getting in the way of the person at the computer:
//!   - writes only to the folders its sessions work in, the tool's own state and caches,
//!     and temp files; nothing else of the user's can be changed;
//!   - keys, keychains, mail, messages, browsers' data and other apps' data (Office's
//!     included) can't be read, nor Hover's own;
//!   - no window server and no Apple Events (srt's macOS profile has neither): no window
//!     drawn, no focus taken, no app launched or scripted;
//!   - the network only through srt's proxy, to the tool's own service, package
//!     registries and GitHub, plus the hosts in allowed-domains.txt.
//!
//! Computer use still works: the agent's cua-driver connects to CuaDriver's daemon, which
//! Hover starts outside the sandbox, over its one socket.
//!
//! The folders are fixed when the tool starts, so a tool started for some is started
//! again (when nothing of it runs) for a session in another. Off with the Sandbox
//! setting; never nested inside another sandbox (HOVER_SANDBOXED=1). macOS and Linux;
//! srt's Windows support is an alpha that can't reach tools installed for the user, which
//! is where Kiro, Cursor and OpenCode go, so it isn't used there. A tool whose sandbox
//! can't be had (srt missing, the setting off) starts as it always did, and hover.log
//! says why.
//!
//! What can be worked out without a disk or a Mac (the settings file's text, the argument
//! list, the folder rules) is in functions of their own, run by the tests on every OS.

use crate::agents::toggles;
use crate::proc::{self, on_path, Link};
use hover_core::json::Json;
use hover_core::model::AgentTool;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The srt release Hover was checked against, and installs.
pub const VERSION: &str = "0.0.78";
pub const PACKAGE: &str = "@anthropic-ai/sandbox-runtime";
/// What Settings shows beside the switch where the sandbox can't run.
pub const UNSUPPORTED: &str = "The sandbox needs macOS or Linux.";

pub fn supported() -> bool { cfg!(any(target_os = "macos", target_os = "linux")) }

/// Why the switch is disabled here, or none.
pub fn note() -> Option<&'static str> { (!supported()).then_some(UNSUPPORTED) }

/// Hover itself runs in a sandbox already (a developer's cargo run under srt, see scripts/sandbox.sh): another one
/// inside it would fail, and the outer one holds.
pub fn inside() -> bool { std::env::var("HOVER_SANDBOXED").is_ok_and(|v| v == "1") }

/// Whether tools are to be started in it.
pub fn wanted() -> bool { toggles().sandbox && supported() && !inside() }

/// Wanted, and everything it needs is there: what decides how a tool starts.
pub fn active() -> bool { wanted() && missing().is_none() }

pub fn exe() -> Option<PathBuf> {
    if let Some(found) = on_path("srt") { return Some(found); }
    let home = proc::home();
    [PathBuf::from("/opt/homebrew/bin/srt"), PathBuf::from("/usr/local/bin/srt"), home.join(".npm-global/bin/srt"), home.join(".local/bin/srt")]
        .into_iter().find(|p| p.is_file())
}

fn tool(name: &str, fallbacks: &[&str]) -> Option<PathBuf> { on_path(name).or_else(|| fallbacks.iter().map(PathBuf::from).find(|p| p.is_file())) }

/// What is missing for the sandbox to run, as one line to show; none when nothing (or
/// when it isn't wanted).
pub fn missing() -> Option<String> {
    if !wanted() { return None; }
    let mac = cfg!(target_os = "macos");
    let mut need: Vec<String> = vec![];
    if exe().is_none() { need.push(format!("npm install -g {PACKAGE}@{VERSION}")); }
    // srt finds the paths it must keep closed with ripgrep (and on Linux needs bubblewrap
    // and socat for the sandbox itself).
    if tool("rg", &["/opt/homebrew/bin/rg", "/usr/local/bin/rg", "/usr/bin/rg"]).is_none() { need.push(if mac { "brew install ripgrep" } else { "install ripgrep" }.into()); }
    if cfg!(target_os = "linux") && tool("bwrap", &["/usr/bin/bwrap"]).is_none() { need.push("install bubblewrap".into()); }
    if cfg!(target_os = "linux") && tool("socat", &["/usr/bin/socat"]).is_none() { need.push("install socat".into()); }
    missing_line(&need)
}

/// The line Settings shows for what is missing.
pub fn missing_line(need: &[String]) -> Option<String> {
    (!need.is_empty()).then(|| format!("Hover runs agents in a sandbox, which isn’t set up yet: {}. (Or turn the sandbox off in Settings.)", need.join(", then ")))
}

// MARK: Folders

static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// A folder a session works in, so the next start of its tool covers it.
pub fn remember(folder: &str) {
    if !supported() || !crate::usable_folder(Some(folder)) { return; }
    let mut seen = SEEN.lock().unwrap();
    seen.insert(full(folder));
    // A task's own worktree commits into the main checkout's .git: the tool may write there too.
    for g in crate::workspace::git_dirs(folder).into_iter().filter(|g| crate::usable_folder(Some(g))) { seen.insert(full(&g)); }
}

/// The folders a tool started now gets: the ones sessions use this run, and the folder
/// new tasks start in.
pub fn folders() -> Vec<String> {
    let mut all = SEEN.lock().unwrap().clone();
    if let Some(f) = toggles().folder.filter(|f| crate::usable_folder(Some(f))) { all.insert(full(&f)); }
    all.into_iter().collect()
}

/// The folder is one of them, or inside one.
pub fn covers(folders: &[String], folder: &str) -> bool {
    let f = full(folder);
    folders.iter().any(|x| f == *x || f.starts_with(&format!("{}/", x.trim_end_matches('/'))))
}

/// Path.GetFullPath on a Unix path, lexically, without the trailing slash.
pub fn full(folder: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for seg in folder.split('/') {
        match seg { "" | "." => {} ".." => { parts.pop(); } x => parts.push(x) }
    }
    format!("/{}", parts.join("/"))
}

/// How a tool's process stands with its sandbox: whether Hover started it (not a test's
/// stand-in), and the folders it was sandboxed for, none when it wasn't.
#[derive(Default)]
pub struct Boxed { own: AtomicBool, folders: Mutex<Option<Vec<String>>> }

/// What a run in a folder finds of the tool's running process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// It serves this folder as it is.
    Fits,
    /// It doesn't, and nothing of it runs: end it, and the start that follows fits.
    Restart,
    /// It doesn't, and it is busy in other folders: this one has to wait.
    Outside,
}

impl Boxed {
    /// Noted by the start: the folders the process was sandboxed for, none when it
    /// wasn't.
    pub fn started(&self, folders: Option<Vec<String>>) {
        self.own.store(true, Ordering::SeqCst);
        *self.folders.lock().unwrap() = folders;
    }

    /// A sandboxed tool reaches only the folders it started with, and the sandbox
    /// switched on or off applies from its next start: one that no longer fits is
    /// started again when nothing of it runs. `active` is Sandbox::active() now.
    pub fn fit(&self, folder: &str, busy: bool, active: bool) -> Fit {
        if !self.own.load(Ordering::SeqCst) { return Fit::Fits; }
        let boxed = self.folders.lock().unwrap().clone();
        let outside = boxed.as_ref().is_some_and(|b| !covers(b, folder));
        if !(outside || boxed.is_some() != active) { return Fit::Fits; }
        if !busy { Fit::Restart } else if outside { Fit::Outside } else { Fit::Fits }
    }
}

/// Said when a run has to wait for a task of its tool in other folders.
pub fn outside_message(name: &str) -> String {
    format!("{name} is working on a task in another folder, and its sandbox reaches only the folders it started with. Start this one when that task is done.")
}

// MARK: The policy

/// Hosts every tool may reach: package registries, GitHub, and the local machine (dev
/// servers, the tool's own local services).
pub const DEV_DOMAINS: &[&str] = &[
    "localhost", "github.com", "*.github.com", "*.githubusercontent.com", "*.githubassets.com",
    "registry.npmjs.org", "*.npmjs.org", "*.npmjs.com", "registry.yarnpkg.com", "*.yarnpkg.com",
    "pypi.org", "*.pypi.org", "files.pythonhosted.org", "crates.io", "*.crates.io",
    "proxy.golang.org", "sum.golang.org", "api.nuget.org", "*.nuget.org", "rubygems.org", "*.rubygems.org",
    "repo.maven.apache.org", "repo1.maven.org", "services.gradle.org", "plugins.gradle.org",
    "jsr.io", "deno.land", "bun.sh", "nodejs.org",
];

/// Each tool's own service (sign-in, models, telemetry it can't run without).
pub fn tool_domains(t: AgentTool) -> &'static [&'static str] {
    match t {
        AgentTool::Kiro => &["kiro.dev", "*.kiro.dev", "*.amazonaws.com", "*.awsapps.com", "*.amazoncognito.com", "*.aws.amazon.com", "*.aws.dev"],
        AgentTool::Codex => &["api.openai.com", "*.openai.com", "chatgpt.com", "*.chatgpt.com", "*.oaiusercontent.com"],
        AgentTool::Cursor => &["cursor.com", "*.cursor.com", "cursor.sh", "*.cursor.sh", "*.cursorapi.com"],
        // OpenCode brings the user's own providers.
        AgentTool::OpenCode => &[
            "opencode.ai", "*.opencode.ai", "models.dev", "api.anthropic.com", "api.openai.com", "openrouter.ai", "*.openrouter.ai",
            "generativelanguage.googleapis.com", "*.githubcopilot.com", "api.x.ai", "api.groq.com", "api.mistral.ai", "api.deepseek.com",
            "api.together.xyz", "api.fireworks.ai", "api.cerebras.ai", "openai.azure.com", "*.openai.azure.com",
        ],
        // Not in 2.x's macOS build. Bedrock and Vertex users add their cloud's hosts to
        // allowed-domains.txt.
        AgentTool::Claude => &["anthropic.com", "*.anthropic.com", "claude.ai", "*.claude.ai", "claude.com", "*.claude.com"],
        // Custom agents run outside the sandbox (their state folders and hosts are unknown).
        AgentTool::Custom => &[],
        // Google's sign-in and its agent backend (Cloud Code), the Gemini API for a key,
        // and Antigravity's own site.
        AgentTool::Agy => &[
            "accounts.google.com", "oauth2.googleapis.com", "openidconnect.googleapis.com", "www.googleapis.com",
            "cloudcode-pa.googleapis.com", "daily-cloudcode-pa.googleapis.com", "generativelanguage.googleapis.com",
            "antigravity.google", "*.antigravity.google", "play.googleapis.com",
        ],
    }
}

/// Where the user's own hosts are: one host per line, # for comments.
pub fn extra_file() -> PathBuf { hover_core::paths::support().join("sandbox").join("allowed-domains.txt") }

const EXTRA_HEADER: &str = "# Hosts Hover's sandboxed agents may reach, beyond their own service, package\n\
# registries and GitHub. One per line, like docs.example.com or *.example.com.\n\
# A tool picks a change up when it next starts.\n";

/// ^(\*\.)?[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+(:\d{1,5})?$ or ^localhost(:\d{1,5})?$
fn valid_host(h: &str) -> bool {
    let (name, port) = match h.rsplit_once(':') { Some((n, p)) => (n, Some(p)), None => (h, None) };
    if port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit())) { return false; }
    if name == "localhost" { return true; }
    let labels: Vec<&str> = name.strip_prefix("*.").unwrap_or(name).split('.').collect();
    labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// The hosts in allowed-domains.txt's text: comments and anything that isn't a host
/// (a URL, a bare "*", "*.com") dropped.
pub fn parse_extra(text: &str) -> Vec<String> {
    text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| valid_host(l)).map(str::to_owned).collect()
}

/// The user's own hosts; the file is written, explained, the first time it is needed.
pub fn extra() -> Vec<String> {
    let file = extra_file();
    if !file.exists() {
        if let Some(dir) = file.parent() { let _ = std::fs::create_dir_all(dir); }
        let _ = std::fs::write(&file, EXTRA_HEADER);
    }
    std::fs::read(&file).map(|b| parse_extra(&hover_core::json::text_of(&b))).unwrap_or_default()
}

/// Where paths come from: the user's home, Hover's data folder, and whether this is a
/// Mac. Given, so the settings text can be made and tested anywhere.
#[derive(Clone, Debug)]
pub struct Ctx { pub home: String, pub support: String, pub macos: bool, pub darwin_temp: Option<String> }

impl Ctx {
    pub fn current() -> Ctx {
        Ctx {
            home: proc::home().to_string_lossy().into_owned(), support: hover_core::paths::support().to_string_lossy().into_owned(),
            macos: cfg!(target_os = "macos"), darwin_temp: if cfg!(target_os = "macos") { darwin_temp() } else { None },
        }
    }

    fn under_home(&self, rel: &str) -> String { format!("{}/{rel}", self.home.trim_end_matches('/')) }
    fn tilde(&self, p: &str) -> String { p.strip_prefix("~/").map_or_else(|| p.to_owned(), |r| self.under_home(r)) }
    fn support(&self) -> String { full(&self.support) }
}

/// Apple's own tools (sips, xcrun, codesign) write here whatever TMPDIR says.
fn darwin_temp() -> Option<String> {
    std::env::var("TMPDIR").ok().filter(|t| t.starts_with("/var/folders/")).map(|t| t.trim_end_matches('/').to_owned())
}

/// Where a tool keeps its own sign-in, settings, logs and caches: writable.
pub fn tool_state(t: AgentTool, ctx: &Ctx) -> Vec<String> {
    let own: &[&str] = match t {
        AgentTool::Kiro => &["~/.kiro", "~/.aws/sso", "~/.aws/cli", "~/Library/Application Support/kiro-cli", "~/.local/share/kiro-cli", "~/.config/kiro-cli"],
        AgentTool::Codex => &["~/.codex"],
        AgentTool::Cursor => &["~/.cursor", "~/.config/cursor", "~/Library/Application Support/Cursor", "~/.local/share/cursor-agent"],
        AgentTool::OpenCode => &["~/.local/share/opencode", "~/.local/state/opencode", "~/.config/opencode", "~/.cache/opencode"],
        AgentTool::Claude => &["~/.claude", "~/.claude.json", "~/.config/claude"],
        AgentTool::Custom => &[],
        // GEMINI_HOME: the ACP server's token and settings (antigravity-acp/) and the CLI's.
        AgentTool::Agy => &["~/.gemini"],
    };
    // What builds and package managers the agents run write to.
    const SHARED: &[&str] = &[
        "~/.cache", "~/.npm", "~/.local/state", "~/Library/Caches", "~/Library/Logs", "~/.nuget", "~/.dotnet", "~/.cargo/registry",
        "~/.cargo/git", "~/go/pkg", "~/.gradle", "~/.m2", "~/.bun", "~/.yarn", "~/.pnpm-store", "~/Library/pnpm", "~/.deno",
    ];
    own.iter().chain(SHARED).map(|p| ctx.tilde(p)).collect()
}

/// What no tool may read.
pub fn private(ctx: &Ctx) -> Vec<String> {
    const P: &[&str] = &[
        "~/.ssh", "~/.gnupg", "~/.netrc", "~/.config/gh", "~/.docker/config.json", "~/.kube", "~/.password-store",
        // Not ~/Library/Keychains: a file keychain is opened in-process (Cursor keeps its
        // sign-in there), and its items stay locked behind securityd and their own access
        // lists.
        "~/Library/Mail", "~/Library/Messages", "~/Library/Safari", "~/Library/Cookies",
        "~/Library/Containers", "~/Library/Group Containers", "~/Library/Calendars", "~/Library/Application Support/AddressBook",
        "~/Library/Application Support/com.apple.TCC", "~/Library/Application Support/Google/Chrome",
        "~/Library/Application Support/BraveSoftware", "~/Library/Application Support/Microsoft Edge",
        "~/Library/Application Support/Firefox", "~/Library/Application Support/Arc", "~/.mozilla", "~/.config/google-chrome",
    ];
    P.iter().map(|p| ctx.tilde(p)).chain([ctx.support()]).collect()
}

/// CuaDriver's daemon socket, which the agent's cua-driver talks to.
pub fn cua_socket(ctx: &Ctx) -> String { ctx.under_home("Library/Caches/cua-driver/cua-driver.sock") }

fn strs(v: &[String]) -> Json { Json::Arr(v.iter().map(|s| Json::str(s.as_str())).collect()) }

/// srt's settings for a tool started for these folders.
pub fn config(tool: AgentTool, folders: &[String], temp: &str, sockets: &[String], extra: &[String], ctx: &Ctx) -> String {
    let mut domains: Vec<String> = vec![];
    for d in tool_domains(tool).iter().copied().chain(DEV_DOMAINS.iter().copied()).chain(extra.iter().map(String::as_str)) {
        if !domains.iter().any(|x| x.eq_ignore_ascii_case(d)) { domains.push(d.to_owned()); }
    }
    let support = ctx.support();
    // Images pasted into a prompt are kept in Hover's folder, and the prompt names them
    // for the agent to read; Cua Driver runs behind Hover's guard, Hover's browser relay
    // and the MCP configs Hover writes are read the same way (readable, not writable).
    let mut read: Vec<String> = folders.to_vec();
    read.extend(["kiro-images", "cua", "browser", "mcp"].map(|d| format!("{support}/{d}")));
    let mut write: Vec<String> = folders.to_vec();
    write.extend(tool_state(tool, ctx));
    write.push(temp.to_owned());
    write.extend(ctx.darwin_temp.clone());
    let config = Json::obj(vec![
        ("network", Json::obj(vec![
            ("allowedDomains", strs(&domains)),
            ("deniedDomains", Json::Arr(vec![])),
            // Dev servers the agent starts, and test runs against them.
            ("allowLocalBinding", Json::Bool(true)),
            ("allowUnixSockets", strs(sockets)),
        ])),
        ("filesystem", Json::obj(vec![
            ("denyRead", strs(&private(ctx))),
            ("allowRead", strs(&read)),
            ("allowWrite", strs(&write)),
            // A folder's .git/hooks and .git/config, shell rc files and the like are
            // closed by srt itself.
            ("denyWrite", Json::Arr(vec![])),
        ])),
        ("allowAppleEvents", Json::Bool(false)),
        // macOS's certificate service: without it .NET, Go (gh) and Security-framework
        // TLS can't verify a certificate, and the tools can't sign in.
        ("enableWeakerNetworkIsolation", Json::Bool(ctx.macos)),
        // Terminals the agent opens for commands.
        ("allowPty", Json::Bool(true)),
    ]);
    config.indented("\n")
}

// MARK: Starting a tool in it

const PERL: &str = "/usr/bin/perl";

/// srt (Node) hands the tool its own stdio, non-blocking, so a write bigger than the
/// pipe's 64 KB buffer fails with EAGAIN and the tool dies. Cua Driver's tool list is
/// 67 KB, so with computer use on Kiro quit at its first session ("failed to forward the
/// v3 engine's output to ACP stdout"), and cua-driver itself with "os error 35". This
/// relay runs the tool on blocking pipes of its own and copies them across, waiting when
/// a pipe is full. It ends when the tool does, and closes the tool's stdin when Hover
/// closes its own.
pub const RELAY: &str = r###"#!/usr/bin/perl
# Runs a tool with pipes of its own and copies them to this process's stdin/stdout.
# srt (Node) shares its stdio with the tool and makes it non-blocking, so a tool's
# write larger than the pipe's 64 KB buffer fails with EAGAIN and the tool dies
# (Kiro: "failed to forward the v3 engine's output"; cua-driver: os error 35). Here
# every read and write waits for its fd, so nothing is lost and nothing fails.
use strict; use warnings;
use POSIX qw(:sys_wait_h EAGAIN EINTR);
use IO::Select;
die "usage: relay.pl tool [args...]\n" unless @ARGV;
pipe(my $in_r, my $in_w) or die "pipe: $!\n";
pipe(my $out_r, my $out_w) or die "pipe: $!\n";
my $pid = fork() // die "fork: $!\n";
if ($pid == 0) {
    close $in_w; close $out_r;
    open(STDIN, '<&', $in_r) or die "stdin: $!\n";
    open(STDOUT, '>&', $out_w) or die "stdout: $!\n";
    close $in_r; close $out_w;
    exec { $ARGV[0] } @ARGV or die "exec $ARGV[0]: $!\n";
}
close $in_r; close $out_w;
# A stop for this process is a stop for the tool.
for my $sig (qw(TERM INT HUP)) { $SIG{$sig} = sub { kill $sig, $pid; } }
$SIG{PIPE} = 'IGNORE';
my $sel = IO::Select->new(\*STDIN, $out_r);
my ($to_tool, $to_host) = ('', '');
my $stdin_open = 1;
sub flush_to {
    my ($fh, $buf) = @_;
    while (length $$buf) {
        my $n = syswrite($fh, $$buf);
        if (!defined $n) {
            return 0 if $! != EAGAIN && $! != EINTR;
            IO::Select->new($fh)->can_write(1);
            next;
        }
        substr($$buf, 0, $n) = '';
    }
    return 1;
}
my $parent = getppid();
while ($sel->count) {
    # Whoever started it went away (srt killed): the tool goes too.
    if (getppid() != $parent) { kill 'TERM', $pid; last; }
    my @ready = $sel->can_read(1);
    for my $fh (@ready) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        if ($fh == $out_r) {
            if ($n == 0) { $sel->remove($out_r); next; }
            $to_host .= $chunk;
            flush_to(\*STDOUT, \$to_host) or exit 1;
        } else {
            if ($n == 0) { $sel->remove(\*STDIN); close $in_w; $stdin_open = 0; next; }
            $to_tool .= $chunk;
            flush_to($in_w, \$to_tool) or do { $sel->remove(\*STDIN); $stdin_open = 0; };
        }
    }
    last if !$sel->exists($out_r) && !$stdin_open;
    last if !$sel->exists($out_r) && waitpid($pid, WNOHANG) > 0;
}
close $in_w if $stdin_open;
waitpid($pid, 0);
exit($? & 127 ? 128 + ($? & 127) : $? >> 8);
"###;

/// The argument list of srt for a tool: its settings, then the tool with its own
/// arguments. srt quotes each argument and runs them with bash -c; env puts the tool's
/// temp folder back, which srt points at its own. The relay (perl, and where its script
/// is) gives the tool pipes of its own (see RELAY).
pub fn srt_args(settings: &str, temp: &str, relay: Option<(&str, &str)>, exe: &str, args: &[&str]) -> Vec<String> {
    let mut a: Vec<String> = ["--settings", settings, "--", "/usr/bin/env"].map(String::from).into();
    a.push(format!("TMPDIR={temp}/"));
    if let Some((perl, script)) = relay { a.extend([perl.to_owned(), script.to_owned()]); }
    a.push(exe.to_owned());
    a.extend(args.iter().map(|s| s.to_string()));
    a
}

/// What the sandbox adds to a tool's environment. A dotnet build the agent runs stays in
/// one process: MSBuild's worker nodes talk over sockets in /tmp that the sandbox can't
/// open without opening every socket there (ssh-agent's among them). The variable tells
/// the agent why.
pub const ENV: &[(&str, &str)] = &[
    ("HOVER_SANDBOXED", "1"), ("MSBUILDDISABLENODEREUSE", "1"), ("DOTNET_CLI_USE_MSBUILD_SERVER", "0"), ("UseSharedCompilation", "false"),
];

/// Temp files and Unix sockets go in a short folder of the tool's own (sockets have a
/// 104-byte path limit), the only place sockets work besides CuaDriver's and Hover's
/// browser's.
pub fn temp_root() -> PathBuf {
    match std::env::var_os("HOVER_SANDBOX_TMP").filter(|r| !r.is_empty()) {
        Some(r) => PathBuf::from(r),
        None if cfg!(target_os = "macos") => PathBuf::from("/private/tmp/claude"),
        None => std::env::temp_dir().join("claude"),
    }
}

/// The folder exists, is the user's own and no link, and only they can enter it.
#[cfg(unix)]
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(std::io::Error::other(format!("{} isn’t a folder of the user’s own", dir.display())));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub fn private_dir(dir: &Path) -> std::io::Result<()> { std::fs::create_dir_all(dir) }

#[cfg(unix)]
fn write_private(file: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(file, text)?;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private(file: &Path, text: &str) -> std::io::Result<()> { std::fs::write(file, text) }

/// A tool's start as it will be made: the program, its arguments and its environment.
/// Sandboxed, that is srt with the tool inside; otherwise the tool as it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Start { pub exe: PathBuf, pub args: Vec<String>, pub env: Vec<(String, String)>, pub boxed: bool }

/// The tool's start, inside srt when the sandbox is wanted and can be had: the settings
/// are written to <data>/sandbox/<tool>.json and the relay to the tool's temp folder.
/// Anything else (switched off, srt missing, the folders can't be made) leaves the start
/// as it was, and hover.log says why.
pub fn plan(tool: AgentTool, exe: &Path, args: &[&str], env: &[(String, String)], folders: &[String]) -> Start {
    let plain = || Start { exe: exe.to_path_buf(), args: args.iter().map(|s| s.to_string()).collect(), env: env.to_vec(), boxed: false };
    if !supported() { return plain(); }
    let name = tool.name();
    if inside() { hover_core::log::line(&format!("sandbox: {name} starts as it is (Hover is in a sandbox already)")); return plain(); }
    if !toggles().sandbox { hover_core::log::line(&format!("sandbox: {name} starts unsandboxed (switched off in Settings)")); return plain(); }
    if let Some(why) = missing() { hover_core::log::line(&format!("sandbox: {name} starts unsandboxed - {why}")); return plain(); }
    match boxed_start(tool, exe, args, env, folders) {
        Ok(s) => { hover_core::log::line(&format!("sandbox: {name} starts in srt for {} folder(s)", folders.len())); s }
        Err(e) => { hover_core::log::line(&format!("sandbox: {name} starts unsandboxed - couldn’t set it up: {e}")); plain() }
    }
}

fn boxed_start(tool: AgentTool, exe: &Path, args: &[&str], env: &[(String, String)], folders: &[String]) -> std::io::Result<Start> {
    let srt = self::exe().ok_or_else(|| std::io::Error::other("srt isn’t installed"))?;
    let ctx = Ctx::current();
    let dir = hover_core::paths::support().join("sandbox");
    std::fs::create_dir_all(&dir)?;
    let root = temp_root();
    std::fs::create_dir_all(&root)?;
    let temp = root.join(format!("hover-{}", tool.id()));
    private_dir(&temp)?;
    let temp_s = temp.to_string_lossy().into_owned();
    let mut sockets = vec![temp_s.clone()];
    let t = toggles();
    if ctx.macos && t.computer_use { sockets.push(cua_socket(&ctx)); }
    // Hover's browser: the relay the agent's tool starts talks to Hover over this one socket.
    if crate::browser::available() && t.agent_browser || crate::spaces::wanted() { sockets.push(crate::browser::socket_path().to_string_lossy().into_owned()); }
    let file = dir.join(format!("{}.json", tool.id()));
    write_private(&file, &config(tool, folders, &temp_s, &sockets, &extra(), &ctx))?;
    let relay = if Path::new(PERL).is_file() {
        let script = temp.join("relay.pl");
        std::fs::write(&script, RELAY)?;
        Some(script.to_string_lossy().into_owned())
    } else { None };
    let a = srt_args(&file.to_string_lossy(), &temp_s, relay.as_deref().map(|r| (PERL, r)), &exe.to_string_lossy(), args);
    let mut env = env.to_vec();
    env.extend(ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Ok(Start { exe: srt, args: a, env, boxed: true })
}

/// The tool as a running process, in the sandbox when it is wanted (see plan), started in
/// `dir` (Claude Code takes its project from there) or in the user's home. `boxed` is
/// told how it started, for the next run's check (Boxed::fit).
pub fn launch(tool: AgentTool, exe: &Path, args: &[&str], env: &[(String, String)], dir: Option<&Path>, boxed: Option<&Boxed>) -> std::io::Result<Link> {
    let folders = folders();
    let s = plan(tool, exe, args, env, &folders);
    if let Some(b) = boxed { b.started(s.boxed.then_some(folders)); }
    let a: Vec<&str> = s.args.iter().map(String::as_str).collect();
    match dir {
        Some(d) => proc::launch_in(&s.exe, &a, &s.env, d),
        None => proc::launch(&s.exe, &a, &s.env),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relay_is_plain_lf_text() {
        assert!(!RELAY.contains('\r') && RELAY.starts_with("#!/usr/bin/perl\n") && RELAY.ends_with("$? >> 8);\n"));
    }

    #[test]
    fn lexical_full_paths() {
        assert_eq!(full("/a/b/"), "/a/b");
        assert_eq!(full("/a/./b/../c"), "/a/c");
        assert_eq!(full("/"), "/");
        assert_eq!(full("/../.."), "/");
    }

    #[test]
    fn hosts_are_checked_the_way_the_regex_did() {
        for ok in ["docs.example.com", "*.example.org", "localhost:8080", "localhost", "a-b.c1.io:65535"] { assert!(valid_host(ok), "{ok}"); }
        for bad in ["", "*", "*.com", "https://bad.example.com/x", "example", "a..b.com", "x.com:", "x.com:123456", "x.com:1a", "*.localhost", "-.", "a b.com"] { assert!(!valid_host(bad), "{bad}"); }
    }
}
