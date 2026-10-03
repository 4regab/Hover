//! Services/ComputerUse.cs: computer use for the agents, through Cua Driver
//! (github.com/trycua/cua, MIT): its `cua-driver mcp` is handed to every session as an MCP
//! server, so an agent can see and drive apps (the app it is building, a browser, a
//! simulator) in the background, without the user's pointer moving or their focus
//! changing. Off until switched on in Settings. Hover never drives anything itself and
//! never passes Cua's own approval-bypass flags; each tool call still goes through the
//! session's tool access (Ask first asks about it in the notch, Read only refuses it:
//! an MCP tool is kind "other" to ask::needs_asking and to every host's permission).
//!
//! On a Mac the grants belong to CuaDriver.app, not to Hover or the agent: the first
//! `cua-driver mcp` starts that app's daemon through LaunchServices in the background and
//! talks through it, so they are given once, to CuaDriver, with `permissions grant`.
//! Install and grant run the maker's own commands.
//!
//! Hover offers it on macOS only. Cua Driver is built for the Mac (on Windows it has no
//! guard, and on Linux it is a pre-release), the guard that keeps it out of the user's way
//! needs perl, and the user decided to keep Cua to the Mac. Elsewhere the switch is off with
//! UNSUPPORTED beside it, no session is given the server, and nothing here installs or
//! runs cua-driver.

