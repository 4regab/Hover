package agents

// tests/handoff.rs and tests/busy_retry.rs: moving a conversation between agents, forking
// it and bringing findings back (the scripted agents note what they are sent); and Kiro's
// "continue when high usage encountered".

import (
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

type logged struct {
	tool   core.AgentTool
	prompt string
	resume *string
}

type runLog struct {
	mu   sync.Mutex
	list []logged
}

func (l *runLog) add(x logged) int {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.list = append(l.list, x)
	return len(l.list)
}

func (l *runLog) n() int { l.mu.Lock(); defer l.mu.Unlock(); return len(l.list) }

func (l *runLog) last() logged { l.mu.Lock(); defer l.mu.Unlock(); return l.list[len(l.list)-1] }

func key8() *core.Crypto {
	var key [32]byte
	for i := range key {
		key[i] = 8
	}
	return core.CryptoWithKey(key)
}

// rig is agents that answer "<tool> says <n>" and name their own conversation
// <tool>-conv; hold makes a "[hold]" turn wait.
func rig(t *testing.T, name string) (*KiroSessions, *runLog, string, *atomic.Bool) {
	log := &runLog{}
	hold := &atomic.Bool{}
	f := filepath.Join(os.TempDir(), fmt.Sprintf("hover-handoff-%s-%d", name, os.Getpid()))
	os.RemoveAll(f)
	os.MkdirAll(f, 0o777)
	t.Cleanup(func() { os.RemoveAll(f) })
	k := NewKiroSessions(func(tool core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			n := log.add(logged{tool, a.Prompt, a.Resume})
			a.Events(KiroEvent{SessionID: sp(tool.ID() + "-conv")})
			if strings.Contains(a.Prompt, "[hold]") {
				for hold.Load() && !a.Ct.IsCancelled() {
					time.Sleep(5 * time.Millisecond)
				}
			}
			return NewResult(core.Completed, fmt.Sprintf("%s says %d", tool.ID(), n))
		}
	}, core.NewAgentHistory(filepath.Join(f, "history"), key8()))
	return k, log, f, hold
}

func doneN(t *testing.T, k *KiroSessions, id int32, n int) {
	t.Helper()
	waitFor20(t, "the turn", func() bool {
		s, ok := k.Get(id)
		return ok && !s.Busy() && len(s.Turns) == n && s.Turns[n-1].Result != nil
	})
}

func targetOf(t *testing.T, id string) Target {
	x, ok := ParseTarget(id)
	if !ok {
		t.Fatal(id)
	}
	return x
}

func TestAConversationMovesToAnotherAgentWithAnAccountAndComesBackToItsOwnMemory(t *testing.T) {
	k, log, f, _ := rig(t, "move")
	s := must(k.Start(core.Kiro, f, "Build the parser. Never allocate in the hot loop.", nil))
	doneN(t, k, s.ID, 1)
	k.Reply(s.ID, "Now the lexer", nil)
	doneN(t, k, s.ID, 2)
	// Kiro -> Codex: a new conversation for Codex, started from an account; the history is untouched.
	sw, err := k.SwitchProvider(s.ID, targetOf(t, "codex"))
	if err != nil || sw.Mode != "portable" || sw.Carried != 2 || sw.Omitted != 0 {
		t.Fatal(sw, err)
	}
	now := must(k.Get(s.ID))
	if now.Tool != core.Codex || len(now.Turns) != 2 || !strings.HasPrefix(now.Turns[1].Result.Text, "kiro says") {
		t.Error("old turns keep the agent that gave them")
	}
	lin := now.Ext.Lineage
	h := lin.Handoffs[0]
	if len(lin.Handoffs) != 1 || h.Turn != 2 || h.From != "kiro" || h.To != "codex" || h.Mode != "portable" {
		t.Errorf("%+v", lin.Handoffs)
	}
	if lin.Pending == nil || !slices.ContainsFunc(lin.Natives, func(n core.Native) bool { return n.Provider == "kiro" && n.ID == "kiro-conv" && n.Seen == 2 }) {
		t.Errorf("%+v", lin.Natives)
	}
	// The next message: the account comes first, the user's words after the line, whole.
	k.Reply(s.ID, "Add error recovery, please.", nil)
	doneN(t, k, s.ID, 3)
	l := log.last()
	if l.tool != core.Codex || l.resume != nil {
		t.Error("Codex starts a conversation of its own")
	}
	if !strings.HasPrefix(l.prompt, "[Hover handoff]") || !strings.Contains(l.prompt, "Never allocate in the hot loop") || !strings.Contains(l.prompt, "2. Now the lexer") || !strings.HasSuffix(l.prompt, "---\nAdd error recovery, please.") {
		t.Error(l.prompt)
	}
	// Sent once: the next reply to Codex carries no account.
	k.Reply(s.ID, "And docs", nil)
	doneN(t, k, s.ID, 4)
	if l = log.last(); l.prompt != "And docs" || deref(l.resume) != "codex-conv" {
		t.Error(l.prompt, deref(l.resume))
	}
	// Back to Kiro: its own conversation resumes, and only the two turns it missed are handed over.
	if sw, err = k.SwitchProvider(s.ID, targetOf(t, "kiro")); err != nil || sw.Mode != "native" || sw.Carried != 2 {
		t.Fatal(sw, err)
	}
	k.Reply(s.ID, "Thanks, carry on", nil)
	doneN(t, k, s.ID, 5)
	l = log.last()
	if l.tool != core.Kiro || deref(l.resume) != "kiro-conv" {
		t.Error(l.tool, deref(l.resume))
	}
	if !strings.Contains(l.prompt, "went on without you for 2 turns") || !strings.Contains(l.prompt, "3. Add error recovery") || strings.Contains(l.prompt, "The original request") || !strings.HasSuffix(l.prompt, "Thanks, carry on") {
		t.Error(l.prompt)
	}
	if len(must(k.Get(s.ID)).Ext.Lineage.Handoffs) != 2 {
		t.Error("handoffs")
	}
	// The same move again is refused, not repeated.
	if _, err := k.SwitchProvider(s.ID, targetOf(t, "kiro")); err == nil || !strings.Contains(err.Error(), "already") {
		t.Error(err)
	}
}

