package agents

// The Terminal panel's "My commands" tab: the user's own shell, in the session's folder,
// run as the user (no sandbox: it is theirs, not an agent's).
//
// One long-lived shell per chat, so `cd` and variables stay from one command to the next:
// PowerShell on Windows (`powershell.exe -NoLogo -NoProfile -NonInteractive -Command -`,
// which reads a command from its input as each line arrives and runs it), bash on Linux and
// macOS. Started through Group, so it and whatever it starts end with Hover.
//
// A command goes to the shell's stdin as data, never on a command line and never pasted
// into the shell's own syntax: the line sent is a fixed wrapper around the command in
// base64, which the shell decodes and runs (Invoke-Expression / eval, in the shell's own
// scope). So a quote or a half-typed string in a command can't break the wrapper. After
// the command the wrapper prints a marker with the exit code and the folder the shell is
// in; Framer finds it.
//
// Ctrl+C ends the running command by ending the shell and starting a new one, in the
// folder the last command left it in. (Variables set in the old shell go with it.) A
// command that waits for typed input is not supported: its input is nothing.

import (
	"encoding/base64"
	"errors"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/4regab/Hover/internal/core"
)

// Output kept for a command, and commands kept.
const (
	linesKept   = 2000
	entriesKept = 200
	historyKept = 200
)

type TermLine struct {
	Text string
	Err  bool
}

type RunKind int

const (
	RunRunning RunKind = iota
	// RunDone: ended on its own, with its exit code and how long it took (ms).
	RunDone
	// RunStopped: ended by Ctrl+C.
	RunStopped
)

type TermRun struct {
	Kind  RunKind
	Start time.Time // RunRunning's
	Code  int32     // RunDone's
	MS    uint64    // RunDone's and RunStopped's
}

// TermEntry is a command the user ran: where, what, what it printed, how it ended.
type TermEntry struct {
	Cwd, Cmd string
	Lines    []TermLine
	Run      TermRun
	Cut      int
}

// TermEv is what the shell printed, cut into events: a line (Done false), or the end of a
// command with its code and folder.
type TermEv struct {
	Done bool
	Line string
	Code int32
	Cwd  string
}

// Framer finds the end-of-command marker in the shell's output. The marker is
// <mark>|<code>|<folder> and a newline; it may arrive in pieces and may follow output that
// had no newline.
type Framer struct {
	mark string
	buf  string
}

func NewFramer(mark string) *Framer { return &Framer{mark: mark} }

// rustI32 is str::parse::<i32>: decimal digits, one sign allowed before them (as ParseInt).
func rustI32(s string) (int32, bool) {
	n, err := strconv.ParseInt(s, 10, 32)
	return int32(n), err == nil
}

func (f *Framer) Feed(chunk string) []TermEv {
	f.buf += chunk
	var out []TermEv
	for {
		if at := strings.Index(f.buf, f.mark); at >= 0 {
			// The record is complete once its line ends.
			rest := f.buf[at+len(f.mark):]
			nl := strings.IndexByte(rest, '\n')
			if nl < 0 {
				break
			}
			record := strings.TrimRight(rest[:nl], "\r")
			before := f.buf[:at]
			f.buf = f.buf[at+len(f.mark)+nl+1:]
			for _, l := range strings.Split(before, "\n") {
				// The last piece is the start of the marker's own line (empty) or output with
				// no newline.
				out = append(out, TermEv{Line: strings.TrimRight(l, "\r")})
			}
			// before ended at the marker: a trailing empty piece is not a line of output.
			if n := len(out); n > 0 && !out[n-1].Done && out[n-1].Line == "" {
				out = out[:n-1]
			}
			parts := strings.SplitN(record, "|", 3)
			code := int32(1)
			if len(parts) > 1 {
				if c, ok := rustI32(strings.TrimSpace(parts[1])); ok {
					code = c
				}
			}
			cwd := ""
			if len(parts) > 2 {
				cwd = parts[2]
			}
			out = append(out, TermEv{Done: true, Code: code, Cwd: cwd})
			continue
		}
		// No marker (yet): whole lines are output. A partial line stays, since the marker
		// may be in it.
		nl := strings.LastIndexByte(f.buf, '\n')
		if nl < 0 {
			break
		}
		done := f.buf[:nl]
		f.buf = f.buf[nl+1:]
		for _, l := range strings.Split(done, "\n") {
			out = append(out, TermEv{Line: strings.TrimRight(l, "\r")})
		}
		break
	}
	return out
}

