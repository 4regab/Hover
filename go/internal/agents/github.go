package agents

// Services/GitHubCli.cs, for Windows, Linux and macOS: the GitHub CLI (gh), which the
// desk's pull request panels read through and Create pull request (desk) runs. Is it
// installed, is it signed in and as whom, a one-click install where the system has a
// package manager Hover may use without asking for a password (winget on Windows,
// Homebrew on a Mac), and gh's own device-code sign-in: `gh auth login --web` prints a
// one-time code, the user enters it at github.com/login/device, and git is then set to use
// gh for GitHub (`gh auth setup-git`) so a push from Hover works.
//
// Where there is no such package manager (Linux, a Mac without Homebrew) Hover does not
// install anything: it says what to run (InstallHint), and never uses sudo. gh keeps its
// sign-in in the system's keychain; the agents never read it.
//
// Everything here blocks; call it from a goroutine of your own (Start makes one).
// RunProgram is the one way git and gh are started, here and in desk: hidden, with no
// prompts, a timeout and a cap on what is read.

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/4regab/Hover/go/internal/core"
	"github.com/dlclark/regexp2"
)

const (
	DeviceURL  = "https://github.com/login/device"
	InstallURL = "https://cli.github.com"
)

const (
	ghNotInstalled = "Install the GitHub CLI to see and open pull requests."
	ghNotSignedIn  = "Sign in to GitHub to see and open pull requests."
)

// Rust's regex knows Unicode in \d, \S and \b; regexp2 (.NET's) does too.
var (
	ghVersion = regexp2.MustCompile(`\d+\.\d+\.\d+`, regexp2.None)
	ghUser    = regexp2.MustCompile(`Logged in to \S+ (?:account|as) ([A-Za-z0-9-]+)`, regexp2.None)
	ghCode    = regexp2.MustCompile(`\b([A-Z0-9]{4}-[A-Z0-9]{4})\b`, regexp2.None)
)

func group1(r *regexp2.Regexp, s string) *string {
	m, _ := r.FindStringMatch(s)
	if m == nil {
		return nil
	}
	return sp(m.GroupByNumber(1).String())
}

// ParseUser is GitHubCli.ParseUser: the account in `gh auth status`'s answer ("Logged in
// to github.com account X").
func ParseUser(text string) *string { return group1(ghUser, text) }

// ParseCode is GitHubCli.ParseCode: the one-time code gh's device flow prints ("First
// copy your one-time code: ABCD-1234").
func ParseCode(line string) *string {
	if !strings.Contains(strings.ToLower(line), "code") {
		return nil
	}
	return group1(ghCode, line)
}

// parseURL is a github.com address in a line of gh's, else nil: only Hover's own default
// is ever offered to open, never a host a line of output names.
func parseURL(line string) *string {
	at := strings.Index(line, "https://github.com/")
	if at < 0 {
		return nil
	}
	rest := line[at:]
	end := strings.IndexFunc(rest, func(c rune) bool { return unicode.IsSpace(c) || strings.ContainsRune(`"'<>`, c) })
	if end >= 0 {
		rest = rest[:end]
	}
	return &rest
}

// MARK: Running a program

// Ran is what a program said: its exit code (-1 when it couldn't run or timed out, 0 when
// its output was cut at the cap), stdout, stderr without colour codes.
type Ran struct {
	Code     int
	Out, Err string
	// Capped: stdout went past the cap, the program was stopped and Out is its start.
	Capped bool
}

func RanFailed(why string) Ran { return Ran{Code: -1, Err: why} }

func (r Ran) OK() bool { return r.Code == 0 }

// isShim: a .cmd or .bat (Windows), one that Hidden would send through cmd.
func isShim(exe string) bool {
	e := strings.ToLower(extension(exe))
	return runtime.GOOS == "windows" && (e == "cmd" || e == "bat")
}

// Command is the program as a hidden child with all three pipes and no prompts: git's and
// gh's (optional locks off, so a status never takes the index lock from an agent that is
// working; no pager, no update notice), and none of the variables a git hook sets. A .cmd
// shim gets Rust's own batch-file command line, which refuses an argument cmd would read
// as more than data; Hidden would pass it through cmd.exe unchecked.
func Command(exe string, args []string, dir string, env [][2]string) *exec.Cmd {
	var c *exec.Cmd
	if isShim(exe) {
		c = batCommand(exe, args)
		c.Env = append(os.Environ(), "NO_COLOR=1", "TERM=dumb")
	} else {
		c = Hidden(exe, args...)
	}
	if dir != "" {
		c.Dir = dir
	}
	c.Env = envWithout(c.Env, "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR")
	c.Env = append(c.Env, "GIT_OPTIONAL_LOCKS=0", "GIT_TERMINAL_PROMPT=0", "GIT_PAGER=cat", "GH_PROMPT_DISABLED=1",
		"GH_NO_UPDATE_NOTIFIER=1", "GH_PAGER=cat", "PAGER=cat")
	for _, kv := range env {
		c.Env = append(c.Env, kv[0]+"="+kv[1])
	}
	return c
}

