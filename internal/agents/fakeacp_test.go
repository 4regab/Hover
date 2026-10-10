package agents

// tests/fakeacp.rs: AcpHost and the sessions against port/tools/FakeAcp, the stand-in agent
// both builds are benchmarked with. Each scenario runs from a recording in
// tests/golden/acp (what Hover sent, "> ", and what FakeAcp answered, "< "), so it needs
// no .NET: the replay checks every line Hover sends against the recording and answers with
// FakeAcp's own bytes. With FAKEACP=<path to the FakeAcp binary> the same scenarios run
// against the real process instead, and HOVER_RECORD=1 writes the recordings again.
//
// What Hover sends is also what the C# AcpHost sends: the recorded "> " lines are the
// anonymous objects of AcpHost.cs as System.Text.Json writes them.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

func repoGolden(parts ...string) string {
	return filepath.Join(append([]string{"..", "..", "tests", "golden"}, parts...)...)
}

func goldenAcp(name string) string { return repoGolden("acp", name+".txt") }

func richAnswer(t *testing.T) string {
	b, err := os.ReadFile(repoGolden("fixtures", "rich.md"))
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func fakeAcpLive() string {
	if p := os.Getenv("FAKEACP"); p != "" && isFile(p) {
		return p
	}
	return ""
}

// escaped is a string as it is inside JSON's quotes.
func escaped(s string) string {
	e := core.JStr(s).Compact()
	return e[1 : len(e)-1]
}

// MARK: Live, with a recorder between Hover and FakeAcp

type recLog struct {
	mu    sync.Mutex
	lines []string
	pids  []string
}

func (l *recLog) push(s string) {
	l.mu.Lock()
	l.lines = append(l.lines, s)
	l.mu.Unlock()
}

// tee keeps each whole line that passes, marked with its way.
type tee struct {
	buf []byte
	dir string
	log *recLog
}

func (t *tee) took(b []byte) {
	t.buf = append(t.buf, b...)
	for {
		i := strings.IndexByte(string(t.buf), '\n')
		if i < 0 {
			return
		}
		t.log.push(t.dir + core.Lossy(t.buf[:i]))
		t.buf = t.buf[i+1:]
	}
}

type teeW struct {
	tee
	w io.WriteCloser
}

func (t *teeW) Write(b []byte) (int, error) {
	n, err := t.w.Write(b)
	t.took(b[:n])
	return n, err
}

func (t *teeW) Close() error { return t.w.Close() }

type teeR struct {
	tee
	r io.ReadCloser
}

func (t *teeR) Read(b []byte) (int, error) {
	n, err := t.r.Read(b)
	t.took(b[:n])
	if n == 0 && err != nil {
		t.log.push("! eof")
	}
	return n, err
}

func (t *teeR) Close() error { return t.r.Close() }

type liveRec struct {
	log    *recLog
	mu     sync.Mutex
	groups []*Group
}

func liveConnect(exe string, env [][2]string) (*liveRec, func() (*Link, error)) {
	l := &liveRec{log: &recLog{}}
	return l, func() (*Link, error) {
		link, group, err := LaunchGrouped(exe, []string{"acp"}, env)
		if err != nil {
			return nil, err
		}
		l.log.mu.Lock()
		l.log.pids = append(l.log.pids, fmt.Sprint(group.Pid()))
		l.log.mu.Unlock()
		l.mu.Lock()
		l.groups = append(l.groups, group)
		l.mu.Unlock()
		return &Link{ToAgent: &teeW{tee{dir: "> ", log: l.log}, link.ToAgent}, FromAgent: &teeR{tee{dir: "< ", log: l.log}, link.FromAgent},
			Kill: link.Kill, Errors: link.Errors}, nil
	}
}

// save writes the recording, with this run's folder and FakeAcp's process ids made stable.
func (l *liveRec) save(t *testing.T, name, folder string) {
	if os.Getenv("HOVER_RECORD") == "" {
		return
	}
	l.log.mu.Lock()
	defer l.log.mu.Unlock()
	esc := escaped(folder)
	var text strings.Builder
	for _, line := range l.log.lines {
		line = strings.ReplaceAll(line, esc, "{folder}")
		for i, pid := range l.log.pids {
			line = strings.ReplaceAll(line, "fake-"+pid+"-", fmt.Sprintf("fake-P%d-", i+1))
		}
		text.WriteString(line + "\n")
	}
	os.MkdirAll(filepath.Dir(goldenAcp(name)), 0o777)
	if err := os.WriteFile(goldenAcp(name), []byte(text.String()), 0o666); err != nil {
		t.Fatal(err)
	}
}

// MARK: Replay

// replayRec plays FakeAcp's side of a recording: each line Hover sends is checked against
// the next "> " line, then the "< " lines that followed it are sent back.
type replayRec struct {
	mu     sync.Mutex
	script []string
	wrong  []string
}

func replayConnect(t *testing.T, name, folder string) (*replayRec, func() (*Link, error)) {
	b, err := os.ReadFile(goldenAcp(name))
	if err != nil {
		t.Fatalf("no recording %s: run with FAKEACP and HOVER_RECORD=1", name)
	}
	esc := escaped(folder)
	rp := &replayRec{}
	for _, l := range rustLines(string(b)) {
		rp.script = append(rp.script, strings.ReplaceAll(l, "{folder}", esc))
	}
	return rp, func() (*Link, error) {
		link, _, err := pipeLink(func(from *bufio.Reader, out *pipeOut) {
			emit := func() {
				for {
					rp.mu.Lock()
					var next string
					ok := len(rp.script) > 0 && (strings.HasPrefix(rp.script[0], "< ") || rp.script[0] == "! eof")
					if ok {
						next, rp.script = rp.script[0], rp.script[1:]
					}
					rp.mu.Unlock()
					switch {
					case !ok:
						return
					case next == "! eof":
						out.take()
						return
					}
					// Paced, so a turn takes some time (a reply can queue behind it).
					time.Sleep(3 * time.Millisecond)
					out.say(next[2:])
				}
			}
			emit()
			eachLine(from, func(line string) {
				rp.mu.Lock()
				want := "none"
				ok := len(rp.script) > 0 && rp.script[0] == "> "+line
				if len(rp.script) > 0 {
					want, rp.script = fmt.Sprintf("%q", rp.script[0]), rp.script[1:]
				}
				if !ok {
					rp.wrong = append(rp.wrong, fmt.Sprintf("sent  %s\nwanted %s", line, want))
				}
				rp.mu.Unlock()
				emit()
			})
		})
		return link, err
	}
}

func (rp *replayRec) check(t *testing.T) {
	rp.mu.Lock()
	defer rp.mu.Unlock()
	if len(rp.wrong) > 0 {
		t.Errorf("Hover sent other lines than recorded:\n%s", strings.Join(rp.wrong, "\n"))
	}
	var left []string
	for _, l := range rp.script {
		if strings.HasPrefix(l, "> ") {
			left = append(left, l)
		}
	}
	if len(left) > 0 {
		t.Errorf("Hover didn't send:\n%s", strings.Join(left, "\n"))
	}
}

// scenario runs body live when FAKEACP is set, else replayed. kill ends the agent from
// outside.
func scenario(t *testing.T, name string, env [][2]string, body func(host *AcpHost, dir string, kill func())) {
	dir := newDir(t, "fakeacp-"+name)
	if exe := fakeAcpLive(); exe != "" {
		l, connect := liveConnect(exe, env)
		host := AcpHostWithConnect(core.Kiro, core.DefaultAgentOptions, connect)
		body(host, dir, func() {
			l.mu.Lock()
			defer l.mu.Unlock()
			if n := len(l.groups); n > 0 {
				l.groups[n-1].Kill()
			}
		})
		host.Shutdown("test")
		time.Sleep(200 * time.Millisecond)
		l.save(t, name, dir)
		return
	}
	rp, connect := replayConnect(t, name, dir)
	host := AcpHostWithConnect(core.Kiro, core.DefaultAgentOptions, connect)
	// Replayed, the recording's end of stream stands for the kill.
	body(host, dir, func() {})
	host.Shutdown("test")
	rp.check(t)
}

// One second at 20 updates a second: two steps (read, search), usage twice, thinking
// between, then the rich answer; a reply goes to the same conversation.
func TestATurnAndAReply(t *testing.T) {
	answer, _ := filepath.Abs(repoGolden("fixtures", "rich.md"))
	rich := richAnswer(t)
	scenario(t, "turn-and-reply", [][2]string{{"FAKEACP_SECONDS", "1"}, {"FAKEACP_RATE", "20"}, {"FAKEACP_ANSWER", answer}}, func(host *AcpHost, dir string, _ func()) {
		rec := &recorders{}
		r := host.Run(dir, `Fix the "flaky" tests & say what changed`, rec.p, NewCancel(), nil, rec.e)
		if r.State != core.Completed {
			t.Fatal(r)
		}
		if r.Text != strings.TrimSpace(rich) {
			t.Error("the answer comes back whole, escapes and all:", r.Text)
		}
		ev := rec.evs()
		sid := rec.sid()
		// The reasoning it sent is kept too, as thoughts between the tool calls.
		var order []string
		for _, e := range ev {
			if e.Step != nil && e.Step.Status == "completed" {
				order = append(order, e.Step.Kind)
			}
		}
		if !reflect.DeepEqual(order, []string{"thought", "read", "thought", "thought", "search", "thought"}) {
			t.Error(order)
		}
		for _, e := range ev {
			if s := e.Step; s != nil && s.Kind == "thought" && s.Status == "completed" && (!strings.HasPrefix(val(s.Output), "thinking") || s.MS == nil) {
				t.Errorf("%+v", s)
			}
		}
		var steps [][4]string
		for _, e := range ev {
			if s := e.Step; s != nil && s.Kind != "thought" {
				steps = append(steps, [4]string{s.ID, s.Kind, s.Status, val(s.Target)})
			}
		}
		if !reflect.DeepEqual(steps, [][4]string{{"t0", "read", "in_progress", "src/file0.cs"}, {"t0", "read", "completed", "src/file0.cs"},
			{"t10", "search", "in_progress", "src/file1.cs"}, {"t10", "search", "completed", "src/file1.cs"}}) {
			t.Error(steps)
		}
		// usage_update 1280 then 1680 of 200000: 0.64 %, then 0.84 % is within half a point.
		var ctx []float64
		for _, e := range ev {
			if e.Context != nil {
				ctx = append(ctx, *e.Context)
			}
		}
		if !reflect.DeepEqual(ctx, []float64{0.64}) {
			t.Error(ctx)
		}
		rec.mu.Lock()
		phases := rec.phases
		rec.mu.Unlock()
		if !reflect.DeepEqual(phases, []KiroPhase{Starting, Reading, Thinking, Searching, Thinking, Writing}) {
			t.Error(phases)
		}
		if again := host.Run(dir, "and the docs", nil, NewCancel(), &sid, nil); again.State != core.Completed {
			t.Error(again)
		}
	})
}

// A long run stopped at its second step: session/cancel, then FakeAcp's cancelled.
func TestARunIsStopped(t *testing.T) {
	scenario(t, "stop", [][2]string{{"FAKEACP_SECONDS", "30"}, {"FAKEACP_RATE", "20"}}, func(host *AcpHost, dir string, _ func()) {
		ct := NewCancel()
		e := func(e KiroEvent) {
			if e.Step != nil && e.Step.ID == "t10" {
				ct.Cancel()
			}
		}
		start := time.Now()
		r := host.Run(dir, "a long task", nil, ct, nil, e)
		if r.State != core.Cancelled || r.Text != "Stopped before Kiro finished." {
			t.Error(r)
		}
		if time.Since(start) >= 5*time.Second {
			t.Error("stopped at once, not after 30 s")
		}
	})
}

// The tool shut down for being idle: the reply starts it again, loads the conversation
// (session/load) and carries on in it.
func TestAnIdleToolComesBackWithTheConversation(t *testing.T) {
	scenario(t, "load", [][2]string{{"FAKEACP_SECONDS", "0.2"}, {"FAKEACP_RATE", "20"}}, func(host *AcpHost, dir string, _ func()) {
		rec := &recorders{}
		host.Run(dir, "first", nil, NewCancel(), nil, rec.e)
		sid := rec.sid()
		host.Shutdown("idle")
		// The reader notices the closed pipe a moment later; under load that can come
		// after this line, so the test waits for it.
		for start := time.Now(); host.Alive() && time.Since(start) < 2*time.Second; {
			time.Sleep(10 * time.Millisecond)
		}
		if host.Alive() {
			t.Fatal("still alive")
		}
		if r := host.Run(dir, "second", nil, NewCancel(), &sid, nil); r.State != core.Completed {
			t.Error(r)
		}
	})
}

// FakeAcp killed mid-run: the run fails, saying so.
func TestAToolThatIsKilledFailsItsRun(t *testing.T) {
	scenario(t, "killed", [][2]string{{"FAKEACP_SECONDS", "30"}, {"FAKEACP_RATE", "20"}}, func(host *AcpHost, dir string, kill func()) {
		var fired atomic.Bool
		// Killed from outside at the first step, as a crash or the OOM killer would.
		e := func(e KiroEvent) {
			if e.Step != nil && !fired.Swap(true) {
				go kill()
			}
		}
		r := host.Run(dir, "a long task", nil, NewCancel(), nil, e)
		if r.State != core.Failed || !strings.HasPrefix(r.Text, "Kiro stopped unexpectedly.") {
			t.Error(r)
		}
	})
}

// Sessions on FakeAcp: a reply sent while the first turn runs waits, then goes to the same
// conversation; no config options are offered, so none are set.
func TestAQueuedReplyFollowsInTheSameConversation(t *testing.T) {
	scenario(t, "queue", [][2]string{{"FAKEACP_SECONDS", "0.5"}, {"FAKEACP_RATE", "20"}}, func(host *AcpHost, dir string, _ func()) {
		var offered atomic.Bool
		host.OnOptionsSeen(func(core.AgentTool, []core.AcpOption) { offered.Store(true) })
		k := NewKiroSessions(func(core.AgentTool) RunTask { return host.Runner() }, nil)
		s := must(k.Start(core.Kiro, dir, "first", nil))
		start := time.Now()
		for must(k.Get(s.ID)).KiroID == nil && time.Since(start) < 10*time.Second {
			time.Sleep(10 * time.Millisecond)
		}
		if !k.Reply(s.ID, "second", nil) || !must(k.Get(s.ID)).Turns[1].Queued {
			t.Fatal("not queued")
		}
		for time.Since(start) < 20*time.Second {
			if !slices.ContainsFunc(must(k.Get(s.ID)).Turns, func(t KiroTurn) bool { return t.Result == nil }) {
				break
			}
			time.Sleep(10 * time.Millisecond)
		}
		s = must(k.Get(s.ID))
		var texts []string
		for _, tt := range s.Turns {
			if tt.Result != nil {
				texts = append(texts, tt.Result.Text)
			}
		}
		if !reflect.DeepEqual(texts, []string{"Done. Nothing needed changing.", "Done. Nothing needed changing."}) {
			t.Error(texts)
		}
		tools := 0
		for _, x := range s.Turns[0].Steps {
			if x.Kind != "thought" {
				tools++
			}
		}
		if tools != 1 {
			t.Error("one tool call in half a second:", tools)
		}
		if s.Turns[0].WokeAt == nil {
			t.Error("never woke")
		}
		if offered.Load() {
			t.Error("options were offered")
		}
	})
}
