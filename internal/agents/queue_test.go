package agents

// tests/queue.rs: the reply queue. Waiting messages can be edited, moved, taken back and
// sent ahead; each operation reaches the message it names, a stop holds the rest, and the
// queue, its chips and its order come back after a restart (held).

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

	"github.com/4regab/Hover/internal/core"
)

func queueFolder(t *testing.T, name string) string {
	d := filepath.Join(os.TempDir(), fmt.Sprintf("hover-queue-%s-%d", name, os.Getpid()))
	os.RemoveAll(d)
	os.MkdirAll(d, 0o777)
	t.Cleanup(func() { os.RemoveAll(d) })
	return d
}

func waitFor20(t *testing.T, what string, f func() bool) {
	t.Helper()
	// A condition seen true is true: it is not asked again, since it may be false the next
	// moment (a run that ends and the next one that starts).
	for start := time.Now(); time.Since(start) < 20*time.Second; time.Sleep(10 * time.Millisecond) {
		if f() {
			return
		}
	}
	t.Fatalf("timed out waiting for %s", what)
}

// quiet is a check, for waitFor20, that nothing runs and nothing waits to start, on three
// polls in a row. A turn that ends marks its session finished and only then starts the
// next queued one, so for an instant Running() is 0 although work is waiting; the Rust
// tests have the same wait and the same gap.
func quiet(k *KiroSessions) func() bool {
	calm := 0
	return func() bool {
		still := k.Running() == 0
		for _, s := range k.All() {
			if still && !s.Held && slices.ContainsFunc(s.Turns, func(t KiroTurn) bool { return t.Queued }) {
				still = false
			}
		}
		if !still {
			calm = 0
			return false
		}
		calm++
		return calm >= 3
	}
}

// scripted is an agent that runs each prompt until go is set; a stop ends it (confirmed)
// or, with never, leaves it unconfirmed.
type scripted struct {
	mu    sync.Mutex
	seen  []string
	open  atomic.Bool
	never atomic.Bool
}

func agent() (*scripted, *KiroSessions) {
	a := &scripted{}
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(r RunArgs) KiroResult {
			a.mu.Lock()
			a.seen = append(a.seen, r.Prompt)
			a.mu.Unlock()
			r.Events(KiroEvent{SessionID: sp("conv-1")})
			for !a.open.Load() && !r.Ct.IsCancelled() {
				time.Sleep(5 * time.Millisecond)
			}
			if r.Ct.IsCancelled() {
				if !a.never.Load() {
					return NewResult(core.Cancelled, "Stopped.")
				}
				res := NewResult(core.Cancelled, "Stop not confirmed.")
				res.Unconfirmed = true
				return res
			}
			return NewResult(core.Completed, "done")
		}
	}, nil)
	return a, k
}

func (a *scripted) prompts() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	out := []string{}
	for _, p := range a.seen {
		out = append(out, firstOf(p))
	}
	return out
}

func firstOf(p string) string {
	if ls := rustLines(p); len(ls) > 0 {
		return ls[0]
	}
	return ""
}

func uids(k *KiroSessions, id int32) [][2]string {
	var out [][2]string
	for _, t := range must(k.Get(id)).Turns {
		if t.Queued {
			out = append(out, [2]string{t.UID, t.Prompt})
		}
	}
	return out
}

func texts(q [][2]string) []string {
	var out []string
	for _, x := range q {
		out = append(out, x[1])
	}
	return out
}