// RunProgram is DeskInfo.Run: a program run hidden in a folder ("" for Hover's own),
// with no prompts, stdout read up to max bytes, and stopped (with what it started) after
// timeout. stdin is written to it and closed; nil closes it at once. Long text (a pull
// request's description, a commit message) goes this way, never on a command line.
func RunProgram(exe, dir string, timeout time.Duration, max int, args []string, stdin []byte, env [][2]string) Ran {
	group, err := Spawn(Command(exe, args, dir, env))
	if err != nil {
		return RanFailed(err.Error())
	}
	defer group.Close()
	sin, sout, serr := group.TakePipes()
	if stdin != nil && sin != nil {
		// Off this goroutine: a pipe holds only so much, and the program may not read it yet.
		go func() { sin.Write(stdin); sin.Close() }()
	} else if sin != nil {
		sin.Close()
	}
	capped := &atomic.Bool{}
	out := pump(sout, max, capped, group)
	errs := pump(serr, 64*1024, nil, nil)
	code, ok := group.WaitTimeout(timeout)
	if !ok {
		group.Kill()
		return RanFailed("Timed out.")
	}
	// The pipes end with the program, unless something it started holds them: then what
	// is there is what there is, and the group's end (deferred) closes them.
	for _, p := range []*pumped{out, errs} {
		select {
		case <-p.done:
		case <-time.After(2 * time.Second):
		}
	}
	if capped.Load() {
		code = 0
	}
	return Ran{Code: code, Out: core.Lossy(out.bytes()), Err: StripANSI(core.Lossy(errs.bytes())), Capped: capped.Load()}
}

type pumped struct {
	mu   sync.Mutex
	buf  []byte
	done chan struct{}
}

func (p *pumped) bytes() []byte {
	p.mu.Lock()
	defer p.mu.Unlock()
	return slices.Clone(p.buf)
}

// pump reads a pipe to its end on a goroutine of its own. With a group to stop: what is
// past the cap ends the program (its output is more than anyone reads); without, it is
// read on and dropped, so the program never blocks on a full pipe.
func pump(pipe *os.File, limit int, capped *atomic.Bool, stop *Group) *pumped {
	p := &pumped{done: make(chan struct{})}
	if pipe == nil {
		close(p.done)
		return p
	}
	go func() {
		defer close(p.done)
		defer pipe.Close()
		chunk := make([]byte, 16384)
		for {
			n, err := pipe.Read(chunk)
			if n > 0 {
				p.mu.Lock()
				room := max(limit-len(p.buf), 0)
				p.buf = append(p.buf, chunk[:min(n, room)]...)
				p.mu.Unlock()
				if n > room && stop != nil {
					capped.Store(true)
					stop.Kill()
					return
				}
			}
			if err != nil {
				return
			}
		}
	}()
	return p
}

type ended struct {
	kind int // endedExit, endedCancelled, endedTimedOut, endedNoStart
	code int
	why  string
}

const (
	endedExit = iota
	endedCancelled
	endedTimedOut
	endedNoStart
)

