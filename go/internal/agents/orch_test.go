package agents

// tests/orch.rs. Orchestration: a lead asks other agents for help through Hover. These run
// the real sessions and the real orchestrator against scripted agents.

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/4regab/Hover/go/internal/core"
)

func val(p *string) string {
	if p == nil {
		return ""
	}
	return *p
}

type stubEnv struct {
	mu     sync.Mutex
	limits core.DelegationLimits
	access string
}

func (e *stubEnv) Providers() []Provider {
	p := func(id string, tool core.AgentTool, ready, leads bool) Provider {
		hint := ""
		if !ready {
			hint = "sign in first"
		}
		return Provider{ID: id, Name: strings.ToUpper(id), Tool: tool, Ready: ready, Hint: hint, ReadOnly: true, Resume: true, Leads: leads}
	}
	return []Provider{p("kiro", core.Kiro, true, true), p("codex", core.Codex, true, true), p("claude", core.Claude, false, true), p("opencode", core.OpenCode, true, false)}
}

func (e *stubEnv) AccessOf(s KiroSession) string {
	if s.Access != nil {
		return *s.Access
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.access
}

func (e *stubEnv) Limits() core.DelegationLimits {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.limits
}

// orchScript is what a scripted agent does: it gets its run arguments and the
// orchestrator.
type orchScript func(a RunArgs, o *Orch, release *atomic.Bool) KiroResult

type orchRig struct {
	k       *KiroSessions
	orch    *Orch
	env     *stubEnv
	root    string
	folder  string
	release *atomic.Bool
	crypto  *core.Crypto
}

// holdRun blocks until released or stopped: the "[hold]" helper.
func holdRun(a RunArgs, release *atomic.Bool) KiroResult {
	for !release.Load() && !a.Ct.IsCancelled() {
		time.Sleep(10 * time.Millisecond)
	}
	if a.Ct.IsCancelled() {
		return NewResult(core.Cancelled, "Stopped.")
	}
	return NewResult(core.Completed, "Held, then done.")
}

func newOrchRig(t *testing.T, name string, limits core.DelegationLimits, script orchScript) *orchRig {
	root := newDir(t, "orch-"+name)
	folder := filepath.Join(root, "project")
	os.MkdirAll(folder, 0o777)
	var key [32]byte
	for i := range key {
		key[i] = 9
	}
	crypto := core.CryptoWithKey(key)
	release := &atomic.Bool{}
	var cell atomic.Pointer[Orch]
	k := NewKiroSessions(func(core.AgentTool) RunTask {
		return func(a RunArgs) KiroResult { return script(a, cell.Load(), release) }
	}, core.NewAgentHistory(filepath.Join(root, "history"), crypto))
	env := &stubEnv{limits: limits, access: "full"}
	o := NewOrch(k, env, core.SealedIn(filepath.Join(root, "orch"), "runs", crypto))
	cell.Store(o)
	// What is still held ends before the folder goes.
	t.Cleanup(func() {
		release.Store(true)
		for start := time.Now(); k.Running() > 0 && time.Since(start) < 5*time.Second; {
			time.Sleep(10 * time.Millisecond)
		}
	})
	return &orchRig{k: k, orch: o, env: env, root: root, folder: folder, release: release, crypto: crypto}
}

func orchLink() core.SessionExt { return core.SessionExt{Orch: &core.OrchLink{Delegation: true}} }

func (r *orchRig) lead(prompt string) KiroSession {
	s, ok := r.k.StartBound(core.Kiro, r.folder, prompt, nil, nil, nil, orchLink())
	if !ok {
		panic("a lead")
	}
	return s
}

func (r *orchRig) ask(lead, provider, brief string, request *string) (RunInfo, error) {
	return r.orch.Delegate(lead, Delegate{Provider: provider, Brief: brief, Request: request})
}

func (r *orchRig) idle(id int32) func() bool {
	return func() bool { s, ok := r.k.Get(id); return ok && !s.Busy() }
}

func errText(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func TestALeadDelegatesWaitsAndGetsTheResultAndTheRecordSurvivesARestart(t *testing.T) {
	var mu sync.Mutex
	var seen []string
	var got *string
	r := newOrchRig(t, "basic", core.DefaultDelegationLimits(), func(a RunArgs, o *Orch, _ *atomic.Bool) KiroResult {
		if strings.HasPrefix(a.Prompt, "[Hover helper task]") {
			mu.Lock()
			seen = append(seen, a.Prompt)
			mu.Unlock()
			return NewResult(core.Completed, "The answer is 42.")
		}
		tag := *a.Tag
		run, err := o.Delegate(tag, Delegate{Provider: "codex", Brief: "Work out the answer.", Role: sp("mathematician"), Request: sp("req-1")})
		if err != nil {
			t.Error(err)
			return NewResult(core.Failed, err.Error())
		}
		again, err := o.Delegate(tag, Delegate{Provider: "codex", Brief: "Work out the answer.", Request: sp("req-1")})
		if err != nil || again.Run != run.Run {
			t.Error("a retry with the same request id makes no second helper", err)
		}
		done, err := o.Wait(tag, run.Run, 10*time.Second)
		if err != nil {
			t.Error(err)
			return NewResult(core.Failed, err.Error())
		}
		mu.Lock()
		got = done.Result
		mu.Unlock()
		return NewResult(core.Completed, "Helper said: "+val(done.Result))
	})
	lead := r.lead("SECRET-LEAD-CONTEXT: please get the answer")
	waitFor20(t, "the lead to finish", r.idle(lead.ID))
	mu.Lock()
	if val(got) != "The answer is 42." {
		t.Errorf("%v", deref(got))
	}
	prompts := slices.Clone(seen)
	mu.Unlock()
	if s := must(r.k.Get(lead.ID)).Result().Text; s != "Helper said: The answer is 42." {
		t.Error(s)
	}
	// One helper, with the brief and role, and nothing of the lead's own conversation.
	if len(prompts) != 1 {
		t.Fatal(prompts)
	}
	if !strings.Contains(prompts[0], "Work out the answer.") || !strings.Contains(prompts[0], "mathematician") || strings.Contains(prompts[0], "SECRET-LEAD-CONTEXT") {
		t.Error(prompts[0])
	}
	if n := len(r.k.All()); n != 2 {
		t.Error(n)
	}
	helpers := r.orch.HelpersOf(lead.Key)
	if len(helpers) != 1 || helpers[0].State != HelperDone || helpers[0].Delivery != DeliveryTaken {
		t.Fatalf("%+v", helpers)
	}
	hs := must(r.k.Find(*helpers[0].Session))
	l := hs.Ext.Orch
	if val(l.Run) != helpers[0].Run || val(l.Parent) != lead.Key || l.Depth != 1 {
		t.Errorf("%+v", l)
	}
	// The record is on disk, sealed, and a new orchestrator reads it: the same run, result
	// and attempt.
	r.orch.Flush()
	again := NewOrch(r.k, r.env, core.SealedIn(filepath.Join(r.root, "orch"), "runs", r.crypto))
	back := again.HelpersOf(lead.Key)
	if len(back) != 1 || back[0].State != HelperDone || val(back[0].Result) != "The answer is 42." {
		t.Errorf("%+v", back)
	}
	entries, _ := os.ReadDir(filepath.Join(r.root, "orch"))
	for _, e := range entries {
		b, _ := os.ReadFile(filepath.Join(r.root, "orch", e.Name()))
		if strings.Contains(string(b), "answer") {
			t.Error("sealed, not plain text")
		}
	}
}

func TestHelpersNeverHaveMoreAccessThanTheLeadAndFailuresSayWhy(t *testing.T) {
	r := newOrchRig(t, "perm", core.DelegationLimits{MaxHelpers: 10, MaxParallel: 10, MaxDepth: 1}, func(a RunArgs, _ *Orch, release *atomic.Bool) KiroResult { return holdRun(a, release) })
	r.env.mu.Lock()
	r.env.access = "risky"
	r.env.mu.Unlock()
	lead := r.lead("hold")
	// A task of its own, started before the helpers fill the three places the app allows at
	// once (they start at once now, with no worktree to wait for).
	plain := must(r.k.Start(core.Codex, r.folder, "hold", nil))
	ask := func(p string, access *string) (RunInfo, error) {
		return r.orch.Delegate(lead.Key, Delegate{Provider: p, Brief: "x", Access: access})
	}
	a, err := ask("codex", sp("full"))
	if err != nil || a.Access != "risky" || !strings.Contains(val(a.Note), "narrowed to risky") {
		t.Errorf("%+v %v", a, err)
	}
	if a, err := ask("kiro", sp("read")); err != nil || a.Access != "read" {
		t.Error("narrower is fine", a, err)
	}
	if a, err := ask("kiro", nil); err != nil || a.Access != "risky" {
		t.Error("the lead's own by default", a, err)
	}
	if _, err := ask("claude", nil); !strings.Contains(errText(err), "isn’t available: sign in first") {
		t.Error(err)
	}
	if _, err := ask("gemini", nil); !strings.Contains(errText(err), "no provider called “gemini”") || !strings.Contains(errText(err), "kiro, codex") {
		t.Error(err)
	}
	if _, err := r.orch.Delegate(lead.Key, Delegate{Provider: "kiro", Brief: "  "}); !strings.Contains(errText(err), "needs a brief") {
		t.Error(err)
	}
	// Delegation off for a task: a clear refusal.
	if _, err := r.orch.Delegate(plain.Key, Delegate{Provider: "kiro", Brief: "x"}); errText(err) != OrchOff {
		t.Error(err)
	}
	r.release.Store(true)
	waitFor20(t, "all to finish", func() bool { return r.k.Running() == 0 })
	// A lead whose turn is over has stale credentials.
	if _, err := ask("kiro", nil); !strings.Contains(errText(err), "no longer valid") {
		t.Error(err)
	}
}

func TestTheUserSetLimitsHoldForCountParallelWorkAndDepth(t *testing.T) {
	r := newOrchRig(t, "limits", core.DelegationLimits{MaxHelpers: 3, MaxParallel: 1, MaxDepth: 1}, func(a RunArgs, o *Orch, release *atomic.Bool) KiroResult {
		if strings.Contains(a.Prompt, "[try-nested]") {
			_, err := o.Delegate(*a.Tag, Delegate{Provider: "kiro", Brief: "deeper"})
			if err == nil {
				return NewResult(core.Completed, "it could delegate")
			}
			return NewResult(core.Completed, err.Error())
		}
		return holdRun(a, release)
	})
	lead := r.lead("hold")
	first, err := r.ask(lead.Key, "kiro", "[try-nested] one", nil)
	if err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "the first to finish", func() bool {
		_, ok := r.orch.RunOf(val(first.Session))
		return ok || r.orch.HelpersOf(lead.Key)[0].State.Finished()
	})
	waitFor20(t, "done", func() bool { return r.orch.HelpersOf(lead.Key)[0].State == HelperDone })
	if nested := val(r.orch.HelpersOf(lead.Key)[0].Result); nested != OrchOff {
		t.Error("a helper at the depth limit can't delegate:", nested)
	}
	second, err := r.ask(lead.Key, "kiro", "hold please", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.ask(lead.Key, "kiro", "too many at once", nil); !strings.Contains(errText(err), "already working") || !strings.Contains(errText(err), "limit is 1") {
		t.Error(err)
	}
	if _, err := r.orch.Cancel(lead.Key, second.Run); err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "cancelled", func() bool { return r.orch.HelpersOf(lead.Key)[1].State == HelperCancelled })
	third, err := r.ask(lead.Key, "kiro", "hold again", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.orch.Cancel(lead.Key, third.Run); err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "cancelled too", func() bool { return r.orch.HelpersOf(lead.Key)[2].State == HelperCancelled })
	if _, err := r.ask(lead.Key, "kiro", "a fourth", nil); !strings.Contains(errText(err), "used its 3 helpers") {
		t.Error(err)
	}
	r.release.Store(true)
}

func TestAWaitThatRunsOutOfTimeDoesNotCancelAndAWaitingLeadGivesUpItsPlace(t *testing.T) {
	r := newOrchRig(t, "wait", core.DefaultDelegationLimits(), func(a RunArgs, _ *Orch, release *atomic.Bool) KiroResult { return holdRun(a, release) })
	r.k.SetMaxRunning(1)
	lead := r.lead("hold")
	// The only place is the lead's. The helper queues, and starts once the lead waits.
	run, err := r.ask(lead.Key, "codex", "slow job", nil)
	if err != nil || run.State != HelperQueued {
		t.Fatal("no place yet", run, err)
	}
	mid, err := r.orch.Wait(lead.Key, run.Run, 600*time.Millisecond)
	if err != nil || mid.State.Finished() {
		t.Fatal("the wait timed out and said the helper is still working", mid, err)
	}
	waitFor20(t, "the helper to be running", func() bool { return r.orch.HelpersOf(lead.Key)[0].State == HelperRunning })
	// Waiting again, the lead holds no place: the helper has the only one.
	type waited struct {
		i   RunInfo
		err error
	}
	waiter := make(chan waited, 1)
	go func() {
		i, err := r.orch.Wait(lead.Key, run.Run, 20*time.Second)
		waiter <- waited{i, err}
	}()
	waitFor20(t, "the lead to be parked", func() bool { return r.k.Running() == 1 })
	r.release.Store(true)
	done := <-waiter
	if done.err != nil || done.i.State != HelperDone || val(done.i.Result) != "Held, then done." {
		t.Errorf("%+v %v", done.i, done.err)
	}
}

func TestStopReachesHelpersAndTheirHelpersAndLateNewsWakesNobody(t *testing.T) {
	r := newOrchRig(t, "stop", core.DelegationLimits{MaxHelpers: 6, MaxParallel: 4, MaxDepth: 2}, func(a RunArgs, o *Orch, release *atomic.Bool) KiroResult {
		if strings.Contains(a.Prompt, "[delegate-then-hold]") {
			if _, err := o.Delegate(*a.Tag, Delegate{Provider: "codex", Brief: "grandchild job"}); err != nil {
				t.Error(err)
			}
		}
		return holdRun(a, release)
	})
	lead := r.lead("hold")
	if _, err := r.ask(lead.Key, "kiro", "[delegate-then-hold] child job", nil); err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "the grandchild to run", func() bool { return len(r.k.All()) == 3 && r.k.Running() == 3 })
	childSession := val(r.orch.HelpersOf(lead.Key)[0].Session)
	r.k.Stop(lead.ID)
	waitFor20(t, "everything to stop", func() bool { return r.k.Running() == 0 })
	waitFor20(t, "runs settled", func() bool { return r.orch.HelpersOf(lead.Key)[0].State == HelperCancelled })
	if n := len(r.orch.HelpersOf(childSession)); n != 1 {
		t.Fatal(n)
	}
	waitFor20(t, "the grandchild stopped", func() bool { return r.orch.HelpersOf(childSession)[0].State == HelperCancelled })
	// Nothing they report afterwards wakes the lead that was stopped, and it may not start
	// new helpers.
	r.release.Store(true)
	time.Sleep(300 * time.Millisecond)
	if n := len(must(r.k.Get(lead.ID)).Turns); n != 1 {
		t.Error("the stopped lead was not restarted", n)
	}
	for _, h := range r.orch.HelpersOf(lead.Key) {
		if h.Delivery == DeliverySent {
			t.Error(h)
		}
	}
	if _, err := r.orch.Delegate(lead.Key, Delegate{Provider: "kiro", Brief: "x"}); err == nil {
		t.Error("a stopped lead delegated")
	}
	if n := r.k.Running(); n != 0 {
		t.Error(n)
	}
}