func TestASwitchAskedForWithAQueuedMessageHappensWhenThatMessageIsSent(t *testing.T) {
	k, log, f, hold := rig(t, "queued")
	hold.Store(true)
	s := must(k.Start(core.Kiro, f, "first [hold]", nil))
	waitFor20(t, "the run", func() bool { return log.n() == 1 })
	if _, err := k.SwitchProvider(s.ID, targetOf(t, "codex")); err == nil || !strings.Contains(err.Error(), "A run is going on") {
		t.Error(err)
	}
	k.ReplyMsg(s.ID, Msg{Text: "then this, on Codex", SwitchTo: sp("codex")})
	if must(k.Get(s.ID)).Tool != core.Kiro {
		t.Error("not while the earlier work runs")
	}
	hold.Store(false)
	doneN(t, k, s.ID, 2)
	if l := log.last(); l.tool != core.Codex || !strings.HasPrefix(l.prompt, "[Hover handoff]") || !strings.HasSuffix(l.prompt, "then this, on Codex") {
		t.Error(l.tool, l.prompt)
	}
	if must(k.Get(s.ID)).Tool != core.Codex {
		t.Error("not moved")
	}
	// One that can't be made is said to the agent that gets the message, and the conversation stays.
	k.ReplyMsg(s.ID, Msg{Text: "and this", SwitchTo: sp("nonesuch")})
	doneN(t, k, s.ID, 3)
	if l := log.last(); l.tool != core.Codex || !strings.HasPrefix(l.prompt, "[Hover] The switch to “nonesuch” couldn’t be made") {
		t.Error(l.tool, l.prompt)
	}
}

func TestAConversationTooLongToCarryStaysWhereItIs(t *testing.T) {
	k, _, f, _ := rig(t, "long")
	s := must(k.Start(core.Kiro, f, "Start. "+strings.Repeat("requirement ", 900), nil))
	doneN(t, k, s.ID, 1)
	for i := 2; i <= 40; i++ {
		k.Reply(s.ID, fmt.Sprintf("Step %d: %s", i, strings.Repeat("constraint ", 45)), nil)
		doneN(t, k, s.ID, i)
	}
	before := must(k.Get(s.ID))
	if _, err := k.SwitchProvider(s.ID, targetOf(t, "codex")); err == nil || !strings.Contains(err.Error(), "too long to carry over") {
		t.Fatal(err)
	}
	after := must(k.Get(s.ID))
	if after.Tool != core.Kiro || after.Ext.Lineage != nil || deref(after.KiroID) != deref(before.KiroID) {
		t.Error("nothing changed")
	}
}

func TestTheAccountWaitingForTheNextMessageSurvivesARestartAndIsSentOnce(t *testing.T) {
	k, log, f, _ := rig(t, "restart")
	s := must(k.Start(core.Kiro, f, "Origin", nil))
	doneN(t, k, s.ID, 1)
	if _, err := k.SwitchProvider(s.ID, targetOf(t, "cursor")); err != nil {
		t.Fatal(err)
	}
	k.History().Flush()
	key := must(k.Get(s.ID)).Key
	again := NewKiroSessions(func(tool core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			log.add(logged{tool, a.Prompt, a.Resume})
			return NewResult(core.Completed, "ok")
		}
	}, core.NewAgentHistory(filepath.Join(f, "history"), key8()))
	woke := must(again.Wake(key))
	if woke.Tool != core.Cursor || woke.Ext.Lineage == nil || woke.Ext.Lineage.Pending == nil {
		t.Fatal("the account was saved with the conversation")
	}
	again.Reply(woke.ID, "go on", nil)
	doneN(t, again, woke.ID, 2)
	if l := log.last(); l.tool != core.Cursor || !strings.HasPrefix(l.prompt, "[Hover handoff]") || !strings.HasSuffix(l.prompt, "go on") {
		t.Error(l.tool, l.prompt)
	}
	if must(again.Get(woke.ID)).Ext.Lineage.Pending != nil {
		t.Error("sent twice")
	}
}