// stream runs a program and hands each line it prints (either pipe; a line ends at \n or
// \r, as a progress bar redraws itself) to onLine, which answers true to have an Enter
// sent to it. Ends with the program, the cancel, or the timeout (which end it and what it
// started).
func stream(exe string, args []string, env [][2]string, timeout time.Duration, cancel *Cancel, keepStdin bool, onLine func(string) bool) ended {
	group, err := Spawn(Command(exe, args, "", env))
	if err != nil {
		return ended{kind: endedNoStart, why: err.Error()}
	}
	defer group.Close()
	sin, sout, serr := group.TakePipes()
	if !keepStdin {
		sin.Close()
		sin = nil
	}
	stopped := make(chan struct{})
	defer close(stopped)
	rx := make(chan string, 64)
	var readers sync.WaitGroup
	for _, r := range []*os.File{sout, serr} {
		if r != nil {
			readers.Add(1)
			go func() { defer readers.Done(); lines(r, rx, stopped) }()
		}
	}
	go func() { readers.Wait(); close(rx) }()
	start := time.Now()
	open := true
	handle := func(line string) {
		if onLine(line) && sin != nil {
			sin.Write([]byte("\n"))
		}
	}
	for {
		if open {
			select {
			case l, ok := <-rx:
				if ok {
					handle(l)
				} else {
					open = false
				}
			case <-time.After(100 * time.Millisecond):
			}
		} else {
			time.Sleep(50 * time.Millisecond)
		}
		if cancel.IsCancelled() {
			group.Kill()
			return ended{kind: endedCancelled}
		}
		if time.Since(start) >= timeout {
			group.Kill()
			return ended{kind: endedTimedOut}
		}
		if code, ok := group.WaitTimeout(0); ok {
			// What it printed last is what the user needs if it failed.
			drainLines(rx, 300*time.Millisecond, handle)
			return ended{kind: endedExit, code: code}
		}
	}
}

// drainLines hands on lines until none comes for wait, or there are no more.
func drainLines(rx <-chan string, wait time.Duration, handle func(string)) {
	for {
		select {
		case l, ok := <-rx:
			if !ok {
				return
			}
			handle(l)
		case <-time.After(wait):
			return
		}
	}
}

// lines sends a pipe as lines, as they arrive. A prompt waits without its newline (gh's
// "Press Enter to open..."), so one that says so is sent as it stands.
func lines(r *os.File, tx chan<- string, stopped <-chan struct{}) {
	defer r.Close()
	var pending []byte
	send := func() bool {
		s := strings.TrimSpace(core.Lossy(pending))
		pending = pending[:0]
		if s == "" {
			return true
		}
		select {
		case tx <- s:
			return true
		case <-stopped:
			return false
		}
	}
	chunk := make([]byte, 4096)
	for {
		n, err := r.Read(chunk)
		for _, b := range chunk[:n] {
			if b == '\n' || b == '\r' {
				if !send() {
					return
				}
			} else {
				pending = append(pending, b)
			}
		}
		if n > 0 && (len(pending) > 64*1024 || strings.Contains(core.Lossy(pending), "Press Enter")) && !send() {
			return
		}
		if err != nil {
			break
		}
	}
	send()
}

// clipChars cuts a line to limit characters (not UTF-16 units), an ellipsis last.
func clipChars(line string, limit int) string {
	if utf8.RuneCountInString(line) <= limit {
		return line
	}
	r := []rune(line)
	return string(r[:limit-1]) + "…"
}

// MARK: The CLI

// GhStatus is what Hover knows: installed, signed in and as whom.
type GhStatus struct {
	Installed, SignedIn bool
	User, Version       *string
	// Hint is what to tell the user when it isn't ready; empty when it is.
	Hint string
}

// GhStep is what a setup is doing.
type GhStep int

const (
	Installing GhStep = iota + 1
	SigningIn
)

// Name is the word desk.js and the C# used: "installing" or "signing-in".
func (s GhStep) Name() string {
	if s == Installing {
		return "installing"
	}
	return "signing-in"
}

// GhProgress is a setup's state: what it is doing (0: none), its newest line, the
// one-time code while sign-in waits (and the address to enter it at), and why it stopped
// if it failed. All empty when no setup runs.
type GhProgress struct {
	Step GhStep
	Line string
	Code *string
	// URL is where the code is entered: always on github.com, DeviceURL unless gh named
	// another page there.
	URL   *string
	Error *string
}

func progressAt(step GhStep, line string) GhProgress { return GhProgress{Step: step, Line: line} }

func progressFailed(why string) GhProgress { return GhProgress{Error: &why} }

// InstallPlan is how gh gets installed from Hover: winget's or Homebrew's program, or
// (Manual) what to tell the user, as Hover doesn't install it here.
type InstallPlan struct {
	Winget, Homebrew string
	Manual           *string
}