func TestAResultThatComesAfterTheLeadFinishedIsSentOnceAndNeverTwice(t *testing.T) {
	r := newOrchRig(t, "late", core.DefaultDelegationLimits(), func(a RunArgs, o *Orch, release *atomic.Bool) KiroResult {
		if strings.HasPrefix(a.Prompt, "[Hover helper task]") {
			return holdRun(a, release)
		}
		if strings.Contains(a.Prompt, "hover-run:") {
			return NewResult(core.Completed, "Thanks, noted.")
		}
		if _, err := o.Delegate(*a.Tag, Delegate{Provider: "codex", Brief: "go and look"}); err != nil {
			t.Error(err)
		}
		return NewResult(core.Completed, "I asked a helper and I'm finished for now.")
	})
	lead := r.lead("delegate and don't wait")
	waitFor20(t, "the lead to finish", r.idle(lead.ID))
	if n := r.orch.PendingFor(lead.Key); n != 1 {
		t.Error("the helper is still at work and the lead's screen can say so", n)
	}
	r.release.Store(true)
	waitFor20(t, "the news to reach the lead", func() bool {
		s, ok := r.k.Get(lead.ID)
		return ok && len(s.Turns) == 2 && s.Turns[1].Result != nil
	})
	s := must(r.k.Get(lead.ID))
	if !strings.Contains(s.Turns[1].Prompt, "hover-run:") || !strings.Contains(s.Turns[1].Prompt, "Held, then done.") {
		t.Error(s.Turns[1].Prompt)
	}
	if d := r.orch.HelpersOf(lead.Key)[0].Delivery; d != DeliverySent {
		t.Error(d)
	}
	// Asking again, or a restart that finds the mark already in the lead's history, sends
	// nothing more.
	r.orch.DeliverTo(lead.Key)
	r.orch.DeliverPending()
	time.Sleep(200 * time.Millisecond)
	if n := len(must(r.k.Get(lead.ID)).Turns); n != 2 {
		t.Error(n)
	}
}

