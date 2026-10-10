package agents

// Starting a tool as a hidden child (Quota.Hidden, AcpHost.Launch) and making sure it
// and everything it starts goes with Hover (ChildJob). Windows: a job object per tool,
// set to kill on close, so a killed or crashed Hover leaves none behind. Linux and
// macOS: the tool leads a process group of its own (setsid), and a small watchdog shell
// kills the whole group when Hover's end of its pipe closes, however Hover ended. Linux
// also sets PR_SET_PDEATHSIG; macOS has no such thing, so the watchdog alone covers it.
// (A Hover killed in the few milliseconds between the tool's start and the watchdog's
// leaves the tool running; Go's own getppid check after the prctl only catches a Hover
// gone before the exec.)

import (
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/4regab/Hover/internal/core"
)

// Link is the pipes to a running agent, and how to end it (AcpLink). Rust ends the
// agent when the link is dropped; Go callers call Kill where it went.
type Link struct {
	ToAgent   io.WriteCloser
	FromAgent io.ReadCloser
	Kill      func()
	// Errors is the end of what the agent printed on stderr, to say why it gave up.
	Errors func() string
}

// StripANSI takes escape codes out of a tool's output: `\x1B\[[0-9;?]*[A-Za-z]` and
// `\x1B\][^\x07]*\x07`.
func StripANSI(text string) string {
	b := []rune(text)
	var out strings.Builder
	out.Grow(len(text))
	digit := func(c rune) bool { return '0' <= c && c <= '9' }
	alpha := func(c rune) bool { return 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' }
	for i := 0; i < len(b); {
		if b[i] == 0x1b && i+1 < len(b) {
			if b[i+1] == '[' {
				j := i + 2
				for j < len(b) && (digit(b[j]) || b[j] == ';' || b[j] == '?') {
					j++
				}
				if j < len(b) && alpha(b[j]) {
					i = j + 1
					continue
				}
			} else if b[i+1] == ']' {
				if k := indexRune(b[i+2:], 7); k >= 0 {
					i = i + 2 + k + 1
					continue
				}
			}
		}
		out.WriteRune(b[i])
		i++
	}
	return out.String()
}

func indexRune(s []rune, r rune) int {
	for i, c := range s {
		if c == r {
			return i
		}
	}
	return -1
}

// OnPath is Quota.OnPath: the first match on PATH, with Windows' executable suffixes
// there; "" when none.
func OnPath(name string) string {
	exts := []string{""}
	if runtime.GOOS == "windows" {
		pe, ok := os.LookupEnv("PATHEXT")
		if !ok {
			pe = ".EXE;.CMD;.BAT"
		}
		exts = nil
		for _, e := range strings.Split(pe, ";") {
			if e != "" {
				exts = append(exts, e)
			}
		}
	}
	for _, dir := range filepath.SplitList(os.Getenv("PATH")) {
		d := strings.Trim(dir, `"`)
		if d == "" {
			continue
		}
		for _, ext := range exts {
			if p := pathJoin(d, name+ext); isFile(p) {
				return p
			}
		}
	}
	return ""
}

// Home is the user's home: where tools are started (Environment.SpecialFolder.UserProfile).
func Home() string {
	v := "HOME"
	if runtime.GOOS == "windows" {
		v = "USERPROFILE"
	}
	if h, ok := os.LookupEnv(v); ok {
		return h
	}
	return "/"
}

// Hidden is Quota.Hidden: no console window, all three pipes, no colour codes. A .cmd
// or .bat shim goes through cmd. The pipes are made by Group.Spawn for each of Stdin,
// Stdout and Stderr left nil; a caller wanting none sets the stream to os.DevNull.
// ponytail: a bare name ("cmd.exe", "powershell.exe") is looked up on PATH only, where
// Rust also tries Hover's own folder and System32; a PATH without System32 is the ceiling.
func Hidden(exe string, args ...string) *exec.Cmd {
	var c *exec.Cmd
	if e := strings.ToLower(extension(exe)); e == "cmd" || e == "bat" {
		c = exec.Command("cmd.exe", append([]string{"/d", "/c", exe}, args...)...)
	} else {
		c = exec.Command(exe, args...)
	}
	c.Env = append(os.Environ(), "NO_COLOR=1", "TERM=dumb")
	hideWindow(c)
	return c
}

// Group is a started tool, grouped so that killing it kills what it started. Rust kills
// it when dropped; Go callers call Close there (and a cleanup does it should one be missed).
type Group struct {
	mu     sync.Mutex
	cmd    *exec.Cmd
	pid    int
	killed bool
	stdin  *os.File
	stdout *os.File
	stderr *os.File
	exit   *exitState
	imp    *groupImp
	clean  runtime.Cleanup
}

// exitState is filled in when the tool ends; apart from the Group, so the goroutine
// waiting on the tool doesn't keep the Group (and its cleanup) alive.
type exitState struct {
	done chan struct{}
	code int
}

// Spawn starts the command in a group of its own.
func Spawn(cmd *exec.Cmd) (*Group, error) {
	g := &Group{cmd: cmd, exit: &exitState{done: make(chan struct{})}}
	// Their ends go to the child and are closed here once it has them.
	var theirs []*os.File
	closeAll := func(fs ...*os.File) {
		for _, f := range fs {
			if f != nil {
				f.Close()
			}
		}
	}
	fail := func(err error) (*Group, error) {
		closeAll(theirs...)
		closeAll(g.stdin, g.stdout, g.stderr)
		return nil, err
	}
	if cmd.Stdin == nil {
		r, w, err := os.Pipe()
		if err != nil {
			return fail(err)
		}
		cmd.Stdin, g.stdin, theirs = r, w, append(theirs, r)
	}
	if cmd.Stdout == nil {
		r, w, err := os.Pipe()
		if err != nil {
			return fail(err)
		}
		cmd.Stdout, g.stdout, theirs = w, r, append(theirs, w)
	}
	if cmd.Stderr == nil {
		r, w, err := os.Pipe()
		if err != nil {
			return fail(err)
		}
		cmd.Stderr, g.stderr, theirs = w, r, append(theirs, w)
	}
	prepare(cmd)
	if err := spawn(cmd); err != nil {
		return fail(err)
	}
	closeAll(theirs...)
	g.pid = cmd.Process.Pid
	exit := g.exit
	go func() {
		cmd.Wait()
		exit.code = -1
		if cmd.ProcessState != nil {
			exit.code = cmd.ProcessState.ExitCode()
		}
		close(exit.done)
	}()
	imp, err := attach(cmd.Process.Pid)
	if err != nil {
		// As Rust: the tool was started, and is let go with its pipes closed.
		closeAll(g.stdin, g.stdout, g.stderr)
		return nil, err
	}
	g.imp = imp
	g.clean = runtime.AddCleanup(g, func(i *groupImp) { i.kill(); i.close() }, imp)
	return g, nil
}

// Pid is the tool's process id; 0 once killed.
func (g *Group) Pid() int {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.killed {
		return 0
	}
	return g.pid
}

// Release lets the group go, not killed: the command ended on its own and what it
// started is meant to outlive it (`cua spaces start` may leave Lume's VM or daemon behind).
func (g *Group) Release() {
	g.clean.Stop()
	g.imp.release()
}

// TakePipes hands over Hover's ends of the pipes Spawn made (nil for the rest); each once.
func (g *Group) TakePipes() (stdin, stdout, stderr *os.File) {
	g.mu.Lock()
	defer g.mu.Unlock()
	stdin, stdout, stderr = g.stdin, g.stdout, g.stderr
	g.stdin, g.stdout, g.stderr = nil, nil, nil
	return
}

// Kill ends the tool and everything in its group; the child is reaped off this goroutine.
func (g *Group) Kill() {
	g.imp.kill()
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.killed {
		g.killed = true
		g.cmd.Process.Kill()
	}
}

// Close is what dropping the Rust group did: the tool killed and the group's handle let go.
func (g *Group) Close() {
	g.clean.Stop()
	g.Kill()
	g.imp.close()
}

// WaitTimeout waits for the tool to exit, up to a limit: its code, or false on timeout.
// A killed tool gives -1 at once.
func (g *Group) WaitTimeout(limit time.Duration) (int, bool) {
	g.mu.Lock()
	killed := g.killed
	g.mu.Unlock()
	if killed {
		return -1, true
	}
	t := time.NewTimer(limit)
	defer t.Stop()
	select {
	case <-g.exit.done:
		return g.exit.code, true
	case <-t.C:
		return 0, false
	}
}

// Launch is AcpHost.Launch: the tool running as an ACP server, its stderr's last 8 KB kept.
func Launch(exe string, args []string, env [][2]string) (*Link, error) {
	l, _, err := LaunchGrouped(exe, args, env)
	return l, err
}

// LaunchGrouped is Launch, with the group handed back too (a test kills the tool from outside).
func LaunchGrouped(exe string, args []string, env [][2]string) (*Link, *Group, error) {
	return launchAt(exe, args, env, Home())
}

// LaunchIn is Launch, started in a folder of its own: Claude Code takes its project from
// the folder it starts in (it has no --cwd).
func LaunchIn(exe string, args []string, env [][2]string, dir string) (*Link, error) {
	l, _, err := launchAt(exe, args, env, dir)
	return l, err
}

func launchAt(exe string, args []string, env [][2]string, dir string) (*Link, *Group, error) {
	cmd := Hidden(exe, args...)
	cmd.Dir = dir
	for _, kv := range env {
		cmd.Env = append(cmd.Env, kv[0]+"="+kv[1])
	}
	group, err := Spawn(cmd)
	if err != nil {
		return nil, nil, err
	}
	stdin, stdout, stderr := group.TakePipes()
	var mu sync.Mutex
	tail := ""
	go func() {
		buf := make([]byte, 2048)
		for {
			n, err := stderr.Read(buf)
			if n > 0 {
				mu.Lock()
				tail += core.Lossy(buf[:n])
				if over := units(tail) - 8192; over > 0 {
					tail = tail[cutUnits(tail, over):]
				}
				mu.Unlock()
			}
			if err != nil {
				stderr.Close()
				return
			}
		}
	}()
	return &Link{
		ToAgent:   stdin,
		FromAgent: stdout,
		Kill:      group.Kill,
		Errors: func() string {
			mu.Lock()
			defer mu.Unlock()
			return tail
		},
	}, group, nil
}

// cutUnits is the byte index just past the first character at which s's UTF-16 count
// reaches n (0 when it never does).
func cutUnits(s string, n int) int {
	u := 0
	for i := 0; i < len(s); {
		c, w := utf8.DecodeRuneInString(s[i:])
		u += utf16.RuneLen(c)
		i += w
		if u >= n {
			return i
		}
	}
	return 0
}

// pathJoin is Rust's Path::join for a plain name: a separator added unless the folder
// ends with one (or is a bare drive, C:), and nothing cleaned (filepath.Join would make
// ./x a bare x, which exec looks up on PATH).
func pathJoin(dir, name string) string {
	if dir == "" {
		return name
	}
	last := dir[len(dir)-1]
	if last == '/' || runtime.GOOS == "windows" && (last == '\\' || len(dir) == 2 && dir[1] == ':') {
		return dir + name
	}
	return dir + string(filepath.Separator) + name
}

// extension is Rust's Path::extension: after the last dot of the file name, "" when it
// has none or only a leading one (.bashrc).
func extension(p string) string {
	name := filepath.Base(p)
	if name == ".." {
		return ""
	}
	if i := strings.LastIndexByte(name, '.'); i > 0 {
		return name[i+1:]
	}
	return ""
}

// fileStem is Rust's Path::file_stem: the file name without its extension.
func fileStem(p string) string {
	name := filepath.Base(p)
	if i := strings.LastIndexByte(name, '.'); i > 0 && name != ".." {
		return name[:i]
	}
	return name
}
