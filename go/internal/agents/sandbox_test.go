package agents

// tests/sandbox.rs (SandboxTests, ported): what srt's settings allow and refuse, which
// folders a tool started for some covers, and how a tool's start is wrapped. Nothing is
// run but the relay; the settings text and the argument list are made from values given
// here, so they are checked on every OS.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func sctx() SandboxCtx {
	return SandboxCtx{Home: "/Users/me", Support: "/Users/me/Library/Application Support/Hover", MacOS: true, DarwinTemp: sp("/var/folders/ab/cd/T")}
}

func jlist(t *testing.T, c core.JSON, a, b string) []string {
	t.Helper()
	items, err := get(c, a, b).Items()
	if err != nil {
		t.Fatal(a, b)
	}
	var out []string
	for _, x := range items {
		s, _ := x.AsStr()
		out = append(out, s)
	}
	return out
}

func sconfig(t *testing.T, tool core.AgentTool, folders, sockets, extra []string) core.JSON {
	return mustJSON(t, SandboxConfig(tool, folders, "/private/tmp/claude/hover-codex", sockets, extra, sctx()))
}

func TestTheSettingsOpenTheFoldersAndCloseTheRest(t *testing.T) {
	cua := CuaSocket(sctx())
	c := sconfig(t, core.Codex, []string{"/work/project"}, []string{"/private/tmp/claude/hover-codex", cua}, nil)
	write := jlist(t, c, "filesystem", "allowWrite")
	for _, w := range []string{"/work/project", "/Users/me/.codex", "/private/tmp/claude/hover-codex", "/var/folders/ab/cd/T"} {
		if !slices.Contains(write, w) {
			t.Errorf("%s not writable", w)
		}
	}
	if slices.Contains(write, "/Users/me") || slices.ContainsFunc(write, func(w string) bool { return strings.Contains(w, "Library/Containers") }) {
		t.Error("the home folder itself isn't writable")
	}
	deny := jlist(t, c, "filesystem", "denyRead")
	for _, p := range []string{".ssh", "Library/Group Containers", "Library/Containers", "Library/Mail", "Library/Messages", "Library/Cookies"} {
		if !slices.Contains(deny, "/Users/me/"+p) {
			t.Error(p)
		}
	}
	if !slices.Contains(deny, "/Users/me/Library/Application Support/Hover") {
		t.Error("Hover's own data")
	}
	read := jlist(t, c, "filesystem", "allowRead")
	if !slices.Contains(read, "/work/project") {
		t.Error("project")
	}
	// Hover's own folder is closed, and the few things in it an agent needs are let through.
	for _, d := range []string{"kiro-images", "cua", "browser", "mcp"} {
		if !slices.Contains(read, "/Users/me/Library/Application Support/Hover/"+d) {
			t.Error(d)
		}
	}
	domains := jlist(t, c, "network", "allowedDomains")
	for _, d := range []string{"api.openai.com", "registry.npmjs.org", "github.com", "localhost"} {
		if !slices.Contains(domains, d) {
			t.Error(d)
		}
	}
	if slices.Contains(domains, "*.cursor.sh") {
		t.Error("another tool's service")
	}
	if !reflect.DeepEqual(jlist(t, c, "network", "allowUnixSockets"), []string{"/private/tmp/claude/hover-codex", cua}) {
		t.Error("sockets")
	}
	if get(c, "allowAppleEvents").Compact() != "false" || get(c, "enableWeakerNetworkIsolation").Compact() != "true" || get(c, "allowPty").Compact() != "true" || get(c, "network", "allowLocalBinding").Compact() != "true" {
		t.Error(c.Compact())
	}
}

func TestLinuxHasNoWeakerIsolationAndNoDarwinTemp(t *testing.T) {
	linux := SandboxCtx{Home: "/home/me", Support: "/home/me/.local/share/Hover"}
	c := mustJSON(t, SandboxConfig(core.Kiro, nil, "/tmp/claude/hover-kiro", nil, nil, linux))
	if get(c, "enableWeakerNetworkIsolation").Compact() != "false" {
		t.Error("weaker")
	}
	write := jlist(t, c, "filesystem", "allowWrite")
	if !slices.Contains(write, "/home/me/.kiro") || !slices.Contains(write, "/tmp/claude/hover-kiro") || slices.ContainsFunc(write, func(w string) bool { return strings.HasPrefix(w, "/var/folders") }) {
		t.Error(write)
	}
}

