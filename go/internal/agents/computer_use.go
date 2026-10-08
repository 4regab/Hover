package agents

// Services/ComputerUse.cs: computer use for the agents, through Cua Driver
// (github.com/trycua/cua, MIT): its `cua-driver mcp` is handed to every session as an MCP
// server, so an agent can see and drive apps (the app it is building, a browser, a
// simulator) in the background, without the user's pointer moving or their focus
// changing. Off until switched on in Settings. Hover never drives anything itself and
// never passes Cua's own approval-bypass flags; each tool call still goes through the
// session's tool access (Ask first asks about it in the notch, Read only refuses it: an
// MCP tool is kind "other" to NeedsAsking and to every host's permission).
//
// On a Mac the grants belong to CuaDriver.app, not to Hover or the agent: the first
// `cua-driver mcp` starts that app's daemon through LaunchServices in the background and
// talks through it, so they are given once, to CuaDriver, with `permissions grant`.
// Install and grant run the maker's own commands.
//
// Hover offers it on macOS only. Cua Driver is built for the Mac (on Windows it has no
// guard, and on Linux it is a pre-release), the guard that keeps it out of the user's way
// needs perl, and the user decided to keep Cua to the Mac. Elsewhere the switch is off with
// CuaUnsupported beside it, no session is given the server, and nothing here installs or
// runs cua-driver.

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

const (
	CuaServerName = "cua-driver"
	CuaRepo       = "github.com/trycua/cua"
	// CuaUnsupported is shown beside the switch, and as the status, where computer use can't run.
	CuaUnsupported = "Computer use needs macOS."
)

// CuaSupported: computer use is a Mac's; see the file's note.
func CuaSupported() bool { return runtime.GOOS == "darwin" }

// McpServer is an MCP server a session is given, started over stdio by the tool itself:
// its name, command, arguments and environment.
type McpServer struct {
	Name, Command string
	Args          []string
	Env           [][2]string
}

func NewMcpServer(name, command string, args ...string) McpServer {
	return McpServer{Name: name, Command: command, Args: append([]string{}, args...)}
}

// CuaStatus is what Hover knows of Cua Driver: installed or not, its version, and (on a
// Mac) whether CuaDriver.app has the Accessibility and Screen Recording grants it needs.
// Permissions is "granted", "partial" (Accessibility only), "missing" or "unknown"; where
// computer use isn't offered it is not installed, "unknown", and the hint is CuaUnsupported.
type CuaStatus struct {
	Installed   bool
	Version     string
	Permissions string
	Hint        string
}

func (s CuaStatus) Ready() bool {
	return s.Installed && (s.Permissions == "granted" || s.Permissions == "partial")
}

// CuaExe is the cua-driver program, or "" when it isn't installed. PATH first; then where
// the installers put it, for a Hover started before the install changed PATH.
func CuaExe() string {
	if p := OnPath("cua-driver"); p != "" {
		return p
	}
	v := []string{filepath.Join(Home(), ".local", "bin", "cua-driver")}
	if runtime.GOOS == "darwin" {
		v = append(v, "/Applications/CuaDriver.app/Contents/MacOS/cua-driver")
	}
	for _, p := range v {
		if isFile(p) {
			return p
		}
	}
	return ""
}

// CuaServers are the MCP servers a new session gets now: Cua Driver's, when computer use is
// on and it is installed; otherwise none, whatever the setting says where it isn't offered.
// Where perl is it runs behind the guard (see CuaGuard), so an agent's computer use never
// takes the user's pointer, keyboard or focus.
func CuaServers() []McpServer {
	if !CuaSupported() {
		return nil
	}
	// An agent with a desktop of its own (spaces) never drives the user's.
	if !CurrentToggles().ComputerUse || SpacesWanted() {
		return nil
	}
	exe := CuaExe()
	if exe == "" {
		return nil
	}
	guard := ""
	if isFile(perlPath) {
		g, err := writeGuard()
		if err != nil {
			// Unguarded computer use would reach the user's pointer: none at all instead.
			core.Logf("computer use: couldn’t write the guard - %v", err)
			return nil
		}
		guard = g
	}
	return []McpServer{CuaServerFor(exe, guard)}
}

