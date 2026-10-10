package agents

// tests/sessions.rs: KiroSessionTests, KiroSessionsTests and AgentHistoryTests
// (tests/Hover.Tests), ported: the shared run state with the runner stubbed out.

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"sort"
	"sync"
	"testing"
	"time"

	"github.com/4regab/Hover/internal/core"
)

func sessFolder(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-sessions-%s-%d", name, os.Getpid()))
	os.RemoveAll(d)
	os.MkdirAll(d, 0o777)
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

func waitUntil(f func() bool) {
	start := time.Now()
	for !f() && time.Since(start) < 5*time.Second {
		time.Sleep(10 * time.Millisecond)
	}
}

// gate is a run that waits for its result, or ends as stopped when cancelled.
type gate struct {
	prompt string
	resume *string
	tool   core.AgentTool
	done   chan KiroResult
}

type gates struct {
	mu   sync.Mutex
	list []*gate
}

func (g *gates) n() int {
	g.mu.Lock()
	defer g.mu.Unlock()
	return len(g.list)
}

func (g *gates) at(i int) *gate {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.list[i]
}

func (g *gates) finish(i int, r KiroResult) { g.at(i).done <- r }

func gated() (func(core.AgentTool) RunTask, *gates) {
	runs := &gates{}
	return func(tool core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			ch := make(chan KiroResult, 2)
			reg := a.Ct.OnCancel(func() { ch <- NewResult(core.Cancelled, "stopped") })
			defer reg.Remove()
			a.Events(KiroEvent{SessionID: sp("sess_9")})
			runs.mu.Lock()
			runs.list = append(runs.list, &gate{a.Prompt, a.Resume, tool, ch})
			runs.mu.Unlock()
			return <-ch
		}
	}, runs
}

func must[T any](v T, ok bool) T {
	if !ok {
		panic("not there")
	}
	return v
}

func i32(n int32) *int32 { return &n }

// KiroSessionTests.A_run_goes_from_idle_to_running_to_completed (a session starts through
// KiroSessions here: the port has no bare session).
func TestARunGoesFromRunningToCompleted(t *testing.T) {
	f := sessFolder(t, "run")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	var mu sync.Mutex
	var ended []KiroResult
	k.OnEnded(func(_ KiroSession, r KiroResult) { mu.Lock(); ended = append(ended, r); mu.Unlock() })
	s, ok := k.Start(core.Kiro, f, "  Write the changelog  ", nil)
	if !ok || s.State != core.Running {
		t.Fatal("not running")
	}
	waitUntil(func() bool { return runs.n() == 1 })
	if runs.at(0).prompt != "Write the changelog" {
		t.Error(runs.at(0).prompt)
	}
	runs.finish(0, KiroResult{State: core.Completed, Text: "Wrote it.", ExitCode: i32(0)})
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	s = must(k.Get(s.ID))
	if s.State != core.Completed || s.Result().Text != "Wrote it." || s.Title() != "Write the changelog" {
		t.Errorf("%v %q %q", s.State, s.Result().Text, s.Title())
	}
	waitUntil(func() bool { mu.Lock(); defer mu.Unlock(); return len(ended) == 1 })
	if ended[0].State != core.Completed {
		t.Error(ended[0].State)
	}
}

func TestItWillNotStartWithoutAUsableFolderOrAPrompt(t *testing.T) {
	f := sessFolder(t, "nostart")
	k := NewKiroSessions(func(core.AgentTool) RunTask { return func(RunArgs) KiroResult { panic("must not run") } }, nil)
	for _, c := range [][2]string{{f + "/missing", "task"}, {"", "task"}, {f, "   "}} {
		if _, ok := k.Start(core.Kiro, c[0], c[1], nil); ok {
			t.Error(c)
		}
	}
	if len(k.All()) != 0 {
		t.Error("sessions")
	}
}

func TestStopCancelsTheRunAndARunnerThatPanicsFails(t *testing.T) {
	f := sessFolder(t, "stop")
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			if a.Prompt == "boom" {
				panic("boom")
			}
			for !a.Ct.IsCancelled() {
				time.Sleep(5 * time.Millisecond)
			}
			return KiroResult{State: core.Failed, Text: "killed", ExitCode: i32(-1)}
		}
	}, nil)
	s := must(k.Start(core.Kiro, f, "long", nil))
	k.Stop(s.ID)
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	if must(k.Get(s.ID)).State != core.Cancelled {
		t.Error("a stopped run reads as stopped, however it ended")
	}
	b := must(k.Start(core.Kiro, f, "boom", nil))
	waitUntil(func() bool { return !must(k.Get(b.ID)).Busy() })
	if r := must(k.Get(b.ID)).Result(); !reflect.DeepEqual(*r, NewResult(core.Failed, "boom")) {
		t.Errorf("%+v", *r)
	}
}

