package agents

// tests/compact.rs. Kiro's auto compact: with the setting on and the context past its
// percent, a /compact turn goes before the next prompt. The session logic against a
// stubbed runner, then the real prompt over ACP pipes, then (with FAKEACP=<fake-agent>)
// the stand-in agent as a process.

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func waitForSecs(secs int, f func() bool) {
	for start := time.Now(); !f() && time.Since(start) < time.Duration(secs)*time.Second; {
		time.Sleep(10 * time.Millisecond)
	}
}

// settle: every turn ended, n of them.
func settle(t *testing.T, k *KiroSessions, id int32, n int) {
	t.Helper()
	waitForSecs(5, func() bool {
		s := must(k.Get(id))
		return !s.Busy() && len(s.Turns) == n && !slices.ContainsFunc(s.Turns, func(t KiroTurn) bool { return t.Result == nil })
	})
	if got := len(must(k.Get(id)).Turns); got != n {
		t.Fatal(got)
	}
}

// compactDoes is what /compact does in the stand-in runner.
type compactDoes struct {
	says   string
	fails  bool
	blocks bool
}

type compactScript struct {
	// report is the context each of the tool's own turns reports when it ends, by turn
	// (nil: nothing reported).
	report  []*float64
	compact compactDoes
	// hold: the first own turn waits for this.
	hold *atomic.Bool
}

type accessSeen struct {
	mu   sync.Mutex
	runs [][2]*string
}

func (a *accessSeen) prompts() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []string
	for _, r := range a.runs {
		out = append(out, *r[0])
	}
	return out
}

func newScript(report ...*float64) compactScript {
	return compactScript{report: report, compact: compactDoes{says: "Compacted the conversation."}}
}

func (c compactScript) make() (func(core.AgentTool) RunTask, *accessSeen) {
	seen := &accessSeen{}
	var own atomic.Int64
	return func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			seen.mu.Lock()
			seen.runs = append(seen.runs, [2]*string{sp(a.Prompt), a.Access})
			seen.mu.Unlock()
			a.Events(KiroEvent{SessionID: sp("s1")})
			if a.Prompt == "/compact" {
				switch {
				case c.compact.fails:
					return NewResult(core.Failed, "Kiro couldn't compact.")
				case c.compact.blocks:
					for !a.Ct.IsCancelled() {
						time.Sleep(5 * time.Millisecond)
					}
					return NewResult(core.Cancelled, "")
				}
				a.Events(KiroEvent{Context: fp(12)})
				return NewResult(core.Completed, c.compact.says)
			}
			n := own.Add(1) - 1
			if n == 0 && c.hold != nil {
				for !c.hold.Load() {
					time.Sleep(5 * time.Millisecond)
				}
			}
			if int(n) < len(c.report) && c.report[n] != nil {
				a.Events(KiroEvent{Context: c.report[n]})
			}
			return NewResult(core.Completed, "ok")
		}
	}, seen
}

func onAt(k *KiroSessions, at *uint8) { k.SetAutoCompact(func() *uint8 { return at }) }

func u8(n uint8) *uint8 { return &n }

func TestOffByDefaultNothingIsCompactedHoweverFull(t *testing.T) {
	f := newDir(t, "compact-off")
	make, seen := newScript(fp(99), fp(99)).make()
	k := NewKiroSessions(make, nil)
	onAt(k, nil)
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "second"}) {
		t.Error(p)
	}
	if len(must(k.Get(s.ID)).Turns[1].Steps) != 0 {
		t.Error("a step")
	}
}

func TestPastThePercentACompactGoesOnceBeforeTheReply(t *testing.T) {
	f := newDir(t, "compact-on")
	make, seen := newScript(fp(85)).make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first"}) {
		t.Error("nothing is compacted before the first prompt", p)
	}
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	replyOrFail(t, k, s.ID, "third")
	settle(t, k, s.ID, 3)
	// The second turn reported nothing new, so the third goes as it is.
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "/compact", "second", "third"}) {
		t.Error(p)
	}
	s = must(k.Get(s.ID))
	step := s.Turns[1].Steps[0]
	if step.Title != "Compacted the conversation (it was 85% full)" || step.Status != "completed" || step.Kind != "other" || step.MS == nil {
		t.Errorf("%+v", step)
	}
	if len(s.Turns[0].Steps) != 0 || len(s.Turns[2].Steps) != 0 {
		t.Error("steps elsewhere")
	}
	if s.Turns[1].Result.Text != "ok" {
		t.Error("the reply's answer, not the compaction's")
	}
	if s.Context == nil || *s.Context != 12 {
		t.Error("what the compaction reported is shown", deref(s.Context))
	}
}

