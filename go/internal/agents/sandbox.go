package agents

// Services/Sandbox.cs: every agent tool runs inside Anthropic's sandbox-runtime (srt,
// Apache-2.0, github.com/anthropics/sandbox-runtime), the sandbox Claude Code runs commands
// in: sandbox-exec with a generated profile on a Mac, bubblewrap on Linux. It works in the
// background without getting in the way of the person at the computer:
//   - writes only to the folders its sessions work in, the tool's own state and caches, and
//     temp files; nothing else of the user's can be changed;
//   - keys, keychains, mail, messages, browsers' data and other apps' data (Office's
//     included) can't be read, nor Hover's own;
//   - no window server and no Apple Events (srt's macOS profile has neither): no window
//     drawn, no focus taken, no app launched or scripted;
//   - the network only through srt's proxy, to the tool's own service, package registries
//     and GitHub, plus the hosts in allowed-domains.txt.
//
// Computer use still works: the agent's cua-driver connects to CuaDriver's daemon, which
// Hover starts outside the sandbox, over its one socket.
//
// The folders are fixed when the tool starts, so a tool started for some is started again
// (when nothing of it runs) for a session in another. Off with the Sandbox setting; never
// nested inside another sandbox (HOVER_SANDBOXED=1). macOS and Linux; srt's Windows support
// is an alpha that can't reach tools installed for the user, which is where Kiro, Cursor
// and OpenCode go, so it isn't used there. A tool whose sandbox can't be had (srt missing,
// the setting off) starts as it always did, and hover.log says why.

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/4regab/Hover/go/internal/core"
)

// SandboxVersion is the srt release Hover was checked against, and installs.
const (
	SandboxVersion = "0.0.78"
	SandboxPackage = "@anthropic-ai/sandbox-runtime"
	// SandboxUnsupported is what Settings shows beside the switch where the sandbox can't run.
	SandboxUnsupported = "The sandbox needs macOS or Linux."
)

func SandboxSupported() bool { return runtime.GOOS == "darwin" || runtime.GOOS == "linux" }

// SandboxNote is why the switch is disabled here, or nil.
func SandboxNote() *string {
	if SandboxSupported() {
		return nil
	}
	return sp(SandboxUnsupported)
}

// SandboxInside: Hover itself runs in a sandbox already (a developer's run under srt):
// another one inside it would fail, and the outer one holds.
func SandboxInside() bool { return os.Getenv("HOVER_SANDBOXED") == "1" }

// SandboxWanted: whether tools are to be started in it.
func SandboxWanted() bool { return CurrentToggles().Sandbox && SandboxSupported() && !SandboxInside() }

// SandboxActive: wanted, and everything it needs is there: what decides how a tool starts.
func SandboxActive() bool { return SandboxWanted() && SandboxMissing() == nil }

// SandboxExe is srt; "" when it isn't installed.
func SandboxExe() string {
	if p := OnPath("srt"); p != "" {
		return p
	}
	h := Home()
	for _, p := range []string{"/opt/homebrew/bin/srt", "/usr/local/bin/srt", filepath.Join(h, ".npm-global/bin/srt"), filepath.Join(h, ".local/bin/srt")} {
		if isFile(p) {
			return p
		}
	}
	return ""
}

func toolAt(name string, fallbacks ...string) string {
	if p := OnPath(name); p != "" {
		return p
	}
	for _, p := range fallbacks {
		if isFile(p) {
			return p
		}
	}
	return ""
}

// SandboxMissing is what is missing for the sandbox to run, as one line to show; nil when
// nothing (or when it isn't wanted).
func SandboxMissing() *string {
	if !SandboxWanted() {
		return nil
	}
	mac := runtime.GOOS == "darwin"
	var need []string
	if SandboxExe() == "" {
		need = append(need, fmt.Sprintf("npm install -g %s@%s", SandboxPackage, SandboxVersion))
	}
	// srt finds the paths it must keep closed with ripgrep (and on Linux needs bubblewrap
	// and socat for the sandbox itself).
	if toolAt("rg", "/opt/homebrew/bin/rg", "/usr/local/bin/rg", "/usr/bin/rg") == "" {
		if mac {
			need = append(need, "brew install ripgrep")
		} else {
			need = append(need, "install ripgrep")
		}
	}
	if runtime.GOOS == "linux" && toolAt("bwrap", "/usr/bin/bwrap") == "" {
		need = append(need, "install bubblewrap")
	}
	if runtime.GOOS == "linux" && toolAt("socat", "/usr/bin/socat") == "" {
		need = append(need, "install socat")
	}
	return MissingLine(need)
}