func lastTurn(s KiroSession) KiroTurn { return s.Turns[len(s.Turns)-1] }

func TestAReplyCarriesOnWithTheToolsIdAndWaitsWhileATurnRuns(t *testing.T) {
	f := sessFolder(t, "reply")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	s := must(k.Start(core.Kiro, f, "first", nil))
	waitUntil(func() bool { return must(k.Get(s.ID)).KiroID != nil })
	if !k.Reply(s.ID, "second", nil) || !lastTurn(must(k.Get(s.ID))).Queued {
		t.Fatal("it waits for the turn that runs")
	}
	runs.finish(0, NewResult(core.Completed, "one"))
	waitUntil(func() bool { return runs.n() == 2 })
	if r := runs.at(1); r.prompt != "second" || deref(r.resume) != "sess_9" {
		t.Errorf("%q %v", r.prompt, deref(r.resume))
	}
	now := must(k.Get(s.ID))
	if !now.Busy() || lastTurn(now).Queued || now.Prompt() != "first" {
		t.Error("the session keeps its first prompt as its title")
	}
	k.Reply(s.ID, "third", nil)
	k.Stop(s.ID)
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	// A stop holds the waiting reply: it stays, in order, and goes only when the user
	// resumes the queue.
	held := must(k.Get(s.ID))
	if !held.Held || !lastTurn(held).Queued || lastTurn(held).Prompt != "third" {
		t.Error("a stop holds the waiting reply")
	}
	time.Sleep(100 * time.Millisecond)
	if runs.n() != 2 {
		t.Error("nothing is sent by itself")
	}
	if !k.ResumeQueue(s.ID) {
		t.Fatal("not resumed")
	}
	waitUntil(func() bool { return runs.n() == 3 })
	if runs.at(2).prompt != "third" || must(k.Get(s.ID)).Held {
		t.Error("third")
	}
	runs.finish(2, NewResult(core.Completed, "three"))
}

func TestImagesGoToTheAgentAsPathsAfterThePrompt(t *testing.T) {
	f := sessFolder(t, "images")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	must(k.Start(core.Kiro, f, "", []string{"/x/a.png", "/x/b.jpg"}))
	waitUntil(func() bool { return runs.n() == 1 })
	if p := runs.at(0).prompt; p != "Look at the attached image.\n\nAttached image (read it from this file): /x/a.png\nAttached image (read it from this file): /x/b.jpg" {
		t.Error(p)
	}
	k.StopAll()
}

func TestTheHostCanRaiseOrLowerTheCap(t *testing.T) {
	// The Mac's Settings offers 1 to 6 at once (Settings.MaxRunning).
	f := sessFolder(t, "cap-set")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	if k.MaxRunningNow() != MaxRunning {
		t.Error(k.MaxRunningNow())
	}
	k.SetMaxRunning(99)
	if k.MaxRunningNow() != MaxKept {
		t.Error("never more than are kept")
	}
	k.SetMaxRunning(4)
	for i, tool := range []core.AgentTool{core.Kiro, core.Codex, core.Cursor, core.Kiro} {
		if _, ok := k.Start(tool, f, fmt.Sprintf("t%d", i), nil); !ok {
			t.Errorf("task %d", i)
		}
	}
	if _, ok := k.Start(core.Codex, f, "five", nil); k.CanStart() || ok {
		t.Error("a fifth")
	}
	waitUntil(func() bool { return runs.n() == 4 })
	k.SetMaxRunning(0)
	if k.MaxRunningNow() != 1 {
		t.Error(k.MaxRunningNow())
	}
	k.StopAll()
}

