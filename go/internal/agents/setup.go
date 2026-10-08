package agents

// Services/AgentSetup.cs: one click from "not installed" to "ready". Installs what a tool
// is missing with its maker's own installer, then opens its own sign-in. Nothing here signs
// in for the user or touches a tool's credentials: sign-in is the tool's command in a
// Terminal window (each one's login is interactive in its own way), and Hover only watches
// the tool's status command until it says yes. macOS only for now (the Windows and Linux
// installers differ): SetupSupported is false elsewhere, with a note for the button.
//
// The installers are run only when the user asks, with no stdin, one at a time.

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
)

// SetupUnsupported is what Settings shows beside the setup button where it can't run.
const SetupUnsupported = "One-click setup is available on macOS."

func SetupSupported() bool { return runtime.GOOS == "darwin" }

// SetupNote is why the button is disabled here, or nil.
func SetupNote() *string {
	if SetupSupported() {
		return nil
	}
	return sp(SetupUnsupported)
}

// SetupProgress is what a setup is doing: "installing" or "signing-in" (computer use's:
// "granting"), the installer's last line, and why it stopped if it failed.
type SetupProgress struct {
	Step  *string
	Line  string
	Error *string
}

// SetupStep is one install step: what it is called and the bash command that does it.
type SetupStep struct{ Title, Command string }

// StepError is how a step ended, short of succeeding: cancelled, or failed with words.
type StepError struct {
	Cancelled bool
	Msg       string
}

func (e *StepError) Error() string {
	if e.Cancelled {
		return "cancelled"
	}
	return e.Msg
}

var stepCancelled = &StepError{Cancelled: true}

// MARK: What to run

// Npm: npm packages go to ~/.local (bins in ~/.local/bin, already on Hover's PATH), so a
// Homebrew or system Node never needs sudo, and the prefix is pinned to the npm that
// installs them (as T3 Code pins its npm updates).
func Npm(packages ...string) string {
	return `npm install --global --no-fund --no-audit --prefix "$HOME/.local" ` + strings.Join(packages, " ")
}

// SandboxNeeds is what the sandbox still needs installed, when it is wanted.
type SandboxNeeds struct{ SrtMissing, RgMissing bool }

// Plan is what a tool still needs, in order; empty when everything is there.
func Plan(t core.AgentTool) []SetupStep {
	// Antigravity's server is unpacked into a folder of its own, not put on PATH.
	has := func(n string) bool { return OnPath(n) != "" || n == "agy_acp_server.par" && Exe(core.Agy) != "" }
	var sandbox *SandboxNeeds
	if SandboxWanted() {
		sandbox = &SandboxNeeds{
			SrtMissing: SandboxExe() == "",
			RgMissing:  !has("rg") && !isFile("/opt/homebrew/bin/rg") && !isFile("/usr/local/bin/rg"),
		}
	}
	return PlanWith(t, has, sandbox)
}