func TestARestartFailsRunsThatWereCutOffAndDoesNotSendAResultTwice(t *testing.T) {
	r := newOrchRig(t, "restart", core.DefaultDelegationLimits(), func(RunArgs, *Orch, *atomic.Bool) KiroResult { return NewResult(core.Completed, "ok") })
	// A record left by an earlier run of Hover: one helper still "running" with no process
	// behind it, and one that finished and whose result is already in the lead's history
	// but was never marked.
	lead := r.lead("the lead")
	waitFor20(t, "the lead to finish", r.idle(lead.ID))
	os.MkdirAll(filepath.Join(r.root, "orch2"), 0o777)
	run := func(id, state string, session *string) core.JSON {
		return core.JObj(core.P("Id", core.JStr(id)), core.P("Parent", core.JStr(lead.Key)), core.P("Root", core.JStr(lead.Key)), core.P("Depth", core.JInt(1)),
			core.P("Provider", core.JStr("codex")), core.P("Brief", core.JStr("b")), core.P("Access", core.JStr("full")), core.P("State", core.JStr(state)), core.P("Result", core.JStr("the result")),
			core.P("Session", core.JOptStr(session)), core.P("Attempts", core.JArr()), core.P("Delivery", core.JStr("pending")), core.P("Created", core.JInt(1)))
	}
	doc := core.SealedIn(filepath.Join(r.root, "orch2"), "runs", r.crypto)
	if err := doc.Write(core.JObj(core.P("Runs", core.JArr(run("r-cut", "running", sp("ghost")), run("r-sent", "done", nil))))); err != nil {
		t.Fatal(err)
	}
	// The lead already has r-sent's result in its history (the earlier run sent it, then
	// died before it wrote that down).
	if !r.k.Reply(lead.ID, "[Hover] A helper finished (hover-run:r-sent; codex; done).\n\nthe result", nil) {
		t.Fatal("no reply")
	}
	waitFor20(t, "that reply", func() bool { s, ok := r.k.Get(lead.ID); return ok && !s.Busy() && len(s.Turns) == 2 })
	before := len(must(r.k.Get(lead.ID)).Turns)
	if before != 2 {
		t.Fatal(before)
	}
	o := NewOrch(r.k, r.env, doc)
	h := o.HelpersOf(lead.Key)
	i := slices.IndexFunc(h, func(x RunInfo) bool { return x.Run == "r-cut" })
	if i < 0 || h[i].State != HelperFailed || !strings.Contains(val(h[i].Note), "Hover closed") {
		t.Fatalf("%+v", h)
	}
	o.DeliverPending()
	// The lead is told about the run that was cut off, once; r-sent is in its history
	// already, so it is marked and not sent again.
	waitFor20(t, "the news of the cut-off run", func() bool { s, ok := r.k.Get(lead.ID); return ok && len(s.Turns) == before+1 && !s.Busy() })
	turns := must(r.k.Get(lead.ID)).Turns
	if told := turns[len(turns)-1].Prompt; !strings.Contains(told, "hover-run:r-cut") || strings.Contains(told, "hover-run:r-sent") {
		t.Error(told)
	}
	o.DeliverPending()
	time.Sleep(200 * time.Millisecond)
	if n := len(must(r.k.Get(lead.ID)).Turns); n != before+1 {
		t.Error("nothing is sent twice", n)
	}
	h = o.HelpersOf(lead.Key)
	if i := slices.IndexFunc(h, func(x RunInfo) bool { return x.Run == "r-sent" }); h[i].Delivery != DeliverySent {
		t.Error(h[i].Delivery)
	}
}

