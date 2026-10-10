package agents

// tests/setup.rs (AgentSetupTests) and setup.rs's own: what one-click setup decides to
// run, from what is on PATH, and how a step runs. Nothing is installed.

import (
	"reflect"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

func have(names ...string) func(string) bool {
	return func(n string) bool { return slices.Contains(names, n) }
}

func cmds(v []SetupStep) []string {
	var out []string
	for _, s := range v {
		out = append(out, s.Command)
	}
	return out
}

func TestCodexNeedsOnlyItsAdapterWhenTheCliIsThere(t *testing.T) {
	p := PlanWith(core.Codex, have("codex", "npm"), nil)
	if len(p) != 1 || !strings.Contains(p[0].Command, "@agentclientprotocol/codex-acp") || strings.Contains(p[0].Command, "@openai/codex ") {
		t.Fatal(p)
	}
	// Into ~/.local, so no sudo and the bin lands on Hover's PATH.
	if !strings.Contains(p[0].Command, `--prefix "$HOME/.local"`) || p[0].Title != "Installing Codex's ACP adapter" {
		t.Error(p[0])
	}
}

func TestCodexWithNothingInstallsBothInOneNpmStep(t *testing.T) {
	p := PlanWith(core.Codex, have("npm"), nil)
	if len(p) != 1 || !strings.Contains(p[0].Command, "@openai/codex") || !strings.Contains(p[0].Command, "@agentclientprotocol/codex-acp") || p[0].Title != "Installing Codex and its ACP adapter" {
		t.Error(p)
	}
}

func TestNodeComesFromHomebrewWhenThereIsNoNpm(t *testing.T) {
	p := PlanWith(core.Codex, have("brew"), nil)
	if len(p) != 2 || p[0].Command != "brew install node" {
		t.Error(p)
	}
	// No npm and no brew: the npm step is left for RunSetup to refuse with a clear message.
	if len(PlanWith(core.Codex, have(), nil)) != 1 {
		t.Error("bare")
	}
}

func TestEachToolUsesItsMakersInstaller(t *testing.T) {
	for tool, want := range map[core.AgentTool]string{
		core.Kiro: "curl -fsSL https://cli.kiro.dev/install | bash", core.Cursor: "curl -fsS https://cursor.com/install | bash",
		core.OpenCode: "curl -fsSL https://opencode.ai/install | bash", core.Claude: "curl -fsSL https://claude.ai/install.sh | bash",
	} {
		if p := PlanWith(tool, have(), nil); len(p) != 1 || p[0].Command != want {
			t.Error(tool, p)
		}
	}
}

func TestNothingToDoWhenEverythingIsInstalled(t *testing.T) {
	all := have("codex", "codex-acp", "kiro-cli", "cursor-agent", "opencode", "claude", "agy_acp_server.par", "npm", "brew")
	for _, tool := range core.AllTools {
		if p := PlanWith(tool, all, nil); len(p) != 0 {
			t.Error(tool, p)
		}
	}
}

func TestTheSandboxIsInstalledWithTheToolWhenItIsWanted(t *testing.T) {
	p := PlanWith(core.Kiro, have("kiro-cli", "npm", "brew"), &SandboxNeeds{true, true})
	if !reflect.DeepEqual(cmds(p), []string{`npm install --global --no-fund --no-audit --prefix "$HOME/.local" @anthropic-ai/sandbox-runtime@0.0.78`, "brew install ripgrep"}) {
		t.Error(cmds(p))
	}
	count := func(p []SetupStep) int {
		n := 0
		for _, s := range p {
			if s.Command == "brew install node" {
				n++
			}
		}
		return n
	}
	// Node from Homebrew first when there is no npm, once.
	p = PlanWith(core.Codex, have("codex-acp", "codex", "brew"), &SandboxNeeds{SrtMissing: true})
	if count(p) != 1 || !strings.Contains(p[1].Command, "sandbox-runtime@0.0.78") {
		t.Error(cmds(p))
	}
	if count(PlanWith(core.Codex, have("brew"), &SandboxNeeds{SrtMissing: true})) != 1 {
		t.Error("node is installed once for both npm steps")
	}
	// Not wanted: nothing of it; ripgrep needs brew.
	if len(PlanWith(core.Kiro, have("kiro-cli"), nil)) != 0 || len(PlanWith(core.Kiro, have("kiro-cli"), &SandboxNeeds{RgMissing: true})) != 0 {
		t.Error("unwanted")
	}
}

func TestEachToolSignsInWithItsOwnCommand(t *testing.T) {
	for tool, want := range map[core.AgentTool]string{core.Codex: "codex login", core.Kiro: "kiro-cli login", core.Cursor: "cursor-agent login", core.OpenCode: "opencode auth login", core.Claude: "claude auth login"} {
		if SignInCommand(tool) != want {
			t.Error(tool)
		}
	}
}

func TestTheSignInScriptCarriesHoversPathQuoted(t *testing.T) {
	s := SignInScript(core.Kiro, "/usr/bin:/Users/o'brien/bin")
	want := "#!/bin/bash\nexport PATH='/usr/bin:/Users/o'\\''brien/bin'\nclear\nprintf '\\n  Hover · Sign in to Kiro\\n\\n'\nkiro-cli login\nprintf '\\n  Done. You can close this window; Hover picks it up by itself.\\n\\n'\n"
	if s != want {
		t.Errorf("%q", s)
	}
}

func TestSetupIsForMacosAndSaysSoElsewhere(t *testing.T) {
	mac := runtime.GOOS == "darwin"
	if SetupSupported() != mac || (SetupNote() == nil) != mac {
		t.Error("note")
	}
	if !mac {
		RunSetup(core.Cursor, nil)
		if p := SetupOf(core.Cursor); p.Step != nil || deref(p.Error) != "One-click setup is available on macOS." || SetupBusy(core.Cursor) {
			t.Errorf("%+v", p)
		}
	}
}

// shellOf is a command that prints script's lines and ends as it says, on either OS
// (PowerShell reads the same echo, ;, exit and sleep).
func shellOf(script string) (string, []string) {
	if runtime.GOOS == "windows" {
		return "powershell.exe", []string{"-NoProfile", "-Command", script}
	}
	return "/bin/sh", []string{"-c", script}
}

func runStep(script string, timeout time.Duration, ct *Cancel) (error, []string) {
	exe, args := shellOf(script)
	var mu sync.Mutex
	var lines []string
	err := streamStep(exe, args, nil, timeout, "Couldn’t do it", ct, func(l string) { mu.Lock(); lines = append(lines, l); mu.Unlock() })
	return err, lines
}

func TestAStepShowsItsLinesAndFailsWithItsLastTwo(t *testing.T) {
	err, lines := runStep("echo one; echo two; echo three; exit 3", 20*time.Second, NewCancel())
	if !reflect.DeepEqual(lines, []string{"one", "two", "three"}) || err == nil || err.Error() != "Couldn’t do it: two · three" {
		t.Error(lines, err)
	}
	if err, lines := runStep("echo fine", 20*time.Second, NewCancel()); err != nil || !reflect.DeepEqual(lines, []string{"fine"}) {
		t.Error(lines, err)
	}
	// No output: the exit code says it.
	if err, _ := runStep("exit 7", 20*time.Second, NewCancel()); err == nil || err.Error() != "Couldn’t do it: exit code 7" {
		t.Error(err)
	}
}

func TestAStepThatTakesTooLongOrIsCancelledIsEnded(t *testing.T) {
	start := time.Now()
	if err, _ := runStep("sleep 30", 300*time.Millisecond, NewCancel()); err == nil || !strings.HasPrefix(err.Error(), "Couldn’t do it: it took longer than") {
		t.Error(err)
	}
	ct := NewCancel()
	go func() { time.Sleep(200 * time.Millisecond); ct.Cancel() }()
	if err, _ := runStep("sleep 30", 20*time.Second, ct); err == nil || !err.(*StepError).Cancelled {
		t.Error(err)
	}
	if time.Since(start) > 15*time.Second {
		t.Error("both were ended, not waited out")
	}
}