// shQuote is a word in single quotes for sh.
func shQuote(s string) string { return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'" }

// PlanWith is Plan, with what is on PATH (has) and the sandbox's needs given.
func PlanWith(t core.AgentTool, has func(string) bool, sandbox *SandboxNeeds) []SetupStep {
	var steps []SetupStep
	var packages []string
	switch t {
	case core.Codex:
		if !has("codex") {
			packages = append(packages, "@openai/codex")
		}
		if !has("codex-acp") {
			packages = append(packages, "@agentclientprotocol/codex-acp")
		}
	case core.Kiro:
		if !has("kiro-cli") {
			steps = append(steps, SetupStep{"Installing Kiro CLI", "curl -fsSL https://cli.kiro.dev/install | bash"})
		}
	case core.Cursor:
		if !has("cursor-agent") {
			steps = append(steps, SetupStep{"Installing the Cursor CLI", "curl -fsS https://cursor.com/install | bash"})
		}
	case core.OpenCode:
		if !has("opencode") {
			steps = append(steps, SetupStep{"Installing OpenCode", "curl -fsSL https://opencode.ai/install | bash"})
		}
	case core.Claude:
		if !has("claude") {
			steps = append(steps, SetupStep{"Installing Claude Code", "curl -fsSL https://claude.ai/install.sh | bash"})
		}
	case core.Agy:
		// Google's ACP server for Antigravity (agents), unpacked where Hover looks for it.
		if !has("agy_acp_server.par") {
			dir := shQuote(AgyACPDir())
			steps = append(steps, SetupStep{"Installing Antigravity’s ACP server", fmt.Sprintf(
				`a=$([ "$(uname -m)" = arm64 ] && echo arm64 || echo x86_64); z=$(mktemp -t agy-acp).zip; `+
					`curl -fL -o "$z" https://dl.google.com/agy-extensions/releases/macos/agy-acp-server-%s-darwin-$a.zip && `+
					`mkdir -p %s && unzip -o -q "$z" -d %s; rm -f "$z"`, AgyACPVersion, dir, dir)})
		}
	}
	node := SetupStep{"Installing Node.js", "brew install node"}
	if len(packages) > 0 {
		// The adapters are Node programs; Homebrew's Node when there is no Node yet.
		if !has("npm") && has("brew") {
			steps = append(steps, node)
		}
		title := "Installing Codex's ACP adapter"
		if len(packages) > 1 {
			title = "Installing Codex and its ACP adapter"
		}
		steps = append(steps, SetupStep{title, Npm(packages...)})
	}
	// Every tool runs in the sandbox: srt, a Node program at the version Hover was checked
	// against, and ripgrep, which srt needs on a Mac.
	if sandbox != nil {
		if sandbox.SrtMissing {
			if !has("npm") && has("brew") && !slices.Contains(steps, node) {
				steps = append(steps, node)
			}
			steps = append(steps, SetupStep{"Installing the agent sandbox (srt)", Npm(SandboxPackage + "@" + SandboxVersion)})
		}
		if sandbox.RgMissing && has("brew") {
			steps = append(steps, SetupStep{"Installing ripgrep for the sandbox", "brew install ripgrep"})
		}
	}
	return steps
}

// SignInCommand is the tool's own sign-in, run in Terminal.
func SignInCommand(t core.AgentTool) string {
	switch t {
	case core.Codex:
		return "codex login"
	case core.Kiro:
		return "kiro-cli login"
	case core.Cursor:
		return "cursor-agent login"
	case core.OpenCode:
		return "opencode auth login"
	case core.Claude:
		return "claude auth login"
	}
	// No command for Antigravity: the server runs Google's sign-in itself at its first task.
	return ""
}

// SignInScript is the .command file Terminal opens. It carries Hover's PATH (from the
// login shell) so Terminal finds the tool just installed even before a new shell would.
func SignInScript(t core.AgentTool, path string) string {
	return fmt.Sprintf("#!/bin/bash\nexport PATH=%s\nclear\nprintf '\\n  Hover · Sign in to %s\\n\\n'\n%s\nprintf '\\n  Done. You can close this window; Hover picks it up by itself.\\n\\n'\n",
		shQuote(path), t.Name(), SignInCommand(t))
}

// MARK: Running a step

// streamStep runs a step with no stdin, showing its newest line (line); fails with its last
// lines. A running step is ended when ct is cancelled or the time is up (its whole tree).
func streamStep(exe string, args []string, env [][2]string, timeout time.Duration, failure string, ct *Cancel, line func(string)) error {
	cmd := Hidden(exe, args...)
	cmd.Dir = Home()
	// Installers that draw progress bars or colours fall back to plain lines.
	cmd.Env = append(cmd.Env, "CI=1")
	for _, kv := range env {
		cmd.Env = append(cmd.Env, kv[0]+"="+kv[1])
	}
	g, err := Spawn(cmd)
	if err != nil {
		return &StepError{Msg: fmt.Sprintf("%s: %v", failure, err)}
	}
	defer g.Close()
	stdin, stdout, stderr := g.TakePipes()
	stdin.Close()
	rx := make(chan string, 256)
	var readers sync.WaitGroup
	for _, p := range []*os.File{stdout, stderr} {
		readers.Add(1)
		go func() {
			defer readers.Done()
			defer p.Close()
			r := bufio.NewReader(p)
			for {
				b, err := r.ReadBytes('\n')
				if len(b) > 0 {
					// A carriage return redraws a progress line: each piece is a line.
					for _, piece := range strings.FieldsFunc(core.Lossy(b), func(c rune) bool { return c == '\n' || c == '\r' }) {
						rx <- piece
					}
				}
				if err != nil {
					return
				}
			}
		}()
	}
	go func() { readers.Wait(); close(rx) }()
	var tail []string
	take := func(raw string) {
		l := strings.TrimSpace(StripANSI(raw))
		if l == "" {
			return
		}
		tail = append(tail, l)
		if len(tail) > 6 {
			tail = tail[1:]
		}
		if utf8.RuneCountInString(l) > 120 {
			l = string([]rune(l)[:119]) + "…"
		}
		line(l)
	}
	began := time.Now()
	var code int
	for {
		for drained := false; !drained; {
			select {
			case l, ok := <-rx:
				if ok {
					take(l)
				} else {
					drained = true
				}
			default:
				drained = true
			}
		}
		if ct.IsCancelled() {
			g.Kill()
			return stepCancelled
		}
		if c, ok := g.WaitTimeout(50 * time.Millisecond); ok {
			code = c
			break
		}
		if time.Since(began) >= timeout {
			g.Kill()
			return &StepError{Msg: fmt.Sprintf("%s: it took longer than %d minutes and was stopped.", failure, int(timeout.Minutes()))}
		}
	}
	// What it printed last, still on its way through the pipes.
	drainUntil(rx, 500*time.Millisecond, take)
	if code != 0 {
		last := tail[max(len(tail)-2, 0):]
		if len(last) == 0 {
			return &StepError{Msg: fmt.Sprintf("%s: exit code %d", failure, code)}
		}
		return &StepError{Msg: failure + ": " + strings.Join(last, " · ")}
	}
	return nil
}

// drainUntil takes what is still coming, a line at a time while lines come within 100 ms
// of each other, and for at most about limit.
func drainUntil(rx <-chan string, limit time.Duration, take func(string)) {
	until := time.Now().Add(limit)
	for {
		select {
		case l, ok := <-rx:
			if !ok {
				return
			}
			take(l)
			if time.Now().After(until) {
				return
			}
		case <-time.After(100 * time.Millisecond):
			return
		}
	}
}

// MARK: The state

var setupState = struct {
	sync.Mutex
	progress map[core.AgentTool]SetupProgress
	running  map[core.AgentTool]*Cancel
}{progress: map[core.AgentTool]SetupProgress{}, running: map[core.AgentTool]*Cancel{}}

// setupGate: npm and the vendors' installers write shared folders (~/.local/bin, the npm
// prefix); one at a time.
var setupGate sync.Mutex

var setupListeners struct {
	sync.Mutex
	list []func(core.AgentTool)
}

// OnSetupChange is called, off the caller's goroutine, whenever a tool's progress changes.
func OnSetupChange(f func(core.AgentTool)) {
	setupListeners.Lock()
	setupListeners.list = append(setupListeners.list, f)
	setupListeners.Unlock()
}

func SetupOf(t core.AgentTool) SetupProgress {
	setupState.Lock()
	defer setupState.Unlock()
	return setupState.progress[t]
}

func setSetup(t core.AgentTool, p SetupProgress) {
	setupState.Lock()
	setupState.progress[t] = p
	setupState.Unlock()
	setupListeners.Lock()
	all := slices.Clone(setupListeners.list)
	setupListeners.Unlock()
	for _, f := range all {
		f(t)
	}
}

func SetupBusy(t core.AgentTool) bool {
	setupState.Lock()
	defer setupState.Unlock()
	_, ok := setupState.running[t]
	return ok
}

func SetupCancel(t core.AgentTool) {
	setupState.Lock()
	c := setupState.running[t]
	setupState.Unlock()
	if c != nil {
		c.Cancel()
	}
}

// RunSetup installs what is missing, then opens sign-in if the tool still isn't signed in.
// One run per tool; a second click while one runs does nothing. openFile opens a script
// (Terminal does when it is nil). Blocks until it is done: run it off the UI goroutine.
func RunSetup(t core.AgentTool, openFile func(string) error) {
	if !SetupSupported() {
		setSetup(t, SetupProgress{Error: sp(SetupUnsupported)})
		return
	}
	ct := NewCancel()
	setupState.Lock()
	if _, ok := setupState.running[t]; ok {
		setupState.Unlock()
		return
	}
	setupState.running[t] = ct
	setupState.Unlock()
	err := setupWork(t, openFile, ct)
	if se, ok := err.(*StepError); ok && !se.Cancelled {
		setSetup(t, SetupProgress{Error: sp(se.Msg)})
	} else {
		setSetup(t, SetupProgress{})
	}
	setupState.Lock()
	delete(setupState.running, t)
	setupState.Unlock()
}

func progressOf(step, line string) SetupProgress { return SetupProgress{Step: sp(step), Line: line} }

func setupWork(t core.AgentTool, openFile func(string) error, ct *Cancel) error {
	steps := Plan(t)
	if len(steps) > 0 {
		npm := slices.ContainsFunc(steps, func(s SetupStep) bool { return strings.HasPrefix(s.Command, "npm ") })
		brew := slices.ContainsFunc(steps, func(s SetupStep) bool { return strings.HasPrefix(s.Command, "brew ") })
		if npm && OnPath("npm") == "" && !brew {
			return &StepError{Msg: "Node.js is needed for this tool's ACP adapter. Install Node.js from nodejs.org, then try again."}
		}
		setSetup(t, progressOf("installing", "Waiting for another install to finish…"))
		for !setupGate.TryLock() {
			if ct.IsCancelled() {
				return stepCancelled
			}
			time.Sleep(200 * time.Millisecond)
		}
		defer setupGate.Unlock()
		for _, s := range steps {
			setSetup(t, progressOf("installing", s.Title+"…"))
			if err := streamStep("/bin/bash", []string{"-c", "set -o pipefail; " + s.Command}, [][2]string{{"HOMEBREW_NO_AUTO_UPDATE", "1"}}, 600*time.Second,
				strings.ReplaceAll(s.Title, "Installing", "Couldn’t install"), ct, func(l string) { setSetup(t, progressOf("installing", l)) }); err != nil {
				return err
			}
		}
	}
	ready := Check(t, true)
	if !ready.Installed {
		return &StepError{Msg: fmt.Sprintf("The installer finished, but %s still isn't found. %s", t.Name(), InstallHint(t))}
	}
	if !ready.SignedIn {
		return signIn(t, openFile, ct)
	}
	return nil
}

// signIn opens the tool's own login in Terminal and waits (up to ten minutes) for its
// status command to say signed in.
func signIn(t core.AgentTool, openFile func(string) error, ct *Cancel) error {
	dir := filepath.Join(core.Support(), "setup")
	io := func(err error) error { return &StepError{Msg: fmt.Sprintf("Setup stopped: %v", err)} }
	if err := os.MkdirAll(dir, 0o777); err != nil {
		return io(err)
	}
	script := filepath.Join(dir, "sign-in-"+t.ID()+".command")
	path, ok := os.LookupEnv("PATH")
	if !ok {
		path = "/usr/bin:/bin"
	}
	if err := os.WriteFile(script, []byte(SignInScript(t, path)), 0o700); err != nil {
		return io(err)
	}
	if err := os.Chmod(script, 0o700); err != nil {
		return io(err)
	}
	setSetup(t, progressOf("signing-in", "Finish signing in in the Terminal window and your browser…"))
	if openFile != nil {
		if err := openFile(script); err != nil {
			return io(err)
		}
	} else {
		Ask("/usr/bin/open", "-a", "Terminal", script)
	}
	until := time.Now().Add(600 * time.Second)
	for time.Now().Before(until) {
		for range 15 {
			if ct.IsCancelled() {
				return stepCancelled
			}
			time.Sleep(200 * time.Millisecond)
		}
		if Check(t, true).SignedIn {
			return nil
		}
	}
	return &StepError{Msg: fmt.Sprintf("Not signed in yet. Click Sign in to try again, or run “%s” in a terminal.", SignInCommand(t))}
}
