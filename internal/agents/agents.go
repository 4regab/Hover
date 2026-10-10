package agents

// Services/Agents.cs: where each tool is, how it starts, and whether it is installed
// and signed in.

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/4regab/Hover/internal/core"
)

// AgentReady says whether a tool can take a task now; the hint says what to do when it can't.
type AgentReady struct {
	Installed bool
	SignedIn  bool
	Hint      string
}

func (r AgentReady) OK() bool { return r.Installed && r.SignedIn }

// userBin: on Linux a tool installed for the user lands in ~/.local/bin, which a session
// started from the desktop doesn't always have on PATH (the Linux counterpart of the
// Cursor shim C# looks for in %LOCALAPPDATA%).
func userBin(name string) string {
	if runtime.GOOS == "windows" {
		return ""
	}
	if p := filepath.Join(Home(), ".local/bin", name); isFile(p) {
		return p
	}
	return ""
}

// Find is a command on PATH, else in the user's own bin folder (a desktop session's PATH
// often lacks ~/.local/bin); "" when neither. The quota read looks for kiro-cli this way too.
func Find(name string) string {
	if p := OnPath(name); p != "" {
		return p
	}
	return userBin(name)
}

// OpenCodeMinVersion is the oldest OpenCode whose server API Hover was checked against
// (T3 Code's floor).
const OpenCodeMinVersion = "1.14.19"

// Toggles is what the macOS build's settings say about the agent integrations, read
// whenever a tool starts or a session is made: computer use (off until switched on), the
// sandbox and Hover's agent browser (on until switched off), and the folder new tasks
// start in (Settings.KiroFolder; the sandbox opens it for the next start; "" for none).
type Toggles struct {
	ComputerUse  bool
	Sandbox      bool
	AgentBrowser bool
	Folder       *string
}

// DefaultToggles are Settings' own defaults, for a host that never gave any.
func DefaultToggles() Toggles { return Toggles{ComputerUse: false, Sandbox: true, AgentBrowser: true} }

var togglesFn struct {
	sync.Mutex
	f func() Toggles
}

// SetToggles says where the toggles come from: the app hands in a reader of its settings
// (hover-core's Settings computer use, sandbox, agent browser and Kiro folder).
func SetToggles(f func() Toggles) {
	togglesFn.Lock()
	togglesFn.f = f
	togglesFn.Unlock()
}

// CurrentToggles are the toggles now: the app's, or Settings' defaults when none were given.
func CurrentToggles() Toggles {
	togglesFn.Lock()
	f := togglesFn.f
	togglesFn.Unlock()
	if f == nil {
		return DefaultToggles()
	}
	return f()
}

// Exe is the program that runs the tool, or "" when it isn't installed.
func Exe(t core.AgentTool) string {
	switch t {
	case core.Kiro:
		return Find("kiro-cli")
	case core.Codex:
		return Find("codex-acp")
	case core.Cursor:
		// Cursor's Windows installer puts it here and adds the folder to PATH, but a
		// Hover started before the install has the old PATH. Its "agent" alias is not
		// used: other tools (Grok) install an "agent" too.
		if p := cursorShim(); p != "" && isFile(p) {
			return p
		}
		return Find("cursor-agent")
	case core.OpenCode:
		return opencodeExe()
	case core.Claude:
		return claudeExe()
	case core.Agy:
		return agyACPExe()
	}
	// Agents of the user's own are gone: a chat that still names one has nothing to start.
	return ""
}

// AgyACPVersion: Google's Antigravity ACP server, as T3 Code runs it (and as the ACP
// registry lists it): not the agy CLI, which has no ACP mode, but its own release
// (dl.google.com/agy-extensions, agy-acp-server-<version>-<os>-<arch>.zip). The zip holds
// the server and the agent harness it runs, which must sit next to it. Looked for on PATH
// (and ~/.local/bin), then where setup unpacks it, AgyACPDir.
const AgyACPVersion = "1.3.0"

func agyServer() string {
	if runtime.GOOS == "windows" {
		return "agy_acp_server.exe"
	}
	return "agy_acp_server.par"
}

func agyHarnessName() string {
	if runtime.GOOS == "windows" {
		return "localharness_external.exe"
	}
	return "localharness_external"
}

// AgyACPDir is where Hover's setup unpacks the server: outside Hover's own data folder,
// which the sandbox keeps the tools from reading.
func AgyACPDir() string {
	if l, ok := os.LookupEnv("LOCALAPPDATA"); ok && runtime.GOOS == "windows" {
		return filepath.Join(l, "antigravity-acp")
	}
	return filepath.Join(Home(), ".local/share/antigravity-acp")
}