// MissingLine is the line Settings shows for what is missing.
func MissingLine(need []string) *string {
	if len(need) == 0 {
		return nil
	}
	return sp(fmt.Sprintf("Hover runs agents in a sandbox, which isn’t set up yet: %s. (Or turn the sandbox off in Settings.)", strings.Join(need, ", then ")))
}

// MARK: Folders

var sandboxSeen = struct {
	sync.Mutex
	set map[string]bool
}{set: map[string]bool{}}

// Remember: a folder a session works in, so the next start of its tool covers it.
func Remember(folder string) {
	if !SandboxSupported() || !UsableFolder(folder) {
		return
	}
	sandboxSeen.Lock()
	defer sandboxSeen.Unlock()
	sandboxSeen.set[SandboxFull(folder)] = true
	// A chat made in a linked worktree (by an earlier version) commits into the main
	// checkout's .git: the tool may write there too.
	for _, g := range GitDirs(folder) {
		if UsableFolder(g) {
			sandboxSeen.set[SandboxFull(g)] = true
		}
	}
}

// SandboxFolders are the folders a tool started now gets: the ones sessions use this run,
// and the folder new tasks start in. Sorted, as Rust's BTreeSet keeps them.
func SandboxFolders() []string {
	sandboxSeen.Lock()
	all := map[string]bool{}
	for f := range sandboxSeen.set {
		all[f] = true
	}
	sandboxSeen.Unlock()
	if f := CurrentToggles().Folder; f != nil && UsableFolder(*f) {
		all[SandboxFull(*f)] = true
	}
	out := make([]string, 0, len(all))
	for f := range all {
		out = append(out, f)
	}
	sort.Strings(out)
	return out
}

// Covers: the folder is one of them, or inside one.
func Covers(folders []string, folder string) bool {
	f := SandboxFull(folder)
	for _, x := range folders {
		if f == x || strings.HasPrefix(f, strings.TrimRight(x, "/")+"/") {
			return true
		}
	}
	return false
}

// SandboxFull is Path.GetFullPath on a Unix path, lexically, without the trailing slash.
func SandboxFull(folder string) string {
	var parts []string
	for _, seg := range strings.Split(folder, "/") {
		switch seg {
		case "", ".":
		case "..":
			if len(parts) > 0 {
				parts = parts[:len(parts)-1]
			}
		default:
			parts = append(parts, seg)
		}
	}
	return "/" + strings.Join(parts, "/")
}

// Boxed is how a tool's process stands with its sandbox: whether Hover started it (not a
// test's stand-in), and the folders it was sandboxed for, nil when it wasn't.
type Boxed struct {
	own     atomic.Bool
	mu      sync.Mutex
	folders []string
	boxed   bool
}

// Fit is what a run in a folder finds of the tool's running process.
type Fit int

const (
	// Fits: it serves this folder as it is.
	Fits Fit = iota
	// Restart: it doesn't, and nothing of it runs: end it, and the start that follows fits.
	Restart
	// Outside: it doesn't, and it is busy in other folders: this one has to wait.
	Outside
)

// Started is noted by the start: the folders the process was sandboxed for, nil when it
// wasn't.
func (b *Boxed) Started(folders []string, boxed bool) {
	b.own.Store(true)
	b.mu.Lock()
	b.folders, b.boxed = folders, boxed
	b.mu.Unlock()
}

// Fit: a sandboxed tool reaches only the folders it started with, and the sandbox switched
// on or off applies from its next start: one that no longer fits is started again when
// nothing of it runs. active is SandboxActive() now.
func (b *Boxed) Fit(folder string, busy, active bool) Fit {
	if !b.own.Load() {
		return Fits
	}
	b.mu.Lock()
	folders, boxed := b.folders, b.boxed
	b.mu.Unlock()
	outside := boxed && !Covers(folders, folder)
	if !(outside || boxed != active) {
		return Fits
	}
	switch {
	case !busy:
		return Restart
	case outside:
		return Outside
	}
	return Fits
}

// OutsideMessage is said when a run has to wait for a task of its tool in other folders.
func OutsideMessage(name string) string {
	return name + " is working on a task in another folder, and its sandbox reaches only the folders it started with. Start this one when that task is done."
}

// MARK: The policy