// newMark is a marker no command has printed by accident: it has this shell's start in it.
func newMark() string {
	return fmt.Sprintf("@@hover-%x-%x@@", os.Getpid(), time.Now().UnixNano())
}

// Wrapper is the line that runs cmd in the shell and then prints the marker.
func Wrapper(cmd, mark string, windows bool) string {
	b64 := base64.StdEncoding.EncodeToString([]byte(cmd))
	if windows {
		// $? after Invoke-Expression; a native program's own code wins when it set one.
		return "$global:LASTEXITCODE=$null; $hvok=$true; try { Invoke-Expression ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('" + b64 +
			"'))); $hvok=$? } catch { $hvok=$false; [Console]::Error.WriteLine($_.Exception.Message) }; Write-Output ('" + mark +
			"|' + $(if ($null -ne $global:LASTEXITCODE) { $global:LASTEXITCODE } elseif ($hvok) { 0 } else { 1 }) + '|' + (Get-Location).Path)\n"
	}
	// The command gets no input: it must not read the lines meant for the shell.
	return "hvc=$(printf %s '" + b64 + "' | base64 -d); eval \"$hvc\" </dev/null; hvs=$?; printf '%s|%s|%s\\n' '" + mark + "' \"$hvs\" \"$PWD\"\n"
}