func TestEachToolGetsItsOwnServiceAndClaudeCodeToo(t *testing.T) {
	has := func(tool core.AgentTool, d string) bool {
		return slices.Contains(jlist(t, sconfig(t, tool, nil, nil, nil), "network", "allowedDomains"), d)
	}
	if !has(core.Kiro, "*.amazonaws.com") || has(core.Codex, "*.amazonaws.com") || !has(core.Cursor, "*.cursor.sh") || !has(core.OpenCode, "models.dev") {
		t.Error("services")
	}
	if !has(core.Claude, "api.anthropic.com") && !has(core.Claude, "*.anthropic.com") || !has(core.Claude, "github.com") {
		t.Error("every tool gets the registries and GitHub")
	}
	if !slices.Contains(jlist(t, sconfig(t, core.Claude, nil, nil, nil), "filesystem", "allowWrite"), "/Users/me/.claude") {
		t.Error("its state is writable")
	}
}

func TestTheUsersOwnHostsAreAddedAndNonsenseIsDropped(t *testing.T) {
	text := "# mine\ndocs.example.com\n*.example.org  # wildcard\n*\nhttps://bad.example.com/x\n*.com\nlocalhost:8080\n"
	if got := ParseExtra(text); !reflect.DeepEqual(got, []string{"docs.example.com", "*.example.org", "localhost:8080"}) {
		t.Error(got)
	}
	domains := jlist(t, sconfig(t, core.Kiro, nil, nil, []string{"docs.example.com"}), "network", "allowedDomains")
	if !slices.Contains(domains, "docs.example.com") || !slices.Contains(domains, "*.amazonaws.com") {
		t.Error(domains)
	}
	// A host given twice (by case) is listed once.
	twice := jlist(t, sconfig(t, core.Kiro, nil, nil, []string{"GitHub.com"}), "network", "allowedDomains")
	n := 0
	for _, d := range twice {
		if strings.EqualFold(d, "github.com") {
			n++
		}
	}
	if n != 1 {
		t.Error(n)
	}
}

func TestATooLCoversItsFoldersAndWhatIsInsideThem(t *testing.T) {
	a := []string{"/work/project"}
	if !Covers(a, "/work/project") || !Covers(a, "/work/project/sub") || !Covers(a, "/work/project/") || Covers(a, "/work/project-other") || Covers(a, "/work") || !Covers([]string{"/"}, "/anything") {
		t.Error("covers")
	}
}

func TestAStartIsWrappedInSrtWithTheToolsOwnArguments(t *testing.T) {
	args := SrtArgs("/data/sandbox/kiro.json", "/t/hover-kiro", "/usr/bin/perl", "/t/hover-kiro/relay.pl", "/usr/local/bin/kiro-cli", []string{"acp", "--agent-engine", "v3"})
	want := []string{"--settings", "/data/sandbox/kiro.json", "--", "/usr/bin/env", "TMPDIR=/t/hover-kiro/", "/usr/bin/perl", "/t/hover-kiro/relay.pl", "/usr/local/bin/kiro-cli", "acp", "--agent-engine", "v3"}
	if !reflect.DeepEqual(args, want) {
		t.Error(args)
	}
	// Without perl the tool runs directly.
	if plain := SrtArgs("/s.json", "/t", "/usr/bin/perl", "", "/bin/tool", nil); !reflect.DeepEqual(plain, []string{"--settings", "/s.json", "--", "/usr/bin/env", "TMPDIR=/t/", "/bin/tool"}) {
		t.Error(plain)
	}
	// The environment the sandbox adds tells the agent where it is, and keeps dotnet in one process.
	if !slices.Contains(SandboxEnv, [2]string{"HOVER_SANDBOXED", "1"}) || !slices.ContainsFunc(SandboxEnv, func(kv [2]string) bool { return kv[0] == "MSBUILDDISABLENODEREUSE" }) {
		t.Error(SandboxEnv)
	}
}

func TestASandboxedToolThatNoLongerFitsIsStartedAgainOnlyWhenIdle(t *testing.T) {
	var b Boxed
	// A process Hover didn't start (a test's stand-in) is left alone.
	if b.Fit("/elsewhere", false, true) != Fits {
		t.Error("stand-in")
	}
	b.Started([]string{"/work/a"}, true)
	if b.Fit("/work/a/sub", true, true) != Fits || b.Fit("/work/b", false, true) != Restart || b.Fit("/work/b", true, true) != Outside {
		t.Error("folders")
	}
	// The sandbox switched off since: restart when idle, carry on when busy.
	if b.Fit("/work/a", false, false) != Restart || b.Fit("/work/a", true, false) != Fits {
		t.Error("switched off")
	}
	// Started without one, and wanted now.
	b.Started(nil, false)
	if b.Fit("/work/a", false, true) != Restart || b.Fit("/work/a", false, false) != Fits {
		t.Error("switched on")
	}
	if !strings.Contains(OutsideMessage("Kiro"), "Kiro is working on a task in another folder") {
		t.Error("message")
	}
}