// CuaGuardDir is where the guard is written: Hover's own folder, which the sandbox lets the
// agent read here but not write, so the agent can't edit its way past it.
func CuaGuardDir() string { return filepath.Join(core.Support(), "cua") }

func writeGuard() (string, error) {
	dir := CuaGuardDir()
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return "", err
	}
	guard := filepath.Join(dir, "guard.pl")
	if b, err := os.ReadFile(guard); err != nil || string(b) != CuaGuard {
		if err := os.WriteFile(guard, []byte(CuaGuard), 0o666); err != nil {
			return "", err
		}
	}
	return guard, nil
}

// CuaServerFor is the server for a cua-driver at exe: `cua-driver mcp`, and behind the
// guard when there is one ("" for none). Never Cua's approval bypass: the session's tool
// access decides.
func CuaServerFor(exe, guard string) McpServer {
	if guard == "" {
		return NewMcpServer(CuaServerName, exe, "mcp")
	}
	return NewMcpServer(CuaServerName, perlPath, guard, exe, "mcp")
}

// CuaGuard sits between the agent and `cua-driver mcp` and keeps its computer use out of
// the user's way: the user goes on working while an agent tests. Every input goes to the
// app the agent names, in the background; input on the desktop scope or with no app named
// is refused; so are bringing an app forward, moving or resizing windows, killing apps,
// the clipboard, replays and changing Cua's own settings. The initialize answer tells the
// agent so. Everything else passes through untouched, a line at a time.
const CuaGuard = `#!/usr/bin/perl
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
`

// MARK: How each tool is given the servers

func pairsObj(env [][2]string) core.JSON {
	props := make([]core.Prop, len(env))
	for i, kv := range env {
		props[i] = core.P(kv[0], core.JStr(kv[1]))
	}
	return core.JObj(props...)
}

// AcpServers are the servers as ACP's session/new and session/load take them (stdio: name,
// command, args, and an env list, which ACP requires even when empty).
func AcpServers(servers []McpServer) core.JSON {
	items := make([]core.JSON, len(servers))
	for i, s := range servers {
		env := make([]core.JSON, len(s.Env))
		for j, kv := range s.Env {
			env[j] = core.JObj(core.P("name", core.JStr(kv[0])), core.P("value", core.JStr(kv[1])))
		}
		items[i] = core.JObj(core.P("name", core.JStr(s.Name)), core.P("command", core.JStr(s.Command)), core.P("args", jstrs(s.Args)), core.P("env", core.JArr(env...)))
	}
	return core.JArr(items...)
}

// Signature is what tells a running tool its servers changed: when it differs from the one
// it started with, the tool is restarted once nothing of it runs (Hover's MCP servers are
// fixed per process for OpenCode and per session for ACP).
func Signature(servers []McpServer) string {
	parts := make([]string, len(servers))
	for i, s := range servers {
		env := ""
		for _, kv := range s.Env {
			env += "\x00" + kv[0] + "=" + kv[1]
		}
		parts[i] = s.Name + "\x00" + s.Command + "\x00" + strings.Join(s.Args, "\x00") + env
	}
	return strings.Join(parts, "\n")
}

// putFirst sets key in an object's properties, keeping its place (the first) when it was there.
func putFirst(o []core.Prop, key string, v core.JSON) []core.Prop {
	if i := slices.IndexFunc(o, func(p core.Prop) bool { return p.Key == key }); i >= 0 {
		o[i].Val = v
		return o
	}
	return append(o, core.P(key, v))
}

