package backend

// A host for the tests: this test binary run again as the backend (TestMain sees
// HOVER_BACKEND_CHILD), with a data folder of its own, so each test sends real JSON lines
// down a real pipe to a real backend process, as the Mac and Windows hosts do. The tools are
// stand-ins (a RunTask the child installs; tool checks are seeded), never a real agent.

import (
	"bufio"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/agents"
	"github.com/4regab/Hover/internal/core"
)

const childEnv = "HOVER_BACKEND_CHILD"

func TestMain(m *testing.M) {
	if os.Getenv(childEnv) == "1" {
		childMain()
		return
	}
	os.Exit(m.Run())
}

// childMain is the backend process: every tool reads as ready, and runs go to the stand-in
// the HOVER_FAKE_RUN variable names.
func childMain() {
	for _, t := range core.AllTools {
		agents.Seed(t, agents.AgentReady{Installed: true, SignedIn: true})
	}
	testRunner = func(core.AgentTool) agents.RunTask { return fakeRun(os.Getenv("HOVER_FAKE_RUN")) }
	Run(os.Stdin, NewOut(os.Stdout))
}

var busyRuns struct {
	sync.Mutex
	n int
}

// fakeRun is the stand-in agent: "busy2" says the model is busy twice and then answers;
// "hold" works until it is stopped; anything else answers at once.
func fakeRun(kind string) agents.RunTask {
	return func(a agents.RunArgs) agents.KiroResult {
		a.Events(agents.KiroEvent{SessionID: strp("s1")})
		a.Progress(agents.Thinking)
		switch kind {
		case "busy2":
			busyRuns.Lock()
			busyRuns.n++
			n := busyRuns.n
			busyRuns.Unlock()
			if n <= 2 {
				return agents.NewResult(core.Failed, "Too many requests, please wait before trying again.")
			}
		case "hold":
			for !a.Ct.IsCancelled() {
				time.Sleep(10 * time.Millisecond)
			}
			return agents.NewResult(core.Cancelled, "stopped")
		}
		return agents.NewResult(core.Completed, "answer to: "+a.Prompt)
	}
}

func strp(s string) *string { return &s }

// testKeyBytes is a history key every test host can use, and testKey is it as the host sends it.
var testKeyBytes = func() []byte {
	b := make([]byte, 32)
	for i := range b {
		b[i] = byte(i)
	}
	return b
}()

var testKey = b64(testKeyBytes)

func b64(b []byte) string { return base64.StdEncoding.EncodeToString(b) }

type msg = map[string]any

// host is one backend process and its pipes.
type host struct {
	t      *testing.T
	dir    string
	cmd    *exec.Cmd
	in     io.WriteCloser
	lines  chan msg
	stderr bytes.Buffer
	mu     sync.Mutex
	seen   []string
	all    []string
}

// startHost starts a backend on the data folder dir (made if it is missing). env is extra
// KEY=value pairs. It does not send `initialize`.
func startHost(t *testing.T, dir string, env ...string) *host {
	t.Helper()
	if dir == "" {
		dir = t.TempDir()
	}
	h := &host{t: t, dir: dir, lines: make(chan msg, 1024)}
	h.cmd = exec.Command(os.Args[0])
	h.cmd.Dir = t.TempDir()
	// The tools look for their own settings and credentials under the home folder: an empty one.
	home := t.TempDir()
	h.cmd.Env = append(os.Environ(), childEnv+"=1", "HOVER_DATA_DIR="+dir, "HOVER_NO_BROWSER=1", "HOME="+home, "USERPROFILE="+home)
	h.cmd.Env = append(h.cmd.Env, env...)
	h.cmd.Stderr = &h.stderr
	in, err := h.cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	out, err := h.cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	h.in = in
	if err := h.cmd.Start(); err != nil {
		t.Fatal(err)
	}
	go func() {
		r := bufio.NewReaderSize(out, 1<<20)
		for {
			line, err := r.ReadString('\n')
			if line != "" {
				var m msg
				if json.Unmarshal([]byte(line), &m) == nil {
					h.mu.Lock()
					h.all = append(h.all, line)
					if len(line) > 300 {
						line = line[:300] + "…"
					}
					h.seen = append(h.seen, line)
					h.mu.Unlock()
					h.lines <- m
				}
			}
			if err != nil {
				close(h.lines)
				return
			}
		}
	}()
	t.Cleanup(func() {
		if h.cmd.ProcessState == nil {
			_ = h.in.Close()
			done := make(chan struct{})
			go func() { _ = h.cmd.Wait(); close(done) }()
			select {
			case <-done:
			case <-time.After(10 * time.Second):
				_ = h.cmd.Process.Kill()
			}
		}
		// A test binary built with -race reports a race in the backend here.
		if strings.Contains(h.stderr.String(), "DATA RACE") {
			t.Errorf("the backend has a data race:\n%s", h.stderr.String())
		}
	})
	return h
}