func TestWhatIsMissingIsSaidInOneLine(t *testing.T) {
	if MissingLine(nil) != nil {
		t.Error("nothing")
	}
	line := MissingLine([]string{fmt.Sprintf("npm install -g %s@%s", SandboxPackage, SandboxVersion), "brew install ripgrep"})
	if *line != "Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78, then brew install ripgrep. (Or turn the sandbox off in Settings.)" {
		t.Error(*line)
	}
}

func TestTheSandboxIsForMacosAndLinux(t *testing.T) {
	if SandboxSupported() != (runtime.GOOS == "darwin" || runtime.GOOS == "linux") {
		t.Error("supported")
	}
	if n := SandboxNote(); SandboxSupported() && n != nil || !SandboxSupported() && deref(n) != "The sandbox needs macOS or Linux." {
		t.Error(deref(n))
	}
	if runtime.GOOS == "windows" {
		if SandboxWanted() || SandboxActive() || SandboxMissing() != nil {
			t.Error("wanted on Windows")
		}
		exe := `C:\tools\kiro-cli.exe`
		s := SandboxPlan(core.Kiro, exe, []string{"acp"}, [][2]string{{"A", "b"}}, nil)
		if s.Exe != exe || !reflect.DeepEqual(s.Args, []string{"acp"}) || s.Boxed || !reflect.DeepEqual(s.Env, [][2]string{{"A", "b"}}) {
			t.Errorf("%+v", s)
		}
		Remember(`C:\work`)
		if len(SandboxFolders()) != 0 {
			t.Error("nothing is remembered where there is no sandbox")
		}
	}
}

func TestLexicalFullPathsAndHosts(t *testing.T) {
	for in, want := range map[string]string{"/a/b/": "/a/b", "/a/./b/../c": "/a/c", "/": "/", "/../..": "/"} {
		if SandboxFull(in) != want {
			t.Error(in)
		}
	}
	for _, ok := range []string{"docs.example.com", "*.example.org", "localhost:8080", "localhost", "a-b.c1.io:65535"} {
		if !validHost(ok) {
			t.Error(ok)
		}
	}
	for _, bad := range []string{"", "*", "*.com", "https://bad.example.com/x", "example", "a..b.com", "x.com:", "x.com:123456", "x.com:1a", "*.localhost", "-.", "a b.com"} {
		if validHost(bad) {
			t.Error(bad)
		}
	}
	if strings.Contains(SandboxRelay, "\r") || !strings.HasPrefix(SandboxRelay, "#!/usr/bin/perl\n") || !strings.HasSuffix(SandboxRelay, "$? >> 8);\n") {
		t.Error("the relay is plain LF text")
	}
}

// srt's stdio is non-blocking: a tool's write past the pipe's 64 KB buffer failed with
// EAGAIN and the tool died. Through the relay the same write arrives whole, and the tool's
// stdin closes with ours.
func TestTheRelayCarriesABigWriteThroughANonblockingPipe(t *testing.T) {
	python := firstFile("/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3")
	if runtime.GOOS == "windows" || !isFile("/usr/bin/perl") || python == "" {
		t.Skip("needs perl and python3")
	}
	dir := t.TempDir()
	relay := filepath.Join(dir, "relay.pl")
	os.WriteFile(relay, []byte(SandboxRelay), 0o666)
	// Like srt: stdout made non-blocking, then 200 KB written in one go, after a line read from stdin.
	p := exec.Command("/usr/bin/perl", "-e", "use Fcntl; for (0,1) { open(my $h,'+<&=',$_) or next; fcntl($h,F_SETFL,fcntl($h,F_GETFL,0)|O_NONBLOCK) } exec @ARGV", "/usr/bin/perl",
		relay, python, "-c", "import sys; sys.stdin.readline(); sys.stdout.write('x'*200000+'\\n'); sys.stdout.flush(); sys.stdin.read(); print('eof')")
	stdin, _ := p.StdinPipe()
	stdout, _ := p.StdoutPipe()
	if err := p.Start(); err != nil {
		t.Fatal(err)
	}
	io.WriteString(stdin, "go\n")
	// Read slowly, so the pipe fills up while the tool writes.
	time.Sleep(300 * time.Millisecond)
	out := bufio.NewReaderSize(stdout, 1<<20)
	line, _ := out.ReadString('\n')
	stdin.Close()
	rest, _ := out.ReadString('\n')
	if err := p.Wait(); err != nil {
		t.Error(err)
	}
	if len(strings.TrimRight(line, "\n")) != 200000 || strings.TrimSpace(rest) != "eof" {
		t.Errorf("%d %q", len(line), rest)
	}
}