// OpencodeConfig is OpenCode's inline config (OPENCODE_CONFIG_CONTENT, applied over the
// user's and the project's) with the servers added as local MCP servers. A config already
// in that variable is kept and added to; the existing text itself when there is nothing to
// add.
func OpencodeConfig(servers []McpServer, existing *string) *string {
	if len(servers) == 0 {
		return existing
	}
	var root []core.Prop
	if existing != nil && strings.TrimSpace(*existing) != "" {
		if v, err := core.ParseJSON(*existing); err == nil && v.Kind() == core.ObjKind {
			p, _ := v.Props()
			root = slices.Clone(p)
		}
	}
	var mcp []core.Prop
	if i := slices.IndexFunc(root, func(p core.Prop) bool { return p.Key == "mcp" }); i >= 0 && root[i].Val.Kind() == core.ObjKind {
		p, _ := root[i].Val.Props()
		mcp = slices.Clone(p)
	}
	for _, s := range servers {
		command := append([]string{s.Command}, s.Args...)
		entry := []core.Prop{core.P("type", core.JStr("local")), core.P("command", jstrs(command)), core.P("enabled", core.JBool(true))}
		if len(s.Env) > 0 {
			entry = append(entry, core.P("environment", pairsObj(s.Env)))
		}
		// Its first call on a Mac may start CuaDriver's daemon; OpenCode's 5 s default for
		// listing tools is short for that.
		entry = append(entry, core.P("timeout", core.JInt(30000)))
		mcp = putFirst(mcp, s.Name, core.JObj(entry...))
	}
	root = putFirst(root, "mcp", core.JObj(mcp...))
	return sp(core.JObj(root...).Compact())
}

// ClaudeConfig is Claude Code's --mcp-config: {"mcpServers": {name: {type, command, args,
// env}}}. The user's own servers (--setting-sources) stay beside them. nil when there is
// nothing.
func ClaudeConfig(servers []McpServer) *string {
	if len(servers) == 0 {
		return nil
	}
	all := make([]core.Prop, len(servers))
	for i, s := range servers {
		all[i] = core.P(s.Name, core.JObj(core.P("type", core.JStr("stdio")), core.P("command", core.JStr(s.Command)), core.P("args", jstrs(s.Args)), core.P("env", pairsObj(s.Env))))
	}
	return sp(core.JObj(core.P("mcpServers", core.JObj(all...))).Compact())
}

func CuaInstallHint() string {
	return `Install Cua Driver: /bin/bash -c "$(curl -fsSL https://cua.ai/driver/install.sh)"`
}

// MARK: Status

var cuaChecks struct {
	sync.Mutex
	known *struct {
		at time.Time
		s  CuaStatus
	}
	asking *struct {
		done chan struct{}
		s    CuaStatus
	}
}

var cuaListeners struct {
	sync.Mutex
	list []func()
}

// OnCuaChange is called, off the caller's goroutine, whenever the status or a setup's
// progress changes.
func OnCuaChange(f func()) {
	cuaListeners.Lock()
	cuaListeners.list = append(cuaListeners.list, f)
	cuaListeners.Unlock()
}

func cuaChanged() {
	cuaListeners.Lock()
	all := slices.Clone(cuaListeners.list)
	cuaListeners.Unlock()
	for _, f := range all {
		f()
	}
}

// CuaKnown is the last check, without running one.
func CuaKnown() (CuaStatus, bool) {
	cuaChecks.Lock()
	defer cuaChecks.Unlock()
	if cuaChecks.known == nil {
		return CuaStatus{}, false
	}
	return cuaChecks.known.s, true
}

// CuaCheck is installed, its version and its grants, from cua-driver's own commands. Kept
// for five minutes; a check already going is shared. Blocks: call it off the UI goroutine.
func CuaCheck(fresh bool) CuaStatus {
	cuaChecks.Lock()
	if !fresh && cuaChecks.known != nil && time.Since(cuaChecks.known.at) < 300*time.Second {
		s := cuaChecks.known.s
		cuaChecks.Unlock()
		return s
	}
	if w := cuaChecks.asking; w != nil {
		cuaChecks.Unlock()
		<-w.done
		return w.s
	}
	w := &struct {
		done chan struct{}
		s    CuaStatus
	}{done: make(chan struct{})}
	cuaChecks.asking = w
	cuaChecks.Unlock()
	s := cuaLook()
	cuaChecks.Lock()
	cuaChecks.known = &struct {
		at time.Time
		s  CuaStatus
	}{time.Now(), s}
	cuaChecks.asking = nil
	w.s = s
	close(w.done)
	cuaChecks.Unlock()
	cuaChanged()
	return s
}