func agyACPExe() string {
	p := Find(agyServer())
	if p == "" {
		p = Find("agy_acp_server")
	}
	if p == "" {
		if u := filepath.Join(AgyACPDir(), agyServer()); isFile(u) {
			p = u
		}
	}
	if p == "" || !isFile(AgyHarness(p)) {
		return ""
	}
	return p
}

// AgyHarness is the agent harness the server runs (ANTIGRAVITY_HARNESS_PATH), next to
// the server. ponytail: filepath.Dir for Path::parent; they differ only for a bare name
// or a root, and the server is always a full path to a file.
func AgyHarness(server string) string {
	return filepath.Join(filepath.Dir(server), agyHarnessName())
}

// Environment is what a tool's process gets in its environment besides Hover's own.
func Environment(t core.AgentTool, exe string) [][2]string {
	if t != core.Agy {
		return nil
	}
	// The server unpacks itself (about 1 GB) into its temp folder on every start, and
	// leaves its logs there: a folder of its own, emptied before each start (one server
	// per Hover), so a killed one leaves nothing behind for long.
	temp := filepath.Join(TempRoot(), "hover-agy")
	os.MkdirAll(temp, 0o777)
	if entries, err := os.ReadDir(temp); err == nil {
		for _, e := range entries {
			// The sandbox's relay is written here just before the start.
			if e.Name() == "relay.pl" {
				continue
			}
			p := filepath.Join(temp, e.Name())
			if isDir(p) {
				os.RemoveAll(p)
			} else {
				os.Remove(p)
			}
		}
	}
	env := [][2]string{
		{"ANTIGRAVITY_HARNESS_PATH", AgyHarness(exe)},
		{"PYTHONUNBUFFERED", "1"},
	}
	if runtime.GOOS == "windows" {
		return append(env, [2]string{"TEMP", temp}, [2]string{"TMP", temp})
	}
	return append(env, [2]string{"TMPDIR", temp})
}

func exeName(n string) string {
	if runtime.GOOS == "windows" {
		return n + ".exe"
	}
	return n
}

// claudeExe: Claude Code's native installer puts it in ~/.local/bin on Windows too, and
// says the folder may not be on PATH; npm's global install is a claude.cmd shim on PATH,
// and the older local install ~/.claude/local/claude.
func claudeExe() string {
	home := Home()
	if p := Find("claude"); p != "" {
		return p
	}
	if p := filepath.Join(home, ".local", "bin", exeName("claude")); isFile(p) {
		return p
	}
	if p := filepath.Join(home, ".claude", "local", exeName("claude")); isFile(p) {
		return p
	}
	return ""
}

// opencodeExe is OpenCode's own exe. npm installs a .cmd shim that runs it on Windows;
// going to the exe it points at saves a cmd.exe per server and keeps the server Hover's
// direct child. On Linux its installer puts it in ~/.opencode/bin, which a desktop
// session's PATH often lacks (as ~/.local/bin).
func opencodeExe() string {
	found := Find("opencode")
	if found == "" && runtime.GOOS != "windows" {
		if p := filepath.Join(Home(), ".opencode/bin/opencode"); isFile(p) {
			found = p
		}
	}
	if found == "" {
		return ""
	}
	if !strings.EqualFold(extension(found), "cmd") {
		return found
	}
	if exe := filepath.Join(filepath.Dir(found), "node_modules", "opencode-ai", "bin", "opencode.exe"); isFile(exe) {
		return exe
	}
	return found
}

func cursorShim() string {
	if runtime.GOOS != "windows" {
		return ""
	}
	if l, ok := os.LookupEnv("LOCALAPPDATA"); ok {
		return filepath.Join(l, "cursor-agent", "cursor-agent.cmd")
	}
	return ""
}

// Arguments are what the tool is started with.
func Arguments(t core.AgentTool) []string {
	switch t {
	case core.Kiro:
		// v3 is the engine with ACP sessions that load; "cli" keeps the sign-in inside
		// kiro-cli rather than asking Hover for tokens.
		return []string{"acp", "--agent-engine", "v3", "--auth-method", "cli"}
	case core.Cursor:
		return []string{"acp"}
	case core.OpenCode:
		// Local only: this address, a port the system picks, and never announced on the
		// network (mDNS), whatever the user's opencode config says.
		return []string{"serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false"}
	case core.Claude:
		// The Agent SDK's own way of running it (T3 Code's): streamed JSON both ways and
		// its control protocol on stdio, so every permission comes to Hover. The rest
		// (access, model, effort, resume) is added per conversation (claude launch args).
		return []string{"--output-format", "stream-json", "--verbose", "--input-format", "stream-json", "--permission-prompt-tool", "stdio", "--include-partial-messages"}
	case core.Agy:
		// On Linux the server wants its user id flag, empty (T3 Code starts it so).
		if runtime.GOOS == "linux" {
			return []string{"--uid="}
		}
	}
	return []string{}
}