// LinuxInstallLine is the line Linux's users are told to run, from /etc/os-release's
// text. Hover never runs it: it would need sudo.
func LinuxInstallLine(osRelease string) string {
	field := func(k string) string {
		for _, l := range rustLines(osRelease) {
			if v, ok := strings.CutPrefix(l, k); ok {
				if v, ok := strings.CutPrefix(v, "="); ok {
					return strings.ToLower(strings.Trim(v, `"`))
				}
			}
		}
		return ""
	}
	ids := strings.Fields(field("ID") + " " + field("ID_LIKE"))
	has := func(names ...string) bool {
		return slices.ContainsFunc(ids, func(i string) bool { return slices.Contains(names, i) })
	}
	switch {
	case has("debian", "ubuntu", "linuxmint", "pop", "raspbian"):
		return "sudo apt install gh"
	case has("fedora", "rhel", "centos", "rocky", "almalinux"):
		return "sudo dnf install gh"
	case has("arch", "manjaro", "endeavouros"):
		return "sudo pacman -S github-cli"
	case has("suse", "opensuse", "opensuse-leap", "opensuse-tumbleweed", "sles"):
		return "sudo zypper install gh"
	case has("alpine"):
		return "sudo apk add github-cli"
	}
	return ""
}

// KnownPlaces is where gh's installers put it that a desktop app's PATH may not list (a
// Mac app started from the Dock has only /usr/bin:/bin:/usr/sbin:/sbin).
func KnownPlaces() []string {
	var v []string
	switch runtime.GOOS {
	case "windows":
		for _, name := range []string{"ProgramFiles", "ProgramW6432"} {
			if p, ok := os.LookupEnv(name); ok {
				v = append(v, filepath.Join(p, "GitHub CLI", "gh.exe"))
			}
		}
		if p, ok := os.LookupEnv("LOCALAPPDATA"); ok {
			v = append(v, filepath.Join(p, "Programs", "GitHub CLI", "gh.exe"))
		}
	case "darwin":
		v = append(v, "/opt/homebrew/bin/gh", "/usr/local/bin/gh", filepath.Join(Home(), ".local", "bin", "gh"), "/usr/bin/gh")
	default:
		v = append(v, "/usr/bin/gh", "/usr/local/bin/gh", filepath.Join(Home(), ".local", "bin", "gh"), "/home/linuxbrew/.linuxbrew/bin/gh", "/snap/bin/gh")
	}
	return v
}

// FindGh is gh, from PATH or where its installers put it; "" when neither.
func FindGh() string {
	if p := OnPath("gh"); p != "" {
		return p
	}
	for _, p := range KnownPlaces() {
		if isFile(p) {
			return p
		}
	}
	return ""
}

var sharedGh = sync.OnceValue(func() *GitHubCli { return NewGitHubCli() })

// SharedGh is the one gh of the app.
func SharedGh() *GitHubCli { return sharedGh() }

type GitHubCli struct {
	// fixed is a gh to use whatever PATH says (tests use a stand-in).
	fixed *string
	// env is more variables for every program started (tests point git away from the
	// user's config).
	env [][2]string
	// plan is an install plan to use whatever the system has (tests use a stand-in for winget).
	plan *InstallPlan

	mu       sync.Mutex
	progress GhProgress
	known    *struct {
		at time.Time
		s  GhStatus
	}
	running *Cancel

	changedMu sync.Mutex
	changed   []func()
}

// errStopped is a setup step ended by its cancel; any other error is why it failed.
var errStopped = errors.New("cancelled")

func NewGitHubCli() *GitHubCli { return &GitHubCli{} }

// With uses this gh (nil: whatever PATH says), and this extra environment for every
// program it starts.
func (g *GitHubCli) With(gh *string, env [][2]string) *GitHubCli {
	g.fixed, g.env = gh, env
	return g
}

// WithInstall installs with this instead of what the system has.
func (g *GitHubCli) WithInstall(plan InstallPlan) *GitHubCli {
	g.plan = &plan
	return g
}

// Env is the extra environment for programs started (Desk passes it on to git).
func (g *GitHubCli) Env() [][2]string { return g.env }

// Exe is GitHubCli.Exe; "" when there is none.
func (g *GitHubCli) Exe() string {
	if g.fixed != nil {
		if isFile(*g.fixed) {
			return *g.fixed
		}
		return ""
	}
	return FindGh()
}

// OnChanged is raised off the caller's goroutine when the status or a setup's progress changes.
func (g *GitHubCli) OnChanged(f func()) {
	g.changedMu.Lock()
	g.changed = append(g.changed, f)
	g.changedMu.Unlock()
}

func (g *GitHubCli) raise() {
	g.changedMu.Lock()
	all := slices.Clone(g.changed)
	g.changedMu.Unlock()
	for _, f := range all {
		f()
	}
}

func (g *GitHubCli) report(p GhProgress) {
	g.mu.Lock()
	g.progress = p
	g.mu.Unlock()
	g.raise()
}