func cuaLook() CuaStatus {
	// Nothing is run where computer use isn't offered: a cua-driver found on PATH is not asked.
	if !CuaSupported() {
		return CuaStatus{Permissions: "unknown", Hint: CuaUnsupported}
	}
	exe := CuaExe()
	if exe == "" {
		return CuaStatus{Permissions: "unknown", Hint: CuaInstallHint()}
	}
	vc, vt := Ask(exe, "--version")
	version := ""
	if vc == 0 {
		lines := strings.Split(strings.TrimSpace(vt), "\n")
		version = strings.TrimSpace(lines[len(lines)-1])
	}
	status := func(p, hint string) CuaStatus { return CuaStatus{true, version, p, hint} }
	ax, sr, ok := cuaPermissions(exe)
	// Only a running daemon can answer for CuaDriver.app. With computer use on, it is
	// started (in the background, as cua-driver itself does) so the answer is real rather
	// than "unknown".
	if !ok && CurrentToggles().ComputerUse && !daemonRunning(exe) {
		startDaemon()
		for range 10 {
			if ok {
				break
			}
			time.Sleep(500 * time.Millisecond)
			ax, sr, ok = cuaPermissions(exe)
		}
	}
	switch {
	case ok && ax && sr:
		return status("granted", "")
	case ok && ax:
		return status("partial", "Screen Recording isn’t granted to CuaDriver, so agents can read and act on windows but not see them.")
	case ok:
		return status("missing", "CuaDriver needs Accessibility and Screen Recording. Grant them once; agents can’t drive apps until then.")
	}
	return status("unknown", "CuaDriver hasn’t been given Accessibility and Screen Recording yet (or hasn’t been asked). Grant them once.")
}

// cuaPermissions are Accessibility and Screen Recording, as CuaDriver's daemon reports
// them; false when it can't say (no daemon, or its permission gate still waiting).
func cuaPermissions(exe string) (ax, sr, ok bool) {
	code, text := Ask(exe, "permissions", "status", "--json")
	if code != 0 {
		return false, false, false
	}
	return ParsePermissions(text)
}

// ParsePermissions reads `cua-driver permissions status --json`. An "unknown" answer
// carries no booleans (cua-driver leaves them out rather than guess), and is false here.
func ParsePermissions(text string) (ax, sr, ok bool) {
	i := strings.IndexByte(text, '{')
	if i < 0 {
		return false, false, false
	}
	v, err := core.ParseJSON(text[i:])
	if err != nil {
		return false, false, false
	}
	a, has := v.Get("accessibility")
	if !has || a.Kind() != core.BoolKind {
		return false, false, false
	}
	ax, _ = a.Bool()
	s, _ := v.Get("screen_recording")
	sr, _ = s.Bool()
	return ax, sr, true
}

func startDaemon() { Ask("/usr/bin/open", "-n", "-g", "-a", "CuaDriver", "--args", "serve") }

// EnsureDaemon: CuaDriver's daemon is up before a sandboxed agent's cua-driver looks for
// it: it can't start the daemon itself from inside the sandbox (no Launch Services there),
// so Hover does, in the background (-g) as cua-driver would. A Mac's; a no-op elsewhere.
// Blocks for up to a few seconds.
func EnsureDaemon() {
	if runtime.GOOS != "darwin" {
		return
	}
	exe := CuaExe()
	if exe == "" || daemonRunning(exe) {
		return
	}
	startDaemon()
	for range 20 {
		if daemonRunning(exe) {
			return
		}
		time.Sleep(250 * time.Millisecond)
	}
}

func daemonRunning(exe string) bool {
	code, text := Ask(exe, "status")
	t := strings.ToLower(text)
	return code == 0 && strings.Contains(t, "is running") && !strings.Contains(t, "not running")
}