func TestHelpersWorkInTheLeadsFolderAndAWritingOneIsToldSo(t *testing.T) {
	r := newOrchRig(t, "tree", core.DefaultDelegationLimits(), func(a RunArgs, o *Orch, _ *atomic.Bool) KiroResult {
		if strings.HasPrefix(a.Prompt, "[Hover helper task]") {
			if val(a.Access) != "read" {
				if err := os.WriteFile(filepath.Join(a.Folder, "by-helper.txt"), []byte("x"), 0o666); err != nil {
					t.Error(err)
				}
			}
			return NewResult(core.Completed, a.Folder)
		}
		tag := *a.Tag
		w, err1 := o.Delegate(tag, Delegate{Provider: "codex", Brief: "write"})
		ro, err2 := o.Delegate(tag, Delegate{Provider: "kiro", Brief: "look", Access: sp("read")})
		if err1 != nil || err2 != nil {
			t.Error(err1, err2)
			return NewResult(core.Failed, "no helpers")
		}
		w, err1 = o.Wait(tag, w.Run, 20*time.Second)
		ro, err2 = o.Wait(tag, ro.Run, 20*time.Second)
		if err1 != nil || err2 != nil || w.State != HelperDone || ro.State != HelperDone {
			t.Error(w, ro, err1, err2)
		}
		return NewResult(core.Completed, val(w.Result)+"|"+val(ro.Result))
	})
	lead := r.lead("ask two helpers")
	waitFor20(t, "the lead to finish", func() bool { s, ok := r.k.Get(lead.ID); return ok && !s.Busy() && s.Result() != nil })
	said := must(r.k.Get(lead.ID)).Result().Text
	writer, reader, ok := strings.Cut(said, "|")
	if !ok {
		t.Fatalf("the lead said: %s", said)
	}
	if writer != r.folder || reader != r.folder {
		t.Error("both helpers work in the lead's folder", writer, reader)
	}
	if !exists(filepath.Join(r.folder, "by-helper.txt")) {
		t.Error("no file by the helper")
	}
	type seen struct {
		access string
		said   bool
	}
	var notes []seen
	for _, h := range r.orch.HelpersOf(lead.Key) {
		notes = append(notes, seen{h.Access, strings.Contains(val(h.Note), "shares the task")})
	}
	if !slices.ContainsFunc(notes, func(n seen) bool { return n.access != "read" && n.said }) || slices.ContainsFunc(notes, func(n seen) bool { return n.access == "read" && n.said }) {
		t.Errorf("only the writer is told: %+v", notes)
	}
}