// DevDomains are hosts every tool may reach: package registries, GitHub, and the local
// machine (dev servers, the tool's own local services).
var DevDomains = []string{
	"localhost", "github.com", "*.github.com", "*.githubusercontent.com", "*.githubassets.com",
	"registry.npmjs.org", "*.npmjs.org", "*.npmjs.com", "registry.yarnpkg.com", "*.yarnpkg.com",
	"pypi.org", "*.pypi.org", "files.pythonhosted.org", "crates.io", "*.crates.io",
	"proxy.golang.org", "sum.golang.org", "api.nuget.org", "*.nuget.org", "rubygems.org", "*.rubygems.org",
	"repo.maven.apache.org", "repo1.maven.org", "services.gradle.org", "plugins.gradle.org",
	"jsr.io", "deno.land", "bun.sh", "nodejs.org",
}

// ToolDomains are each tool's own service (sign-in, models, telemetry it can't run without).
func ToolDomains(t core.AgentTool) []string {
	switch t {
	case core.Kiro:
		return []string{"kiro.dev", "*.kiro.dev", "*.amazonaws.com", "*.awsapps.com", "*.amazoncognito.com", "*.aws.amazon.com", "*.aws.dev"}
	case core.Codex:
		return []string{"api.openai.com", "*.openai.com", "chatgpt.com", "*.chatgpt.com", "*.oaiusercontent.com"}
	case core.Cursor:
		return []string{"cursor.com", "*.cursor.com", "cursor.sh", "*.cursor.sh", "*.cursorapi.com"}
	case core.OpenCode:
		// OpenCode brings the user's own providers.
		return []string{
			"opencode.ai", "*.opencode.ai", "models.dev", "api.anthropic.com", "api.openai.com", "openrouter.ai", "*.openrouter.ai",
			"generativelanguage.googleapis.com", "*.githubcopilot.com", "api.x.ai", "api.groq.com", "api.mistral.ai", "api.deepseek.com",
			"api.together.xyz", "api.fireworks.ai", "api.cerebras.ai", "openai.azure.com", "*.openai.azure.com",
		}
	case core.Claude:
		// Bedrock and Vertex users add their cloud's hosts to allowed-domains.txt.
		return []string{"anthropic.com", "*.anthropic.com", "claude.ai", "*.claude.ai", "claude.com", "*.claude.com"}
	case core.Agy:
		// Google's sign-in and its agent backend (Cloud Code), the Gemini API for a key, and
		// Antigravity's own site.
		return []string{
			"accounts.google.com", "oauth2.googleapis.com", "openidconnect.googleapis.com", "www.googleapis.com",
			"cloudcode-pa.googleapis.com", "daily-cloudcode-pa.googleapis.com", "generativelanguage.googleapis.com",
			"antigravity.google", "*.antigravity.google", "play.googleapis.com",
		}
	}
	// Only old chats have Custom (an agent of the user's own, gone from Hover); nothing starts for it.
	return nil
}

// ExtraFile is where the user's own hosts are: one host per line, # for comments.
func ExtraFile() string { return filepath.Join(core.Support(), "sandbox", "allowed-domains.txt") }

const extraHeader = "# Hosts Hover's sandboxed agents may reach, beyond their own service, package\n" +
	"# registries and GitHub. One per line, like docs.example.com or *.example.com.\n" +
	"# A tool picks a change up when it next starts.\n"

// validHost is ^(\*\.)?[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+(:\d{1,5})?$ or ^localhost(:\d{1,5})?$
func validHost(h string) bool {
	name := h
	if i := strings.LastIndexByte(h, ':'); i >= 0 {
		name = h[:i]
		port := h[i+1:]
		if port == "" || len(port) > 5 || strings.Trim(port, "0123456789") != "" {
			return false
		}
	}
	if name == "localhost" {
		return true
	}
	labels := strings.Split(strings.TrimPrefix(name, "*."), ".")
	if len(labels) < 2 {
		return false
	}
	for _, l := range labels {
		if l == "" || !allRunes(l, func(c rune) bool { return asciiAlnum(c) || c == '-' }) {
			return false
		}
	}
	return true
}

// ParseExtra is the hosts in allowed-domains.txt's text: comments and anything that isn't
// a host (a URL, a bare "*", "*.com") dropped.
func ParseExtra(text string) []string {
	var out []string
	for _, l := range rustLines(text) {
		l, _, _ = strings.Cut(l, "#")
		if l = strings.TrimSpace(l); validHost(l) {
			out = append(out, l)
		}
	}
	return out
}