// Setup is what a setup is doing now (empty when none is).
func (g *GitHubCli) Setup() GhProgress {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.progress
}

func (g *GitHubCli) Busy() bool {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.running != nil
}

// Known is the last status read, however old.
func (g *GitHubCli) Known() (GhStatus, bool) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.known == nil {
		return GhStatus{}, false
	}
	return g.known.s, true
}

// Check is GitHubCli.Check: installed and signed in, from gh itself. Kept a minute unless fresh.
func (g *GitHubCli) Check(fresh bool) GhStatus {
	if !fresh {
		g.mu.Lock()
		k := g.known
		g.mu.Unlock()
		if k != nil && time.Since(k.at) < time.Minute {
			return k.s
		}
	}
	var s GhStatus
	if gh := g.Exe(); gh == "" {
		s = GhStatus{Hint: ghNotInstalled}
	} else {
		v := RunProgram(gh, "", 15*time.Second, 64*1024, []string{"--version"}, nil, g.env)
		var version *string
		if v.OK() {
			if m, _ := ghVersion.FindStringMatch(v.Out); m != nil {
				version = sp(m.String())
			}
		}
		a := RunProgram(gh, "", 20*time.Second, 64*1024, []string{"auth", "status", "--hostname", "github.com"}, nil, g.env)
		if a.OK() {
			// gh prints this on stdout, older versions on stderr.
			user := ParseUser(a.Out)
			if user == nil {
				user = ParseUser(a.Err)
			}
			s = GhStatus{Installed: true, SignedIn: true, User: user, Version: version}
		} else {
			s = GhStatus{Installed: true, Version: version, Hint: ghNotSignedIn}
		}
	}
	g.mu.Lock()
	g.known = &struct {
		at time.Time
		s  GhStatus
	}{time.Now(), s}
	g.mu.Unlock()
	g.raise()
	return s
}

// Cancel ends a setup under way (its sign-in waiting for the code, or its install).
func (g *GitHubCli) Cancel() {
	g.mu.Lock()
	c := g.running
	g.mu.Unlock()
	if c != nil {
		c.Cancel()
	}
}

// MARK: Install

// InstallPlan is how gh would be installed here: with winget (Windows) or Homebrew (a
// Mac) if the system has it, else by hand.
func (g *GitHubCli) InstallPlan() InstallPlan {
	if g.plan != nil {
		return *g.plan
	}
	site := strings.TrimPrefix(InstallURL, "https://")
	switch runtime.GOOS {
	case "windows":
		if w := OnPath("winget"); w != "" {
			return InstallPlan{Winget: w}
		}
		return InstallPlan{Manual: sp(fmt.Sprintf("Install the GitHub CLI from %s. (winget isn’t available.)", site))}
	case "darwin":
		brew := OnPath("brew")
		for _, p := range []string{"/opt/homebrew/bin/brew", "/usr/local/bin/brew"} {
			if brew == "" && isFile(p) {
				brew = p
			}
		}
		if brew != "" {
			return InstallPlan{Homebrew: brew}
		}
		return InstallPlan{Manual: sp(fmt.Sprintf("Install the GitHub CLI with Homebrew (brew install gh) or from %s.", site))}
	}
	line := ""
	if b, err := os.ReadFile("/etc/os-release"); err == nil && utf8.Valid(b) {
		line = LinuxInstallLine(string(b))
	}
	if line == "" {
		return InstallPlan{Manual: sp(fmt.Sprintf("Install the GitHub CLI with your package manager, or from %s.", site))}
	}
	return InstallPlan{Manual: sp(fmt.Sprintf("Install the GitHub CLI by running `%s` in a terminal, or see %s.", line, site))}
}

// CanInstall: whether one click can install gh here.
func (g *GitHubCli) CanInstall() bool { return g.InstallPlan().Manual == nil }

// InstallHint is what to tell the user when one click can't install it; nil when it can.
func (g *GitHubCli) InstallHint() *string { return g.InstallPlan().Manual }