// InstallHint is what it takes to install the tool, for the greyed-out choice.
func InstallHint(t core.AgentTool) string {
	win := runtime.GOOS == "windows"
	switch t {
	case core.Kiro:
		return "Install kiro-cli from kiro.dev/cli."
	case core.Codex:
		return "Install Codex and its ACP adapter: npm i -g @openai/codex @agentclientprotocol/codex-acp"
	case core.Cursor:
		if win {
			return "Install the Cursor CLI: irm 'https://cursor.com/install?win32=true' | iex"
		}
		return "Install the Cursor CLI: curl https://cursor.com/install -fsS | bash"
	case core.OpenCode:
		return fmt.Sprintf("Install OpenCode %s or newer from opencode.ai.", OpenCodeMinVersion)
	case core.Claude:
		if win {
			return "Install Claude Code: irm https://claude.ai/install.ps1 | iex"
		}
		return "Install Claude Code: curl -fsSL https://claude.ai/install.sh | bash"
	case core.Custom:
		return "Check the agent’s program in Settings."
	}
	return fmt.Sprintf("Install Google’s Antigravity ACP server %s: unzip agy-acp-server-%s-<os>-<arch>.zip from dl.google.com/agy-extensions/releases into %s.",
		AgyACPVersion, AgyACPVersion, AgyACPDir())
}

func SignInHint(t core.AgentTool) string {
	switch t {
	case core.Kiro:
		return "Sign in: run “kiro-cli login” in a terminal."
	case core.Codex:
		return "Sign in: run “codex login” in a terminal."
	case core.Cursor:
		return "Sign in: run “cursor-agent login” in a terminal."
	case core.OpenCode:
		// OpenCode keeps its own providers: API keys, cloud sign-ins, local models.
		return "Add a model provider: run “opencode auth login”, or set one up in your opencode config."
	case core.Claude:
		// An API key in its environment, Bedrock or Vertex count as signed in too (its
		// auth status says so).
		return "Sign in: run “claude auth login” in a terminal."
	case core.Custom:
		return "Sign in with the agent’s own method in Settings."
	}
	// The server runs Google's sign-in itself when Hover starts it (acp), or takes
	// GEMINI_API_KEY from the environment.
	return "Sign in: start an Antigravity task and finish Google’s sign-in in the browser it opens, or set GEMINI_API_KEY."
}

// ReadOnlyWorks: read only holds for Kiro (writes wait for an approval Hover refuses),
// Cursor (Ask mode) and OpenCode (session rules its server enforces). Codex's read-only
// mode leans on a sandbox it doesn't have on Windows, so there it wrote files anyway; on
// Linux it has one (Landlock), so it is offered.
func ReadOnlyWorks(t core.AgentTool) bool { return t != core.Codex || runtime.GOOS != "windows" }

// asking is a check under way, and its answer once there is one.
type asking struct {
	done  chan struct{}
	ready AgentReady
}

type checkDone struct {
	at    time.Time
	ready AgentReady
}

var checks = struct {
	sync.Mutex
	done   map[core.AgentTool]checkDone
	asking map[core.AgentTool]*asking
	// What the status command printed, while signed in (Kiro's says who is signed in).
	said map[core.AgentTool]string
}{done: map[core.AgentTool]checkDone{}, asking: map[core.AgentTool]*asking{}, said: map[core.AgentTool]string{}}

// Known is the last check, if any, without running one.
func Known(t core.AgentTool) (AgentReady, bool) {
	checks.Lock()
	defer checks.Unlock()
	c, ok := checks.done[t]
	return c.ready, ok
}

// Seed records a check's answer without running one (the screenshots and tests, on a
// machine without the tool).
func Seed(t core.AgentTool, ready AgentReady) {
	checks.Lock()
	checks.done[t] = checkDone{time.Now(), ready}
	checks.Unlock()
}

// Said is what the last check's status command printed, if it said signed in.
func Said(t core.AgentTool) (string, bool) {
	checks.Lock()
	defer checks.Unlock()
	s, ok := checks.said[t]
	return s, ok
}

// Check says installed and signed in, by the tool's own status command. Kept five
// minutes; a check already under way is shared rather than run twice. Blocks; call it
// off the UI thread.
func Check(t core.AgentTool, fresh bool) AgentReady {
	checks.Lock()
	if !fresh {
		if c, ok := checks.done[t]; ok && time.Since(c.at) < 300*time.Second {
			checks.Unlock()
			return c.ready
		}
	}
	if w, ok := checks.asking[t]; ok {
		checks.Unlock()
		<-w.done
		return w.ready
	}
	w := &asking{done: make(chan struct{})}
	checks.asking[t] = w
	checks.Unlock()

	ready, said := look(t)
	checks.Lock()
	checks.done[t] = checkDone{time.Now(), ready}
	if ready.SignedIn {
		checks.said[t] = said
	} else {
		delete(checks.said, t)
	}
	if w, ok := checks.asking[t]; ok {
		delete(checks.asking, t)
		w.ready = ready
		close(w.done)
	}
	checks.Unlock()
	return ready
}