func TestTasksRunSideBySideUpToTheCapAcrossTools(t *testing.T) {
	f := sessFolder(t, "cap")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	a := must(k.Start(core.Kiro, f, "one", nil))
	b := must(k.Start(core.Codex, f, "two", nil))
	c := must(k.Start(core.Cursor, f, "three", nil))
	if k.Running() != MaxRunning {
		t.Error(k.Running())
	}
	if _, ok := k.Start(core.Kiro, f, "four", nil); ok {
		t.Error("three running in all, whatever the tools")
	}
	if sel := k.Selected(); sel == nil || *sel != c.ID {
		t.Error("a new task is the one shown")
	}
	waitUntil(func() bool { return runs.n() == 3 })
	var tools []core.AgentTool
	bi := -1
	for i := range 3 {
		tools = append(tools, runs.at(i).tool)
		if runs.at(i).prompt == "two" {
			bi = i
		}
	}
	sort.Slice(tools, func(i, j int) bool { return tools[i] < tools[j] })
	if !reflect.DeepEqual(tools, []core.AgentTool{core.Kiro, core.Codex, core.Cursor}) {
		t.Error("each on its own tool")
	}
	runs.finish(bi, NewResult(core.Completed, "done"))
	waitUntil(func() bool { return k.Running() == 2 })
	if must(k.Get(b.ID)).State != core.Completed || !must(k.Get(a.ID)).Busy() || !must(k.Get(c.ID)).Busy() {
		t.Error("the others carry on")
	}
	if _, ok := k.Start(core.Kiro, f, "four", nil); !ok {
		t.Error("a free slot takes a new task")
	}
	if k.Reply(b.ID, "more", nil) {
		t.Error("no fourth run by a reply either")
	}
	k.StopAll()
	waitUntil(func() bool { return k.Running() == 0 })
	n := 0
	var seats [][2]int
	for _, s := range k.All() {
		if s.State == core.Cancelled {
			n++
		}
		seats = append(seats, [2]int{s.Seat, s.Bot})
	}
	if n != 3 {
		t.Error(n)
	}
	// Seats and bots: the lowest free, in start order.
	if !reflect.DeepEqual(seats, [][2]int{{0, 0}, {1, 1}, {2, 2}, {3, 3}}) {
		t.Error(seats)
	}
}

func TestOnlyTheNewestAreKeptAndDismissRemovesAFinishedTask(t *testing.T) {
	f := sessFolder(t, "kept")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	for i := range MaxKept + 2 {
		must(k.Start(core.Kiro, f, fmt.Sprintf("task %d", i), nil))
		waitUntil(func() bool { return runs.n() == i+1 })
		runs.finish(i, NewResult(core.Completed, "ok"))
		waitUntil(func() bool { return k.Running() == 0 })
	}
	if len(k.All()) != MaxKept || k.All()[0].Prompt() != "task 2" {
		t.Error(len(k.All()))
	}
	first := k.All()[0]
	k.Select(&first.ID)
	k.Dismiss(first.ID)
	if len(k.All()) != MaxKept-1 || k.Selected() != nil {
		t.Error("dismissing the shown task goes back to a new one")
	}
}

func testHistory(t *testing.T, name string) (*core.AgentHistory, string) {
	dir := filepath.Join(sessFolder(t, name), "agents")
	var key [32]byte
	for i := range key {
		key[i] = 1
	}
	return core.NewAgentHistory(dir, core.CryptoWithKey(key)), dir
}

// answering is AgentHistoryTests' runs that answer at once, saying which conversation
// they resumed.
func answering(mu *sync.Mutex, resumed *[]*string) func(core.AgentTool) RunTask {
	return func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			mu.Lock()
			*resumed = append(*resumed, a.Resume)
			mu.Unlock()
			a.Events(KiroEvent{SessionID: sp("acp-1")})
			return NewResult(core.Completed, "answer to "+a.Prompt)
		}
	}
}