func TestAForkCopiesTheTurnsUpToAStablePointKeepsItsLineageAndLeavesTheSourceAlone(t *testing.T) {
	k, log, f, hold := rig(t, "fork")
	s := must(k.Start(core.Kiro, f, "Design the cache", nil))
	doneN(t, k, s.ID, 1)
	k.Reply(s.ID, "Pick a size", nil)
	doneN(t, k, s.ID, 2)
	key := must(k.Get(s.ID)).Key
	other := filepath.Join(os.TempDir(), fmt.Sprintf("hover-handoff-fork-dir-%d", os.Getpid()))
	os.MkdirAll(other, 0o777)
	defer os.RemoveAll(other)
	fk, err := k.Fork(key, 0, targetOf(t, "codex"), other, nil)
	if err != nil {
		t.Fatal(err)
	}
	if fk.Key == key || fk.Tool != core.Codex || len(fk.Turns) != 1 || fk.Folder != other || fk.Turns[0].Prompt != "Design the cache" {
		t.Errorf("%+v", fk)
	}
	lin := fk.Ext.Lineage
	if lin.Fork == nil || lin.Fork.Key != key || lin.Fork.Turn != 0 || len(lin.Handoffs) != 1 || lin.Pending == nil {
		t.Errorf("%+v", lin)
	}
	// The source is as it was: its turns, provider and conversation.
	if src := must(k.Get(s.ID)); len(src.Turns) != 2 || src.Tool != core.Kiro || deref(src.KiroID) != "kiro-conv" {
		t.Error("the source changed")
	}
	// The fork's agent starts from an account; the fork goes on on its own.
	k.Reply(fk.ID, "What about 64 MB?", nil)
	doneN(t, k, fk.ID, 2)
	if l := log.last(); l.tool != core.Codex || l.resume != nil || !strings.Contains(l.prompt, "a fork of another, taken after turn 1") || !strings.HasSuffix(l.prompt, "What about 64 MB?") {
		t.Error(l.prompt)
	}
	if len(must(k.Get(s.ID)).Turns) != 2 {
		t.Error("the source went on")
	}
	// Only from a turn that ended: not a running one, not one that isn't there.
	hold.Store(true)
	k.Reply(s.ID, "third [hold]", nil)
	waitFor20(t, "the hold", func() bool { return must(k.Get(s.ID)).Busy() })
	if _, err := k.Fork(key, 2, targetOf(t, "kiro"), f, nil); err == nil || !strings.Contains(err.Error(), "has ended") {
		t.Error(err)
	}
	if _, err := k.Fork(key, 9, targetOf(t, "kiro"), f, nil); err == nil || !strings.Contains(err.Error(), "isn’t there") {
		t.Error(err)
	}
	hold.Store(false)
	doneN(t, k, s.ID, 3)
}