// send writes one JSON line: a string as it is, anything else encoded.
func (h *host) send(v any) {
	h.t.Helper()
	var line string
	if s, ok := v.(string); ok {
		line = s
	} else {
		b, err := json.Marshal(v)
		if err != nil {
			h.t.Fatal(err)
		}
		line = string(b)
	}
	if _, err := io.WriteString(h.in, line+"\n"); err != nil {
		h.t.Fatalf("the backend is gone: %v\n%s", err, h.stderr.String())
	}
}

// until reads messages until one satisfies f (the others are dropped).
func (h *host) until(what string, f func(msg) bool) msg {
	h.t.Helper()
	end := time.After(30 * time.Second)
	for {
		select {
		case m, ok := <-h.lines:
			if !ok {
				h.t.Fatalf("the backend ended while waiting for %s\n%s", what, h.stderr.String())
			}
			if f(m) {
				return m
			}
		case <-end:
			h.mu.Lock()
			seen := strings.Join(h.seen, "\n")
			h.mu.Unlock()
			h.t.Fatalf("timed out waiting for %s; messages so far:\n%s\nstderr:\n%s", what, seen, h.stderr.String())
		}
	}
}

// next is the next message of that type.
func (h *host) next(typ string) msg {
	h.t.Helper()
	return h.until(typ, func(m msg) bool { return m["type"] == typ })
}

// ask sends the command and reads the next message of the answer's type.
func (h *host) ask(command msg, answer string) msg {
	h.t.Helper()
	h.send(command)
	return h.next(answer)
}

// start sends `initialize` with the test key and waits for `initialized`.
func (h *host) start() {
	h.t.Helper()
	h.send(msg{"type": "initialize", "key": testKey})
	h.until("initialized", func(m msg) bool {
		if m["type"] == "backendFailure" || m["type"] == "toast" {
			h.t.Fatalf("start failed: %v", m)
		}
		return m["type"] == "initialized"
	})
}

// state asks for the office state. (`setModel` without a tool changes nothing and answers
// with a state; `ready` would also read the quotas.)
func (h *host) state() msg {
	h.t.Helper()
	return h.ask(msg{"type": "setModel"}, "state")
}

// toast is the next message when it should be a toast with this text in it.
func (h *host) toast(contains string) {
	h.t.Helper()
	m := h.until("a toast", func(m msg) bool { return m["type"] == "toast" })
	if s, _ := m["text"].(string); !strings.Contains(s, contains) {
		h.t.Fatalf("toast %q does not say %q", s, contains)
	}
}

// stop closes the backend's input and returns its exit code.
func (h *host) stop() int {
	h.t.Helper()
	_ = h.in.Close()
	done := make(chan error, 1)
	go func() { done <- h.cmd.Wait() }()
	select {
	case err := <-done:
		if err == nil {
			return 0
		}
		if x, ok := err.(*exec.ExitError); ok {
			return x.ExitCode()
		}
		h.t.Fatal(err)
	case <-time.After(20 * time.Second):
		_ = h.cmd.Process.Kill()
		h.t.Fatal("the backend did not end when its input closed")
	}
	return -1
}