func TestWaitingMessagesAreEditedMovedAndTakenBackByNameAndOnlyInTheirOwnSession(t *testing.T) {
	a, k := agent()
	f := queueFolder(t, "ops")
	s := must(k.Start(core.Kiro, f, "first", nil))
	other := must(k.Start(core.Codex, f, "elsewhere", nil))
	waitFor20(t, "both to run", func() bool { return len(a.prompts()) == 2 })
	for _, x := range []string{"two", "three", "four"} {
		if !k.Reply(s.ID, x, nil) {
			t.Fatal(x)
		}
	}
	k.Reply(other.ID, "other two", nil)
	q := uids(k, s.ID)
	if !reflect.DeepEqual(texts(q), []string{"two", "three", "four"}) {
		t.Fatal(q)
	}
	// Edit one; the others and the other session are untouched.
	chip := core.Chip{Kind: "file", Label: "a.rs", Source: "a.rs", Live: true}
	if err := k.EditQueued(s.ID, q[1][0], Msg{Text: "three, edited", Chips: []core.Chip{chip}, Images: []string{"/tmp/p.png"}}); err != nil {
		t.Fatal(err)
	}
	now := must(k.Get(s.ID))
	if !reflect.DeepEqual(texts(uids(k, s.ID)), []string{"two", "three, edited", "four"}) ||
		!reflect.DeepEqual(now.Turns[2].Chips, []core.Chip{chip}) || !reflect.DeepEqual(now.Turns[2].Images, []string{"/tmp/p.png"}) {
		t.Error(uids(k, s.ID))
	}
	if uids(k, other.ID)[0][1] != "other two" {
		t.Error("other")
	}
	// The id of a message in another session reaches nothing there.
	if err := k.EditQueued(other.ID, q[0][0], MsgText("x")); err == nil || !err.Gone {
		t.Error(err)
	}
	if _, err := k.RemoveQueued(other.ID, q[2][0]); err == nil || !err.Gone {
		t.Error(err)
	}
	// Move the last to the front, then take the middle one back.
	if err := k.MoveQueued(s.ID, q[2][0], 0); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(texts(uids(k, s.ID)), []string{"four", "two", "three, edited"}) {
		t.Error(uids(k, s.ID))
	}
	if back, err := k.RemoveQueued(s.ID, q[0][0]); err != nil || back.Text != "two" {
		t.Error(back, err)
	}
	if _, err := k.RemoveQueued(s.ID, q[0][0]); err == nil || !err.Gone {
		t.Error("a second click finds it gone")
	}
	if !reflect.DeepEqual(texts(uids(k, s.ID)), []string{"four", "three, edited"}) {
		t.Error(uids(k, s.ID))
	}
	// An empty message is not a message.
	if err := k.EditQueued(s.ID, q[1][0], MsgText("  ")); err == nil || err.Invalid == "" {
		t.Error(err)
	}
	a.open.Store(true)
	waitFor20(t, "all to finish", quiet(k))
	n := 0
	for _, p := range a.prompts() {
		if strings.HasPrefix(p, "four") || strings.HasPrefix(p, "three") {
			n++
		}
	}
	if n != 2 {
		t.Error(a.prompts())
	}
}

func TestAMessageThatStartsWhileItIsBeingEditedComesBackWithItsEditedText(t *testing.T) {
	a, k := agent()
	f := queueFolder(t, "race")
	s := must(k.Start(core.Kiro, f, "first", nil))
	waitFor20(t, "the run", func() bool { return len(a.prompts()) == 1 })
	k.Reply(s.ID, "second", nil)
	uid := uids(k, s.ID)[0][0]
	a.open.Store(true)
	waitFor20(t, "the second to start", func() bool { return len(a.prompts()) == 2 })
	err := k.EditQueued(s.ID, uid, Msg{Text: "second, but better"})
	if err == nil || err.Started == nil || err.Started.Text != "second, but better" {
		t.Errorf("the edited text is handed back for the composer: %v", err)
	}
	if a.prompts()[1] != "second" {
		t.Error("the message that started is the one that was queued")
	}
}

func TestSendNowStopsThroughTheToolAndSendsOnceAndAnUnconfirmedStopSendsNothing(t *testing.T) {
	a, k := agent()
	f := queueFolder(t, "steer")
	s := must(k.Start(core.Kiro, f, "first", nil))
	waitFor20(t, "the run", func() bool { return len(a.prompts()) == 1 })
	k.Reply(s.ID, "later", nil)
	k.Reply(s.ID, "urgent", nil)
	urgent := uids(k, s.ID)[1][0]
	if r, err := k.SendNowQueued(s.ID, urgent); err != nil || r != SendSteering {
		t.Fatal(r, err)
	}
	if _, err := k.SendNowQueued(s.ID, urgent); err != nil && err.Started == nil && !err.Gone {
		t.Error("a second click is harmless")
	}
	waitFor20(t, "the urgent one to start", func() bool { return len(a.prompts()) == 2 })
	if a.prompts()[1] != "urgent" {
		t.Error("ahead of the message that waited longer")
	}
	n := 0
	for _, p := range a.prompts() {
		if p == "urgent" {
			n++
		}
	}
	if n != 1 || !reflect.DeepEqual(texts(uids(k, s.ID)), []string{"later"}) {
		t.Error("sent once")
	}
	if must(k.Get(s.ID)).Turns[0].Result.State != core.Cancelled {
		t.Error("the first attempt keeps its place in the history")
	}
	// A tool that never confirms the stop: nothing more goes, so no second writer starts.
	a.never.Store(true)
	later := uids(k, s.ID)[0][0]
	if r, err := k.SendNowQueued(s.ID, later); err != nil || r != SendSteering {
		t.Fatal(r, err)
	}
	waitFor20(t, "the stop to end the run", func() bool { return !must(k.Get(s.ID)).Busy() })
	time.Sleep(150 * time.Millisecond)
	if len(a.prompts()) != 2 {
		t.Error("nothing was sent after an unconfirmed stop")
	}
	if !must(k.Get(s.ID)).Held || len(uids(k, s.ID)) != 1 {
		t.Error("the message waits, held")
	}
	// The user resuming it is the explicit step.
	a.never.Store(false)
	a.open.Store(true)
	if !k.ResumeQueue(s.ID) {
		t.Fatal("not resumed")
	}
	waitFor20(t, "the held message to go", func() bool { return len(a.prompts()) == 3 })
}