func (g *GitHubCli) install(cancel *Cancel) error {
	plan := g.InstallPlan()
	var exe string
	var args []string
	switch {
	case plan.Manual != nil:
		return errors.New(*plan.Manual)
	case plan.Winget != "":
		exe, args = plan.Winget, []string{"install", "--id", "GitHub.cli", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"}
	default:
		exe, args = plan.Homebrew, []string{"install", "gh"}
	}
	env := append(slices.Clone(g.env), [2]string{"HOMEBREW_NO_AUTO_UPDATE", "1"}, [2]string{"NONINTERACTIVE", "1"})
	tail := ""
	e := stream(exe, args, env, 10*time.Minute, cancel, false, func(l string) bool {
		tail = strings.TrimSpace(StripANSI(l))
		if tail != "" {
			g.report(progressAt(Installing, clipChars(tail, 140)))
		}
		return false
	})
	switch e.kind {
	case endedExit:
		if e.code == 0 {
			return nil
		}
		if tail == "" {
			tail = fmt.Sprintf("exit code %d", e.code)
		}
		return fmt.Errorf("Couldn’t install the GitHub CLI: %s", tail)
	case endedCancelled:
		return errStopped
	case endedTimedOut:
		return errors.New("The install took too long and was stopped.")
	}
	return fmt.Errorf("Couldn’t install the GitHub CLI: %s", e.why)
}

// MARK: Sign in

// signIn is gh's device flow: it prints a one-time code, the user enters it at
// github.com/login/device, and gh waits until they have.
func (g *GitHubCli) signIn(cancel *Cancel) error {
	gh := g.Exe()
	if gh == "" {
		return errors.New("gh isn’t installed.")
	}
	g.report(progressAt(SigningIn, "Asking GitHub for a sign-in code…"))
	var code *string
	url := DeviceURL
	e := stream(gh, []string{"auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web"}, g.env, 15*time.Minute, cancel, true, func(raw string) bool {
		l := strings.TrimSpace(StripANSI(raw))
		if l == "" {
			return false
		}
		if code == nil {
			code = ParseCode(l)
		}
		if u := parseURL(l); u != nil {
			url = *u
		}
		p := GhProgress{Step: SigningIn, Code: code}
		if code == nil {
			p.Line = clipChars(l, 140)
		} else {
			p.Line = fmt.Sprintf("Enter the code at %s, then come back.", strings.TrimPrefix(url, "https://"))
			p.URL = sp(url)
		}
		g.report(p)
		// "Press Enter to open github.com in your browser..."
		return strings.Contains(strings.ToLower(l), "press enter")
	})
	switch e.kind {
	case endedCancelled:
		return errStopped
	case endedTimedOut:
		return errors.New("GitHub sign-in timed out.")
	case endedExit:
		if e.code == 0 {
			break
		}
		fallthrough
	default:
		return errors.New("GitHub sign-in didn’t finish.")
	}
	g.report(progressAt(SigningIn, "Setting up git to use your GitHub sign-in…"))
	g.SetupGit(gh)
	return nil
}

// SetupGit is `gh auth setup-git`: git asks gh for GitHub's credentials, so a push from
// Hover works.
func (g *GitHubCli) SetupGit(gh string) bool {
	return RunProgram(gh, "", 30*time.Second, 64*1024, []string{"auth", "setup-git", "--hostname", "github.com"}, nil, g.env).OK()
}

// MARK: One click

// Start starts RunSetup on a goroutine of its own; false when a setup already runs.
func (g *GitHubCli) Start() bool {
	if g.Busy() {
		return false
	}
	go g.RunSetup()
	return true
}

// RunSetup is GitHubCli.Run: one click. Installs gh if it is missing, then signs in if it
// isn't. Blocks until done, failed or cancelled; Setup says how it went (its Error).
func (g *GitHubCli) RunSetup() {
	g.mu.Lock()
	if g.running != nil {
		g.mu.Unlock()
		return
	}
	cancel := NewCancel()
	g.running = cancel
	g.mu.Unlock()
	// However this ends (a panic too), the next click can start.
	freed := false
	free := func() {
		if !freed {
			freed = true
			g.mu.Lock()
			g.running = nil
			g.mu.Unlock()
		}
	}
	defer free()
	err := func() error {
		s := g.Check(true)
		if !s.Installed {
			g.report(progressAt(Installing, "Installing the GitHub CLI…"))
			if err := g.install(cancel); err != nil {
				return err
			}
			if s = g.Check(true); !s.Installed {
				return errors.New("The install finished, but gh still isn’t found.")
			}
		}
		if !s.SignedIn {
			if err := g.signIn(cancel); err != nil {
				return err
			}
			if s = g.Check(true); !s.SignedIn {
				return errors.New("GitHub sign-in didn’t finish. Try again.")
			}
		}
		return nil
	}()
	if err == nil || err == errStopped {
		g.report(GhProgress{})
	} else {
		g.report(progressFailed(err.Error()))
	}
	// Free before the last read of the status, which can take a while.
	free()
	g.Check(true)
}