// look is the answer, and what the status command printed ("" when none ran).
func look(t core.AgentTool) (AgentReady, string) {
	exe := Exe(t)
	if exe == "" || t == core.Custom {
		return AgentReady{false, false, InstallHint(t)}, ""
	}
	if t == core.OpenCode {
		// Having no sign-in doesn't mean it can't run: API keys in the environment and
		// local models count too. The version is all that is checked here; a provider
		// that can't answer shows up as the task's own error.
		vc, vt := Ask(exe, "--version")
		lines := strings.Split(strings.TrimSpace(vt), "\n")
		version := strings.TrimSpace(lines[len(lines)-1])
		bad := func(hint string) AgentReady { return AgentReady{true, false, hint} }
		have, ok := ParseVersion(strings.SplitN(version, "-", 2)[0])
		if vc == 0 && ok {
			least, _ := ParseVersion(OpenCodeMinVersion)
			if versionLess(have, least) {
				return bad(fmt.Sprintf("OpenCode %s is too old for Hover. %s", version, InstallHint(t))), ""
			}
			return AgentReady{true, true, ""}, ""
		}
		return bad("Couldn’t read OpenCode’s version. " + InstallHint(t)), ""
	}
	if t == core.Agy {
		// Never started to ask: it unpacks about 1 GB per start (T3 Code doesn't either).
		// It has no status command; a missing sign-in shows as the task's own error.
		return AgentReady{true, true, ""}, ""
	}
	cmd, args := exe, []string(nil)
	switch t {
	case core.Kiro:
		args = []string{"whoami"}
	case core.Codex:
		cmd, args = Find("codex"), []string{"login", "status"}
	case core.Cursor:
		args = []string{"status"}
	case core.Claude:
		// Exit 0 and {"loggedIn": true} when signed in, 1 when not.
		args = []string{"auth", "status"}
	}
	// The adapter can carry its own Codex; without the CLI there is nothing to ask.
	if cmd == "" {
		return AgentReady{true, true, ""}, ""
	}
	code, text := Ask(cmd, args...)
	lower := strings.ToLower(text)
	signedIn := code == 0 && !strings.Contains(lower, "not logged in") && !strings.Contains(lower, "not signed in") && !strings.Contains(lower, "logged out")
	hint := ""
	if !signedIn {
		hint = SignInHint(t)
	}
	return AgentReady{true, signedIn, hint}, text
}

// Ask runs a status command with its input closed: its code and its output, or -1 after
// 20 s (the tree killed).
func Ask(exe string, args ...string) (int, string) {
	g, err := Spawn(Hidden(exe, args...))
	if err != nil {
		return -1, err.Error()
	}
	defer g.Close()
	stdin, stdout, stderr := g.TakePipes()
	stdin.Close()
	read := func(p *os.File) chan string {
		ch := make(chan string, 1)
		go func() {
			var s []byte
			if p != nil {
				buf := make([]byte, 4096)
				for {
					n, err := p.Read(buf)
					s = append(s, buf[:n]...)
					if err != nil {
						break
					}
				}
				p.Close()
			}
			ch <- core.Lossy(s)
		}()
		return ch
	}
	o, e := read(stdout), read(stderr)
	code, ok := g.WaitTimeout(20 * time.Second)
	if !ok {
		g.Kill()
		return -1, ""
	}
	return code, StripANSI(<-o + "\n" + <-e)
}

// ParseVersion is System.Version.TryParse: two to four whole numbers between dots, none
// negative; the parts left out compare as lower (-1), as Version does.
func ParseVersion(s string) ([4]int64, bool) {
	v := [4]int64{-1, -1, -1, -1}
	parts := strings.Split(strings.TrimSpace(s), ".")
	if len(parts) < 2 || len(parts) > 4 {
		return v, false
	}
	for i, p := range parts {
		p = strings.TrimSpace(p)
		if p == "" || strings.Trim(p, "0123456789") != "" {
			return [4]int64{}, false
		}
		n, err := strconv.ParseInt(p, 10, 64)
		if err != nil {
			return [4]int64{}, false
		}
		v[i] = n
	}
	return v, true
}

// versionLess compares part by part, as Rust's arrays (and Version) do.
func versionLess(a, b [4]int64) bool {
	for i := range a {
		if a[i] != b[i] {
			return a[i] < b[i]
		}
	}
	return false
}