func TestStopHoldsWhatWaitsAndOnlyTheUserLetsItGo(t *testing.T) {
	a, k := agent()
	f := queueFolder(t, "hold")
	s := must(k.Start(core.Kiro, f, "first", nil))
	waitFor20(t, "the run", func() bool { return len(a.prompts()) == 1 })
	k.Reply(s.ID, "second", nil)
	k.Reply(s.ID, "third", nil)
	k.Stop(s.ID)
	waitFor20(t, "the stop", func() bool { return !must(k.Get(s.ID)).Busy() })
	time.Sleep(100 * time.Millisecond)
	if !must(k.Get(s.ID)).Held || len(a.prompts()) != 1 {
		t.Error("held")
	}
	// A message the user sends now is their own action: the oldest waiting one goes first,
	// in order.
	a.open.Store(true)
	if !k.Reply(s.ID, "fourth", nil) {
		t.Fatal("fourth")
	}
	waitFor20(t, "all three to go, in order", func() bool { return len(a.prompts()) == 4 })
	if !reflect.DeepEqual(a.prompts(), []string{"first", "second", "third", "fourth"}) {
		t.Error(a.prompts())
	}
}

func TestTheQueueItsOrderAndItsChipsComeBackAfterARestartHeld(t *testing.T) {
	root := queueFolder(t, "restart")
	var key [32]byte
	for i := range key {
		key[i] = 5
	}
	crypto := core.CryptoWithKey(key)
	hist := filepath.Join(root, "history")
	// One session with a run that waits and two waiting messages, saved as it goes.
	var gate atomic.Bool
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(r RunArgs) KiroResult {
			for !gate.Load() && !r.Ct.IsCancelled() {
				time.Sleep(5 * time.Millisecond)
			}
			return NewResult(core.Completed, "ok")
		}
	}, core.NewAgentHistory(hist, crypto))
	defer gate.Store(true)
	s := must(k.Start(core.Kiro, root, "first", nil))
	chip := core.Chip{Kind: "quote", Label: "a quote", Source: "k:0", Text: sp("quoted")}
	k.ReplyMsg(s.ID, Msg{Text: "second", Chips: []core.Chip{chip}})
	k.ReplyMsg(s.ID, Msg{Text: "third", Images: []string{"/tmp/x.png"}, SwitchTo: sp("codex")})
	before := uids(k, s.ID)
	skey := must(k.Get(s.ID)).Key
	k.History().Flush()
	// A new start of Hover: the same waiting messages, in order, with their chips,
	// pictures and ids, and held.
	again := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(RunArgs) KiroResult { return NewResult(core.Completed, "after restart") }
	}, core.NewAgentHistory(hist, crypto))
	woke := must(again.Wake(skey))
	if !woke.Held || woke.Busy() {
		t.Fatal("not held")
	}
	var q []KiroTurn
	var got [][2]string
	for _, tt := range woke.Turns {
		if tt.Queued {
			q = append(q, tt)
			got = append(got, [2]string{tt.UID, tt.Prompt})
		}
	}
	if !reflect.DeepEqual(got, before) {
		t.Errorf("%q %q", got, before)
	}
	if !reflect.DeepEqual(q[0].Chips, []core.Chip{chip}) || !reflect.DeepEqual(q[1].Images, []string{"/tmp/x.png"}) || deref(q[1].SwitchTo) != "codex" {
		t.Errorf("%+v", q)
	}
	if woke.Turns[0].Result.State != core.Cancelled {
		t.Error("the run that was going reads as stopped")
	}
	time.Sleep(150 * time.Millisecond)
	if must(again.Get(woke.ID)).Busy() {
		t.Error("saved messages alone start nothing")
	}
	// The user resumes: both go, in order.
	if !again.ResumeQueue(woke.ID) {
		t.Fatal("not resumed")
	}
	waitFor20(t, "the queue to run", func() bool {
		s, ok := again.Get(woke.ID)
		if !ok || s.Busy() {
			return false
		}
		for _, tt := range s.Turns {
			if tt.Result == nil {
				return false
			}
		}
		return true
	})
}