func TestFindingsComeBackAsOneMessageOnceAndChangeNoFiles(t *testing.T) {
	k, _, f, _ := rig(t, "back")
	parent := must(k.Start(core.Kiro, f, "Design the cache", nil))
	doneN(t, k, parent.ID, 1)
	pkey := must(k.Get(parent.ID)).Key
	fk, err := k.Fork(pkey, 0, targetOf(t, "codex"), f, nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := k.BringFindingsBack(fk.Key, nil); err == nil || !strings.Contains(err.Error(), "Nothing was asked") {
		t.Error("nothing to bring yet")
	}
	k.Reply(fk.ID, "Try the risky approach", nil)
	doneN(t, k, fk.ID, 2)
	entries := func() int { e, _ := os.ReadDir(f); return len(e) }
	filesBefore := entries()
	if n, err := k.BringFindingsBack(fk.Key, nil); err != nil || n == 0 {
		t.Fatal(n, err)
	}
	doneN(t, k, parent.ID, 2)
	p := must(k.Get(parent.ID))
	msg := p.Turns[1].Prompt
	if !strings.Contains(msg, "hover-return:") || !strings.Contains(msg, "Asked: Try the risky approach") || !strings.Contains(msg, "does not merge any code") || !strings.Contains(msg, "Found: codex says") {
		t.Error(msg)
	}
	if p.Turns[1].Chips[0].Kind != "thread" || len(p.Ext.Lineage.Returned) != 1 || entries() != filesBefore {
		t.Error("the fork is referenced, not copied; no file moved")
	}
	// A retry finds it done; a fork that went on has new findings.
	if n, _ := k.BringFindingsBack(fk.Key, nil); n != 0 || len(must(k.Get(parent.ID)).Turns) != 2 {
		t.Error("no second message")
	}
	k.Reply(fk.ID, "And measure it", nil)
	doneN(t, k, fk.ID, 3)
	if n, _ := k.BringFindingsBack(fk.Key, nil); n == 0 {
		t.Error("new findings")
	}
	doneN(t, k, parent.ID, 3)
}

// MARK: busy_retry.rs

type seenRuns struct {
	mu   sync.Mutex
	list [][2]any
}

// busyRunner is busy busy times, then answers. It records each prompt and resume id.
func busyRunner(busy int, said string) (func(core.AgentTool) RunTask, *seenRuns) {
	seen := &seenRuns{}
	return func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult {
			seen.mu.Lock()
			seen.list = append(seen.list, [2]any{a.Prompt, deref(a.Resume)})
			n := len(seen.list)
			seen.mu.Unlock()
			a.Events(KiroEvent{SessionID: sp("s1")})
			if n <= busy {
				return NewResult(core.Failed, said)
			}
			return NewResult(core.Completed, "done")
		}
	}, seen
}

func (s *seenRuns) n() int { s.mu.Lock(); defer s.mu.Unlock(); return len(s.list) }

func endedSession(k *KiroSessions, id int32) KiroSession {
	start := time.Now()
	for must(k.Get(id)).Busy() && time.Since(start) < 15*time.Second {
		time.Sleep(20 * time.Millisecond)
	}
	return must(k.Get(id))
}

func TestABusyModelIsContinuedInTheSameTurnUntilItAnswers(t *testing.T) {
	f := sessFolder(t, "busy-on")
	make, seen := busyRunner(2, "Too many requests, please wait before trying again.")
	k := NewKiroSessions(make, nil)
	k.SetRetryWhenBusy(func() bool { return true })
	s := endedSession(k, must(k.Start(core.Kiro, f, "fix the bug", nil)).ID)
	if s.State != core.Completed || len(s.Turns) != 1 {
		t.Error("no extra turns in the chat")
	}
	// The first try had no conversation yet, so it is sent as it was; then it continues
	// the one Kiro named.
	if !reflect.DeepEqual(seen.list, [][2]any{{"fix the bug", nil}, {"continue", "s1"}, {"continue", "s1"}}) {
		t.Error(seen.list)
	}
	i := slices.IndexFunc(s.Turns[0].Steps, func(x core.KiroStep) bool { return x.ID == "hover-retry" })
	if i < 0 || s.Turns[0].Steps[i].Title != "Retrying after high demand (2)" || s.Turns[0].Steps[i].Status != "completed" {
		t.Error("its one quiet step")
	}
}

func TestOffOrAnotherFailureOrAnotherToolIsNotContinued(t *testing.T) {
	f := sessFolder(t, "busy-off")
	for _, c := range []struct {
		on   bool
		said string
		tool core.AgentTool
	}{
		{false, "The model you've selected is experiencing a high volume of traffic.", core.Kiro},
		{true, "Kiro isn't signed in.", core.Kiro},
		{true, "Too many requests, please wait before trying again.", core.Codex},
	} {
		make, seen := busyRunner(1, c.said)
		k := NewKiroSessions(make, nil)
		on := c.on
		k.SetRetryWhenBusy(func() bool { return on })
		s := endedSession(k, must(k.Start(c.tool, f, "a", nil)).ID)
		if s.State != core.Failed || seen.n() != 1 {
			t.Errorf("%+v: %v %d", c, s.State, seen.n())
		}
	}
}

func TestStopEndsAModelThatStaysBusy(t *testing.T) {
	f := sessFolder(t, "busy-stop")
	make, seen := busyRunner(1<<30, "Too many requests, please wait before trying again.")
	k := NewKiroSessions(make, nil)
	k.SetRetryWhenBusy(func() bool { return true })
	id := must(k.Start(core.Kiro, f, "a", nil)).ID
	start := time.Now()
	for seen.n() < 3 && time.Since(start) < 10*time.Second {
		time.Sleep(20 * time.Millisecond)
	}
	if seen.n() < 3 {
		t.Fatal("it keeps going on its own")
	}
	k.Stop(id)
	if s := endedSession(k, id); s.State != core.Cancelled {
		t.Error(s.State)
	}
	n := seen.n()
	time.Sleep(1500 * time.Millisecond)
	if seen.n() != n {
		t.Error("nothing is sent after Stop")
	}
}
