package agents

import (
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"
	"unicode/utf8"
)

func termLines(evs []TermEv) []string {
	out := []string{}
	for _, e := range evs {
		if !e.Done {
			out = append(out, e.Line)
		}
	}
	return out
}

// The marker ends a command however the shell's output is cut into reads, and a marker
// that follows output with no newline still ends it.
func TestTheEndOfACommandIsFoundHoweverTheOutputArrives(t *testing.T) {
	m := "@@hover-1-2@@"
	whole := "one\r\ntwo\r\n" + m + "|3|C:\\work dir\r\n"
	for cut := 0; cut <= len(whole); cut++ {
		if cut < len(whole) && !utf8.RuneStart(whole[cut]) {
			continue
		}
		f := NewFramer(m)
		evs := append(f.Feed(whole[:cut]), f.Feed(whole[cut:])...)
		if !reflect.DeepEqual(termLines(evs), []string{"one", "two"}) {
			t.Errorf("cut at %d: %q", cut, termLines(evs))
		}
		if last := evs[len(evs)-1]; last != (TermEv{Done: true, Code: 3, Cwd: `C:\work dir`}) {
			t.Errorf("cut at %d: %+v", cut, last)
		}
		dones := 0
		for _, e := range evs {
			if e.Done {
				dones++
			}
		}
		if dones != 1 {
			t.Errorf("cut at %d: %d ends", cut, dones)
		}
	}
	// Output without a newline, then the marker on the same line.
	evs := NewFramer(m).Feed("no newline" + m + "|0|/tmp\n")
	if !reflect.DeepEqual(termLines(evs), []string{"no newline"}) || evs[len(evs)-1] != (TermEv{Done: true, Code: 0, Cwd: "/tmp"}) {
		t.Errorf("%+v", evs)
	}
	// Two commands in one read, and nothing left over for the next.
	f := NewFramer(m)
	evs = f.Feed(fmt.Sprintf("a\n%s|0|/x\nb\n%s|1|/y\n", m, m))
	want := []TermEv{{Line: "a"}, {Done: true, Code: 0, Cwd: "/x"}, {Line: "b"}, {Done: true, Code: 1, Cwd: "/y"}}
	if !reflect.DeepEqual(evs, want) {
		t.Errorf("%+v", evs)
	}
	if len(f.Feed("")) != 0 {
		t.Error("left over")
	}
}

// The command is in the wrapper as base64 only, so what it contains can't reach the shell
// as syntax.
func TestACommandIsSentAsDataNotAsShellText(t *testing.T) {
	cmd := "echo \"it's\" `x` $(y); 'unclosed"
	for _, windows := range []bool{true, false} {
		line := Wrapper(cmd, "@@m@@", windows)
		if !strings.HasSuffix(line, "\n") || strings.Count(line, "\n") != 1 {
			t.Error("one line")
		}
		if strings.Contains(line, "unclosed") || strings.Contains(line, "it's") {
			t.Error(line)
		}
		if !strings.Contains(line, base64.StdEncoding.EncodeToString([]byte(cmd))) || !strings.Contains(line, "@@m@@") {
			t.Error(line)
		}
	}
}

func TestALongPathLosesItsMiddleFolders(t *testing.T) {
	if Shorten(`C:\a\b`, 44) != `C:\a\b` || Shorten(`C:\Users\james\code\deep\deeper\Hover-rust-8fb777af`, 30) != `C:\…\Hover-rust-8fb777af` ||
		Shorten("/home/james/code/deep/deeper/Hover-rust", 20) != "/home/…/Hover-rust" {
		t.Error("shorten")
	}
}

// A real shell: a command's output and exit code come back, cd lasts to the next command,
// and Ctrl+C ends a command that would never end.
func TestTheShellKeepsItsFolderAndCanBeStopped(t *testing.T) {
	dir := filepath.Join(os.TempDir(), fmt.Sprintf("hover-term-%d", os.Getpid()))
	os.MkdirAll(filepath.Join(dir, "sub"), 0o777)
	defer os.RemoveAll(dir)
	ping := make(chan struct{}, 1)
	term := NewTerm(dir, func() {
		select {
		case ping <- struct{}{}:
		default:
		}
	})
	wait := func(until func() bool) bool {
		for range 600 {
			if until() {
				return true
			}
			select {
			case <-ping:
			case <-time.After(50 * time.Millisecond):
			}
		}
		return false
	}
	win := runtime.GOOS == "windows"
	pick := func(w, u string) string {
		if win {
			return w
		}
		return u
	}
	last := func() (e TermEntry, cwd string) {
		term.View(func(es []TermEntry, c string, _ bool) { e, cwd = es[len(es)-1], c })
		return
	}
	texts := func(e TermEntry) []string {
		out := []string{}
		for _, l := range e.Lines {
			out = append(out, l.Text)
		}
		return out
	}
	term.Run(pick("echo hello; Set-Location sub", "echo hello; cd sub"))
	if !wait(func() bool { return !term.Running() }) {
		t.Fatal("the command didn't end")
	}
	e, cwd := last()
	if !reflect.DeepEqual(texts(e), []string{"hello"}) || e.Run.Kind != RunDone || e.Run.Code != 0 || !strings.HasSuffix(cwd, "sub") {
		t.Fatalf("%q %+v %s", texts(e), e.Run, cwd)
	}
	term.Run(pick("(Get-Location).Path; cmd /c exit 7", "pwd; (exit 7)"))
	if !wait(func() bool { return !term.Running() }) {
		t.Fatal("the command didn't end")
	}
	e, _ = last()
	if len(e.Lines) == 0 || !strings.HasSuffix(e.Lines[0].Text, "sub") || e.Run.Kind != RunDone || e.Run.Code != 7 {
		t.Errorf("it ran where cd left it: %q %+v", texts(e), e.Run)
	}
	term.Run(pick("Start-Sleep 60", "sleep 60"))
	if !wait(term.Running) {
		t.Fatal("not running")
	}
	term.Interrupt()
	if term.Running() {
		t.Error("still running")
	}
	if e, _ = last(); e.Run.Kind != RunStopped || e.Lines[len(e.Lines)-1].Text != "^C" {
		t.Errorf("%+v", e)
	}
	term.Run("echo again")
	if !wait(func() bool { return !term.Running() }) {
		t.Fatal("the command didn't end")
	}
	e, cwd = last()
	if !reflect.DeepEqual(texts(e), []string{"again"}) {
		t.Errorf("a new shell works after Ctrl+C: %q", texts(e))
	}
	if !strings.HasSuffix(cwd, "sub") {
		t.Errorf("and starts where the last one was: %s", cwd)
	}
}