// Extra is the user's own hosts; the file is written, explained, the first time it is needed.
func Extra() []string {
	file := ExtraFile()
	if _, err := os.Stat(file); err != nil {
		os.MkdirAll(filepath.Dir(file), 0o777)
		os.WriteFile(file, []byte(extraHeader), 0o666)
	}
	b, err := os.ReadFile(file)
	if err != nil {
		return nil
	}
	return ParseExtra(core.TextOf(b))
}

// SandboxCtx is where paths come from: the user's home, Hover's data folder, and whether
// this is a Mac. Given, so the settings text can be made and tested anywhere.
type SandboxCtx struct {
	Home, Support string
	MacOS         bool
	DarwinTemp    *string
}

func CurrentSandboxCtx() SandboxCtx {
	c := SandboxCtx{Home: Home(), Support: core.Support(), MacOS: runtime.GOOS == "darwin"}
	if c.MacOS {
		c.DarwinTemp = darwinTemp()
	}
	return c
}

func (c SandboxCtx) underHome(rel string) string { return strings.TrimRight(c.Home, "/") + "/" + rel }

func (c SandboxCtx) tilde(p string) string {
	if r, ok := strings.CutPrefix(p, "~/"); ok {
		return c.underHome(r)
	}
	return p
}

func (c SandboxCtx) support() string { return SandboxFull(c.Support) }

// darwinTemp: Apple's own tools (sips, xcrun, codesign) write here whatever TMPDIR says.
func darwinTemp() *string {
	if t, ok := os.LookupEnv("TMPDIR"); ok && strings.HasPrefix(t, "/var/folders/") {
		return sp(strings.TrimRight(t, "/"))
	}
	return nil
}

// ToolState is where a tool keeps its own sign-in, settings, logs and caches: writable.
func ToolState(t core.AgentTool, c SandboxCtx) []string {
	var own []string
	switch t {
	case core.Kiro:
		own = []string{"~/.kiro", "~/.aws/sso", "~/.aws/cli", "~/Library/Application Support/kiro-cli", "~/.local/share/kiro-cli", "~/.config/kiro-cli"}
	case core.Codex:
		own = []string{"~/.codex"}
	case core.Cursor:
		own = []string{"~/.cursor", "~/.config/cursor", "~/Library/Application Support/Cursor", "~/.local/share/cursor-agent"}
	case core.OpenCode:
		own = []string{"~/.local/share/opencode", "~/.local/state/opencode", "~/.config/opencode", "~/.cache/opencode"}
	case core.Claude:
		own = []string{"~/.claude", "~/.claude.json", "~/.config/claude"}
	case core.Agy:
		// GEMINI_HOME: the ACP server's token and settings (antigravity-acp/) and the CLI's.
		own = []string{"~/.gemini"}
	}
	// What builds and package managers the agents run write to.
	shared := []string{
		"~/.cache", "~/.npm", "~/.local/state", "~/Library/Caches", "~/Library/Logs", "~/.nuget", "~/.dotnet", "~/.cargo/registry",
		"~/.cargo/git", "~/go/pkg", "~/.gradle", "~/.m2", "~/.bun", "~/.yarn", "~/.pnpm-store", "~/Library/pnpm", "~/.deno",
	}
	var out []string
	for _, p := range append(own, shared...) {
		out = append(out, c.tilde(p))
	}
	return out
}

// Private is what no tool may read.
func Private(c SandboxCtx) []string {
	p := []string{
		"~/.ssh", "~/.gnupg", "~/.netrc", "~/.config/gh", "~/.docker/config.json", "~/.kube", "~/.password-store",
		// Not ~/Library/Keychains: a file keychain is opened in-process (Cursor keeps its
		// sign-in there), and its items stay locked behind securityd and their own access
		// lists.
		"~/Library/Mail", "~/Library/Messages", "~/Library/Safari", "~/Library/Cookies",
		"~/Library/Containers", "~/Library/Group Containers", "~/Library/Calendars", "~/Library/Application Support/AddressBook",
		"~/Library/Application Support/com.apple.TCC", "~/Library/Application Support/Google/Chrome",
		"~/Library/Application Support/BraveSoftware", "~/Library/Application Support/Microsoft Edge",
		"~/Library/Application Support/Firefox", "~/Library/Application Support/Arc", "~/.mozilla", "~/.config/google-chrome",
	}
	var out []string
	for _, x := range p {
		out = append(out, c.tilde(x))
	}
	return append(out, c.support())
}