func TestASessionThatLeftItsDeskWakesOnAReplyAndCarriesOnItsConversation(t *testing.T) {
	f := sessFolder(t, "wake")
	h, dir := testHistory(t, "wake-h")
	var mu sync.Mutex
	var resumed []*string
	k := NewKiroSessions(answering(&mu, &resumed), h)
	first := must(k.Start(core.Cursor, f, "first", nil))
	waitUntil(func() bool { return !must(k.Get(first.ID)).Busy() })
	for i := range MaxKept {
		s := must(k.Start(core.Kiro, f, fmt.Sprintf("task %d", i), nil))
		waitUntil(func() bool { x, ok := k.Get(s.ID); return ok && !x.Busy() })
	}
	if slices.ContainsFunc(k.All(), func(x KiroSession) bool { return x.Key == first.Key }) {
		t.Fatal("still at a desk")
	}
	h.Flush()
	if !slices.ContainsFunc(h.Entries(), func(e core.HistoryEntry) bool { return e.Key == first.Key }) {
		t.Fatal("not in the history")
	}
	woken := must(k.Wake(first.Key))
	if woken.Turns[0].Result.Text != "answer to first" {
		t.Error(woken.Turns[0].Result.Text)
	}
	if !k.Reply(woken.ID, "and then?", nil) {
		t.Fatal("no reply")
	}
	waitUntil(func() bool { return !must(k.Get(woken.ID)).Busy() })
	w := must(k.Get(woken.ID))
	mu.Lock()
	last := resumed[len(resumed)-1]
	mu.Unlock()
	if w.Tool != core.Cursor || deref(last) != "acp-1" || len(w.Turns) != 2 || len(k.All()) != MaxKept {
		t.Errorf("the reply resumed the saved conversation: %v %v %d", w.Tool, deref(last), len(w.Turns))
	}
	// Sealed on disk and whole again.
	h.Flush()
	var key [32]byte
	for i := range key {
		key[i] = 1
	}
	saved, ok := core.NewAgentHistory(dir, core.CryptoWithKey(key)).Load(first.Key)
	if !ok || saved.Tool != core.Cursor || deref(saved.AcpID) != "acp-1" || len(saved.Turns) != 2 {
		t.Errorf("%+v", saved)
	}
}

func TestDeleteTakesASessionOutOfTheOfficeAndTheHistory(t *testing.T) {
	f := sessFolder(t, "delete")
	h, _ := testHistory(t, "delete-h")
	var mu sync.Mutex
	var resumed []*string
	k := NewKiroSessions(answering(&mu, &resumed), h)
	s := must(k.Start(core.Kiro, f, "one", nil))
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	k.Delete(s.Key)
	h.Flush()
	if len(k.All()) != 0 || len(h.Entries()) != 0 {
		t.Error("still there")
	}
	if _, ok := h.Load(s.Key); ok {
		t.Error("loaded")
	}
	if _, ok := k.Saved(s.Key); ok {
		t.Error("saved")
	}
}

func question(id, kind string, command, path *string, reason string, danger bool) AgentAsk {
	return AgentAsk{ID: id, Kind: kind, Title: kind, Command: command, Path: path, Reason: reason, Danger: danger}
}

type answers struct {
	mu  sync.Mutex
	got []AskAnswer
}

func (a *answers) reply() func(AskAnswer) {
	return func(x AskAnswer) { a.mu.Lock(); a.got = append(a.got, x); a.mu.Unlock() }
}

func (a *answers) list() []AskAnswer {
	a.mu.Lock()
	defer a.mu.Unlock()
	return slices.Clone(a.got)
}

// KiroSessionTests.A_question_waits_for_its_answer_and_a_stop_turns_it_down. The question
// reaches the session through its tool and conversation id, as OwlApp's Asking hands it over.
func TestAQuestionWaitsForItsAnswerAndAStopTurnsItDown(t *testing.T) {
	f := sessFolder(t, "ask")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	idle := &answers{}
	k.Ask(core.Kiro, "sess_9", question("x", "edit", nil, sp("a.cs"), "Edits a file", false), NewCancel(), idle.reply())
	if !reflect.DeepEqual(idle.list(), []AskAnswer{Deny}) {
		t.Error("a session that isn't running has nothing to ask")
	}
	s := must(k.StartAs(core.Kiro, f, "long", nil, sp("always")))
	waitUntil(func() bool { return runs.n() == 1 && must(k.Get(s.ID)).KiroID != nil })
	got := &answers{}
	ct := NewCancel()
	k.Ask(core.Codex, "sess_9", question("0", "execute", sp("ls"), nil, "Runs a command", false), ct, got.reply())
	if !reflect.DeepEqual(got.list(), []AskAnswer{Deny}) {
		t.Error("another tool's conversation isn't this session's")
	}
	got = &answers{}
	k.Ask(core.Kiro, "sess_9", question("1", "execute", sp("npm test"), nil, "Runs a command", false), ct, got.reply())
	k.Ask(core.Kiro, "sess_9", question("2", "delete", nil, sp("old.snap"), "Deletes files", true), ct, got.reply())
	now := must(k.Get(s.ID))
	if !now.Waiting() || now.Asking().ID != "1" || deref(now.Access) != "always" {
		t.Error("oldest first")
	}
	if k.Answer(s.ID, "nope", Allow) || !k.Answer(s.ID, "1", Trust) {
		t.Error("answer")
	}
	if !reflect.DeepEqual(got.list(), []AskAnswer{Trust}) || must(k.Get(s.ID)).Asking().ID != "2" {
		t.Error(got.list())
	}
	k.Stop(s.ID)
	if !reflect.DeepEqual(got.list(), []AskAnswer{Trust, Deny}) || must(k.Get(s.ID)).Waiting() {
		t.Error(got.list())
	}
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
}