// MARK: Install and grant

var cuaSetup struct {
	sync.Mutex
	progress SetupProgress
	running  *Cancel
}

func CuaSetup() SetupProgress {
	cuaSetup.Lock()
	defer cuaSetup.Unlock()
	return cuaSetup.progress
}

func CuaBusy() bool {
	cuaSetup.Lock()
	defer cuaSetup.Unlock()
	return cuaSetup.running != nil
}

// CuaCanGrant: granting is a Mac's; elsewhere there is nothing to grant.
func CuaCanGrant() bool { return runtime.GOOS == "darwin" }

func cuaReport(p SetupProgress) {
	cuaSetup.Lock()
	cuaSetup.progress = p
	cuaSetup.Unlock()
	cuaChanged()
}

func CuaCancel() {
	cuaSetup.Lock()
	c := cuaSetup.running
	cuaSetup.Unlock()
	if c != nil {
		c.Cancel()
	}
}

// CuaInstall installs Cua Driver with its maker's installer (CuaDriver.app in
// /Applications and cua-driver in ~/.local/bin), then asks for its grants. Blocks until
// done; it runs only when the user asks, and never where computer use isn't offered.
func CuaInstall() {
	cuaGo("installing", "Installing Cua Driver…", func(ct *Cancel) error {
		if !CuaSupported() {
			return &StepError{Msg: CuaUnsupported}
		}
		// Its PATH line isn't added to the user's shell files: ~/.local/bin is on Hover's
		// PATH already, and the tools find cua-driver through Hover.
		if err := cuaStep("installing", "/bin/bash", []string{"-c", "set -o pipefail; curl -fsSL https://cua.ai/driver/install.sh | bash -s -- --no-modify-path"}, 600*time.Second, "Couldn’t install Cua Driver", ct); err != nil {
			return err
		}
		s := CuaCheck(true)
		if !s.Installed {
			return &StepError{Msg: "The installer finished, but cua-driver still isn’t found. " + CuaInstallHint()}
		}
		if CuaCanGrant() && s.Permissions != "granted" && s.Permissions != "partial" {
			return grantSteps(ct)
		}
		return nil
	})
}

// CuaGrant asks macOS for CuaDriver's Accessibility and Screen Recording (its own grant
// command; the dialogs name CuaDriver), and waits up to four minutes for them. Blocks.
func CuaGrant() {
	cuaGo("granting", "Approve CuaDriver in the dialogs and in System Settings…", grantSteps)
}

func grantSteps(ct *Cancel) error {
	if !CuaCanGrant() {
		return nil
	}
	exe := CuaExe()
	if exe == "" {
		return &StepError{Msg: CuaInstallHint()}
	}
	cuaReport(SetupProgress{Step: sp("granting"), Line: "Approve CuaDriver in the dialogs and in System Settings → Privacy & Security…"})
	err := cuaStep("granting", exe, []string{"permissions", "grant"}, 240*time.Second, "Permissions weren’t granted", ct)
	CuaCheck(true)
	return err
}

func cuaStep(name, exe string, args []string, timeout time.Duration, failure string, ct *Cancel) error {
	return streamStep(exe, args, nil, timeout, failure, ct, func(l string) { cuaReport(SetupProgress{Step: sp(name), Line: l}) })
}

func cuaGo(step, line string, work func(*Cancel) error) {
	ct := NewCancel()
	cuaSetup.Lock()
	if cuaSetup.running != nil {
		cuaSetup.Unlock()
		return
	}
	cuaSetup.running = ct
	cuaSetup.Unlock()
	cuaReport(SetupProgress{Step: sp(step), Line: line})
	err := work(ct)
	// No longer under way before the end is told, so its message never says busy beside
	// the result.
	cuaSetup.Lock()
	cuaSetup.running = nil
	cuaSetup.Unlock()
	if se, ok := err.(*StepError); ok && !se.Cancelled {
		cuaReport(SetupProgress{Error: sp(se.Msg)})
	} else {
		cuaReport(SetupProgress{})
	}
	cuaChanged()
}