use crate::agents::{ask, toggles};
use crate::cancel::Cancel;
use crate::proc::{home, on_path};
use crate::setup::{stream_step, StepError};
use hover_core::json::{self, Json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const SERVER_NAME: &str = "cua-driver";
pub const REPO: &str = "github.com/trycua/cua";
/// Shown beside the switch, and as the status, where computer use can't run.
pub const UNSUPPORTED: &str = "Computer use needs macOS.";

/// Computer use is a Mac's; see the module note.
pub fn supported() -> bool { cfg!(target_os = "macos") }

/// An MCP server a session is given, started over stdio by the tool itself: its name,
/// command, arguments and environment.
#[derive(Clone, Debug, PartialEq)]
pub struct McpServer { pub name: String, pub command: String, pub args: Vec<String>, pub env: Vec<(String, String)> }

impl McpServer {
    pub fn new(name: &str, command: &str, args: &[&str]) -> McpServer {
        McpServer { name: name.into(), command: command.into(), args: args.iter().map(|s| s.to_string()).collect(), env: vec![] }
    }
}

/// What Hover knows of Cua Driver: installed or not, its version, and (on a Mac) whether
/// CuaDriver.app has the Accessibility and Screen Recording grants it needs. Permissions
/// is "granted", "partial" (Accessibility only), "missing" or "unknown"; where computer use
/// isn't offered it is not installed, "unknown", and the hint is UNSUPPORTED.
#[derive(Clone, Debug, PartialEq)]
pub struct Status { pub installed: bool, pub version: String, pub permissions: &'static str, pub hint: String }

impl Status {
    pub fn ready(&self) -> bool { self.installed && matches!(self.permissions, "granted" | "partial") }
}

/// The cua-driver program, or none when it isn't installed. PATH first; then where the
/// installers put it, for a Hover started before the install changed PATH.
pub fn exe() -> Option<PathBuf> { on_path("cua-driver").or_else(|| fallbacks().into_iter().find(|p| p.is_file())) }

fn fallbacks() -> Vec<PathBuf> {
    let mut v = vec![home().join(".local").join("bin").join("cua-driver")];
    if cfg!(target_os = "macos") { v.push(PathBuf::from("/Applications/CuaDriver.app/Contents/MacOS/cua-driver")); }
    v
}

/// The MCP servers a new session gets now: Cua Driver's, when computer use is on and it
/// is installed; otherwise none, whatever the setting says where it isn't offered. Where
/// perl is it runs behind the guard (see GUARD), so an agent's computer use never takes the
/// user's pointer, keyboard or focus.
pub fn servers() -> Vec<McpServer> {
    if !supported() { return vec![]; }
    // An agent with a desktop of its own (spaces.rs) never drives the user's.
    if !toggles().computer_use || crate::spaces::wanted() { return vec![]; }
    let Some(exe) = exe() else { return vec![] };
    let guard = if !Path::new(PERL).is_file() { None } else {
        match write_guard() {
            Ok(g) => Some(g),
            // Unguarded computer use would reach the user's pointer: none at all instead.
            Err(e) => { hover_core::log::line(&format!("computer use: couldn’t write the guard - {e}")); return vec![]; }
        }
    };
    vec![server_for(&exe.to_string_lossy(), guard.as_deref().map(|g| g.to_string_lossy()).as_deref())]
}

const PERL: &str = "/usr/bin/perl";

/// Where the guard is written: Hover's own folder, which the sandbox lets the agent read
/// here but not write, so the agent can't edit its way past it.
pub fn guard_dir() -> PathBuf { hover_core::paths::support().join("cua") }

fn write_guard() -> std::io::Result<PathBuf> {
    let dir = guard_dir();
    std::fs::create_dir_all(&dir)?;
    let guard = dir.join("guard.pl");
    if std::fs::read_to_string(&guard).ok().as_deref() != Some(GUARD) { std::fs::write(&guard, GUARD)?; }
    Ok(guard)
}

/// The server for a cua-driver at `exe`: `cua-driver mcp`, and behind the guard when
/// there is one. Never Cua's approval bypass: the session's tool access decides.
pub fn server_for(exe: &str, guard: Option<&str>) -> McpServer {
    match guard {
        None => McpServer::new(SERVER_NAME, exe, &["mcp"]),
        Some(g) => McpServer::new(SERVER_NAME, PERL, &[g, exe, "mcp"]),
    }
}

/// Sits between the agent and `cua-driver mcp` and keeps its computer use out of the
/// user's way: the user goes on working while an agent tests. Cua can act in the
/// background (AX actions, events posted to one app, its own drawn cursor), but its
/// ladder ends in "foreground", which fronts the window and moves the real pointer, and
/// some tools act on the whole desktop. Here every input goes to the app the agent names,
/// in the background: delivery_mode is taken out of the tool list and a "foreground"
/// asked for anyway becomes "background"; input on the desktop scope or with no app named
/// (it would land in whatever the user is typing in) is refused; so are bringing an app
/// forward, moving or resizing windows, killing apps, the clipboard, replays and changing
/// Cua's own settings. The initialize answer tells the agent so. Everything else passes
/// through untouched, a line at a time (MCP's stdio framing), with blocking writes, as in
/// the sandbox's relay.
pub const GUARD: &str = r###"#!/usr/bin/perl
# Hover's guard for cua-driver mcp: computer use stays in the background, out of the
# user's way. See GUARD in Hover's source (hover-agents/src/computer_use.rs).
use strict; use warnings;
use POSIX qw(:sys_wait_h EAGAIN EINTR);
use IO::Select;
use JSON::PP;
die "usage: guard.pl cua-driver mcp\n" unless @ARGV;
my $json = JSON::PP->new->utf8->canonical;
my %blocked = map { $_ => 1 } qw(bring_to_front set_window_frame kill_app clipboard_read clipboard_write
    replay_trajectory set_config escalate_session browser_prepare install_extension install_ffmpeg);
my %input = map { $_ => 1 } qw(click double_click right_click drag scroll press_key hotkey type_text set_value move_cursor);
my $note = "Hover runs computer use in the background so the user can keep working: every action goes to the app "
    . "you name (pid and window_id, or an element_token), never to the frontmost app, the user's pointer or keyboard. "
    . "Prefer element_token; x/y clicks are posted to the window. Foreground delivery, desktop-scope input, "
    . "bring_to_front, moving windows, killing apps and the clipboard are turned off. Launch apps with launch_app (it "
    . "stays in the background) and check results with get_window_state.";
my %listing;   # ids of the agent's tools/list and initialize requests, to fix their answers

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
for my $sig (qw(TERM INT HUP)) { $SIG{$sig} = sub { kill $sig, $pid; } }
$SIG{PIPE} = 'IGNORE';

sub put {
    my ($fh, $s) = @_;
    while (length $s) {
        my $n = syswrite($fh, $s);
        if (!defined $n) { return 0 if $! != EAGAIN && $! != EINTR; IO::Select->new($fh)->can_write(1); next; }
        substr($s, 0, $n) = '';
    }
    return 1;
}
sub refuse {
    my ($id, $why) = @_;
    put(\*STDOUT, $json->encode({ jsonrpc => '2.0', id => $id, result => { isError => JSON::PP::true,
        content => [{ type => 'text', text => "$why $note" }] } }) . "\n");
}
sub named { my ($a) = @_; defined $a->{pid} || defined $a->{element_token} || (ref $a->{target} eq 'HASH' && defined $a->{target}{pid}) }
sub desktop { my ($a) = @_; ($a->{scope} // '') eq 'desktop' || (ref $a->{target} eq 'HASH' && defined $a->{target}{display_id}) }

# One message from the agent: undef when it was answered here, else the line to pass on.
sub from_agent {
    my ($m) = @_;
    return 1 unless ref $m eq 'HASH';
    my $method = $m->{method} // '';
    $listing{$m->{id}} = 1 if defined $m->{id} && ($method eq 'tools/list' || $method eq 'initialize');
    return 1 unless $method eq 'tools/call' && ref $m->{params} eq 'HASH';
    my $name = $m->{params}{name} // '';
    my $a = $m->{params}{arguments}; $a = $m->{params}{arguments} = {} unless ref $a eq 'HASH';
    if ($blocked{$name}) { refuse($m->{id}, "$name is turned off in Hover."); return undef; }
    return 1 unless $input{$name};
    if ($name eq 'move_cursor') {
        if (desktop($a)) { refuse($m->{id}, 'Moving the real pointer is turned off in Hover; the agent cursor moves without scope.'); return undef; }
        return 1;
    }
    if (desktop($a) || !named($a)) { refuse($m->{id}, "$name needs the app it acts on (pid and window_id, or an element_token)."); return undef; }
    $a->{delivery_mode} = 'background' if exists $a->{delivery_mode};
    return 2;
}

# One answer from cua-driver: its tool list without what is off, and the note.
sub from_driver {
    my ($m) = @_;
    return 0 unless ref $m eq 'HASH' && defined $m->{id} && delete $listing{$m->{id}} && ref $m->{result} eq 'HASH';
    my $r = $m->{result};
    if (ref $r->{tools} eq 'ARRAY') {
        $r->{tools} = [grep { !$blocked{$_->{name} // ''} } @{$r->{tools}}];
        for my $t (@{$r->{tools}}) {
            my $p = ref $t->{inputSchema} eq 'HASH' ? $t->{inputSchema}{properties} : undef;
            next unless ref $p eq 'HASH';
            delete $p->{delivery_mode};
            $p->{scope}{enum} = ['window'] if $input{$t->{name} // ''} && ref $p->{scope} eq 'HASH';
        }
    }
    $r->{instructions} = join("\n\n", grep { length } ($r->{instructions} // ''), $note) if defined $r->{protocolVersion};
    return 1;
}

my $sel = IO::Select->new(\*STDIN, $out_r);
my ($from_agent, $from_driver) = ('', '');
my $stdin_open = 1;
my $parent = getppid();
while ($sel->count) {
    if (getppid() != $parent) { kill 'TERM', $pid; last; }
    for my $fh ($sel->can_read(1)) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        if ($fh == $out_r) {
            if ($n == 0) { $sel->remove($out_r); put(\*STDOUT, $from_driver) if length $from_driver; $from_driver = ''; next; }
            $from_driver .= $chunk;
            while ((my $i = index($from_driver, "\n")) >= 0) {
                my $line = substr($from_driver, 0, $i + 1, '');
                # Only the answers to a listing are read; screenshots pass as they are.
                if (%listing && $line =~ /"id"/) {
                    my $m = eval { $json->decode($line) };
                    $line = $json->encode($m) . "\n" if $m && from_driver($m);
                }
                put(\*STDOUT, $line) or exit 1;
            }
        } else {
            if ($n == 0) { $sel->remove(\*STDIN); close $in_w; $stdin_open = 0; next; }
            $from_agent .= $chunk;
            while ((my $i = index($from_agent, "\n")) >= 0) {
                my $line = substr($from_agent, 0, $i + 1, '');
                my $m = $line =~ /\S/ ? eval { $json->decode($line) } : undef;
                if (ref $m eq 'ARRAY') {
                    # A batch: what is off is answered here, the rest goes on.
                    my @keep = grep { defined from_agent($_) } @$m;
                    next unless @keep;
                    $line = $json->encode(\@keep) . "\n";
                } elsif ($m) {
                    my $k = from_agent($m);
                    next unless defined $k;
                    $line = $json->encode($m) . "\n" if $k == 2;
                }
                put($in_w, $line) or do { $sel->remove(\*STDIN); $stdin_open = 0; last; };
            }
        }
    }
    last if !$sel->exists($out_r) && !$stdin_open;
    last if !$sel->exists($out_r) && waitpid($pid, WNOHANG) > 0;
}
close $in_w if $stdin_open;
waitpid($pid, 0);
exit($? & 127 ? 128 + ($? & 127) : $? >> 8);
"###;

// MARK: How each tool is given the servers

fn strs(v: &[String]) -> Json { Json::Arr(v.iter().map(|s| Json::str(s.as_str())).collect()) }
fn pairs(env: &[(String, String)]) -> Json { Json::Obj(env.iter().map(|(k, v)| (k.clone(), Json::str(v.as_str()))).collect()) }

/// The servers as ACP's session/new and session/load take them (stdio: name, command,
/// args, and an env list, which ACP requires even when empty).
pub fn acp(servers: &[McpServer]) -> Json {
    Json::Arr(servers.iter().map(|s| Json::obj(vec![
        ("name", Json::str(s.name.as_str())), ("command", Json::str(s.command.as_str())), ("args", strs(&s.args)),
        ("env", Json::Arr(s.env.iter().map(|(k, v)| Json::obj(vec![("name", Json::str(k.as_str())), ("value", Json::str(v.as_str()))])).collect())),
    ])).collect())
}

/// What tells a running tool its servers changed: when it differs from the one it started
/// with, the tool is restarted once nothing of it runs (Hover's MCP servers are fixed per
/// process for OpenCode and per session for ACP).
pub fn signature(servers: &[McpServer]) -> String {
    servers.iter().map(|s| {
        let env: String = s.env.iter().map(|(k, v)| format!("\0{k}={v}")).collect();
        format!("{}\0{}\0{}{env}", s.name, s.command, s.args.join("\0"))
    }).collect::<Vec<_>>().join("\n")
}

/// Sets `key` in an object, keeping its place when it was there.
fn put(obj: &mut Vec<(String, Json)>, key: &str, v: Json) {
    match obj.iter_mut().find(|(k, _)| k == key) { Some(slot) => slot.1 = v, None => obj.push((key.into(), v)) }
}

/// OpenCode's inline config (OPENCODE_CONFIG_CONTENT, applied over the user's and the
/// project's) with the servers added as local MCP servers. A config already in that
/// variable is kept and added to; the existing text itself when there is nothing to add.
pub fn opencode_config(servers: &[McpServer], existing: Option<&str>) -> Option<String> {
    if servers.is_empty() { return existing.map(str::to_owned); }
    let mut root = match existing.filter(|e| !e.trim().is_empty()).and_then(|e| json::parse(e).ok()) { Some(Json::Obj(o)) => o, _ => vec![] };
    let mut mcp = match root.iter().find(|(k, _)| k == "mcp") { Some((_, Json::Obj(o))) => o.clone(), _ => vec![] };
    for s in servers {
        let mut command = vec![s.command.clone()];
        command.extend(s.args.iter().cloned());
        let mut entry = vec![("type", Json::str("local")), ("command", strs(&command)), ("enabled", Json::Bool(true))];
        if !s.env.is_empty() { entry.push(("environment", pairs(&s.env))); }
        // Its first call on a Mac may start CuaDriver's daemon; OpenCode's 5 s default
        // for listing tools is short for that.
        entry.push(("timeout", Json::int(30000)));
        put(&mut mcp, &s.name, Json::obj(entry));
    }
    put(&mut root, "mcp", Json::Obj(mcp));
    Some(Json::Obj(root).compact())
}

/// Claude Code's --mcp-config: {"mcpServers": {name: {type, command, args, env}}}. The
/// user's own servers (--setting-sources) stay beside them. None when there is nothing.
pub fn claude_config(servers: &[McpServer]) -> Option<String> {
    if servers.is_empty() { return None; }
    let all = servers.iter().map(|s| (s.name.clone(), Json::obj(vec![
        ("type", Json::str("stdio")), ("command", Json::str(s.command.as_str())), ("args", strs(&s.args)), ("env", pairs(&s.env)),
    ]))).collect();
    Some(Json::obj(vec![("mcpServers", Json::Obj(all))]).compact())
}

pub fn install_hint() -> &'static str { "Install Cua Driver: /bin/bash -c \"$(curl -fsSL https://cua.ai/driver/install.sh)\"" }

// MARK: Status

struct Checks { known: Option<(Instant, Status)>, asking: Option<Arc<(Mutex<Option<Status>>, Condvar)>> }

static CHECKS: Mutex<Checks> = Mutex::new(Checks { known: None, asking: None });

type Listener = Arc<dyn Fn() + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Called, off the caller's thread, whenever the status or a setup's progress changes.
pub fn on_change(f: impl Fn() + Send + Sync + 'static) { LISTENERS.lock().unwrap().push(Arc::new(f)); }

fn changed() {
    let all: Vec<Listener> = LISTENERS.lock().unwrap().clone();
    for f in all { f(); }
}

/// The last check, without running one.
pub fn known() -> Option<Status> { CHECKS.lock().unwrap().known.as_ref().map(|k| k.1.clone()) }

/// Installed, its version and its grants, from cua-driver's own commands. Kept for five
/// minutes; a check already going is shared. Blocks: call it off the UI thread.
pub fn check(fresh: bool) -> Status {
    let wait = {
        let mut c = CHECKS.lock().unwrap();
        if !fresh { if let Some((at, s)) = &c.known { if at.elapsed() < Duration::from_secs(300) { return s.clone(); } } }
        match &c.asking {
            Some(w) => Some(w.clone()),
            None => { c.asking = Some(Arc::new((Mutex::new(None), Condvar::new()))); None }
        }
    };
    if let Some(w) = wait {
        let g = w.1.wait_while(w.0.lock().unwrap(), |r| r.is_none()).unwrap();
        return g.clone().unwrap();
    }
    let s = look();
    {
        let mut c = CHECKS.lock().unwrap();
        c.known = Some((Instant::now(), s.clone()));
        if let Some(w) = c.asking.take() { *w.0.lock().unwrap() = Some(s.clone()); w.1.notify_all(); }
    }
    changed();
    s
}

fn look() -> Status {
    // Nothing is run where computer use isn't offered: a cua-driver found on PATH is not asked.
    if !supported() { return Status { installed: false, version: String::new(), permissions: "unknown", hint: UNSUPPORTED.into() }; }
    let Some(exe) = exe() else { return Status { installed: false, version: String::new(), permissions: "unknown", hint: install_hint().into() } };
    let (vc, vt) = ask(&exe, &["--version"]);
    let version = if vc == 0 { vt.trim().split('\n').next_back().unwrap_or("").trim().to_owned() } else { String::new() };
    let status = |permissions, hint: &str| Status { installed: true, version: version.clone(), permissions, hint: hint.into() };
    let mut perms = permissions(&exe);
    // Only a running daemon can answer for CuaDriver.app. With computer use on, it is
    // started (in the background, as cua-driver itself does) so the answer is real
    // rather than "unknown".
    if perms.is_none() && toggles().computer_use && !daemon_running(&exe) {
        start_daemon();
        for _ in 0..10 {
            if perms.is_some() { break; }
            std::thread::sleep(Duration::from_millis(500));
            perms = permissions(&exe);
        }
    }
    match perms {
        Some((true, true)) => status("granted", ""),
        Some((true, false)) => status("partial", "Screen Recording isn’t granted to CuaDriver, so agents can read and act on windows but not see them."),
        Some((false, _)) => status("missing", "CuaDriver needs Accessibility and Screen Recording. Grant them once; agents can’t drive apps until then."),
        None => status("unknown", "CuaDriver hasn’t been given Accessibility and Screen Recording yet (or hasn’t been asked). Grant them once."),
    }
}

/// Accessibility and Screen Recording, as CuaDriver's daemon reports them; none when it
/// can't say (no daemon, or its permission gate still waiting).
fn permissions(exe: &Path) -> Option<(bool, bool)> {
    let (code, text) = ask(exe, &["permissions", "status", "--json"]);
    if code != 0 { return None; }
    parse_permissions(&text)
}

/// Reads `cua-driver permissions status --json`. An "unknown" answer carries no booleans
/// (cua-driver leaves them out rather than guess), and is none here.
pub fn parse_permissions(text: &str) -> Option<(bool, bool)> {
    let v = json::parse(&text[text.find('{')?..]).ok()?;
    let Some(Json::Bool(ax)) = v.get("accessibility") else { return None };
    Some((*ax, matches!(v.get("screen_recording"), Some(Json::Bool(true)))))
}

fn start_daemon() {
    // A test never opens the user's CuaDriver.app.
    if hover_core::in_test_sandbox() { return; }
    let _ = ask(Path::new("/usr/bin/open"), &["-n", "-g", "-a", "CuaDriver", "--args", "serve"]);
}

/// CuaDriver's daemon is up before a sandboxed agent's cua-driver looks for it: it can't
/// start the daemon itself from inside the sandbox (no Launch Services there), so Hover
/// does, in the background (-g) as cua-driver would. A Mac's; a no-op elsewhere. Blocks
/// for up to a few seconds.
pub fn ensure_daemon() {
    if !cfg!(target_os = "macos") { return; }
    let Some(exe) = exe() else { return };
    if daemon_running(&exe) { return; }
    start_daemon();
    for _ in 0..20 {
        if daemon_running(&exe) { return; }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn daemon_running(exe: &Path) -> bool {
    let (code, text) = ask(exe, &["status"]);
    let t = text.to_lowercase();
    code == 0 && t.contains("is running") && !t.contains("not running")
}

// MARK: Install and grant

/// What a setup is doing ("installing" or "granting"), its newest line, and why it
/// stopped if it failed.
pub use crate::setup::Progress;

static PROGRESS: Mutex<Progress> = Mutex::new(Progress { step: None, line: String::new(), error: None });
static RUNNING: Mutex<Option<Cancel>> = Mutex::new(None);

pub fn setup() -> Progress { PROGRESS.lock().unwrap().clone() }
pub fn busy() -> bool { RUNNING.lock().unwrap().is_some() }

/// Granting is a Mac's: elsewhere there is nothing to grant.
pub fn can_grant() -> bool { cfg!(target_os = "macos") }

fn report(p: Progress) { *PROGRESS.lock().unwrap() = p; changed(); }

pub fn cancel() { if let Some(c) = RUNNING.lock().unwrap().as_ref() { c.cancel(); } }

/// Installs Cua Driver with its maker's installer (CuaDriver.app in /Applications and
/// cua-driver in ~/.local/bin), then asks for its grants. Blocks until done; it runs only
/// when the user asks, and never where computer use isn't offered.
pub fn install() {
    go("installing", "Installing Cua Driver…", |ct| {
        if !supported() { return Err(StepError::Failed(UNSUPPORTED.into())); }
        // Its PATH line isn't added to the user's shell files: ~/.local/bin is on
        // Hover's PATH already, and the tools find cua-driver through Hover.
        let args = ["-c", "set -o pipefail; curl -fsSL https://cua.ai/driver/install.sh | bash -s -- --no-modify-path"];
        step("installing", Path::new("/bin/bash"), &args, Duration::from_secs(600), "Couldn’t install Cua Driver", ct)?;
        let s = check(true);
        if !s.installed { return Err(StepError::Failed(format!("The installer finished, but cua-driver still isn’t found. {}", install_hint()))); }
        if can_grant() && !matches!(s.permissions, "granted" | "partial") { grant_steps(ct)?; }
        Ok(())
    });
}

/// Asks macOS for CuaDriver's Accessibility and Screen Recording (its own grant command;
/// the dialogs name CuaDriver), and waits up to four minutes for them. Blocks.
pub fn grant() { go("granting", "Approve CuaDriver in the dialogs and in System Settings…", grant_steps); }

fn grant_steps(ct: &Cancel) -> Result<(), StepError> {
    if !can_grant() { return Ok(()); }
    let Some(exe) = exe() else { return Err(StepError::Failed(install_hint().into())) };
    report(Progress { step: Some("granting".into()), line: "Approve CuaDriver in the dialogs and in System Settings → Privacy & Security…".into(), error: None });
    let r = step("granting", &exe, &["permissions", "grant"], Duration::from_secs(240), "Permissions weren’t granted", ct);
    check(true);
    r
}

fn step(name: &str, exe: &Path, args: &[&str], timeout: Duration, failure: &str, ct: &Cancel) -> Result<(), StepError> {
    stream_step(exe, args, &[], timeout, failure, ct, &|l| report(Progress { step: Some(name.into()), line: l, error: None }))
}

fn go(step: &str, line: &str, work: impl FnOnce(&Cancel) -> Result<(), StepError>) {
    let ct = Cancel::new();
    {
        let mut r = RUNNING.lock().unwrap();
        if r.is_some() { return; }
        *r = Some(ct.clone());
    }
    report(Progress { step: Some(step.into()), line: line.into(), error: None });
    match work(&ct) {
        Ok(()) | Err(StepError::Cancelled) => report(Progress::default()),
        Err(StepError::Failed(m)) => report(Progress { step: None, line: String::new(), error: Some(m) }),
    }
    *RUNNING.lock().unwrap() = None;
    changed();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_is_plain_lf_text() {
        assert!(!GUARD.contains('\r') && GUARD.starts_with("#!/usr/bin/perl\n") && GUARD.ends_with("$? >> 8);\n"));
    }

    #[test]
    fn permission_reports_are_read_only_when_cua_driver_vouches_for_them() {
        // As cua-driver 0.31 prints them: no booleans at all when it can't say.
        assert_eq!(parse_permissions("{\n  \"daemon_running\": true,\n  \"reason\": \"…\",\n  \"status\": \"unknown\"\n}"), None);
        assert_eq!(parse_permissions(r#"{"accessibility":true,"screen_recording":true,"source":{"attribution":"driver-daemon"}}"#), Some((true, true)));
        assert_eq!(parse_permissions("note: proxying\n{\"accessibility\":true,\"screen_recording\":false}"), Some((true, false)));
        assert_eq!(parse_permissions(r#"{"accessibility":false}"#), Some((false, false)));
        assert_eq!(parse_permissions("garbage"), None);
    }
}