// A question the run's own token withdraws (the run was stopped at the tool's end).
func TestAWithdrawnQuestionIsDeniedAndLeavesTheSession(t *testing.T) {
	f := sessFolder(t, "withdraw")
	make, runs := gated()
	k := NewKiroSessions(make, nil)
	s := must(k.Start(core.Kiro, f, "long", nil))
	waitUntil(func() bool { return runs.n() == 1 && must(k.Get(s.ID)).KiroID != nil })
	got := &answers{}
	ct := NewCancel()
	k.Ask(core.Kiro, "sess_9", question("1", "edit", nil, sp("a.cs"), "Edits a file", false), ct, got.reply())
	ct.Cancel()
	if !reflect.DeepEqual(got.list(), []AskAnswer{Deny}) || must(k.Get(s.ID)).Waiting() {
		t.Error(got.list())
	}
	runs.finish(0, NewResult(core.Completed, "done"))
}

// KiroSessionTests.The_notch_says_the_file_or_the_command_not_the_path.
func TestTheNotchSaysTheFileOrTheCommandNotThePath(t *testing.T) {
	run := question("r", "execute", sp("npm install three@0.171.0"), nil, "Installs packages or uses the network", false)
	edit := question("e", "edit", nil, sp("src/auth/refresh.ts"), "Changes 2 lines", false)
	edit.Preview, edit.Added, edit.Removed = sp("- a\n+ b"), 1, 1
	if deref(Short(sp(`C:\Projects\Hover\src\Hover\Owl\Notch.cs`))) != "Notch.cs" && os.PathSeparator == '\\' {
		t.Error("Notch.cs")
	}
	if deref(Short(sp("src/auth/refresh.ts"))) != "refresh.ts" || deref(Short(sp(`dotnet test .\Hover.slnx -c Release`))) != "dotnet test" || Short(sp("  ")) != nil {
		t.Error("short")
	}
	if v, o := AskLine(&run); v != "Wants to run" || o != "npm install" {
		t.Error(v, o)
	}
	if AskTitle(&edit) != "Wants to edit refresh.ts" || AskAllow(&run) != "Run" {
		t.Error("title")
	}
	s := NewKiroSession(core.Kiro)
	if v, o := Activity(&s); v != "Ready" || o != "" {
		t.Error(v, o)
	}
}

// slowRun is a run that ends only when the test says, even after a cancel: what a tool
// that takes its time to confirm a stop looks like.
type slowRun struct {
	prompt string
	ct     *Cancel
	done   chan KiroResult
}

type slowRuns struct {
	mu   sync.Mutex
	list []*slowRun
}

func (s *slowRuns) n() int { s.mu.Lock(); defer s.mu.Unlock(); return len(s.list) }

func (s *slowRuns) at(i int) *slowRun { s.mu.Lock(); defer s.mu.Unlock(); return s.list[i] }

func (s *slowRuns) prompts() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []string
	for _, r := range s.list {
		out = append(out, r.prompt)
	}
	return out
}

func slowToStop() (func(core.AgentTool) RunTask, *slowRuns) {
	runs := &slowRuns{}
	return func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			ch := make(chan KiroResult, 1)
			a.Events(KiroEvent{SessionID: sp("sess_p")})
			runs.mu.Lock()
			runs.list = append(runs.list, &slowRun{a.Prompt, a.Ct, ch})
			runs.mu.Unlock()
			return <-ch
		}
	}, runs
}