// Shorten cuts a path to about max characters by taking out middle folders: C:\Users\…\Hover.
func Shorten(path string, max int) string {
	if utf8.RuneCountInString(path) <= max {
		return path
	}
	sep := "/"
	if strings.Contains(path, `\`) {
		sep = `\`
	}
	var parts []string
	for _, p := range strings.Split(path, sep) {
		if p != "" {
			parts = append(parts, p)
		}
	}
	if len(parts) < 3 {
		return path
	}
	lead := ""
	if strings.HasPrefix(path, sep) {
		lead = sep
	}
	return lead + parts[0] + sep + "…" + sep + parts[len(parts)-1]
}

// Prompt is the prompt the shell shows: `PS C:\…\project> ` or `user@host:~/project$ `.
func Prompt(cwd string) string {
	if runtime.GOOS == "windows" {
		return "PS " + Shorten(cwd, 44) + "> "
	}
	home := strings.TrimRight(Home(), "/")
	shown := cwd
	if r, ok := strings.CutPrefix(cwd, home); ok && (r == "" || strings.HasPrefix(r, "/")) {
		shown = "~" + r
	}
	user, ok := os.LookupEnv("USER")
	if !ok {
		if user, ok = os.LookupEnv("USERNAME"); !ok {
			user = "user"
		}
	}
	host := ""
	if b, err := os.ReadFile("/etc/hostname"); err == nil && utf8.Valid(b) {
		host = strings.TrimSpace(string(b))
	}
	if host == "" {
		if h, ok := os.LookupEnv("HOSTNAME"); ok {
			host = h
		} else {
			host = "localhost"
		}
	}
	return fmt.Sprintf("%s@%s:%s$ ", user, host, Shorten(shown, 40))
}

// Banner is the dim line at the top: the shell, and that it runs as the user.
func Banner() string {
	if runtime.GOOS == "windows" {
		return "PowerShell · runs as you (Windows has no sandbox)"
	}
	return "bash · runs as you (your own shell, outside the agents’ sandbox)"
}

type shell struct {
	group *Group
	stdin *os.File
	mark  string
}

type termInner struct {
	mu      sync.Mutex
	folder  string
	cwd     string
	entries []TermEntry
	history []string
	// Which shell is current; a shell that was ended by Ctrl+C has an older number, and
	// what it still prints is dropped.
	gen   uint64
	shell *shell
	// Where the history walk (↑/↓) is: the number of steps back, 0 at the prompt.
	back int
}

// endShell is Rust dropping the shell: it and what it started are ended.
func (g *termInner) endShell() {
	if g.shell != nil {
		g.shell.stdin.Close()
		g.shell.group.Close()
		g.shell = nil
	}
}

// Term is the user's shell for one chat.
type Term struct {
	inner *termInner
	// notify is called from the shell's goroutines when something changed.
	notify func()
}

func NewTerm(folder string, notify func()) *Term {
	return &Term{inner: &termInner{folder: folder, cwd: folder}, notify: notify}
}

func isRunning(e []TermEntry) bool { return len(e) > 0 && e[len(e)-1].Run.Kind == RunRunning }

// View reads the entries, the folder the shell is in, and whether a command runs.
func (t *Term) View(f func(entries []TermEntry, cwd string, running bool)) {
	g := t.inner
	g.mu.Lock()
	defer g.mu.Unlock()
	f(g.entries, g.cwd, isRunning(g.entries))
}

func (t *Term) Running() bool {
	var r bool
	t.View(func(_ []TermEntry, _ string, running bool) { r = running })
	return r
}

// Run runs a command. Ignored when empty or when one runs already. clear, cls and
// Clear-Host empty the screen.
func (t *Term) Run(cmd string) {
	cmd = strings.TrimSpace(cmd)
	if cmd == "" {
		return
	}
	g := t.inner
	g.mu.Lock()
	if isRunning(g.entries) {
		g.mu.Unlock()
		return
	}
	if n := len(g.history); n == 0 || g.history[n-1] != cmd {
		g.history = append(g.history, cmd)
		if n := len(g.history); n > historyKept {
			g.history = g.history[n-historyKept:]
		}
	}
	g.back = 0
	switch asciiLower(cmd) {
	case "clear", "cls", "clear-host":
		g.entries = nil
		g.mu.Unlock()
		t.notify()
		return
	}
	g.entries = append(g.entries, TermEntry{Cwd: g.cwd, Cmd: cmd, Run: TermRun{Kind: RunRunning, Start: time.Now()}})
	if n := len(g.entries); n > entriesKept {
		g.entries = g.entries[n-entriesKept:]
	}
	if err := t.ensure(g); err != nil {
		finish(g, 1, sp("Couldn’t start the shell: "+err.Error()))
		g.mu.Unlock()
		t.notify()
		return
	}
	line := Wrapper(cmd, g.shell.mark, runtime.GOOS == "windows")
	if _, err := g.shell.stdin.Write([]byte(line)); err != nil {
		g.endShell()
		finish(g, 1, sp("The shell isn’t taking commands: "+err.Error()))
	}
	g.mu.Unlock()
	t.notify()
}

// Interrupt is Ctrl+C: it ends the running command (and the shell with it; the next
// command starts a new one in the same folder).
func (t *Term) Interrupt() {
	g := t.inner
	g.mu.Lock()
	if !isRunning(g.entries) {
		g.mu.Unlock()
		return
	}
	e := &g.entries[len(g.entries)-1]
	e.Run = TermRun{Kind: RunStopped, MS: uint64(time.Since(e.Run.Start).Milliseconds())}
	e.Lines = append(e.Lines, TermLine{Text: "^C"})
	g.gen++
	g.endShell()
	g.mu.Unlock()
	t.notify()
}

// Seed is for the screenshots: these commands show, without a shell behind them.
func (t *Term) Seed(entries []TermEntry) {
	t.inner.mu.Lock()
	t.inner.entries = entries
	t.inner.mu.Unlock()
}

func (t *Term) Clear() {
	g := t.inner
	g.mu.Lock()
	if isRunning(g.entries) {
		g.mu.Unlock()
		return
	}
	g.entries = nil
	g.mu.Unlock()
	t.notify()
}

// History is ↑ (dir -1) and ↓ (+1) through the commands run, newest first: the text to
// put in the box.
func (t *Term) History(dir int) string {
	g := t.inner
	g.mu.Lock()
	defer g.mu.Unlock()
	n := len(g.history)
	if dir < 0 {
		g.back = min(g.back+1, n)
	} else {
		g.back = max(g.back-1, 0)
	}
	if g.back == 0 {
		return ""
	}
	return g.history[n-g.back]
}

// ensure starts the shell if there is none.
func (t *Term) ensure(g *termInner) error {
	if g.shell != nil {
		return nil
	}
	if !UsableFolder(g.cwd) {
		g.cwd = g.folder
	}
	var cmd = Hidden("/bin/bash", "--norc", "--noprofile")
	if runtime.GOOS == "windows" {
		cmd = Hidden("powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-")
	}
	cmd.Dir = g.cwd
	// Colour codes are for a terminal; this one is drawn by Hover.
	cmd.Env = append(cmd.Env, "NO_COLOR=1", "TERM=dumb")
	group, err := Spawn(cmd)
	if err != nil {
		return err
	}
	stdin, stdout, stderr := group.TakePipes()
	if stdin == nil || stdout == nil {
		group.Close()
		return errors.New("no input or output")
	}
	if runtime.GOOS == "windows" {
		// Output in UTF-8, and no progress bars in it.
		if _, err := stdin.Write([]byte("[Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'\n")); err != nil {
			stdin.Close()
			group.Close()
			return err
		}
	}
	mark := newMark()
	t.readOut(stdout, g.gen, mark)
	if stderr != nil {
		t.readErr(stderr, g.gen)
	}
	g.shell = &shell{group, stdin, mark}
	return nil
}

// incomplete is how many bytes at b's end are the start of a character the next read
// finishes (Utf8Error::error_len() being None); 0 when the first fault is a real one.
func incomplete(b []byte) int {
	for i := 0; i < len(b); {
		r, w := utf8.DecodeRune(b[i:])
		if r == utf8.RuneError && w == 1 {
			if !utf8.FullRune(b[i:]) {
				return len(b) - i
			}
			return 0
		}
		i += w
	}
	return 0
}

func (t *Term) readOut(out *os.File, gen uint64, mark string) {
	g := t.inner
	go func() {
		defer out.Close()
		framer := NewFramer(mark)
		buf := make([]byte, 4096)
		var pending []byte
		for {
			n, err := out.Read(buf)
			if n == 0 && err != nil {
				break
			}
			pending = append(pending, buf[:n]...)
			// A character cut between two reads waits for its other half.
			keep := incomplete(pending)
			text := core.Lossy(pending[:len(pending)-keep])
			pending = append([]byte(nil), pending[len(pending)-keep:]...)
			evs := framer.Feed(text)
			if len(evs) == 0 {
				continue
			}
			g.mu.Lock()
			if g.gen != gen {
				g.mu.Unlock()
				return
			}
			for _, ev := range evs {
				if !ev.Done {
					pushLine(g, ev.Line, false)
					continue
				}
				if ev.Cwd != "" {
					g.cwd = ev.Cwd
				}
				finish(g, ev.Code, nil)
			}
			g.mu.Unlock()
			t.notify()
		}
		// The shell's output ended: it is gone.
		g.mu.Lock()
		if g.gen != gen {
			g.mu.Unlock()
			return
		}
		g.endShell()
		if isRunning(g.entries) {
			finish(g, 1, sp("The shell ended."))
		}
		g.mu.Unlock()
		t.notify()
	}()
}

func (t *Term) readErr(errs *os.File, gen uint64) {
	g := t.inner
	go func() {
		defer errs.Close()
		buf := make([]byte, 4096)
		carry := ""
		for {
			n, err := errs.Read(buf)
			if n == 0 && err != nil {
				return
			}
			carry += core.Lossy(buf[:n])
			nl := strings.LastIndexByte(carry, '\n')
			if nl < 0 {
				continue
			}
			done := carry[:nl]
			carry = carry[nl+1:]
			g.mu.Lock()
			if g.gen != gen {
				g.mu.Unlock()
				return
			}
			for _, l := range strings.Split(done, "\n") {
				pushLine(g, strings.TrimRight(l, "\r"), true)
			}
			g.mu.Unlock()
			t.notify()
		}
	}()
}

func pushLine(g *termInner, text string, isErr bool) {
	if !isRunning(g.entries) {
		return
	}
	e := &g.entries[len(g.entries)-1]
	e.Lines = append(e.Lines, TermLine{text, isErr})
	if len(e.Lines) > linesKept {
		e.Lines = e.Lines[1:]
		e.Cut++
	}
}

// finish: the running command ended with code (and perhaps a last word).
func finish(g *termInner, code int32, say *string) {
	if !isRunning(g.entries) {
		return
	}
	e := &g.entries[len(g.entries)-1]
	if say != nil {
		e.Lines = append(e.Lines, TermLine{*say, true})
	}
	// The blank lines PowerShell puts around a table are not output.
	for len(e.Lines) > 0 && strings.TrimSpace(e.Lines[len(e.Lines)-1].Text) == "" {
		e.Lines = e.Lines[:len(e.Lines)-1]
	}
	skip := 0
	for skip < len(e.Lines) && strings.TrimSpace(e.Lines[skip].Text) == "" {
		skip++
	}
	e.Lines = e.Lines[skip:]
	e.Run = TermRun{Kind: RunDone, Code: code, MS: uint64(time.Since(e.Run.Start).Milliseconds())}
}