func TestANewReportAfterEachReplyCompactsAgainAndThePercentItselfCounts(t *testing.T) {
	f := newDir(t, "compact-again")
	make, seen := newScript(fp(80), fp(90), fp(79.9)).make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "a", nil))
	settle(t, k, s.ID, 1)
	for i, tt := range []string{"b", "c", "d"} {
		replyOrFail(t, k, s.ID, tt)
		settle(t, k, s.ID, i+2)
	}
	// 80 is at the percent (compacts), 90 compacts, 79.9 is under it.
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"a", "/compact", "b", "/compact", "c", "d"}) {
		t.Error(p)
	}
}

func TestUnderThePercentNothingIsCompacted(t *testing.T) {
	f := newDir(t, "compact-under")
	make, seen := newScript(fp(50)).make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "second"}) {
		t.Error(p)
	}
}

func TestOnlyKiroIsCompacted(t *testing.T) {
	f := newDir(t, "compact-others")
	for _, tool := range []core.AgentTool{core.Codex, core.Cursor, core.OpenCode, core.Claude} {
		make, seen := newScript(fp(97)).make()
		k := NewKiroSessions(make, nil)
		onAt(k, u8(50))
		s := must(k.Start(tool, f, "first", nil))
		settle(t, k, s.ID, 1)
		replyOrFail(t, k, s.ID, "second")
		settle(t, k, s.ID, 2)
		if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "second"}) {
			t.Error(tool.ID(), p)
		}
	}
}

func TestQueuedRepliesKeepTheirOrderAndOneCompactGoesBeforeTheFirst(t *testing.T) {
	f := newDir(t, "compact-queue")
	hold := &atomic.Bool{}
	script := newScript(fp(90))
	script.hold = hold
	make, seen := script.make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "a", nil))
	waitForSecs(5, func() bool { return len(seen.prompts()) > 0 })
	if !k.Reply(s.ID, "b", nil) || !k.Reply(s.ID, "c", nil) {
		t.Fatal("no reply")
	}
	hold.Store(true)
	settle(t, k, s.ID, 3)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"a", "/compact", "b", "c"}) {
		t.Error(p)
	}
	s = must(k.Get(s.ID))
	var prompts []string
	for _, tt := range s.Turns {
		prompts = append(prompts, tt.Prompt)
	}
	if !reflect.DeepEqual(prompts, []string{"a", "b", "c"}) {
		t.Error(prompts)
	}
	if len(s.Turns[1].Steps) != 1 || len(s.Turns[2].Steps) != 0 {
		t.Error("the step sits with the reply it came before")
	}
}

func TestStoppingDuringTheCompactionStopsTheReplyAndWhatWaitedBehindIt(t *testing.T) {
	f := newDir(t, "compact-stop")
	script := newScript(fp(90))
	script.compact = compactDoes{blocks: true}
	make, seen := script.make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	waitForSecs(5, func() bool { return slices.Contains(seen.prompts(), "/compact") })
	replyOrFail(t, k, s.ID, "third")
	k.Stop(s.ID)
	settle(t, k, s.ID, 3)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "/compact"}) {
		t.Error("the reply was never sent", p)
	}
	s = must(k.Get(s.ID))
	if s.Turns[1].Result.State != core.Cancelled {
		t.Error(s.Turns[1].Result)
	}
	if !s.Turns[2].Queued || !s.Held || s.Turns[2].Result != nil {
		t.Error("the reply behind it is held, not dropped")
	}
	if step := s.Turns[1].Steps[0]; step.Title != "Stopped while compacting the conversation (90% full)" || step.Status != "failed" {
		t.Errorf("%+v", step)
	}
}

func TestNothingToCompactIsSaidQuietlyAndTheReplyGoesOn(t *testing.T) {
	f := newDir(t, "compact-nothing")
	script := newScript(fp(85))
	script.compact = compactDoes{says: "Nothing to compact yet. The conversation is still short."}
	make, seen := script.make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "/compact", "second"}) {
		t.Error(p)
	}
	s = must(k.Get(s.ID))
	if step := s.Turns[1].Steps[0]; step.Title != "Nothing to compact yet (the context is 85% full)" || step.Status != "completed" {
		t.Errorf("%+v", step)
	}
	if s.Turns[1].Result.State != core.Completed {
		t.Error(s.Turns[1].Result)
	}
}