func TestALeadReadsMessagesAndStopsOnlyTheThreadsItLaunched(t *testing.T) {
	r := newOrchRig(t, "threads", core.DefaultDelegationLimits(), func(a RunArgs, _ *Orch, release *atomic.Bool) KiroResult {
		if strings.Contains(a.Prompt, "[hold]") {
			return holdRun(a, release)
		}
		return NewResult(core.Completed, "reply to: "+a.Prompt)
	})
	lead := r.lead("[hold]")
	other := must(r.k.StartBound(core.Codex, r.folder, "[hold]", nil, nil, nil, orchLink()))
	th, err := r.orch.ThreadLaunch(lead.Key, "codex", "first message", nil)
	if err != nil {
		t.Fatal(err)
	}
	reads := func(from int, want string) func() bool {
		return func() bool {
			text, _, _, err := r.orch.ThreadRead(lead.Key, th, from, 10_000)
			return err == nil && strings.Contains(text, want)
		}
	}
	waitFor20(t, "the thread to answer", reads(0, "reply to: first message"))
	if err := r.orch.ThreadSend(lead.Key, th, "second [hold]"); err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "the second turn to run", reads(1, "second [hold]"))
	// Reading is bounded, in pages, from a turn.
	page, next, more, err := r.orch.ThreadRead(lead.Key, th, 0, 70)
	if err != nil || !strings.Contains(page, "first message") || next != 1 || !more {
		t.Error(page, next, more, err)
	}
	// Another task that has the id can neither read nor change it.
	_, _, _, e1 := r.orch.ThreadRead(other.Key, th, 0, 100)
	for _, e := range []error{e1, r.orch.ThreadSend(other.Key, th, "hi"), r.orch.ThreadInterrupt(other.Key, th)} {
		if !strings.Contains(errText(e), "belongs to another task") {
			t.Error(e)
		}
	}
	if err := r.orch.ThreadInterrupt(lead.Key, th); err != nil {
		t.Fatal(err)
	}
	waitFor20(t, "the thread to stop", reads(1, "Stopped."))
	r.release.Store(true)
}