// CuaSocket is CuaDriver's daemon socket, which the agent's cua-driver talks to.
func CuaSocket(c SandboxCtx) string { return c.underHome("Library/Caches/cua-driver/cua-driver.sock") }

func jstrs(v []string) core.JSON {
	items := make([]core.JSON, len(v))
	for i, s := range v {
		items[i] = core.JStr(s)
	}
	return core.JArr(items...)
}

// SandboxConfig is srt's settings for a tool started for these folders.
func SandboxConfig(tool core.AgentTool, folders []string, temp string, sockets, extra []string, c SandboxCtx) string {
	var domains []string
	for _, d := range append(append(ToolDomains(tool), DevDomains...), extra...) {
		if !slices.ContainsFunc(domains, func(x string) bool { return strings.EqualFold(x, d) }) {
			domains = append(domains, d)
		}
	}
	support := c.support()
	// Images pasted into a prompt are kept in Hover's folder, and the prompt names them for
	// the agent to read; Cua Driver runs behind Hover's guard, Hover's browser relay and the
	// MCP configs Hover writes are read the same way (readable, not writable).
	read := slices.Clone(folders)
	for _, d := range []string{"kiro-images", "cua", "browser", "mcp"} {
		read = append(read, support+"/"+d)
	}
	write := slices.Clone(folders)
	write = append(write, ToolState(tool, c)...)
	write = append(write, temp)
	if c.DarwinTemp != nil {
		write = append(write, *c.DarwinTemp)
	}
	cfg := core.JObj(
		core.P("network", core.JObj(
			core.P("allowedDomains", jstrs(domains)),
			core.P("deniedDomains", core.JArr()),
			// Dev servers the agent starts, and test runs against them.
			core.P("allowLocalBinding", core.JBool(true)),
			core.P("allowUnixSockets", jstrs(sockets)),
		)),
		core.P("filesystem", core.JObj(
			core.P("denyRead", jstrs(Private(c))),
			core.P("allowRead", jstrs(read)),
			core.P("allowWrite", jstrs(write)),
			// A folder's .git/hooks and .git/config, shell rc files and the like are closed
			// by srt itself.
			core.P("denyWrite", core.JArr()),
		)),
		core.P("allowAppleEvents", core.JBool(false)),
		// macOS's certificate service: without it .NET, Go (gh) and Security-framework TLS
		// can't verify a certificate, and the tools can't sign in.
		core.P("enableWeakerNetworkIsolation", core.JBool(c.MacOS)),
		// Terminals the agent opens for commands.
		core.P("allowPty", core.JBool(true)),
	)
	return cfg.Indented("\n")
}

// MARK: Starting a tool in it

const perlPath = "/usr/bin/perl"

// SandboxRelay: srt (Node) hands the tool its own stdio, non-blocking, so a write bigger
// than the pipe's 64 KB buffer fails with EAGAIN and the tool dies. Cua Driver's tool list
// is 67 KB, so with computer use on Kiro quit at its first session, and cua-driver itself
// with "os error 35". This relay runs the tool on blocking pipes of its own and copies them
// across, waiting when a pipe is full. It ends when the tool does, and closes the tool's
// stdin when Hover closes its own.
const SandboxRelay = `#!/usr/bin/perl
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
`

// SrtArgs is the argument list of srt for a tool: its settings, then the tool with its own
// arguments. srt quotes each argument and runs them with bash -c; env puts the tool's temp
// folder back, which srt points at its own. The relay (perl, and where its script is; ""
// for none) gives the tool pipes of its own.
func SrtArgs(settings, temp, perl, script, exe string, args []string) []string {
	a := []string{"--settings", settings, "--", "/usr/bin/env", "TMPDIR=" + temp + "/"}
	if script != "" {
		a = append(a, perl, script)
	}
	return append(append(a, exe), args...)
}

// SandboxEnv is what the sandbox adds to a tool's environment. A dotnet build the agent
// runs stays in one process: MSBuild's worker nodes talk over sockets in /tmp that the
// sandbox can't open without opening every socket there (ssh-agent's among them). The
// variable tells the agent why.
var SandboxEnv = [][2]string{
	{"HOVER_SANDBOXED", "1"}, {"MSBUILDDISABLENODEREUSE", "1"}, {"DOTNET_CLI_USE_MSBUILD_SERVER", "0"}, {"UseSharedCompilation", "false"},
}