// Pause cancels the run through the tool, keeps the conversation, and sends the next
// queued reply exactly once, only after the tool has said the turn ended.
func TestPauseSendsTheNextQueuedReplyOnceTheStopIsConfirmed(t *testing.T) {
	f := sessFolder(t, "pause")
	make, runs := slowToStop()
	k := NewKiroSessions(make, nil)
	s := must(k.Start(core.Codex, f, "first", nil))
	waitUntil(func() bool { return must(k.Get(s.ID)).KiroID != nil })
	if !k.Reply(s.ID, "second", nil) || !k.Reply(s.ID, "third", nil) || !k.Pause(s.ID) {
		t.Fatal("reply, pause")
	}
	if !runs.at(0).ct.IsCancelled() {
		t.Error("the tool was asked to stop")
	}
	if now := must(k.Get(s.ID)); !now.Busy() || !now.Stopping {
		t.Error("not stopped until the tool says so")
	}
	time.Sleep(100 * time.Millisecond)
	if runs.n() != 1 {
		t.Error("nothing new starts while the stop is unresolved")
	}
	runs.at(0).done <- NewResult(core.Cancelled, "Partial answer")
	waitUntil(func() bool { return runs.n() == 2 })
	time.Sleep(100 * time.Millisecond)
	if !reflect.DeepEqual(runs.prompts(), []string{"first", "second"}) {
		t.Error("the next one, once")
	}
	now := must(k.Get(s.ID))
	if now.Turns[0].Result.Text != "Partial answer" || !now.Turns[2].Queued || now.Stopping {
		t.Error("what it said so far is kept")
	}
	runs.at(1).done <- NewResult(core.Completed, "two")
	waitUntil(func() bool { return runs.n() == 3 })
	runs.at(2).done <- NewResult(core.Completed, "three")
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	// Nothing queued: a pause leaves the session idle, its conversation intact.
	if !k.Reply(s.ID, "fourth", nil) {
		t.Fatal("fourth")
	}
	waitUntil(func() bool { return runs.n() == 4 })
	k.Pause(s.ID)
	runs.at(3).done <- NewResult(core.Cancelled, "")
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	time.Sleep(50 * time.Millisecond)
	if runs.n() != 4 || deref(must(k.Get(s.ID)).KiroID) != "sess_p" {
		t.Error("a later reply carries on the same conversation")
	}
}

// A stop the tool never confirmed: nothing queued goes behind it (a pause keeps them
// queued, to go with the next reply); a queued reply can be taken back.
func TestAnUnconfirmedStopSendsNothingAndQueuedRepliesCanBeCancelled(t *testing.T) {
	f := sessFolder(t, "unconfirmed")
	make, runs := slowToStop()
	k := NewKiroSessions(make, nil)
	s := must(k.Start(core.Kiro, f, "first", nil))
	waitUntil(func() bool { return runs.n() == 1 })
	k.Reply(s.ID, "second", nil)
	k.Reply(s.ID, "third", nil)
	if !k.CancelQueued(s.ID, 1) || k.CancelQueued(s.ID, 0) {
		t.Error("the first queued one taken back; a turn that runs isn't a queued one")
	}
	var prompts []string
	for _, tt := range must(k.Get(s.ID)).Turns {
		prompts = append(prompts, tt.Prompt)
	}
	if !reflect.DeepEqual(prompts, []string{"first", "third"}) {
		t.Error(prompts)
	}
	k.Pause(s.ID)
	r := NewResult(core.Failed, "Kiro didn’t confirm it stopped.")
	r.Unconfirmed = true
	runs.at(0).done <- r
	waitUntil(func() bool { return !must(k.Get(s.ID)).Busy() })
	time.Sleep(100 * time.Millisecond)
	now := must(k.Get(s.ID))
	if runs.n() != 1 || now.State != core.Failed || !now.Turns[1].Queued {
		t.Error("nothing sent behind an unconfirmed stop; said as it is, the reply still queued")
	}
	if !k.Reply(s.ID, "fourth", nil) {
		t.Fatal("fourth")
	}
	waitUntil(func() bool { return runs.n() == 2 })
	if runs.at(1).prompt != "third" {
		t.Error("the queued one goes first, in order")
	}
	runs.at(1).done <- NewResult(core.Completed, "3")
	waitUntil(func() bool { return runs.n() == 3 })
	if runs.at(2).prompt != "fourth" {
		t.Error(runs.at(2).prompt)
	}
	runs.at(2).done <- NewResult(core.Completed, "4")
}