func TestTheMcpToolsAnswerALeadAndRefuseAStaleOne(t *testing.T) {
	r := newOrchRig(t, "mcp", core.DefaultDelegationLimits(), func(a RunArgs, o *Orch, _ *atomic.Bool) KiroResult {
		if strings.HasPrefix(a.Prompt, "[Hover helper task]") {
			return NewResult(core.Completed, "found it")
		}
		tag := *a.Tag
		call := func(name string, args ...core.Prop) (string, bool) {
			m := core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", core.JInt(1)), core.P("method", core.JStr("tools/call")), core.P("params", core.JObj(core.P("name", core.JStr(name)), core.P("arguments", core.JObj(args...)))))
			out, ok := o.Answer(tag, m)
			if !ok {
				t.Error("no reply")
				return "", true
			}
			res := get(out, "result")
			items, _ := get(res, "content").Items()
			text, _ := get(items[0], "text").AsStr()
			bad := false
			if b, ok := res.Get("isError"); ok {
				bad, _ = b.Bool()
			}
			return text, bad
		}
		listing, bad := call("list_providers")
		if bad || !strings.Contains(listing, "codex — CODEX: ready") || !strings.Contains(listing, "claude — CLAUDE: not ready (sign in first)") {
			t.Error(listing)
		}
		text, bad := call("delegate_task", core.P("provider", core.JStr("codex")), core.P("brief", core.JStr("find it")), core.P("request_id", core.JStr("q1")))
		if bad || !strings.Contains(text, "run_id: r-") {
			t.Error(text)
			return NewResult(core.Failed, text)
		}
		runID := strings.TrimPrefix(strings.SplitN(text, "\n", 2)[0], "run_id: ")
		if text, bad := call("wait_for_task", core.P("run_id", core.JStr(runID)), core.P("timeout_secs", core.JInt(10))); bad || !strings.Contains(text, "state: done") || !strings.Contains(text, "found it") {
			t.Error(text)
		}
		if text, bad := call("task_result", core.P("run_id", core.JStr("r-nope"))); !bad || !strings.Contains(text, "no helper with that id") {
			t.Error(text)
		}
		if text, bad := call("delegate_task", core.P("provider", core.JStr("codex"))); !bad || !strings.Contains(text, "brief is needed") {
			t.Error(text)
		}
		return NewResult(core.Completed, "ok")
	})
	lead := r.lead("use the tools")
	waitFor20(t, "the lead to finish", func() bool { s, ok := r.k.Get(lead.ID); return ok && !s.Busy() && s.Result() != nil })
	if res := must(r.k.Get(lead.ID)).Result(); res.State != core.Completed {
		t.Error(res.Text)
	}
	// The server lists the tools, and a call from the finished turn is refused as stale.
	list, _ := r.orch.Answer(lead.Key, core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", core.JInt(2)), core.P("method", core.JStr("tools/list"))))
	if c := list.Compact(); !strings.Contains(c, "delegate_task") || !strings.Contains(c, "interrupt_thread") {
		t.Error(c)
	}
	stale, _ := r.orch.Answer(lead.Key, core.JObj(core.P("jsonrpc", core.JStr("2.0")), core.P("id", core.JInt(3)), core.P("method", core.JStr("tools/call")),
		core.P("params", core.JObj(core.P("name", core.JStr("list_providers")), core.P("arguments", core.JObj())))))
	if c := stale.Compact(); !strings.Contains(c, "no longer valid") || !strings.Contains(c, `"isError":true`) {
		t.Error(c)
	}
	if _, ok := r.orch.Answer(lead.Key, core.JObj(core.P("method", core.JStr("notifications/initialized")))); ok {
		t.Error("a notification gets no reply")
	}
}

// The whole way an agent reaches it: the MCP command Hover hands out (perl's relay) joined
// to Hover's socket, JSON lines in and out. A task without delegation is handed no server
// at all.
func TestAnAgentReachesTheHelpersThroughTheRelayAndTheSocket(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the socket is Unix's")
	}
	if !isFile(perlPath) {
		t.Skip("needs perl")
	}
	short := fmt.Sprintf("/tmp/hvo-%d", os.Getpid())
	os.MkdirAll(short, 0o777)
	defer os.RemoveAll(short)
	t.Setenv("HOVER_BROWSER_SOCKET", filepath.Join(short, "b.sock"))
	r := newOrchRig(t, "socket", core.DefaultDelegationLimits(), func(a RunArgs, _ *Orch, release *atomic.Bool) KiroResult { return holdRun(a, release) })
	r.orch.Install()
	lead := r.lead("[hold]")
	plain := must(r.k.Start(core.Codex, r.folder, "[hold]", nil))
	if len(OrchServers(&plain.Key)) != 0 {
		t.Error("no delegation, no server")
	}
	if len(OrchServers(nil)) != 0 {
		t.Error("a server for no session")
	}
	servers := OrchServers(&lead.Key)
	if len(servers) != 1 {
		t.Fatal("a Unix host with perl hands the server out", servers)
	}
	s := servers[0]
	if s.Name != OrchServerName {
		t.Error(s.Name)
	}
	if slices.ContainsFunc(s.Args, func(a string) bool { return strings.Contains(a, "HELLO") }) {
		t.Error("no token on the command line")
	}
	cmd := exec.Command(s.Command, s.Args...)
	cmd.Env = os.Environ()
	for _, e := range s.Env {
		cmd.Env = append(cmd.Env, e[0]+"="+e[1])
	}
	input, _ := cmd.StdinPipe()
	stdout, _ := cmd.StdoutPipe()
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	out := bufio.NewReader(stdout)
	ask := func(line string) core.JSON {
		io.WriteString(input, line+"\n")
		l, _ := out.ReadString('\n')
		v, err := core.ParseJSON(strings.TrimSpace(l))
		if err != nil {
			t.Fatal(l, err)
		}
		return v
	}
	init := ask(`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}`)
	if n, _ := get(get(get(init, "result"), "serverInfo"), "name").AsStr(); n != OrchServerName {
		t.Error(init.Compact())
	}
	if list := ask(`{"jsonrpc":"2.0","id":2,"method":"tools/list"}`); !strings.Contains(list.Compact(), "delegate_task") {
		t.Error(list.Compact())
	}
	call := ask(`{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"delegate_task","arguments":{"provider":"codex","brief":"look around","request_id":"x1"}}}`)
	if !strings.Contains(call.Compact(), "run_id: r-") {
		t.Error(call.Compact())
	}
	// The call was made for the lead whose token this is, and nobody else.
	if len(r.orch.HelpersOf(lead.Key)) != 1 || len(r.orch.HelpersOf(plain.Key)) != 0 {
		t.Error(r.orch.HelpersOf(lead.Key), r.orch.HelpersOf(plain.Key))
	}
	input.Close()
	cmd.Wait()
	r.release.Store(true)
	waitFor20(t, "all done", func() bool { return r.k.Running() == 0 })
	BrowserStop()
}