// MARK: Reading what a message holds

func str(m msg, key string) string  { s, _ := m[key].(string); return s }
func num(m msg, key string) float64 { f, _ := m[key].(float64); return f }
func arr(m msg, key string) []any   { a, _ := m[key].([]any); return a }
func obj(m msg, key string) msg     { o, _ := m[key].(map[string]any); return o }

// sessionAt is the state's session at i.
func sessionAt(st msg, i int) msg {
	s := arr(st, "sessions")
	if i >= len(s) {
		return nil
	}
	return s[i].(map[string]any)
}

// tree is every file under dir with its bytes, to see that a folder was left as it was.
func tree(t *testing.T, dir string) map[string]string {
	t.Helper()
	out := map[string]string{}
	_ = filepath.WalkDir(dir, func(p string, d os.DirEntry, err error) error {
		if err == nil && !d.IsDir() && d.Name() != "hover.log" {
			rel, _ := filepath.Rel(dir, p)
			b, _ := os.ReadFile(p)
			out[filepath.ToSlash(rel)] = fmt.Sprintf("%x", b)
		}
		return nil
	})
	return out
}

// taskHost is a started backend whose agent-access notice was reviewed, so tasks can start,
// with the stand-in agent kind names (see fakeRun).
func taskHost(t *testing.T, fake string, more ...func(*core.Settings)) *host {
	t.Helper()
	dir := t.TempDir()
	noticed(t, dir, more...)
	h := startHost(t, dir, "HOVER_FAKE_RUN="+fake)
	h.start()
	return h
}

// newTask starts a task for the tool in a folder of its own and returns its session id and
// folder, once the office shows it.
func (h *host) newTask(prompt string, extra ...msg) (id float64, folder string) {
	h.t.Helper()
	folder = h.t.TempDir()
	c := msg{"type": "new", "tool": "kiro", "folder": folder, "prompt": prompt}
	for _, e := range extra {
		for k, v := range e {
			c[k] = v
		}
	}
	h.send(c)
	st := h.until("the task in the office", func(m msg) bool { return m["type"] == "state" && sessionAt(m, 0) != nil })
	return num(sessionAt(st, 0), "id"), folder
}

// stage waits for a state in which session id is at that stage.
func (h *host) stage(id float64, stage string) msg {
	h.t.Helper()
	return h.until("session "+stage, func(m msg) bool {
		if m["type"] != "state" {
			return false
		}
		for _, x := range arr(m, "sessions") {
			if s := x.(map[string]any); num(s, "id") == id && s["stage"] == stage {
				return true
			}
		}
		return false
	})
}

// sessionByID is the state's session with that id.
func sessionByID(st msg, id float64) msg {
	for _, x := range arr(st, "sessions") {
		if s := x.(map[string]any); num(s, "id") == id {
			return s
		}
	}
	return nil
}

// refused sends the command and expects it to be refused: a toast, or the answer event with
// ok false, either of which names a reason. It returns that reason.
func (h *host) refused(command msg, answer string) string {
	h.t.Helper()
	h.send(command)
	m := h.until("a refusal of "+fmt.Sprint(command["type"]), func(m msg) bool {
		return m["type"] == "toast" || answer != "" && m["type"] == answer
	})
	if m["type"] == "toast" {
		return str(m, "text")
	}
	if m["ok"] != false {
		h.t.Fatalf("%v was accepted: %v", command, m)
	}
	if str(m, "error") == "" {
		h.t.Fatalf("%v: refused without a reason: %v", command["type"], m)
	}
	return str(m, "error")
}

// allLines is every line the backend has sent so far, whole.
func (h *host) allLines() []string {
	h.mu.Lock()
	defer h.mu.Unlock()
	return append([]string(nil), h.all...)
}

// equalJSON compares two decoded messages.
func equalJSON(a, b any) bool {
	x, _ := json.Marshal(a)
	y, _ := json.Marshal(b)
	return string(x) == string(y)
}