func TestACompactionThatFailsIsSaidAndTheReplyStillGoes(t *testing.T) {
	f := newDir(t, "compact-fails")
	script := newScript(fp(85))
	script.compact = compactDoes{fails: true}
	make, seen := script.make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	if p := seen.prompts(); !reflect.DeepEqual(p, []string{"first", "/compact", "second"}) {
		t.Error(p)
	}
	s = must(k.Get(s.ID))
	if st := s.Turns[1].Steps[0]; st.Title != "Couldn't compact the conversation (it was 85% full)" || st.Status != "failed" {
		t.Errorf("%+v", st)
	}
	if r := s.Turns[1].Result; r.State != core.Completed || r.Text != "ok" {
		t.Error(r)
	}
}

func TestAReadOnlySessionStillCompactsWithItsOwnAccess(t *testing.T) {
	f := newDir(t, "compact-readonly")
	make, seen := newScript(fp(85)).make()
	k := NewKiroSessions(make, nil)
	onAt(k, u8(80))
	s := must(k.StartAs(core.Kiro, f, "first", nil, sp("read-only")))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	seen.mu.Lock()
	defer seen.mu.Unlock()
	var got [][2]string
	for _, r := range seen.runs {
		got = append(got, [2]string{*r[0], val(r[1])})
	}
	if !reflect.DeepEqual(got, [][2]string{{"first", "read-only"}, {"/compact", "read-only"}, {"second", "read-only"}}) {
		t.Error(got)
	}
}

func TestTheStepsTitleSaysWhatHappened(t *testing.T) {
	done, failed, cancelled := core.Completed, core.Failed, core.Cancelled
	for _, c := range []struct {
		state *core.KiroState
		said  string
		want  string
	}{
		{nil, "", "Compacting the conversation (83% full)"},
		{&done, "Compacted.", "Compacted the conversation (it was 83% full)"},
		{&done, "NOTHING TO COMPACT yet", "Nothing to compact yet (the context is 83% full)"},
		{&failed, "", "Couldn't compact the conversation (it was 83% full)"},
		{&cancelled, "", "Stopped while compacting the conversation (83% full)"},
	} {
		if got := CompactTitle(83.4, c.state, c.said); got != c.want {
			t.Error(got)
		}
	}
}

func replyOrFail(t *testing.T, k *KiroSessions, id int32, text string) {
	t.Helper()
	if !k.Reply(id, text, nil) {
		t.Fatal("no reply:", text)
	}
}

type said struct {
	mu  sync.Mutex
	all []string
}

func (s *said) push(x string) {
	s.mu.Lock()
	s.all = append(s.all, x)
	s.mu.Unlock()
}

func (s *said) list() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.all...)
}

