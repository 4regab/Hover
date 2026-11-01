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
	h.cmd.Env = append(os.Environ(), childEnv+"=1", "HOVER_DATA_DIR="+dir, "HOVER_NO_BROWSER=1")
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