func TestAnAgentSentAConversationReferenceReadsItInPagesAndCannotReadOthers(t *testing.T) {
	r := newOrchRig(t, "readconv", core.DefaultDelegationLimits(), func(a RunArgs, o *Orch, _ *atomic.Bool) KiroResult {
		tag := *a.Tag
		if !strings.Contains(a.Prompt, "[Attached by Hover]") {
			who := "earlier"
			if strings.Contains(a.Prompt, "other") {
				who = "other"
			}
			return NewResult(core.Completed, fmt.Sprintf("I am %s. The secret of this one is: blue.", who))
		}
		_, rest, _ := strings.Cut(a.Prompt, "(key ")
		key, _, _ := strings.Cut(rest, ")")
		page, next, more, err := o.ReadConversation(tag, key, 0, 10_000)
		if err != nil {
			t.Error(err)
		}
		_, _, _, denied := o.ReadConversation(tag, "not-given", 0, 100)
		return NewResult(core.Completed, fmt.Sprintf("%s|%d|%t|%s", page, next, more, errText(denied)))
	})
	earlier := must(r.k.Start(core.Codex, r.folder, "earlier topic", nil))
	must(r.k.Start(core.Codex, r.folder, "other topic", nil))
	me := must(r.k.Start(core.Kiro, r.folder, "hello", nil))
	waitFor20(t, "all", func() bool { return r.k.Running() == 0 })
	chip := ThreadChip(earlier.Key, "Earlier topic")
	if !r.k.ReplyMsg(me.ID, Msg{Text: "please read it", Chips: []core.Chip{chip}}) {
		t.Fatal("no reply")
	}
	waitFor20(t, "the reply", func() bool {
		s, ok := r.k.Get(me.ID)
		return ok && !s.Busy() && len(s.Turns) == 2 && s.Turns[1].Result != nil
	})
	said := must(r.k.Get(me.ID)).Result().Text
	if !strings.Contains(said, "[0] User: earlier topic") || !strings.Contains(said, "secret of this one is: blue") || !strings.Contains(said, "|1|false|") {
		t.Error(said)
	}
	if strings.Contains(said, "other topic") {
		t.Error("only the referenced conversation")
	}
	if !strings.Contains(said, "You were not given a reference to that conversation") {
		t.Error(said)
	}
	listed, _ := r.orch.Answer(me.Key, core.JObj(core.P("id", core.JInt(1)), core.P("method", core.JStr("tools/list"))))
	if c := listed.Compact(); !strings.Contains(c, "read_conversation") || strings.Contains(c, "delegate_task") {
		t.Error("without delegation only the reading tool is listed:", c)
	}
}