// kiroPipe is Kiro over ACP pipes: it reports its context after a turn as
// session_info_update; its compaction is the _kiro/session/compact request (a /compact
// prompt is only chat to Kiro's model). What arrives is recorded: "prompt: TEXT" or
// "compact". With hang, the compaction never answers.
func kiroPipe(seen *said, hang bool) func() (*Link, error) {
	return func() (*Link, error) {
		link, _, err := pipeLink(func(from *bufio.Reader, out *pipeOut) {
			upd := func(u string) string {
				return fmt.Sprintf(`{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":%s}}`, u)
			}
			eachLine(from, func(line string) {
				m := jsonOf(line)
				method, ok := str(m, "method")
				v, hasID := m.Get("id")
				id, err := v.I64()
				if !ok || !hasID || err != nil {
					return
				}
				var result string
				switch method {
				case "initialize":
					result = `{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}`
				case "session/new":
					result = `{"sessionId":"s1","configOptions":[]}`
				case "_kiro/session/compact":
					seen.push("compact")
					if hang {
						return
					}
					out.say(upd(`{"sessionUpdate":"session_info_update","_meta":{"kiro":{"summarization":{"status":"success","summary":{"conversationSummary":"short","truncated":false}}}}}`))
					result = `{"success":true}`
				case "session/prompt":
					items, _ := get(m, "params", "prompt").Items()
					seen.push("prompt: " + strAt(items[0], "text"))
					out.say(upd(`{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}`))
					out.say(upd(`{"sessionUpdate":"session_info_update","_meta":{"kiro":{"contextUsage":{"usagePercentage":85.0}}}}`))
					result = `{"stopReason":"end_turn"}`
				default:
					result = "{}"
				}
				out.say(fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"result":%s}`, id, result))
			})
		})
		return link, err
	}
}

func pipeSessions(t *testing.T, hang bool, at *uint8) (*KiroSessions, *AcpHost, *said) {
	seen := &said{}
	host := AcpHostWithConnect(core.Kiro, core.DefaultAgentOptions, kiroPipe(seen, hang))
	t.Cleanup(func() { host.Shutdown("test") })
	k := NewKiroSessions(func(core.AgentTool) RunTask { return host.Runner() }, nil)
	onAt(k, at)
	return k, host, seen
}

func TestOverAcpTheCompactionIsKirosOwnRequestNotAPrompt(t *testing.T) {
	k, _, seen := pipeSessions(t, false, u8(80))
	s := must(k.Start(core.Kiro, newDir(t, "compact-acp"), "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	settle(t, k, s.ID, 2)
	if got := seen.list(); !reflect.DeepEqual(got, []string{"prompt: first", "compact", "prompt: second"}) {
		t.Error("no /compact prompt: Kiro's model would only talk about it", got)
	}
	s = must(k.Get(s.ID))
	if s.Turns[1].Steps[0].Title != "Compacted the conversation (it was 85% full)" || s.Turns[1].Result.Text != "Done." {
		t.Error(s.Turns[1].Steps[0].Title, s.Turns[1].Result.Text)
	}
}

// Compaction is a model call that can take a while: a stop ends it, and the reply behind
// it is not sent.
func TestOverAcpAStopDuringTheCompactionEndsItAndSendsNoReply(t *testing.T) {
	k, _, seen := pipeSessions(t, true, u8(80))
	s := must(k.Start(core.Kiro, newDir(t, "compact-acp-stop"), "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "second")
	waitForSecs(5, func() bool { return slices.Contains(seen.list(), "compact") })
	k.Stop(s.ID)
	settle(t, k, s.ID, 2)
	if got := seen.list(); !reflect.DeepEqual(got, []string{"prompt: first", "compact"}) {
		t.Error(got)
	}
	if r := must(k.Get(s.ID)).Turns[1].Result; r.State != core.Cancelled {
		t.Error(r)
	}
}

// A reply of exactly /compact compacts for real too (the chat's way of asking).
func TestOverAcpAReplyOfSlashCompactCompacts(t *testing.T) {
	k, _, seen := pipeSessions(t, false, nil)
	s := must(k.Start(core.Kiro, newDir(t, "compact-acp-typed"), "first", nil))
	settle(t, k, s.ID, 1)
	replyOrFail(t, k, s.ID, "/compact")
	settle(t, k, s.ID, 2)
	if got := seen.list(); !reflect.DeepEqual(got, []string{"prompt: first", "compact"}) {
		t.Error(got)
	}
	if r := must(k.Get(s.ID)).Turns[1].Result; r.State != core.Completed {
		t.Error(r)
	}
}

// With FAKEACP=<path to fake-agent>: the stand-in process reports 85 % after a turn and
// takes /compact as Kiro does (down to 20 %).
func TestTheStandInAgentAsAProcess(t *testing.T) {
	exe := fakeAcpLive()
	if exe == "" {
		return
	}
	f := newDir(t, "compact-proc")
	log := filepath.Join(f, "sent.log")
	env := [][2]string{{"FAKEACP_CONTEXT", "85"}, {"FAKEACP_COMPACT_TO", "20"}, {"FAKEACP_SECONDS", "0.2"}, {"FAKEACP_LOG", log}}
	var mu sync.Mutex
	var groups []*Group
	host := AcpHostWithConnect(core.Kiro, core.DefaultAgentOptions, func() (*Link, error) {
		link, g, err := LaunchGrouped(exe, []string{"acp"}, env)
		if err == nil {
			mu.Lock()
			groups = append(groups, g)
			mu.Unlock()
		}
		return link, err
	})
	k := NewKiroSessions(func(core.AgentTool) RunTask { return host.Runner() }, nil)
	onAt(k, u8(80))
	s := must(k.Start(core.Kiro, f, "first", nil))
	allDone := func(n int) func() bool {
		return func() bool {
			s := must(k.Get(s.ID))
			return !s.Busy() && (n == 0 || len(s.Turns) == n) && !slices.ContainsFunc(s.Turns, func(t KiroTurn) bool { return t.Result == nil })
		}
	}
	waitForSecs(20, allDone(0))
	replyOrFail(t, k, s.ID, "second")
	waitForSecs(20, allDone(2))
	s = must(k.Get(s.ID))
	if s.Turns[1].Steps[0].Title != "Compacted the conversation (it was 85% full)" || s.Turns[1].Result.State != core.Completed {
		t.Error(s.Turns[1].Steps[0].Title, s.Turns[1].Result)
	}
	host.Shutdown("test")
	mu.Lock()
	for _, g := range groups {
		g.Kill()
	}
	mu.Unlock()
	b, _ := os.ReadFile(log)
	sent := string(b)
	at := func(needle string) int {
		i := strings.Index(sent, needle)
		if i < 0 {
			t.Fatalf("%s in %s", needle, sent)
		}
		return i
	}
	if !(at(`"text":"first`) < at(`"method":"_kiro/session/compact"`) && at(`"method":"_kiro/session/compact"`) < at(`"text":"second`)) {
		t.Error(sent)
	}
	if strings.Count(sent, "_kiro/session/compact") != 1 || strings.Contains(sent, `"text":"/compact"`) {
		t.Error("never as a prompt:", sent)
	}
}

// The real kiro-cli (costs a little credit), Rust's #[ignore]d test: run with
// HOVER_REAL_KIRO=1 go test -run TestRealKiroCli -v. The threshold is 1 %, so the second
// reply is preceded by a /compact.
func TestRealKiroCliCompactsBeforeTheSecondReply(t *testing.T) {
	if os.Getenv("HOVER_REAL_KIRO") == "" {
		t.Skip("set HOVER_REAL_KIRO=1 to run against the real kiro-cli")
	}
	f := newDir(t, "compact-real")
	host := NewAcpHost(core.Kiro, core.DefaultAgentOptions)
	k := NewKiroSessions(func(core.AgentTool) RunTask { return host.Runner() }, nil)
	onAt(k, u8(1))
	s := must(k.Start(core.Kiro, f, "Reply with just the word: ok", nil))
	ended := func(n int) func() bool {
		return func() bool {
			s := must(k.Get(s.ID))
			return !s.Busy() && (n == 0 || len(s.Turns) == n) && !slices.ContainsFunc(s.Turns, func(t KiroTurn) bool { return t.Result == nil })
		}
	}
	waitForSecs(120, ended(0))
	one := must(k.Get(s.ID))
	t.Logf("turn 1: %+v %d context %v", one.Turns[0].Result, len(one.Turns[0].Steps), deref(one.Context))
	replyOrFail(t, k, s.ID, "Reply with just the word: done")
	waitForSecs(180, ended(2))
	two := must(k.Get(s.ID))
	for _, x := range two.Turns[1].Steps {
		t.Logf("step: %s | %s | %v", x.Title, x.Status, deref(x.Output))
	}
	t.Logf("turn 2: %+v context %v", two.Turns[1].Result, deref(two.Context))
	k.Stop(s.ID)
	host.Shutdown("test")
	if st := two.Turns[1].Steps[0]; st.Status != "completed" || !strings.HasPrefix(st.Title, "Compacted") && !strings.HasPrefix(st.Title, "Nothing to compact") {
		t.Errorf("%+v", st)
	}
	if two.Turns[1].Result.State != core.Completed {
		t.Error(two.Turns[1].Result)
	}
	// Kiro's own log of the run it just had: the compaction, in its words.
	logs := filepath.Join(Home(), ".kiro", "logs")
	entries, _ := os.ReadDir(logs)
	var newest os.DirEntry
	var at time.Time
	for _, e := range entries {
		if i, err := e.Info(); err == nil && i.ModTime().After(at) {
			newest, at = e, i.ModTime()
		}
	}
	if newest != nil {
		b, _ := os.ReadFile(filepath.Join(logs, newest.Name(), "kiro.log"))
		n := 0
		for _, l := range strings.Split(string(b), "\n") {
			if low := strings.ToLower(l); n < 12 && (strings.Contains(low, "compact") || strings.Contains(low, "summariz")) {
				t.Logf("kiro.log: %s", l[:min(len(l), 300)])
				n++
			}
		}
	}
}