type noProviders struct{}

func (noProviders) Providers() []Provider         { return nil }
func (noProviders) AccessOf(KiroSession) string   { return "full" }
func (noProviders) Limits() core.DelegationLimits { return core.DefaultDelegationLimits() }

func TestChipsReachTheAgentAsTextStayInTheHistoryAndAConversationReferenceLetsItReadOnly(t *testing.T) {
	root := queueFolder(t, "chips")
	var key [32]byte
	for i := range key {
		key[i] = 6
	}
	crypto := core.CryptoWithKey(key)
	var mu sync.Mutex
	var seen []string
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(r RunArgs) KiroResult {
			mu.Lock()
			seen = append(seen, r.Prompt)
			mu.Unlock()
			return NewResult(core.Completed, "The earlier talk said: use retries.")
		}
	}, core.NewAgentHistory(filepath.Join(root, "history"), crypto))
	os.WriteFile(filepath.Join(root, "a.rs"), []byte("fn a() {}\nfn b() {}\n"), 0o666)
	// An earlier conversation to refer to.
	earlier := must(k.Start(core.Codex, root, "How should we handle flaky calls?", nil))
	waitFor20(t, "the earlier one", func() bool { s, ok := k.Get(earlier.ID); return ok && !s.Busy() })
	// A new task is sent a snapshot of lines, a live file and a reference to that
	// conversation.
	lines, err := Lines(root, "a.rs", 2, 2)
	if err != nil {
		t.Fatal(err)
	}
	live, err := FileLive(root, "a.rs")
	if err != nil {
		t.Fatal(err)
	}
	chips := []core.Chip{lines, live, ThreadChip(earlier.Key, "Flaky calls")}
	s := must(k.StartBound(core.Kiro, root, "Use what we decided", nil, nil, nil, core.SessionExt{}))
	waitFor20(t, "that one", func() bool { x, ok := k.Get(s.ID); return ok && !x.Busy() })
	if !k.ReplyMsg(s.ID, Msg{Text: "Now apply it to this", Chips: chips}) {
		t.Fatal("no reply")
	}
	waitFor20(t, "the reply", func() bool {
		x, ok := k.Get(s.ID)
		return ok && !x.Busy() && len(x.Turns) == 2 && x.Turns[1].Result != nil
	})
	mu.Lock()
	sent := seen[len(seen)-1]
	mu.Unlock()
	if !strings.HasPrefix(sent, "Now apply it to this\n\n[Attached by Hover]") || !strings.Contains(sent, "fn b() {}") || !strings.Contains(sent, "Conversation “Flaky calls”") {
		t.Error(sent)
	}
	if strings.Contains(sent, "How should we handle flaky calls?") {
		t.Error("the other conversation is a reference, not a copy")
	}
	// The history keeps the chips with the sent message.
	k.History().Flush()
	if got := must(k.History().Load(s.Key)).Turns[1].Ext.Chips; !reflect.DeepEqual(got, chips) {
		t.Errorf("%+v", got)
	}
	// The agent that was sent the reference can read that conversation, in pages; another
	// can't.
	o := NewOrch(k, noProviders{}, nil)
	if !o.CanRead(s.Key, earlier.Key) {
		t.Error("the reference doesn't let it read")
	}
	if o.CanRead(earlier.Key, s.Key) {
		t.Error("a reference is one way")
	}
	if o.CanRead("someone-else", earlier.Key) {
		t.Error("someone else reads it")
	}
}