// TempRoot: temp files and Unix sockets go in a short folder of the tool's own (sockets
// have a 104-byte path limit), the only place sockets work besides CuaDriver's and Hover's
// browser's.
func TempRoot() string {
	if r := os.Getenv("HOVER_SANDBOX_TMP"); r != "" {
		return r
	}
	if runtime.GOOS == "darwin" {
		return "/private/tmp/claude"
	}
	return filepath.Join(os.TempDir(), "claude")
}

// Start is a tool's start as it will be made: the program, its arguments and its
// environment. Sandboxed, that is srt with the tool inside; otherwise the tool as it was.
type Start struct {
	Exe   string
	Args  []string
	Env   [][2]string
	Boxed bool
}

// SandboxPlan is the tool's start, inside srt when the sandbox is wanted and can be had:
// the settings are written to <data>/sandbox/<tool>.json and the relay to the tool's temp
// folder. Anything else (switched off, srt missing, the folders can't be made) leaves the
// start as it was, and hover.log says why.
func SandboxPlan(tool core.AgentTool, exe string, args []string, env [][2]string, folders []string) Start {
	plain := Start{Exe: exe, Args: slices.Clone(args), Env: slices.Clone(env)}
	if !SandboxSupported() {
		return plain
	}
	name := tool.Name()
	if SandboxInside() {
		core.Logf("sandbox: %s starts as it is (Hover is in a sandbox already)", name)
		return plain
	}
	if !CurrentToggles().Sandbox {
		core.Logf("sandbox: %s starts unsandboxed (switched off in Settings)", name)
		return plain
	}
	if why := SandboxMissing(); why != nil {
		core.Logf("sandbox: %s starts unsandboxed - %s", name, *why)
		return plain
	}
	s, err := boxedStart(tool, exe, args, env, folders)
	if err != nil {
		core.Logf("sandbox: %s starts unsandboxed - couldn’t set it up: %v", name, err)
		return plain
	}
	core.Logf("sandbox: %s starts in srt for %d folder(s)", name, len(folders))
	return s
}

func boxedStart(tool core.AgentTool, exe string, args []string, env [][2]string, folders []string) (Start, error) {
	srt := SandboxExe()
	if srt == "" {
		return Start{}, errors.New("srt isn’t installed")
	}
	c := CurrentSandboxCtx()
	dir := filepath.Join(core.Support(), "sandbox")
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return Start{}, err
	}
	root := TempRoot()
	if err := os.MkdirAll(root, 0o777); err != nil {
		return Start{}, err
	}
	temp := filepath.Join(root, "hover-"+tool.ID())
	if err := PrivateDir(temp); err != nil {
		return Start{}, err
	}
	sockets := []string{temp}
	t := CurrentToggles()
	if c.MacOS && t.ComputerUse {
		sockets = append(sockets, CuaSocket(c))
	}
	// Hover's browser: the relay the agent's tool starts talks to Hover over this one socket.
	if BrowserAvailable() && t.AgentBrowser || SpacesWanted() {
		sockets = append(sockets, BrowserSocketPath())
	}
	file := filepath.Join(dir, tool.ID()+".json")
	if err := writePrivate(file, SandboxConfig(tool, folders, temp, sockets, Extra(), c)); err != nil {
		return Start{}, err
	}
	script := ""
	if isFile(perlPath) {
		script = filepath.Join(temp, "relay.pl")
		if err := os.WriteFile(script, []byte(SandboxRelay), 0o666); err != nil {
			return Start{}, err
		}
	}
	a := SrtArgs(file, temp, perlPath, script, exe, args)
	return Start{Exe: srt, Args: a, Env: append(slices.Clone(env), SandboxEnv...), Boxed: true}, nil
}

// SandboxLaunch is the tool as a running process, in the sandbox when it is wanted (see
// SandboxPlan), started in dir (Claude Code takes its project from there; "" for the
// user's home). boxed is told how it started, for the next run's check (Boxed.Fit).
func SandboxLaunch(tool core.AgentTool, exe string, args []string, env [][2]string, dir string, boxed *Boxed) (*Link, error) {
	folders := SandboxFolders()
	s := SandboxPlan(tool, exe, args, env, folders)
	if boxed != nil {
		boxed.Started(folders, s.Boxed)
	}
	if dir != "" {
		return LaunchIn(s.Exe, s.Args, s.Env, dir)
	}
	return Launch(s.Exe, s.Args, s.Env)
}
